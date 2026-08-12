package com.bitfun.glasses.ui

import android.app.Activity
import android.content.ActivityNotFoundException
import android.content.ComponentName
import android.content.Context
import android.content.Intent
import android.content.pm.PackageManager
import android.media.AudioManager
import android.os.Build
import android.os.Bundle
import android.os.Handler
import android.os.Looper
import android.provider.Settings
import android.speech.RecognitionListener
import android.speech.RecognitionService
import android.speech.RecognizerIntent
import android.speech.SpeechRecognizer
import android.util.Log
import androidx.activity.result.ActivityResultLauncher
import com.bitfun.glasses.R
import com.huawei.hiai.asr.AsrConstants
import com.huawei.hiai.asr.AsrError
import com.huawei.hiai.asr.AsrListener
import com.huawei.hiai.asr.AsrRecognizer
import org.json.JSONArray
import org.json.JSONObject
import java.util.Locale

/**
 * Glasses voice input.
 *
 * Primary path on RayNeo X3: capture wearer mic PCM locally, then transcribe
 * with the paired desktop's current Voice Input local ASR via relay
 * [RemoteCommand::TranscribeSpeech]. System SpeechRecognizer is a stub on this
 * device and is only a last-resort fallback.
 */
class VoiceInputController(
    private val context: Context,
    private val launcher: ActivityResultLauncher<Intent>,
    private val listener: Listener,
) {
    interface Listener {
        fun onVoiceStarted()
        fun onVoiceResult(text: String)
        fun onVoiceError(message: String)
        fun onVoiceEnded()
        /** Diagnostic / progress toast for SPA. */
        fun onVoiceInfo(message: String) {}
        /** Ask SPA to pull PCM and call desktop ASR over relay. */
        fun onDesktopAsrRequest(requestId: String) {}
    }

    private enum class Mode { NONE, DESKTOP, HIAI, IN_PROCESS, ACTIVITY }

    private val mainHandler = Handler(Looper.getMainLooper())
    private val audioManager =
        context.applicationContext.getSystemService(Context.AUDIO_SERVICE) as AudioManager
    private var recognizer: SpeechRecognizer? = null
    private var hiaiRecognizer: AsrRecognizer? = null
    private var pcmCapture: VoicePcmCapture? = null
    private var desktopSession: DesktopAsrSession? = null
    private var hiaiReady = false
    private var hiaiInitFailed = false
    private var pendingHiaiStart = false
    private var listening = false
    private var mode = Mode.NONE
    /** Last non-empty HiAI partial transcript for this listen session. */
    private var lastPartialText: String = ""
    /** True after result/error was delivered for the current HiAI session. */
    private var hiaiResultDelivered = false
    /** True after VAD end-of-speech triggered stopListening finalize. */
    private var hiaiFinalizing = false
    /** Controller-owned RayNeo mic mode for HiAI RECORD path (PCM path owns its own). */
    private var rayNeoModeApplied = false
    private var hiaiUsesPcmFeed = false
    /** One automatic recover attempt after ERROR_RECOGNIZER_BUSY per user start. */
    private var hiaiBusyRetryPending = false

    private val listenWatchdog = Runnable {
        if (!listening || hiaiResultDelivered) return@Runnable
        Log.w(TAG, "listen watchdog mode=$mode — forcing finalize")
        when (mode) {
            Mode.HIAI -> {
                finalizeHiaiAfterSpeech()
                mainHandler.postDelayed({
                    if (listening && mode == Mode.HIAI && !hiaiResultDelivered) {
                        deliverHiaiOutcome(
                            text = lastPartialText,
                            fallbackError = context.getString(R.string.voice_error_no_match),
                        )
                    }
                }, STOP_WATCHDOG_MS)
            }
            Mode.IN_PROCESS -> {
                try {
                    recognizer?.stopListening()
                    scheduleStopWatchdog()
                } catch (_: Exception) {
                    deliverInProcessOutcome(lastPartialText)
                }
            }
            else -> Unit
        }
    }

    private val stopWatchdog = Runnable {
        if (!listening || hiaiResultDelivered) return@Runnable
        Log.w(TAG, "stop watchdog mode=$mode — forcing outcome")
        when (mode) {
            Mode.HIAI -> deliverHiaiOutcome(
                text = lastPartialText,
                fallbackError = context.getString(R.string.voice_error_no_match),
            )
            Mode.IN_PROCESS -> deliverInProcessOutcome(lastPartialText)
            else -> finishListening()
        }
    }

    /** UI always offers the mic; real availability is discovered when starting. */
    fun isAvailable(): Boolean = true

    fun isListening(): Boolean = listening

    fun start() {
        Log.i(TAG, "start() mode=$mode listening=$listening")
        abortInProcessRecognizer()
        // Quiet reset only — abort(notifyEnded) races the new session's onStarted
        // and makes the SPA drop listening before the user can speak (2nd press).
        desktopSession?.resetForRestart()
        if (listening || pendingHiaiStart || mode == Mode.HIAI || mode == Mode.DESKTOP) {
            hardResetEngine(keepEngine = true, notifyEnded = false)
        }
        hiaiBusyRetryPending = false
        hiaiInitFailed = false
        lastPartialText = ""
        hiaiResultDelivered = false
        mode = Mode.NONE
        listening = false

        if (startDesktopAsr()) return
        if (isHiaiPackagePresent() && startHiai()) return
        if (startInProcess()) return
        if (startActivityUi()) return
        listener.onVoiceError(context.getString(R.string.voice_unavailable))
        listener.onVoiceEnded()
    }

    fun takePendingAsrPcm(requestId: String): String? =
        desktopSession?.takePendingPcmBase64(requestId)

    fun completeDesktopAsr(requestId: String, text: String?, error: String?) {
        desktopSession?.completeTranscription(requestId, text, error)
    }

    private fun startDesktopAsr(): Boolean {
        val session = desktopSession ?: DesktopAsrSession(
            context,
            object : DesktopAsrSession.Listener {
                override fun onStarted() {
                    mode = Mode.DESKTOP
                    listening = true
                    lastPartialText = ""
                    hiaiResultDelivered = false
                    listener.onVoiceStarted()
                }

                override fun onInfo(message: String) {
                    listener.onVoiceInfo(message)
                }

                override fun onRequestDesktopTranscribe(requestId: String) {
                    listener.onDesktopAsrRequest(requestId)
                }

                override fun onFinal(text: String) {
                    if (hiaiResultDelivered) return
                    hiaiResultDelivered = true
                    listening = false
                    mode = Mode.NONE
                    listener.onVoiceResult(text)
                }

                override fun onError(message: String) {
                    if (hiaiResultDelivered) return
                    hiaiResultDelivered = true
                    listening = false
                    mode = Mode.NONE
                    listener.onVoiceError(message)
                }

                override fun onEnded() {
                    listening = false
                    mode = Mode.NONE
                    listener.onVoiceEnded()
                }
            },
        ).also { desktopSession = it }
        Log.i(TAG, "Starting desktop Voice Input ASR")
        return session.start()
    }

    /**
     * Ask the active engine to finalize. Prefer stopListening over cancel so
     * RayNeoRecognitionService can still deliver onResults / last partial.
     */
    fun stop() {
        Log.i(TAG, "stop() mode=$mode listening=$listening delivered=$hiaiResultDelivered")
        when (mode) {
            Mode.DESKTOP -> {
                desktopSession?.stop()
            }
            Mode.IN_PROCESS -> {
                if (!listening) return
                try {
                    recognizer?.stopListening()
                    scheduleStopWatchdog()
                } catch (error: Exception) {
                    Log.e(TAG, "IN_PROCESS stopListening failed", error)
                    deliverInProcessOutcome(lastPartialText)
                }
            }
            Mode.HIAI -> {
                if (!listening && !pendingHiaiStart) return
                try {
                    stopPcmCapture()
                    hiaiRecognizer?.stopListening()
                    scheduleStopWatchdog()
                } catch (error: Exception) {
                    Log.e(TAG, "HiAI stopListening failed", error)
                    deliverHiaiOutcome(
                        text = lastPartialText,
                        fallbackError = context.getString(R.string.voice_error_no_match),
                    )
                }
            }
            Mode.ACTIVITY -> {
                if (listening) finishListening()
            }
            Mode.NONE -> {
                pendingHiaiStart = false
            }
        }
    }

    /** Hard abort used when SPA starts while a remote session is still busy. */
    fun abortAndReadyForRestart() {
        Log.i(TAG, "abortAndReadyForRestart mode=$mode")
        clearWatchdogs()
        abortInProcessRecognizer()
        hardResetEngine(keepEngine = true, notifyEnded = true)
        listening = false
        mode = Mode.NONE
        pendingHiaiStart = false
        hiaiResultDelivered = false
        lastPartialText = ""
    }

    fun destroy() {
        pendingHiaiStart = false
        clearWatchdogs()
        stopPcmCapture()
        releaseRayNeoMicMode()
        abortInProcessRecognizer()
        desktopSession?.destroy()
        desktopSession = null
        try {
            hiaiRecognizer?.cancel()
            hiaiRecognizer?.destroy()
        } catch (_: Exception) {
        }
        hiaiRecognizer = null
        hiaiReady = false
        listening = false
        mode = Mode.NONE
    }

    fun onActivityResult(resultCode: Int, data: Intent?) {
        if (mode != Mode.ACTIVITY) return
        try {
            if (resultCode == Activity.RESULT_OK) {
                val texts = data
                    ?.getStringArrayListExtra(RecognizerIntent.EXTRA_RESULTS)
                    .orEmpty()
                val text = texts.firstOrNull()?.trim().orEmpty()
                if (text.isNotEmpty()) {
                    listener.onVoiceResult(text)
                } else {
                    listener.onVoiceError(context.getString(R.string.voice_error_no_match))
                }
            }
        } catch (error: Exception) {
            Log.e(TAG, "Failed to read speech activity result", error)
            listener.onVoiceError(context.getString(R.string.voice_error_generic))
        } finally {
            finishListening()
        }
    }

    private fun startHiai(): Boolean {
        if (hiaiInitFailed || !isHiaiPackagePresent()) return false
        return try {
            val engine = ensureHiaiRecognizer() ?: return false
            if (!hiaiReady) {
                pendingHiaiStart = true
                mode = Mode.HIAI
                Log.i(TAG, "HiAI init in progress; will start after onInit")
                return true
            }
            beginHiaiListening(engine)
        } catch (error: Exception) {
            Log.e(TAG, "Failed to start HiAI ASR", error)
            hiaiInitFailed = true
            false
        }
    }

    private fun ensureHiaiRecognizer(): AsrRecognizer? {
        hiaiRecognizer?.let { return it }
        val created = try {
            AsrRecognizer.createAsrRecognizer(context)
        } catch (error: Exception) {
            Log.e(TAG, "AsrRecognizer.createAsrRecognizer failed", error)
            hiaiInitFailed = true
            return null
        }
        val initIntent = Intent().apply {
            putExtra(AsrConstants.ASR_AUDIO_SRC_TYPE, AsrConstants.ASR_SRC_TYPE_PCM)
            putExtra(AsrConstants.ASR_SAMPLE_RATE, VoicePcmCapture.SAMPLE_RATE)
            putExtra(AsrConstants.ASR_ENGINE_MODE, AsrConstants.HW_LOCAL_RECOGNIZER)
            putExtra(AsrConstants.ASR_VAD_FRONT_WAIT_MS, 6000)
            putExtra(AsrConstants.ASR_VAD_END_WAIT_MS, 1800)
            putExtra(AsrConstants.ASR_TIMEOUT_THRESHOLD_MS, 15000)
            putExtra(AsrConstants.LANGUAGE, preferredLocaleTag())
            putExtra(AsrConstants.REGION, AsrConstants.REGION_ZH)
        }
        Log.e(TAG, "HiAI init PCM/RayNeo mic model=${Build.MODEL}")
        created.init(initIntent, object : AsrListener {
            override fun onInit(params: Bundle?) {
                val errorCode = params?.getInt(AsrConstants.ASR_ERROR_CODE, AsrError.SUCCESS)
                    ?: AsrError.SUCCESS
                Log.e(TAG, "HiAI onInit errorCode=$errorCode")
                if (errorCode != AsrError.SUCCESS) {
                    hiaiReady = false
                    hiaiInitFailed = true
                    val waiting = pendingHiaiStart
                    pendingHiaiStart = false
                    mode = Mode.NONE
                    if (waiting) {
                        // Do not fall back to FakeRecognitionService on RayNeo.
                        listener.onVoiceError(context.getString(R.string.voice_error_init))
                        listener.onVoiceEnded()
                    }
                    return
                }
                hiaiReady = true
                if (pendingHiaiStart) {
                    pendingHiaiStart = false
                    val engine = hiaiRecognizer
                    if (engine == null || !beginHiaiListening(engine)) {
                        listener.onVoiceError(context.getString(R.string.voice_start_failed))
                        listener.onVoiceEnded()
                    }
                }
            }

            override fun onBeginningOfSpeech() {
                Log.w(TAG, "HiAI onBeginningOfSpeech")
            }

            override fun onRmsChanged(rmsdB: Float) = Unit

            override fun onBufferReceived(buffer: ByteArray?) = Unit

            override fun onEndOfSpeech() {
                // PCM source does not auto-finalize: stop mic + stopListening so
                // onResults is produced. Without this, sessions often end empty.
                Log.w(TAG, "HiAI onEndOfSpeech")
                finalizeHiaiAfterSpeech()
            }

            override fun onError(error: Int) {
                Log.e(TAG, "HiAI onError=$error")
                stopPcmCapture()
                if (pendingHiaiStart && !listening) {
                    pendingHiaiStart = false
                    hiaiInitFailed = true
                    mode = Mode.NONE
                    listener.onVoiceError(hiaiErrorMessage(error))
                    listener.onVoiceEnded()
                    return
                }
                if (mode != Mode.HIAI || hiaiResultDelivered) return
                if (error == AsrError.ERROR_RECOGNIZER_BUSY && !hiaiBusyRetryPending) {
                    hiaiBusyRetryPending = true
                    Log.e(TAG, "HiAI BUSY — destroy engine and retry once")
                    hardResetEngine(keepEngine = false, notifyEnded = false)
                    mainHandler.postDelayed({
                        if (listening || hiaiResultDelivered) return@postDelayed
                        hiaiInitFailed = false
                        if (startHiai()) return@postDelayed
                        listener.onVoiceError(context.getString(R.string.voice_error_busy))
                        listener.onVoiceEnded()
                    }, BUSY_RETRY_DELAY_MS)
                    return
                }
                when (error) {
                    AsrError.ERROR_NO_MATCH,
                    AsrError.ERROR_SPEECH_TIMEOUT,
                    -> {
                        deliverHiaiOutcome(
                            text = lastPartialText,
                            fallbackError = context.getString(R.string.voice_error_no_match),
                        )
                    }
                    else -> {
                        deliverHiaiOutcome(
                            text = lastPartialText,
                            fallbackError = hiaiErrorMessage(error),
                        )
                    }
                }
            }

            override fun onResults(results: Bundle?) {
                if (mode != Mode.HIAI || hiaiResultDelivered) return
                stopPcmCapture()
                val text = extractHiaiText(results).ifBlank { lastPartialText }
                Log.w(TAG, "HiAI onResults text='$text'")
                deliverHiaiOutcome(
                    text = text,
                    fallbackError = context.getString(R.string.voice_error_no_match),
                )
            }

            override fun onPartialResults(partialResults: Bundle?) {
                val text = extractHiaiText(partialResults)
                if (text.isNotEmpty()) {
                    lastPartialText = text
                    Log.w(TAG, "HiAI onPartialResults text='$text'")
                }
            }

            override fun onEnd() {
                Log.w(TAG, "HiAI onEnd delivered=$hiaiResultDelivered partial='${lastPartialText.take(40)}'")
                stopPcmCapture()
                if (mode != Mode.HIAI) return
                if (!hiaiResultDelivered) {
                    deliverHiaiOutcome(
                        text = lastPartialText,
                        fallbackError = context.getString(R.string.voice_error_no_match),
                    )
                    return
                }
                if (listening) finishListening()
            }

            override fun onEvent(eventType: Int, params: Bundle?) = Unit

            override fun onLexiconUpdated(lexiconName: String?, errorCode: Int) = Unit

            override fun onRecordStart() {
                Log.i(TAG, "HiAI onRecordStart")
            }

            override fun onRecordEnd() {
                Log.i(TAG, "HiAI onRecordEnd")
            }
        })
        hiaiRecognizer = created
        Log.i(TAG, "Created HiAI AsrRecognizer")
        return created
    }

    private fun beginHiaiListening(engine: AsrRecognizer): Boolean {
        // PCM first: app owns RayNeo SPEAKER_MIC feed. RECORD previously fell
        // through to broken system ASR after engine cancel storms.
        if (beginHiaiPcmListening(engine)) return true
        Log.e(TAG, "HiAI PCM path failed; trying RECORD")
        return beginHiaiRecordListening(engine)
    }

    /**
     * Preferred RayNeo path: HiAI owns AudioRecord after
     * audio_source_record=voice_recognition (same vendor mic mode as the
     * official recognition sample, without app-side writePcm).
     */
    private fun beginHiaiRecordListening(engine: AsrRecognizer): Boolean {
        return try {
            applyRayNeoMicMode()
            resetHiaiSessionFlags()
            hiaiUsesPcmFeed = false
            mode = Mode.HIAI
            listening = true
            val listenIntent = Intent().apply {
                putExtra(AsrConstants.ASR_AUDIO_SRC_TYPE, AsrConstants.ASR_SRC_TYPE_RECORD)
                putExtra(AsrConstants.ASR_SAMPLE_RATE, VoicePcmCapture.SAMPLE_RATE)
                putExtra(AsrConstants.ASR_VAD_FRONT_WAIT_MS, 6000)
                putExtra(AsrConstants.ASR_VAD_END_WAIT_MS, 1800)
                putExtra(AsrConstants.ASR_TIMEOUT_THRESHOLD_MS, 15000)
                putExtra(AsrConstants.LANGUAGE, preferredLocaleTag())
            }
            engine.startListening(listenIntent)
            listener.onVoiceStarted()
            scheduleListenWatchdog()
            Log.e(TAG, "Started HiAI RECORD + RayNeo mic mode")
            true
        } catch (error: Exception) {
            Log.e(TAG, "HiAI RECORD startListening failed", error)
            releaseRayNeoMicMode()
            listening = false
            mode = Mode.NONE
            hiaiFinalizing = false
            false
        }
    }

    /**
     * Tear down an in-flight listen. When [keepEngine] is false the HiAI
     * recognizer is destroyed so a BUSY engine can be recreated.
     */
    private fun hardResetEngine(keepEngine: Boolean, notifyEnded: Boolean) {
        clearWatchdogs()
        stopPcmCapture()
        releaseRayNeoMicMode()
        // Quiet reset — abort(notifyEnded) races a subsequent onStarted.
        desktopSession?.resetForRestart()
        pendingHiaiStart = false
        listening = false
        hiaiFinalizing = false
        hiaiUsesPcmFeed = false
        lastPartialText = ""
        hiaiResultDelivered = false
        mode = Mode.NONE
        try {
            hiaiRecognizer?.cancel()
        } catch (error: Exception) {
            Log.e(TAG, "HiAI cancel during hard reset failed", error)
        }
        abortInProcessRecognizer()
        if (!keepEngine) {
            try {
                hiaiRecognizer?.destroy()
            } catch (error: Exception) {
                Log.e(TAG, "HiAI destroy during hard reset failed", error)
            }
            hiaiRecognizer = null
            hiaiReady = false
            hiaiInitFailed = false
        }
        if (notifyEnded) {
            mainHandler.post { listener.onVoiceEnded() }
        }
    }

    /** Fallback: app captures mic and feeds [AsrRecognizer.writePcm]. */
    private fun beginHiaiPcmListening(engine: AsrRecognizer): Boolean {
        return try {
            val listenIntent = Intent().apply {
                putExtra(AsrConstants.ASR_AUDIO_SRC_TYPE, AsrConstants.ASR_SRC_TYPE_PCM)
                putExtra(AsrConstants.ASR_SAMPLE_RATE, VoicePcmCapture.SAMPLE_RATE)
                putExtra(AsrConstants.ASR_VAD_FRONT_WAIT_MS, 6000)
                putExtra(AsrConstants.ASR_VAD_END_WAIT_MS, 2200)
                putExtra(AsrConstants.ASR_TIMEOUT_THRESHOLD_MS, 15000)
                putExtra(AsrConstants.LANGUAGE, preferredLocaleTag())
            }
            // Engine must be listening before any writePcm; otherwise early frames
            // (and often the whole utterance) are discarded and VAD never fires.
            resetHiaiSessionFlags()
            hiaiUsesPcmFeed = true
            mode = Mode.HIAI
            listening = true
            engine.startListening(listenIntent)
            Log.e(TAG, "HiAI startListening (PCM) ok; opening RayNeo mic")

            val capture = VoicePcmCapture(
                context,
                onPcm = { bytes, length ->
                    if (!listening || mode != Mode.HIAI) return@VoicePcmCapture
                    try {
                        engine.writePcm(bytes, length)
                    } catch (error: Exception) {
                        Log.e(TAG, "HiAI writePcm failed", error)
                        mainHandler.post {
                            if (!listening || mode != Mode.HIAI) return@post
                            deliverHiaiOutcome(
                                text = "",
                                fallbackError = context.getString(R.string.voice_error_audio),
                            )
                        }
                    }
                },
                onCaptureError = { message ->
                    mainHandler.post {
                        if (!listening || mode != Mode.HIAI) return@post
                        Log.e(TAG, "PCM capture error: $message")
                        deliverHiaiOutcome(
                            text = "",
                            fallbackError = context.getString(R.string.voice_error_audio),
                        )
                    }
                },
            )
            if (!capture.start()) {
                Log.e(TAG, "PCM capture failed to start")
                listening = false
                mode = Mode.NONE
                try {
                    engine.cancel()
                } catch (error: Exception) {
                    Log.e(TAG, "HiAI cancel after PCM failure", error)
                }
                return false
            }
            pcmCapture = capture
            listener.onVoiceStarted()
            scheduleListenWatchdog()
            Log.e(TAG, "Started HiAI with PCM feed (RayNeo mic mode)")
            true
        } catch (error: Exception) {
            Log.e(TAG, "HiAI PCM startListening failed", error)
            stopPcmCapture()
            listening = false
            mode = Mode.NONE
            hiaiFinalizing = false
            false
        }
    }

    private fun resetHiaiSessionFlags() {
        lastPartialText = ""
        hiaiResultDelivered = false
        hiaiFinalizing = false
        clearWatchdogs()
    }

    /**
     * After VAD end-of-speech, explicitly stopListening so the engine emits
     * onResults (required for PCM; also stabilizes RECORD sessions).
     */
    private fun finalizeHiaiAfterSpeech() {
        if (mode != Mode.HIAI || hiaiResultDelivered || hiaiFinalizing) return
        hiaiFinalizing = true
        stopPcmCapture()
        try {
            Log.e(TAG, "HiAI finalize: stopListening after end-of-speech pcm=$hiaiUsesPcmFeed")
            hiaiRecognizer?.stopListening()
            scheduleStopWatchdog()
        } catch (error: Exception) {
            Log.e(TAG, "HiAI stopListening after end-of-speech failed", error)
            deliverHiaiOutcome(
                text = lastPartialText,
                fallbackError = context.getString(R.string.voice_error_no_match),
            )
        }
    }

    private fun deliverHiaiOutcome(text: String, fallbackError: String) {
        if (hiaiResultDelivered) return
        hiaiResultDelivered = true
        clearWatchdogs()
        val peak = pcmCapture?.maxPeak() ?: -1
        stopPcmCapture()
        releaseRayNeoMicMode()
        val trimmed = text.trim()
        if (trimmed.isNotEmpty()) {
            Log.e(TAG, "HiAI deliver result='$trimmed' peak=$peak")
            listener.onVoiceResult(trimmed)
        } else {
            Log.e(TAG, "HiAI deliver error='$fallbackError' peak=$peak")
            listener.onVoiceError(fallbackError)
        }
        finishListening()
    }

    private fun scheduleListenWatchdog() {
        mainHandler.removeCallbacks(listenWatchdog)
        mainHandler.postDelayed(listenWatchdog, LISTEN_WATCHDOG_MS)
    }

    private fun scheduleStopWatchdog() {
        mainHandler.removeCallbacks(stopWatchdog)
        mainHandler.postDelayed(stopWatchdog, STOP_WATCHDOG_MS)
    }

    private fun clearWatchdogs() {
        mainHandler.removeCallbacks(listenWatchdog)
        mainHandler.removeCallbacks(stopWatchdog)
    }

    private fun applyRayNeoMicMode() {
        try {
            audioManager.setParameters(RAYNEO_PARAM_VOICE_RECOGNITION)
            rayNeoModeApplied = true
            Log.e(TAG, "RayNeo mic mode: $RAYNEO_PARAM_VOICE_RECOGNITION")
        } catch (error: Exception) {
            Log.e(TAG, "Failed to apply RayNeo mic mode", error)
        }
    }

    private fun releaseRayNeoMicMode() {
        if (!rayNeoModeApplied) return
        rayNeoModeApplied = false
        try {
            audioManager.setParameters(RAYNEO_PARAM_OFF)
            Log.e(TAG, "RayNeo mic mode: $RAYNEO_PARAM_OFF")
        } catch (error: Exception) {
            Log.e(TAG, "Failed to release RayNeo mic mode", error)
        }
    }

    private fun stopPcmCapture() {
        val capture = pcmCapture ?: return
        pcmCapture = null
        try {
            capture.stop()
        } catch (error: Exception) {
            Log.e(TAG, "PCM capture stop failed", error)
        }
    }

    private fun isHiaiPackagePresent(): Boolean =
        try {
            context.packageManager.getPackageInfo("com.huawei.hiai", 0)
            true
        } catch (_: PackageManager.NameNotFoundException) {
            false
        } catch (error: Exception) {
            Log.w(TAG, "HiAI package probe failed", error)
            false
        }

    private fun startInProcess(): Boolean {
        // Fresh instance every session — reused instances stay "listening" in the
        // RemoteSpeechRecognitionService binder and return BUSY / no_match.
        abortInProcessRecognizer()
        val speech = ensureRecognizer() ?: return false
        val intent = buildRecognizeIntent()
        return try {
            mode = Mode.IN_PROCESS
            listening = true
            lastPartialText = ""
            hiaiResultDelivered = false
            listener.onVoiceStarted()
            speech.startListening(intent)
            scheduleListenWatchdog()
            Log.e(TAG, "Started RayNeo/system SpeechRecognizer")
            true
        } catch (error: Exception) {
            Log.e(TAG, "Failed to start in-process recognizer", error)
            abortInProcessRecognizer()
            listening = false
            mode = Mode.NONE
            false
        }
    }

    private fun abortInProcessRecognizer() {
        val current = recognizer
        recognizer = null
        if (current == null) return
        try {
            current.cancel()
        } catch (error: Exception) {
            Log.e(TAG, "SpeechRecognizer cancel failed", error)
        }
        try {
            current.destroy()
        } catch (error: Exception) {
            Log.e(TAG, "SpeechRecognizer destroy failed", error)
        }
    }

    private fun startActivityUi(): Boolean {
        if (!canLaunchSpeechActivity()) return false
        val intent = buildRecognizeIntent().apply {
            putExtra(RecognizerIntent.EXTRA_PROMPT, context.getString(R.string.voice_prompt))
        }
        return try {
            mode = Mode.ACTIVITY
            listening = true
            listener.onVoiceStarted()
            launcher.launch(intent)
            Log.i(TAG, "Launched system speech activity")
            true
        } catch (error: ActivityNotFoundException) {
            Log.e(TAG, "No activity for speech recognition", error)
            listening = false
            mode = Mode.NONE
            false
        } catch (error: Exception) {
            Log.e(TAG, "Failed to launch speech activity", error)
            listening = false
            mode = Mode.NONE
            false
        }
    }

    private fun ensureRecognizer(): SpeechRecognizer? {
        recognizer?.let { return it }
        val created = createBestAvailableRecognizer() ?: return null
        created.setRecognitionListener(object : RecognitionListener {
            override fun onReadyForSpeech(params: Bundle?) {
                Log.e(TAG, "onReadyForSpeech")
            }

            override fun onBeginningOfSpeech() {
                Log.e(TAG, "onBeginningOfSpeech")
            }

            override fun onRmsChanged(rmsdB: Float) = Unit
            override fun onBufferReceived(buffer: ByteArray?) = Unit

            override fun onPartialResults(partialResults: Bundle?) {
                val text = partialResults
                    ?.getStringArrayList(SpeechRecognizer.RESULTS_RECOGNITION)
                    ?.firstOrNull()
                    ?.trim()
                    .orEmpty()
                if (text.isNotEmpty()) {
                    lastPartialText = text
                    Log.e(TAG, "onPartialResults text='$text'")
                }
            }

            override fun onEvent(eventType: Int, params: Bundle?) = Unit
            override fun onEndOfSpeech() {
                Log.e(TAG, "onEndOfSpeech")
            }

            override fun onError(error: Int) {
                val message = androidSpeechErrorMessage(error)
                Log.e(TAG, "SpeechRecognizer error=$error message=$message")
                if (!listening || mode != Mode.IN_PROCESS || hiaiResultDelivered) return
                if (error == SpeechRecognizer.ERROR_RECOGNIZER_BUSY && !hiaiBusyRetryPending) {
                    hiaiBusyRetryPending = true
                    Log.e(TAG, "SpeechRecognizer BUSY — recreate and retry once")
                    abortInProcessRecognizer()
                    listening = false
                    mode = Mode.NONE
                    mainHandler.postDelayed({
                        if (listening || hiaiResultDelivered) return@postDelayed
                        if (startInProcess()) return@postDelayed
                        listener.onVoiceError(context.getString(R.string.voice_error_busy))
                        listener.onVoiceEnded()
                    }, BUSY_RETRY_DELAY_MS)
                    return
                }
                when (error) {
                    SpeechRecognizer.ERROR_NO_MATCH,
                    SpeechRecognizer.ERROR_SPEECH_TIMEOUT,
                    -> deliverInProcessOutcome(lastPartialText)
                    else -> deliverInProcessOutcome(
                        text = lastPartialText,
                        fallbackError = message,
                    )
                }
            }

            override fun onResults(results: Bundle?) {
                if (!listening || mode != Mode.IN_PROCESS || hiaiResultDelivered) return
                val texts = results
                    ?.getStringArrayList(SpeechRecognizer.RESULTS_RECOGNITION)
                    .orEmpty()
                val text = texts.firstOrNull()?.trim().orEmpty().ifBlank { lastPartialText }
                Log.e(TAG, "onResults text='$text'")
                deliverInProcessOutcome(text)
            }
        })
        recognizer = created
        return created
    }

    private fun createBestAvailableRecognizer(): SpeechRecognizer? {
        val candidates = linkedSetOf<ComponentName>()
        // X3 Pro default: com.rayneo.aispeech/.wakeup.RayNeoRecognitionService
        if (isPackageInstalled(RAYNEO_AISPEECH_PACKAGE)) {
            candidates += ComponentName(RAYNEO_AISPEECH_PACKAGE, RAYNEO_RECOGNITION_SERVICE)
        }
        readSecureVoiceRecognitionComponent()?.let { candidates += it }
        candidates += queryRecognitionServiceComponents()

        for (component in candidates) {
            if (isFakeRecognitionService(component)) {
                Log.e(TAG, "Skipping fake RecognitionService ${component.flattenToShortString()}")
                continue
            }
            try {
                val speech = SpeechRecognizer.createSpeechRecognizer(context, component)
                Log.e(TAG, "Created SpeechRecognizer for ${component.flattenToShortString()}")
                return speech
            } catch (error: Exception) {
                Log.e(TAG, "Failed recognizer for ${component.flattenToShortString()}", error)
            }
        }

        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.S) {
            try {
                if (SpeechRecognizer.isOnDeviceRecognitionAvailable(context)) {
                    val speech = SpeechRecognizer.createOnDeviceSpeechRecognizer(context)
                    Log.e(TAG, "Created on-device SpeechRecognizer")
                    return speech
                }
            } catch (error: Exception) {
                Log.e(TAG, "On-device SpeechRecognizer unavailable", error)
            }
        }

        Log.e(TAG, "No usable Android SpeechRecognizer component")
        return null
    }

    private fun isPackageInstalled(packageName: String): Boolean =
        try {
            context.packageManager.getPackageInfo(packageName, 0)
            true
        } catch (_: PackageManager.NameNotFoundException) {
            false
        } catch (_: Exception) {
            false
        }

    private fun deliverInProcessOutcome(
        text: String,
        fallbackError: String = context.getString(R.string.voice_error_no_match),
    ) {
        if (hiaiResultDelivered) return
        hiaiResultDelivered = true
        clearWatchdogs()
        val trimmed = text.trim()
        if (trimmed.isNotEmpty()) {
            Log.e(TAG, "IN_PROCESS deliver result='$trimmed'")
            listener.onVoiceResult(trimmed)
        } else {
            Log.e(TAG, "IN_PROCESS deliver error='$fallbackError'")
            listener.onVoiceError(fallbackError)
        }
        // Destroy after deliver so the next start cannot hit "listening in progress".
        abortInProcessRecognizer()
        finishListening()
    }

    private fun isFakeRecognitionService(component: ComponentName): Boolean {
        val flat = component.flattenToShortString()
        return flat.contains("FakeRecognition", ignoreCase = true) ||
            flat.contains("Fake", ignoreCase = true) &&
            flat.contains("Recognition", ignoreCase = true)
    }

    private fun readSecureVoiceRecognitionComponent(): ComponentName? {
        val flat = try {
            Settings.Secure.getString(
                context.contentResolver,
                "voice_recognition_service",
            )
        } catch (error: Exception) {
            Log.w(TAG, "Failed to read voice_recognition_service", error)
            null
        }
        if (flat.isNullOrBlank()) return null
        return ComponentName.unflattenFromString(flat)
    }

    private fun queryRecognitionServiceComponents(): List<ComponentName> {
        val intent = Intent(RecognitionService.SERVICE_INTERFACE)
        val flags = PackageManager.MATCH_ALL
        val resolveInfos = try {
            context.packageManager.queryIntentServices(intent, flags)
        } catch (error: Exception) {
            Log.w(TAG, "queryIntentServices RecognitionService failed", error)
            emptyList()
        }
        return resolveInfos.mapNotNull { info ->
            val service = info.serviceInfo ?: return@mapNotNull null
            ComponentName(service.packageName, service.name)
        }
    }

    private fun buildRecognizeIntent(): Intent =
        Intent(RecognizerIntent.ACTION_RECOGNIZE_SPEECH).apply {
            putExtra(
                RecognizerIntent.EXTRA_LANGUAGE_MODEL,
                RecognizerIntent.LANGUAGE_MODEL_FREE_FORM,
            )
            putExtra(RecognizerIntent.EXTRA_PARTIAL_RESULTS, true)
            putExtra(RecognizerIntent.EXTRA_MAX_RESULTS, 3)
            putExtra(RecognizerIntent.EXTRA_LANGUAGE, preferredLocaleTag())
            putExtra(RecognizerIntent.EXTRA_CALLING_PACKAGE, context.packageName)
            putExtra(RecognizerIntent.EXTRA_SPEECH_INPUT_MINIMUM_LENGTH_MILLIS, 1200L)
            putExtra(RecognizerIntent.EXTRA_SPEECH_INPUT_COMPLETE_SILENCE_LENGTH_MILLIS, 1600L)
            putExtra(
                RecognizerIntent.EXTRA_SPEECH_INPUT_POSSIBLY_COMPLETE_SILENCE_LENGTH_MILLIS,
                1600L,
            )
        }

    private fun canLaunchSpeechActivity(): Boolean {
        val intent = Intent(RecognizerIntent.ACTION_RECOGNIZE_SPEECH)
        return intent.resolveActivity(context.packageManager) != null
    }

    private fun finishListening() {
        if (!listening && mode == Mode.NONE && !pendingHiaiStart) return
        clearWatchdogs()
        stopPcmCapture()
        releaseRayNeoMicMode()
        // Keep OEM binder free for the next session.
        if (mode == Mode.IN_PROCESS) {
            abortInProcessRecognizer()
        }
        listening = false
        pendingHiaiStart = false
        mode = Mode.NONE
        hiaiFinalizing = false
        hiaiUsesPcmFeed = false
        mainHandler.post { listener.onVoiceEnded() }
    }

    private fun preferredLocaleTag(): String {
        val locale = Locale.getDefault()
        return when {
            locale.language.equals("zh", ignoreCase = true) -> "zh-CN"
            else -> locale.toLanguageTag()
        }
    }

    /**
     * Official HiAI demo reads a JSON string:
     * `{"result":[{"word":"...","confidence":0.9}, ...]}`
     * via [Bundle.getString], not [Bundle.getStringArrayList].
     */
    private fun extractHiaiText(results: Bundle?): String {
        if (results == null) return ""
        Log.i(TAG, "HiAI result keys=${results.keySet()}")
        val fromFinal = parseHiaiOfficialJson(results.getString(AsrConstants.RESULTS_RECOGNITION))
        if (fromFinal.isNotEmpty()) return fromFinal
        val fromPartial = parseHiaiOfficialJson(results.getString(AsrConstants.RESULTS_PARTIAL))
        if (fromPartial.isNotEmpty()) return fromPartial
        val list = results.getStringArrayList(AsrConstants.RESULTS_RECOGNITION)
        if (!list.isNullOrEmpty()) {
            for (item in list) {
                val parsed = parseHiaiOfficialJson(item).ifBlank { item.trim() }
                if (parsed.isNotEmpty()) return parsed
            }
        }
        val raw = results.getString(AsrConstants.ASR_RAW_CONTENT)?.trim().orEmpty()
        if (raw.isNotEmpty()) {
            return parseHiaiOfficialJson(raw).ifBlank { raw }
        }
        return ""
    }

    private fun parseHiaiOfficialJson(raw: String?): String {
        val trimmed = raw?.trim().orEmpty()
        if (trimmed.isEmpty()) return ""
        if (!trimmed.startsWith("{") && !trimmed.startsWith("[")) {
            return trimmed
        }
        return try {
            if (trimmed.startsWith("[")) {
                val arr = JSONArray(trimmed)
                val sb = StringBuilder()
                for (i in 0 until arr.length()) {
                    val item = arr.optJSONObject(i) ?: continue
                    val word = item.optString("word").ifBlank { item.optString("text") }.trim()
                    if (word.isNotEmpty()) sb.append(word)
                }
                sb.toString()
            } else {
                val obj = JSONObject(trimmed)
                val nested = obj.optJSONArray("result")
                if (nested != null) {
                    val sb = StringBuilder()
                    for (i in 0 until nested.length()) {
                        val item = nested.optJSONObject(i) ?: continue
                        val word = item.optString("word").ifBlank { item.optString("text") }.trim()
                        if (word.isNotEmpty()) sb.append(word)
                    }
                    if (sb.isNotEmpty()) return sb.toString()
                }
                obj.optString("word").ifBlank { obj.optString("text") }.trim()
            }
        } catch (error: Exception) {
            Log.w(TAG, "Failed to parse HiAI JSON result: $trimmed", error)
            trimmed
        }
    }

    private fun hiaiErrorMessage(error: Int): String =
        when (error) {
            AsrError.ERROR_AUDIO ->
                context.getString(R.string.voice_error_audio)
            AsrError.ERROR_CLIENT ->
                context.getString(R.string.voice_error_client)
            AsrError.ERROR_CLIENT_INSUFFICIENT_PERMISSIONS,
            AsrError.ERROR_SERVER_INSUFFICIENT_PERMISSIONS,
            ->
                context.getString(R.string.voice_error_permission)
            AsrError.ERROR_NETWORK,
            AsrError.ERROR_NETWORK_TIMEOUT,
            ->
                context.getString(R.string.voice_error_network)
            AsrError.ERROR_RECOGNIZER_BUSY ->
                context.getString(R.string.voice_error_busy)
            AsrError.ERROR_SERVER,
            AsrError.ERROR_AI_ENGINE_CLOSED,
            AsrError.ERROR_GET_MODEL_PATH,
            AsrError.ERROR_MODEL_NOT_MATCH,
            AsrError.ERROR_INIT_FAIL,
            ->
                context.getString(R.string.voice_error_server)
            AsrError.ERROR_NO_MATCH,
            AsrError.ERROR_SPEECH_TIMEOUT,
            ->
                context.getString(R.string.voice_error_no_match)
            else ->
                context.getString(R.string.voice_error_generic)
        }

    private fun androidSpeechErrorMessage(error: Int): String =
        when (error) {
            SpeechRecognizer.ERROR_AUDIO ->
                context.getString(R.string.voice_error_audio)
            SpeechRecognizer.ERROR_CLIENT ->
                context.getString(R.string.voice_error_client)
            SpeechRecognizer.ERROR_INSUFFICIENT_PERMISSIONS ->
                context.getString(R.string.voice_error_permission)
            SpeechRecognizer.ERROR_NETWORK,
            SpeechRecognizer.ERROR_NETWORK_TIMEOUT,
            ->
                context.getString(R.string.voice_error_network)
            SpeechRecognizer.ERROR_RECOGNIZER_BUSY ->
                context.getString(R.string.voice_error_busy)
            SpeechRecognizer.ERROR_SERVER ->
                context.getString(R.string.voice_error_server)
            SpeechRecognizer.ERROR_NO_MATCH,
            SpeechRecognizer.ERROR_SPEECH_TIMEOUT,
            ->
                context.getString(R.string.voice_error_no_match)
            else ->
                context.getString(R.string.voice_error_generic)
        }

    companion object {
        private const val TAG = "BitFunGlassesVoice"
        private const val LISTEN_WATCHDOG_MS = 12_000L
        private const val STOP_WATCHDOG_MS = 2_500L
        private const val BUSY_RETRY_DELAY_MS = 700L
        private const val RAYNEO_PARAM_VOICE_RECOGNITION = "audio_source_record=voice_recognition"
        private const val RAYNEO_PARAM_OFF = "audio_source_record=off"
        private const val RAYNEO_AISPEECH_PACKAGE = "com.rayneo.aispeech"
        private const val RAYNEO_RECOGNITION_SERVICE =
            "com.rayneo.aispeech.wakeup.RayNeoRecognitionService"
    }
}
