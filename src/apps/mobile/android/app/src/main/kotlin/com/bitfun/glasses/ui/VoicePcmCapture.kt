package com.bitfun.glasses.ui

import android.content.Context
import android.media.AudioDeviceInfo
import android.media.AudioFormat
import android.media.AudioManager
import android.media.AudioRecord
import android.media.MediaRecorder
import android.os.Build
import android.os.Process
import android.util.Log
import java.util.concurrent.atomic.AtomicBoolean

/**
 * 16 kHz mono PCM16 capture for HiAI [AsrRecognizer.writePcm].
 *
 * Phone-first: use standard [MediaRecorder.AudioSource.VOICE_RECOGNITION].
 * On RayNeo X3, also set `audio_source_record=VOICE_RECOGNITION` and prefer
 * the `SPEAKER_MIC` input device per vendor docs.
 */
class VoicePcmCapture(
    context: Context,
    private val onPcm: (ByteArray, Int) -> Unit,
    private val onCaptureError: (String) -> Unit,
) {
    private val appContext = context.applicationContext
    private val audioManager =
        appContext.getSystemService(Context.AUDIO_SERVICE) as AudioManager
    private val rayNeoDevice = isRayNeoDevice()
    private val running = AtomicBoolean(false)
    private var record: AudioRecord? = null
    private var worker: Thread? = null
    private var rayNeoModeApplied = false

    fun start(): Boolean {
        if (!running.compareAndSet(false, true)) return true
        return try {
            if (rayNeoDevice) {
                audioManager.setParameters(PARAM_VOICE_RECOGNITION)
                rayNeoModeApplied = true
                Log.i(TAG, "RayNeo mic mode: $PARAM_VOICE_RECOGNITION")
            } else {
                Log.i(TAG, "Phone mic mode: standard VOICE_RECOGNITION AudioRecord")
            }
            val created = createAudioRecord()
            if (created.state != AudioRecord.STATE_INITIALIZED) {
                running.set(false)
                releaseRecordQuietly(created)
                releaseRayNeoMicMode()
                onCaptureError("AudioRecord init failed")
                return false
            }
            if (rayNeoDevice) {
                preferSpeakerMic(created)
            }
            created.startRecording()
            if (created.recordingState != AudioRecord.RECORDSTATE_RECORDING) {
                running.set(false)
                releaseRecordQuietly(created)
                releaseRayNeoMicMode()
                onCaptureError("AudioRecord start failed")
                return false
            }
            record = created
            worker = Thread({ captureLoop(created) }, "BitFunVoicePcm").also {
                it.isDaemon = true
                it.start()
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

    private fun stopInternal() {
        val current = record
        record = null
        try {
            current?.stop()
        } catch (_: Exception) {
        }
        releaseRecordQuietly(current)
        try {
            worker?.join(500)
        } catch (_: Exception) {
        }
        worker = null
        releaseRayNeoMicMode()
    }

    private fun captureLoop(audioRecord: AudioRecord) {
        Process.setThreadPriority(Process.THREAD_PRIORITY_AUDIO)
        val buffer = ByteArray(BUFFER_BYTES)
        var frames = 0
        var peakAbs = 0
        while (running.get()) {
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
                peakAbs = maxOf(peakAbs, peakAbsPcm16(buffer, read))
                if (frames % 25 == 0) {
                    Log.i(TAG, "PCM frames=$frames peak=$peakAbs rayNeo=$rayNeoDevice")
                    peakAbs = 0
                }
                try {
                    onPcm(buffer.copyOf(read), read)
                } catch (error: Exception) {
                    Log.e(TAG, "onPcm failed", error)
                }
            } else if (read < 0) {
                Log.w(TAG, "AudioRecord.read returned $read")
                if (running.get()) {
                    onCaptureError("AudioRecord read error $read")
                }
                break
            }
        }
        Log.i(TAG, "PCM capture loop ended frames=$frames")
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

    private fun preferSpeakerMic(audioRecord: AudioRecord) {
        val devices = try {
            audioManager.getDevices(AudioManager.GET_DEVICES_INPUTS)
        } catch (error: Exception) {
            Log.w(TAG, "getDevices failed", error)
            return
        }
        for (device in devices) {
            Log.i(
                TAG,
                "input device id=${device.id} type=${device.type} " +
                    "product=${device.productName} address=${device.address}",
            )
        }
        val preferred = devices.firstOrNull { isSpeakerMic(it) }
            ?: devices.firstOrNull { it.type == AudioDeviceInfo.TYPE_BUILTIN_MIC }
        if (preferred == null) {
            Log.w(TAG, "No preferred SPEAKER_MIC / builtin mic found")
            return
        }
        val ok = audioRecord.setPreferredDevice(preferred)
        Log.i(
            TAG,
            "setPreferredDevice id=${preferred.id} product=${preferred.productName} ok=$ok",
        )
    }

    private fun isSpeakerMic(device: AudioDeviceInfo): Boolean {
        val product = device.productName?.toString().orEmpty()
        val address = device.address.orEmpty()
        return product.contains("SPEAKER_MIC", ignoreCase = true) ||
            address.contains("SPEAKER_MIC", ignoreCase = true)
    }

    private fun releaseRayNeoMicMode() {
        if (!rayNeoModeApplied) return
        rayNeoModeApplied = false
        try {
            audioManager.setParameters(PARAM_OFF)
            Log.i(TAG, "RayNeo mic mode: $PARAM_OFF")
        } catch (error: Exception) {
            Log.w(TAG, "Failed to release audio_source_record", error)
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
        private const val BUFFER_BYTES = 1280 // 40ms @ 16kHz mono PCM16
        private const val PARAM_VOICE_RECOGNITION = "audio_source_record=VOICE_RECOGNITION"
        private const val PARAM_OFF = "audio_source_record=off"

        fun isRayNeoDevice(): Boolean {
            val model = Build.MODEL.orEmpty()
            val brand = Build.BRAND.orEmpty()
            val manufacturer = Build.MANUFACTURER.orEmpty()
            val product = Build.PRODUCT.orEmpty()
            val haystack = "$model $brand $manufacturer $product"
            return haystack.contains("rayneo", ignoreCase = true) ||
                haystack.contains("ffalcon", ignoreCase = true) ||
                model.contains("BRQ", ignoreCase = true)
        }
    }
}
