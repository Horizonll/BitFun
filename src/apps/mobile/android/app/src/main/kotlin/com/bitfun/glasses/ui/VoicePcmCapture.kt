package com.bitfun.glasses.ui

import android.content.Context
import android.media.AudioDeviceInfo
import android.media.AudioFormat
import android.media.AudioManager
import android.media.AudioRecord
import android.media.MediaRecorder
import android.os.Handler
import android.os.Looper
import android.os.Process
import android.util.Log
import java.util.concurrent.atomic.AtomicBoolean
import java.util.concurrent.atomic.AtomicInteger

/**
 * RayNeo X3 wearer-only mic capture for ASR (16 kHz mono PCM16).
 *
 * Matches vendor ExampleRecordRecognition:
 * - AudioRecord source = [MediaRecorder.AudioSource.VOICE_RECOGNITION]
 * - setParameters("audio_source_record=voice_recognition")
 * - preferred device = SPEAKER_MIC (by product/address name; id=23 is only a hint)
 * - release with setParameters("audio_source_record=off")
 *
 * If forced preferred-device routing yields near-silence, automatically reopen
 * on the default route so recognition is not stuck at peak=0.
 */
class VoicePcmCapture(
    context: Context,
    private val onPcm: (ByteArray, Int) -> Unit,
    private val onCaptureError: (String) -> Unit,
) {
    private val appContext = context.applicationContext
    private val audioManager =
        appContext.getSystemService(Context.AUDIO_SERVICE) as AudioManager
    private val mainHandler = Handler(Looper.getMainLooper())
    private val running = AtomicBoolean(false)
    private val peakLifetime = AtomicInteger(0)
    private var record: AudioRecord? = null
    private var worker: Thread? = null
    private var rayNeoModeApplied = false
    private var usingPreferredDevice = false

    fun start(): Boolean {
        if (!running.compareAndSet(false, true)) return true
        peakLifetime.set(0)
        return try {
            audioManager.setParameters(PARAM_VOICE_RECOGNITION)
            rayNeoModeApplied = true
            Log.i(TAG, "RayNeo mic mode: $PARAM_VOICE_RECOGNITION")

            if (!openAndStart(preferSpeakerMic = true)) {
                running.set(false)
                releaseRayNeoMicMode()
                return false
            }
            true
        } catch (error: Exception) {
            Log.e(TAG, "Failed to start PCM capture", error)
            running.set(false)
            stopInternal()
            onCaptureError(error.message ?: "PCM capture failed")
            false
        }
    }

    fun stop() {
        if (!running.getAndSet(false)) {
            releaseRayNeoMicMode()
            return
        }
        stopInternal()
    }

    /** Peak absolute PCM16 sample seen since start (0..32767). */
    fun maxPeak(): Int = peakLifetime.get()

    private fun stopInternal() {
        val current = record
        record = null
        try {
            if (current?.recordingState == AudioRecord.RECORDSTATE_RECORDING) {
                current.stop()
            }
        } catch (_: Exception) {
        }
        releaseRecordQuietly(current)
        try {
            worker?.join(500)
        } catch (_: Exception) {
        }
        worker = null
        releaseRayNeoMicMode()
        Log.i(TAG, "PCM stopped maxPeak=${peakLifetime.get()} preferred=$usingPreferredDevice")
    }

    private fun openAndStart(preferSpeakerMic: Boolean): Boolean {
        val created = createAudioRecord()
        usingPreferredDevice = false
        if (preferSpeakerMic) {
            usingPreferredDevice = preferSpeakerMic(created)
        } else {
            Log.i(TAG, "Opening AudioRecord on default input route")
        }

        if (created.state != AudioRecord.STATE_INITIALIZED) {
            Log.e(TAG, "AudioRecord init failed state=${created.state}")
            releaseRecordQuietly(created)
            onCaptureError("AudioRecord init failed")
            return false
        }

        created.startRecording()
        if (created.recordingState != AudioRecord.RECORDSTATE_RECORDING) {
            Log.e(TAG, "AudioRecord start failed state=${created.recordingState}")
            releaseRecordQuietly(created)
            onCaptureError("AudioRecord start failed")
            return false
        }

        record = created
        worker = Thread({ captureLoop(created) }, "BitFunVoicePcm").also {
            it.isDaemon = true
            it.start()
        }
        Log.i(
            TAG,
            "PCM capture started sampleRate=$SAMPLE_RATE preferred=$usingPreferredDevice " +
                "device=${created.preferredDevice?.id}/${created.preferredDevice?.productName}",
        )
        return true
    }

    private fun recreateWithoutPreferredDevice() {
        if (!running.get() || !usingPreferredDevice) return
        Log.i(
            TAG,
            "PCM near-silent on preferred mic (peak=${peakLifetime.get()}); falling back to default route",
        )
        val current = record
        val currentWorker = worker
        record = null
        worker = null
        try {
            if (current?.recordingState == AudioRecord.RECORDSTATE_RECORDING) {
                current.stop()
            }
        } catch (_: Exception) {
        }
        releaseRecordQuietly(current)
        // Never join the capture thread from itself — reopen on the main looper.
        val reopen = Runnable {
            if (!running.get()) return@Runnable
            if (!openAndStart(preferSpeakerMic = false)) {
                running.set(false)
                releaseRayNeoMicMode()
                onCaptureError("PCM fallback open failed")
            }
        }
        if (currentWorker != null && currentWorker !== Thread.currentThread()) {
            try {
                currentWorker.join(300)
            } catch (_: Exception) {
            }
            mainHandler.post(reopen)
        } else {
            mainHandler.post(reopen)
        }
    }

    private fun captureLoop(audioRecord: AudioRecord) {
        Process.setThreadPriority(Process.THREAD_PRIORITY_AUDIO)
        val buffer = ByteArray(BUFFER_BYTES)
        var frames = 0
        var peakWindow = 0
        var fallbackChecked = false
        while (running.get() && record === audioRecord) {
            val read = try {
                audioRecord.read(buffer, 0, buffer.size)
            } catch (error: Exception) {
                Log.e(TAG, "AudioRecord.read failed", error)
                if (running.get()) {
                    onCaptureError(error.message ?: "AudioRecord read failed")
                }
                break
            }
            if (read > 0) {
                frames += 1
                val peak = peakAbsPcm16(buffer, read)
                peakWindow = maxOf(peakWindow, peak)
                peakLifetime.accumulateAndGet(peak) { prev, next -> maxOf(prev, next) }
                if (frames % 25 == 0) {
                    Log.d(TAG, "PCM frames=$frames peak=$peakWindow lifetime=${peakLifetime.get()}")
                    peakWindow = 0
                }
                // ~1s at 16kHz / 1280-byte frames (40ms): if still silent, drop forced device.
                if (!fallbackChecked && frames >= 25 && usingPreferredDevice) {
                    fallbackChecked = true
                    if (peakLifetime.get() < SILENCE_PEAK_THRESHOLD) {
                        recreateWithoutPreferredDevice()
                        break
                    }
                }
                try {
                    onPcm(buffer.copyOf(read), read)
                } catch (error: Exception) {
                    Log.e(TAG, "onPcm failed", error)
                }
            } else if (read < 0) {
                Log.e(TAG, "AudioRecord.read returned $read")
                if (running.get()) {
                    onCaptureError("AudioRecord read error $read")
                }
                break
            }
        }
        Log.d(TAG, "PCM capture loop ended frames=$frames maxPeak=${peakLifetime.get()}")
    }

    private fun createAudioRecord(): AudioRecord {
        val min = AudioRecord.getMinBufferSize(
            SAMPLE_RATE,
            AudioFormat.CHANNEL_IN_MONO,
            AudioFormat.ENCODING_PCM_16BIT,
        )
        val bufferSize = maxOf(min, BUFFER_BYTES * 2)
        return AudioRecord(
            MediaRecorder.AudioSource.VOICE_RECOGNITION,
            SAMPLE_RATE,
            AudioFormat.CHANNEL_IN_MONO,
            AudioFormat.ENCODING_PCM_16BIT,
            bufferSize,
        )
    }

    /**
     * Prefer SPEAKER_MIC by product/address name. Only use hardcoded id=23 when
     * that device also looks like SPEAKER_MIC — on X3 Pro a bare id can route
     * to a silent input and yield peak=0.
     */
    private fun preferSpeakerMic(audioRecord: AudioRecord): Boolean {
        val devices = try {
            audioManager.getDevices(AudioManager.GET_DEVICES_INPUTS)
        } catch (error: Exception) {
            Log.e(TAG, "getDevices failed", error)
            return false
        }
        for (device in devices) {
            Log.e(
                TAG,
                "input device id=${device.id} type=${device.type} " +
                    "product=${device.productName} address=${device.address}",
            )
        }
        val byName = devices.firstOrNull { device ->
            val product = device.productName?.toString().orEmpty()
            val address = device.address.orEmpty()
            product.contains("SPEAKER_MIC", ignoreCase = true) ||
                address.contains("SPEAKER_MIC", ignoreCase = true)
        }
        val byIdHint = devices.firstOrNull { device ->
            device.id == SPEAKER_MIC_ID_HINT && isLikelySpeakerMic(device)
        }
        val preferred = byName ?: byIdHint
        if (preferred == null) {
            Log.e(TAG, "SPEAKER_MIC not found by name; using default route")
            return false
        }
        val ok = audioRecord.setPreferredDevice(preferred)
        Log.e(
            TAG,
            "setPreferredDevice id=${preferred.id} type=${preferred.type} " +
                "product=${preferred.productName} ok=$ok",
        )
        return ok
    }

    private fun isLikelySpeakerMic(device: AudioDeviceInfo): Boolean {
        val product = device.productName?.toString().orEmpty()
        val address = device.address.orEmpty()
        return product.contains("SPEAKER", ignoreCase = true) ||
            product.contains("MIC", ignoreCase = true) ||
            address.contains("SPEAKER", ignoreCase = true) ||
            device.type == AudioDeviceInfo.TYPE_BUILTIN_MIC
    }

    private fun releaseRayNeoMicMode() {
        if (!rayNeoModeApplied) return
        rayNeoModeApplied = false
        try {
            audioManager.setParameters(PARAM_OFF)
            Log.e(TAG, "RayNeo mic mode: $PARAM_OFF")
        } catch (error: Exception) {
            Log.e(TAG, "Failed to release audio_source_record", error)
        }
    }

    private fun releaseRecordQuietly(audioRecord: AudioRecord?) {
        try {
            audioRecord?.release()
        } catch (_: Exception) {
        }
    }

    private fun peakAbsPcm16(buffer: ByteArray, length: Int): Int {
        var peak = 0
        var i = 0
        while (i + 1 < length) {
            val sample = (buffer[i].toInt() and 0xff) or (buffer[i + 1].toInt() shl 8)
            val signed = sample.toShort().toInt()
            val abs = kotlin.math.abs(signed)
            if (abs > peak) peak = abs
            i += 2
        }
        return peak
    }

    companion object {
        private const val TAG = "BitFunGlassesVoice"
        const val SAMPLE_RATE = 16_000
        /** Vendor sample hint only — never force by id alone. */
        private const val SPEAKER_MIC_ID_HINT = 23
        private const val BUFFER_BYTES = 1280
        private const val SILENCE_PEAK_THRESHOLD = 200
        private const val PARAM_VOICE_RECOGNITION = "audio_source_record=voice_recognition"
        private const val PARAM_OFF = "audio_source_record=off"
    }
}
