package com.bitfun.glasses.ui

import android.app.Activity
import android.content.ActivityNotFoundException
import android.content.ComponentName
import android.content.Context
import android.content.Intent
import android.content.pm.PackageManager
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
 * RayNeo / Huawei devices ship a real on-device ASR inside [com.huawei.hiai]
 * (`asr.apk`), while the Android [SpeechRecognizer] default points at
 * Huawei's `FakeRecognitionService` (no results). Prefer HiAI first.
 *
 * Strategy:
 * 1) Huawei HiAI [AsrRecognizer] + app-owned 16kHz PCM ([VoicePcmCapture])
 * 2) Non-fake [SpeechRecognizer] components
 * 3) System [RecognizerIntent] activity UI
 *
 * Phone-first: capture with standard VOICE_RECOGNITION AudioRecord.
 * RayNeo X3 additionally enables vendor `audio_source_record=VOICE_RECOGNITION`.
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
    }

    private enum class Mode { NONE, HIAI, IN_PROCESS, ACTIVITY }

    private val mainHandler = Handler(Looper.getMainLooper())
    private var recognizer: SpeechRecognizer? = null
    private var hiaiRecognizer: AsrRecognizer? = null
    private var pcmCapture: VoicePcmCapture? = null
    private var hiaiReady = false
    private var hiaiInitFailed = false
    private var pendingHiaiStart = false
    private var listening = false
    private var mode = Mode.NONE

    /** UI always offers the mic; real availability is discovered when starting. */
    fun isAvailable(): Boolean = true

    fun isListening(): Boolean = listening

    fun start() {
        if (listening || pendingHiaiStart) return
        if (startHiai()) return
        if (startInProcess()) return
        if (startActivityUi()) return
        listener.onVoiceError(context.getString(R.string.voice_unavailable))
        listener.onVoiceEnded()
    }

    fun stop() {
        when (mode) {
            Mode.HIAI -> {
                pendingHiaiStart = false
                if (!listening) {
                    stopPcmCapture()
                    return
                }
                try {
                    // Stop mic first so writePcm drains, then ask engine to finalize.
                    stopPcmCapture()
                    hiaiRecognizer?.stopListening()
                } catch (error: Exception) {
                    Log.w(TAG, "HiAI stopListening failed", error)
                    finishListening()
                }
            }
            Mode.IN_PROCESS -> {
                if (!listening) return
                try {
                    recognizer?.stopListening()
                } catch (error: Exception) {
                    Log.w(TAG, "stopListening failed", error)
                    finishListening()
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

    fun destroy() {
        pendingHiaiStart = false
        stopPcmCapture()
        try {
            recognizer?.cancel()
        } catch (_: Exception) {
        }
        recognizer?.destroy()
        recognizer = null
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
        val usePcm = VoicePcmCapture.isRayNeoDevice()
        val initIntent = Intent().apply {
            if (usePcm) {
                putExtra(AsrConstants.ASR_AUDIO_SRC_TYPE, AsrConstants.ASR_SRC_TYPE_PCM)
                putExtra(AsrConstants.ASR_SAMPLE_RATE, VoicePcmCapture.SAMPLE_RATE)
            } else {
                // Official phone demo uses RECORD (engine owns microphone).
                putExtra(AsrConstants.ASR_AUDIO_SRC_TYPE, AsrConstants.ASR_SRC_TYPE_RECORD)
            }
            putExtra(AsrConstants.ASR_ENGINE_MODE, AsrConstants.HW_LOCAL_RECOGNIZER)
            putExtra(AsrConstants.ASR_VAD_FRONT_WAIT_MS, 4000)
            putExtra(AsrConstants.ASR_VAD_END_WAIT_MS, 2000)
            putExtra(AsrConstants.ASR_TIMEOUT_THRESHOLD_MS, 20000)
            putExtra(AsrConstants.LANGUAGE, preferredLocaleTag())
            putExtra(AsrConstants.REGION, AsrConstants.REGION_ZH)
        }
        Log.i(
            TAG,
            "HiAI init usePcm=$usePcm model=${Build.MODEL} rayNeo=${VoicePcmCapture.isRayNeoDevice()}",
        )
        created.init(initIntent, object : AsrListener {
            override fun onInit(params: Bundle?) {
                val errorCode = params?.getInt(AsrConstants.ASR_ERROR_CODE, AsrError.SUCCESS)
                    ?: AsrError.SUCCESS
                Log.i(TAG, "HiAI onInit errorCode=$errorCode")
                if (errorCode != AsrError.SUCCESS) {
                    hiaiReady = false
                    hiaiInitFailed = true
                    val waiting = pendingHiaiStart
                    pendingHiaiStart = false
                    mode = Mode.NONE
                    if (waiting && !startInProcess() && !startActivityUi()) {
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
                        if (!startInProcess() && !startActivityUi()) {
                            listener.onVoiceError(context.getString(R.string.voice_start_failed))
                            listener.onVoiceEnded()
                        }
                    }
                }
            }

            override fun onBeginningOfSpeech() {
                Log.i(TAG, "HiAI onBeginningOfSpeech")
            }

            override fun onRmsChanged(rmsdB: Float) = Unit

            override fun onBufferReceived(buffer: ByteArray?) = Unit

            override fun onEndOfSpeech() {
                Log.i(TAG, "HiAI onEndOfSpeech")
            }

            override fun onError(error: Int) {
                Log.w(TAG, "HiAI onError=$error")
                stopPcmCapture()
                if (pendingHiaiStart && !listening) {
                    pendingHiaiStart = false
                    hiaiInitFailed = true
                    mode = Mode.NONE
                    if (!startInProcess() && !startActivityUi()) {
                        listener.onVoiceError(hiaiErrorMessage(error))
                        listener.onVoiceEnded()
                    }
                    return
                }
                if (!listening || mode != Mode.HIAI) return
                when (error) {
                    AsrError.ERROR_NO_MATCH,
                    AsrError.ERROR_SPEECH_TIMEOUT,
                    -> {
                        listener.onVoiceError(context.getString(R.string.voice_error_no_match))
                        finishListening()
                    }
                    else -> {
                        listener.onVoiceError(hiaiErrorMessage(error))
                        finishListening()
                    }
                }
            }

            override fun onResults(results: Bundle?) {
                if (!listening || mode != Mode.HIAI) return
                stopPcmCapture()
                val text = extractHiaiText(results)
                Log.i(TAG, "HiAI onResults text='$text'")
                if (text.isNotEmpty()) {
                    listener.onVoiceResult(text)
                } else {
                    listener.onVoiceError(context.getString(R.string.voice_error_no_match))
                }
                finishListening()
            }

            override fun onPartialResults(partialResults: Bundle?) {
                val text = extractHiaiText(partialResults)
                if (text.isNotEmpty()) {
                    Log.i(TAG, "HiAI onPartialResults text='$text'")
                }
            }

            override fun onEnd() {
                Log.i(TAG, "HiAI onEnd")
                stopPcmCapture()
                if (listening && mode == Mode.HIAI) {
                    finishListening()
                }
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
        // Match official HiAI demo: RECORD mode first (engine owns mic).
        // PCM writePcm is fallback for RayNeo / engines that need app audio.
        return if (VoicePcmCapture.isRayNeoDevice()) {
            beginHiaiPcmListening(engine)
        } else {
            beginHiaiRecordListening(engine)
        }
    }

    /** Standard phone path: HiAI records from the built-in microphone. */
    private fun beginHiaiRecordListening(engine: AsrRecognizer): Boolean {
        return try {
            val listenIntent = Intent().apply {
                putExtra(AsrConstants.ASR_AUDIO_SRC_TYPE, AsrConstants.ASR_SRC_TYPE_RECORD)
                putExtra(AsrConstants.ASR_VAD_FRONT_WAIT_MS, 4000)
                putExtra(AsrConstants.ASR_VAD_END_WAIT_MS, 2000)
                putExtra(AsrConstants.ASR_TIMEOUT_THRESHOLD_MS, 20000)
                putExtra(AsrConstants.LANGUAGE, preferredLocaleTag())
            }
            mode = Mode.HIAI
            listening = true
            listener.onVoiceStarted()
            engine.startListening(listenIntent)
            Log.i(TAG, "Started HiAI RECORD listening (phone mic)")
            true
        } catch (error: Exception) {
            Log.e(TAG, "HiAI RECORD startListening failed", error)
            listening = false
            mode = Mode.NONE
            false
        }
    }

    /** RayNeo / PCM path: app captures mic and feeds [AsrRecognizer.writePcm]. */
    private fun beginHiaiPcmListening(engine: AsrRecognizer): Boolean {
        return try {
            val listenIntent = Intent().apply {
                putExtra(AsrConstants.ASR_AUDIO_SRC_TYPE, AsrConstants.ASR_SRC_TYPE_PCM)
                putExtra(AsrConstants.ASR_SAMPLE_RATE, VoicePcmCapture.SAMPLE_RATE)
                putExtra(AsrConstants.ASR_VAD_END_WAIT_MS, 2200)
                putExtra(AsrConstants.LANGUAGE, preferredLocaleTag())
            }
            mode = Mode.HIAI
            listening = true
            listener.onVoiceStarted()
            engine.startListening(listenIntent)
            val capture = VoicePcmCapture(
                context,
                onPcm = { bytes, length ->
                    if (!listening || mode != Mode.HIAI) return@VoicePcmCapture
                    // HiAI docs: engine APIs must run on the UI/main thread.
                    mainHandler.post {
                        if (!listening || mode != Mode.HIAI) return@post
                        try {
                            engine.writePcm(bytes, length)
                        } catch (error: Exception) {
                            Log.e(TAG, "HiAI writePcm failed", error)
                        }
                    }
                },
                onCaptureError = { message ->
                    mainHandler.post {
                        if (!listening || mode != Mode.HIAI) return@post
                        Log.e(TAG, "PCM capture error: $message")
                        listener.onVoiceError(context.getString(R.string.voice_error_audio))
                        finishListening()
                    }
                },
            )
            if (!capture.start()) {
                Log.e(TAG, "PCM capture failed to start")
                try {
                    engine.cancel()
                } catch (_: Exception) {
                }
                listening = false
                mode = Mode.NONE
                return false
            }
            pcmCapture = capture
            Log.i(TAG, "Started HiAI with PCM feed (RayNeo mic mode)")
            true
        } catch (error: Exception) {
            Log.e(TAG, "HiAI PCM startListening failed", error)
            stopPcmCapture()
            listening = false
            mode = Mode.NONE
            false
        }
    }

    private fun stopPcmCapture() {
        val capture = pcmCapture ?: return
        pcmCapture = null
        try {
            capture.stop()
        } catch (error: Exception) {
            Log.w(TAG, "PCM capture stop failed", error)
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
        val speech = ensureRecognizer() ?: return false
        val intent = buildRecognizeIntent()
        return try {
            mode = Mode.IN_PROCESS
            listening = true
            listener.onVoiceStarted()
            speech.startListening(intent)
            Log.i(TAG, "Started in-process SpeechRecognizer")
            true
        } catch (error: Exception) {
            Log.e(TAG, "Failed to start in-process recognizer", error)
            listening = false
            mode = Mode.NONE
            false
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
                Log.i(TAG, "onReadyForSpeech")
            }

            override fun onBeginningOfSpeech() {
                Log.i(TAG, "onBeginningOfSpeech")
            }

            override fun onRmsChanged(rmsdB: Float) = Unit
            override fun onBufferReceived(buffer: ByteArray?) = Unit
            override fun onPartialResults(partialResults: Bundle?) = Unit
            override fun onEvent(eventType: Int, params: Bundle?) = Unit
            override fun onEndOfSpeech() {
                Log.i(TAG, "onEndOfSpeech")
            }

            override fun onError(error: Int) {
                val message = androidSpeechErrorMessage(error)
                Log.w(TAG, "SpeechRecognizer error=$error message=$message")
                if (!listening || mode != Mode.IN_PROCESS) return
                when (error) {
                    SpeechRecognizer.ERROR_NO_MATCH,
                    SpeechRecognizer.ERROR_SPEECH_TIMEOUT,
                    -> {
                        listener.onVoiceError(context.getString(R.string.voice_error_no_match))
                        finishListening()
                    }
                    SpeechRecognizer.ERROR_CLIENT,
                    SpeechRecognizer.ERROR_RECOGNIZER_BUSY,
                    -> {
                        listening = false
                        mode = Mode.NONE
                        if (!startActivityUi()) {
                            listener.onVoiceError(message)
                            finishListening()
                        }
                    }
                    else -> {
                        listener.onVoiceError(message)
                        finishListening()
                    }
                }
            }

            override fun onResults(results: Bundle?) {
                if (!listening || mode != Mode.IN_PROCESS) return
                val texts = results
                    ?.getStringArrayList(SpeechRecognizer.RESULTS_RECOGNITION)
                    .orEmpty()
                val text = texts.firstOrNull()?.trim().orEmpty()
                Log.i(TAG, "onResults textLen=${text.length}")
                if (text.isNotEmpty()) {
                    listener.onVoiceResult(text)
                } else {
                    listener.onVoiceError(context.getString(R.string.voice_error_no_match))
                }
                finishListening()
            }
        })
        recognizer = created
        return created
    }

    private fun createBestAvailableRecognizer(): SpeechRecognizer? {
        val candidates = linkedSetOf<ComponentName>()
        readSecureVoiceRecognitionComponent()?.let { candidates += it }
        candidates += queryRecognitionServiceComponents()

        for (component in candidates) {
            if (isFakeRecognitionService(component)) {
                Log.w(TAG, "Skipping fake RecognitionService ${component.flattenToShortString()}")
                continue
            }
            try {
                val speech = SpeechRecognizer.createSpeechRecognizer(context, component)
                Log.i(TAG, "Created SpeechRecognizer for ${component.flattenToShortString()}")
                return speech
            } catch (error: Exception) {
                Log.w(TAG, "Failed recognizer for ${component.flattenToShortString()}", error)
            }
        }

        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.S) {
            try {
                if (SpeechRecognizer.isOnDeviceRecognitionAvailable(context)) {
                    val speech = SpeechRecognizer.createOnDeviceSpeechRecognizer(context)
                    Log.i(TAG, "Created on-device SpeechRecognizer")
                    return speech
                }
            } catch (error: Exception) {
                Log.w(TAG, "On-device SpeechRecognizer unavailable", error)
            }
        }

        // Default SpeechRecognizer on this device binds to FakeRecognitionService.
        Log.w(TAG, "No usable Android SpeechRecognizer component")
        return null
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
            putExtra(RecognizerIntent.EXTRA_PARTIAL_RESULTS, false)
            putExtra(RecognizerIntent.EXTRA_MAX_RESULTS, 1)
            putExtra(RecognizerIntent.EXTRA_LANGUAGE, preferredLocaleTag())
            putExtra(RecognizerIntent.EXTRA_CALLING_PACKAGE, context.packageName)
        }

    private fun canLaunchSpeechActivity(): Boolean {
        val intent = Intent(RecognizerIntent.ACTION_RECOGNIZE_SPEECH)
        return intent.resolveActivity(context.packageManager) != null
    }

    private fun finishListening() {
        if (!listening && mode == Mode.NONE && !pendingHiaiStart) return
        stopPcmCapture()
        listening = false
        pendingHiaiStart = false
        mode = Mode.NONE
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
    }
}
