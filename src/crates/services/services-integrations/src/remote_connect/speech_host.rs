//! Desktop-registered host for room-channel [super::RemoteCommand::TranscribeSpeech].
//!
//! Glasses remote control sends PCM over the paired room `/command` path (not
//! device RPC). Product `RemoteServer::dispatch` therefore needs a host hook
//! that can reach the desktop SpeechService.

use std::future::Future;
use std::pin::Pin;
use std::sync::{Arc, OnceLock};

use super::RemoteResponse;

pub struct RemoteSpeechTranscribeRequest {
    pub pcm16_base64: String,
    pub sample_rate: Option<u32>,
    pub model_id: Option<String>,
    pub language: Option<String>,
}

pub struct RemoteSpeechTranscribeResult {
    pub text: String,
    pub language: String,
    pub duration_ms: u64,
    pub audio_duration_seconds: f64,
    pub model_id: String,
}

pub type RemoteSpeechTranscribeFuture =
    Pin<Box<dyn Future<Output = Result<RemoteSpeechTranscribeResult, String>> + Send>>;

pub type RemoteSpeechTranscribeHandler =
    Arc<dyn Fn(RemoteSpeechTranscribeRequest) -> RemoteSpeechTranscribeFuture + Send + Sync>;

static REMOTE_SPEECH_TRANSCRIBE_HANDLER: OnceLock<RemoteSpeechTranscribeHandler> = OnceLock::new();

/// Register the desktop SpeechService-backed transcriber for room commands.
pub fn set_remote_speech_transcribe_handler(handler: RemoteSpeechTranscribeHandler) {
    let _ = REMOTE_SPEECH_TRANSCRIBE_HANDLER.set(handler);
}

pub async fn invoke_remote_speech_transcribe(
    request: RemoteSpeechTranscribeRequest,
) -> Option<Result<RemoteSpeechTranscribeResult, String>> {
    let handler = REMOTE_SPEECH_TRANSCRIBE_HANDLER.get()?;
    Some(handler(request).await)
}

pub async fn dispatch_remote_speech_transcribe(
    pcm16_base64: String,
    sample_rate: Option<u32>,
    model_id: Option<String>,
    language: Option<String>,
) -> RemoteResponse {
    match invoke_remote_speech_transcribe(RemoteSpeechTranscribeRequest {
        pcm16_base64,
        sample_rate,
        model_id,
        language,
    })
    .await
    {
        Some(Ok(result)) => RemoteResponse::SpeechTranscription {
            text: result.text,
            language: result.language,
            duration_ms: result.duration_ms,
            audio_duration_seconds: result.audio_duration_seconds,
            model_id: result.model_id,
        },
        Some(Err(message)) => RemoteResponse::Error { message },
        None => RemoteResponse::Error {
            message: "Speech transcription is not available on this host".to_string(),
        },
    }
}
