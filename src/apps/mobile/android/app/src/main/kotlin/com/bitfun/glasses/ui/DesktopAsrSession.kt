package com.bitfun.glasses.ui

import android.content.Context
import android.os.Handler
import android.os.Looper
import android.util.Base64
import android.util.Log
import java.io.ByteArrayOutputStream
import java.util.UUID
import java.util.concurrent.ConcurrentHashMap
import java.util.concurrent.atomic.AtomicBoolean

/**
 * Record RayNeo mic PCM, then ask glasses-web to transcribe via the paired
 * desktop Voice Input local ASR ([RemoteCommand::TranscribeSpeech]).
 */
class DesktopAsrSession(
    private val context: Context,
    private val listener: Listener,
) {
    interface Listener {
        fun onStarted()
        fun onInfo(message: String)
        fun onRequestDesktopTranscribe(requestId: String)
        fun onFinal(text: String)
        fun onError(message: String)
        fun onEnded()
    }

    private val mainHandler = Handler(Looper.getMainLooper())
    private val pcmBuffer = ByteArrayOutputStream()
    private val lock = Any()
    private var capture: VoicePcmCapture? = null
    private var active = false
    private var delivered = AtomicBoolean(false)
    private var heardSpeech = false
    private var lastLoudAtMs = 0L
    private var speechStartedAtMs = 0L
    private var sessionStartedAtMs = 0L
    private var pendingRequestId: String? = null
    private var sessionEpoch = 0

    private val maxDurationWatch = Runnable {
        if (active && !delivered.get()) {
            finalizeCapture("max-duration")
        }
    }

    private lateinit var silenceWatch: Runnable

    init {
        silenceWatch = Runnable {
            if (!active || delivered.get()) return@Runnable
            // Keep polling until real speech is heard.
            if (!heardSpeech) {
                mainHandler.postDelayed(silenceWatch, 200L)
                return@Runnable
            }
            val now = System.currentTimeMillis()
            val spokenMs = now - speechStartedAtMs
            val idle = now - lastLoudAtMs
            // Require a minimum utterance so mic-open clicks cannot end the turn.
            if (spokenMs >= MIN_SPEECH_MS && idle >= SILENCE_END_MS) {
                finalizeCapture("silence")
            } else {
                mainHandler.postDelayed(silenceWatch, 200L)
            }
        }
    }

    fun isAvailable(): Boolean = true

    fun start(): Boolean {
        abortInternal(notifyEnded = false)
        delivered.set(false)
        heardSpeech = false
        lastLoudAtMs = 0L
        speechStartedAtMs = 0L
        pendingRequestId = null
        sessionEpoch += 1
        val epoch = sessionEpoch
        sessionStartedAtMs = System.currentTimeMillis()
        synchronized(lock) { pcmBuffer.reset() }
        active = true
        listener.onStarted()

        val sessionCapture = VoicePcmCapture(
            context,
            onPcm = { buffer, length ->
                if (!active || delivered.get() || length <= 0 || epoch != sessionEpoch) {
                    return@VoicePcmCapture
                }
                val chunk = buffer.copyOf(length)
                synchronized(lock) { pcmBuffer.write(chunk) }
                val now = System.currentTimeMillis()
                // Ignore mic-open / AGC priming noise.
                if (now - sessionStartedAtMs < PRIMING_MS) return@VoicePcmCapture
                val peak = peakAbsPcm16(chunk, chunk.size)
                if (peak >= SPEECH_PEAK) {
                    val firstSpeech = !heardSpeech
                    heardSpeech = true
                    lastLoudAtMs = now
                    if (firstSpeech) {
                        speechStartedAtMs = now
                        mainHandler.removeCallbacks(silenceWatch)
                        mainHandler.postDelayed(silenceWatch, 200L)
                    }
                }
            },
            onCaptureError = { message ->
                mainHandler.post {
                    if (epoch != sessionEpoch) return@post
                    fail(message)
                }
            },
        )
        capture = sessionCapture
        if (!sessionCapture.start()) {
            fail("无法打开麦克风")
            return false
        }
        mainHandler.postDelayed(maxDurationWatch, MAX_RECORD_MS)
        mainHandler.postDelayed(silenceWatch, PRIMING_MS)
        Log.e(TAG, "DesktopAsrSession started epoch=$epoch")
        return true
    }

    fun stop() {
        if (!active || delivered.get()) return
        finalizeCapture("user-stop")
    }

    fun abort() {
        abortInternal(notifyEnded = true)
    }

    /** Reset without emitting ended — used when starting a fresh capture. */
    fun resetForRestart() {
        abortInternal(notifyEnded = false)
    }

    fun destroy() {
        abortInternal(notifyEnded = false)
    }

    /** Called from SPA after desktop transcription finishes. */
    fun completeTranscription(requestId: String, text: String?, error: String?) {
        if (pendingRequestId != requestId) {
            Log.e(TAG, "Stale desktop ASR completion requestId=$requestId")
            return
        }
        if (!delivered.compareAndSet(false, true)) return
        pendingPcm.remove(requestId)
        pendingRequestId = null
        active = false
        val trimmed = text?.trim().orEmpty()
        if (trimmed.isNotEmpty()) {
            listener.onFinal(trimmed)
        } else {
            listener.onError(error?.takeIf { it.isNotBlank() } ?: "未识别到语音，请靠近麦克风后重试")
        }
        listener.onEnded()
    }

    fun takePendingPcmBase64(requestId: String): String? {
        val pcm = pendingPcm.remove(requestId) ?: return null
        return Base64.encodeToString(pcm, Base64.NO_WRAP)
    }

    private fun finalizeCapture(reason: String) {
        if (!active || delivered.get()) return
        mainHandler.removeCallbacks(maxDurationWatch)
        mainHandler.removeCallbacks(silenceWatch)
        capture?.stop()
        capture = null
        active = false

        val pcm = synchronized(lock) { pcmBuffer.toByteArray() }
        val peak = peakAbsPcm16(pcm, pcm.size)
        Log.e(TAG, "DesktopAsr finalize reason=$reason bytes=${pcm.size} peak=$peak heard=$heardSpeech")
        if (pcm.isEmpty() || peak < SPEECH_PEAK) {
            fail("未识别到语音，请靠近麦克风后重试")
            return
        }
        val requestId = UUID.randomUUID().toString()
        pendingRequestId = requestId
        pendingPcm[requestId] = pcm
        listener.onRequestDesktopTranscribe(requestId)
        mainHandler.postDelayed({
            if (pendingRequestId == requestId && !delivered.get()) {
                fail("桌面语音识别超时，请确认桌面已打开且已在设置中安装本地 ASR 模型")
            }
        }, TRANSCRIBE_TIMEOUT_MS)
    }

    private fun fail(message: String) {
        if (!delivered.compareAndSet(false, true)) return
        active = false
        mainHandler.removeCallbacks(maxDurationWatch)
        mainHandler.removeCallbacks(silenceWatch)
        capture?.stop()
        capture = null
        pendingRequestId?.let { pendingPcm.remove(it) }
        pendingRequestId = null
        listener.onError(message)
        listener.onEnded()
    }

    private fun abortInternal(notifyEnded: Boolean) {
        mainHandler.removeCallbacks(maxDurationWatch)
        mainHandler.removeCallbacks(silenceWatch)
        active = false
        delivered.set(true)
        capture?.stop()
        capture = null
        pendingRequestId?.let { pendingPcm.remove(it) }
        pendingRequestId = null
        synchronized(lock) { pcmBuffer.reset() }
        if (notifyEnded) {
            mainHandler.post { listener.onEnded() }
        }
    }

    private fun peakAbsPcm16(buffer: ByteArray, length: Int): Int {
        var peak = 0
        var i = 0
        while (i + 1 < length) {
            val sample = (buffer[i].toInt() and 0xff) or (buffer[i + 1].toInt() shl 8)
            val abs = kotlin.math.abs(sample.toShort().toInt())
            if (abs > peak) peak = abs
            i += 2
        }
        return peak
    }

    companion object {
        private const val TAG = "BitFunGlassesVoice"
        private const val SPEECH_PEAK = 350
        /** Ignore mic-open click / AGC ramp before VAD. */
        private const val PRIMING_MS = 500L
        /** Minimum voiced span before silence can auto-finalize. */
        private const val MIN_SPEECH_MS = 700L
        /** End-of-utterance → auto start desktop ASR (click-stop still works). */
        private const val SILENCE_END_MS = 2_000L
        private const val MAX_RECORD_MS = 12_000L
        private const val TRANSCRIBE_TIMEOUT_MS = 90_000L
        private val pendingPcm = ConcurrentHashMap<String, ByteArray>()
    }
}
