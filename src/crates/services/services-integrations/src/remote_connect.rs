//! Remote-connect integration contracts.
//!
//! This module owns remote-connect wire assembly, runtime-port request
//! construction, compatibility re-exports, and remote session tracker state.
//! Pairing, encryption, QR generation, relay websocket lifecycle primitives,
//! wire command routing, and provider-neutral IM bot support live here. Product
//! assembly, concrete runtime hosts, and IM bot command routing that still needs
//! session/runtime hosts stay in `bitfun-core` until their ports are explicit.

pub mod account;
pub mod bot;
mod chat_projection;
pub mod device;
pub mod encryption;
mod lan;
mod mobile_web_upload;
mod ngrok;
pub mod pairing;
pub mod qr_generator;
pub mod relay_client;
pub mod session_store;
pub mod sync_state;

use bitfun_events::AgenticEvent;
use bitfun_runtime_ports::{
    AgentInputAttachment, AgentSessionCreateRequest, AgentSubmissionRequest, AgentSubmissionSource,
    RemoteControlStateSnapshot,
};
pub use bitfun_runtime_ports::{
    RemoteAssistantWorkspaceFacts, RemoteFileChunkRange, RemoteInitialSyncRuntimeHost,
    RemoteProjectionPort, RemoteRecentWorkspaceFacts, RemoteSessionMetadata,
    RemoteSessionWorkspaceIdentity, RemoteWorkspaceFacts, RemoteWorkspaceFileChunk,
    RemoteWorkspaceFileContent, RemoteWorkspaceFileInfo, RemoteWorkspaceFileRuntimeHost,
    RemoteWorkspaceKind, RemoteWorkspacePort, RemoteWorkspaceRuntimeHost, RemoteWorkspaceUpdate,
};
pub use chat_projection::{
    agent_input_attachment_from_remote_image_context, project_remote_chat_user,
    RemoteChatUserProjection,
};
pub use device::DeviceIdentity;
pub use encryption::{decrypt_from_base64, encrypt_to_base64, KeyPair};
pub use lan::{
    build_lan_relay_url, build_lan_relay_url_with_ip, get_local_ip, list_local_ips,
    LocalNetworkInterface,
};
use log::info;
pub use mobile_web_upload::upload_mobile_web_to_relay;
pub use ngrok::{
    cleanup_all_ngrok, detect_running_ngrok, is_ngrok_available, start_ngrok_tunnel, NgrokTunnel,
};
pub use pairing::{
    LanMonitorPairingGuard, PairingChallenge, PairingProtocol, PairingResponse, PairingState,
    QrPayload,
};
pub use qr_generator::QrGenerator;
pub use relay_client::{
    ensure_rustls_crypto_provider, ConnectionState, RelayClient, RelayEvent, RelayMessage,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, RwLock};
use tokio::io::{AsyncReadExt, AsyncSeekExt};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RemoteConnectSubmissionSource {
    Relay,
    Bot,
    LanMonitor,
}

impl RemoteConnectSubmissionSource {
    pub const fn agent_submission_source(self) -> AgentSubmissionSource {
        match self {
            RemoteConnectSubmissionSource::Relay => AgentSubmissionSource::RemoteRelay,
            RemoteConnectSubmissionSource::Bot => AgentSubmissionSource::Bot,
            RemoteConnectSubmissionSource::LanMonitor => AgentSubmissionSource::RemoteRelay,
        }
    }

    pub const fn metadata_source(self) -> &'static str {
        match self {
            RemoteConnectSubmissionSource::Relay => "remote_relay",
            RemoteConnectSubmissionSource::Bot => "bot",
            RemoteConnectSubmissionSource::LanMonitor => "lan_monitor",
        }
    }
}

pub fn build_remote_session_create_request(
    session_name: impl Into<String>,
    agent_type: impl Into<String>,
    workspace_path: Option<impl Into<String>>,
    workspace_identity: RemoteSessionWorkspaceIdentity,
    source: RemoteConnectSubmissionSource,
) -> AgentSessionCreateRequest {
    let mut metadata = serde_json::Map::new();
    metadata.insert(
        "source".to_string(),
        serde_json::Value::String(source.metadata_source().to_string()),
    );

    AgentSessionCreateRequest {
        session_name: session_name.into(),
        agent_type: agent_type.into(),
        workspace_path: workspace_path.map(Into::into),
        remote_connection_id: workspace_identity.remote_connection_id,
        remote_ssh_host: workspace_identity.remote_ssh_host,
        metadata,
    }
}

pub fn build_remote_submission_request(
    session_id: impl Into<String>,
    message: impl Into<String>,
    turn_id: Option<String>,
    source: RemoteConnectSubmissionSource,
) -> AgentSubmissionRequest {
    AgentSubmissionRequest {
        session_id: session_id.into(),
        message: message.into(),
        turn_id,
        source: Some(source.agent_submission_source()),
        attachments: Vec::new(),
        metadata: serde_json::Map::new(),
    }
}

pub fn build_remote_image_attachment(
    index: usize,
    attachment: &ImageAttachment,
) -> AgentInputAttachment {
    AgentInputAttachment::remote_image(
        format!("remote-image-{}", index + 1),
        attachment.name.clone(),
        attachment.data_url.clone(),
    )
}

pub fn build_remote_image_submission_request(
    session_id: impl Into<String>,
    message: impl Into<String>,
    turn_id: Option<String>,
    source: RemoteConnectSubmissionSource,
    images: &[ImageAttachment],
) -> AgentSubmissionRequest {
    AgentSubmissionRequest {
        session_id: session_id.into(),
        message: message.into(),
        turn_id,
        source: Some(source.agent_submission_source()),
        attachments: images
            .iter()
            .enumerate()
            .map(|(index, image)| build_remote_image_attachment(index, image))
            .collect(),
        metadata: serde_json::Map::new(),
    }
}

/// Portable image context produced from legacy remote image payloads.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RemoteImageContext {
    pub id: String,
    pub image_path: Option<String>,
    pub data_url: Option<String>,
    pub mime_type: String,
    pub metadata: Option<serde_json::Value>,
}

pub trait RemoteImageContextAdapter {
    fn from_remote_image_context(context: RemoteImageContext) -> Self;
}

pub fn build_remote_image_contexts(images: Option<&[ImageAttachment]>) -> Vec<RemoteImageContext> {
    let Some(images) = images.filter(|images| !images.is_empty()) else {
        return Vec::new();
    };

    images
        .iter()
        .map(|image| {
            let mime_type = image
                .data_url
                .split_once(',')
                .and_then(|(header, _)| {
                    header
                        .strip_prefix("data:")
                        .and_then(|rest| rest.split(';').next())
                })
                .unwrap_or("image/png")
                .to_string();

            RemoteImageContext {
                id: format!("remote_img_{}", uuid::Uuid::new_v4()),
                image_path: None,
                data_url: Some(image.data_url.clone()),
                mime_type,
                metadata: Some(serde_json::json!({
                    "name": image.name,
                    "source": "remote"
                })),
            }
        })
        .collect()
}

pub fn resolve_remote_execution_image_contexts<T>(
    legacy_images: Option<&[ImageAttachment]>,
    image_contexts: Option<Vec<T>>,
    legacy_contexts: impl FnOnce(Option<&[ImageAttachment]>) -> Vec<T>,
) -> Vec<T> {
    image_contexts.unwrap_or_else(|| legacy_contexts(legacy_images))
}

pub fn remote_session_restore_target(
    session_exists: bool,
    binding_workspace: Option<&RemoteDialogWorkspaceBinding>,
) -> Option<RemoteDialogWorkspaceBinding> {
    if session_exists {
        None
    } else {
        binding_workspace.cloned()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RemoteCancelDecision {
    CancelCurrent(String),
    StaleRequestedTurn,
    AlreadyFinished,
    NoRunningTask,
}

pub fn resolve_remote_cancel_decision(
    running_turn_id: Option<&str>,
    requested_turn_id: Option<&str>,
) -> RemoteCancelDecision {
    match (running_turn_id, requested_turn_id) {
        (Some(current_turn_id), Some(req_id)) if req_id != current_turn_id => {
            RemoteCancelDecision::StaleRequestedTurn
        }
        (Some(current_turn_id), _) => {
            RemoteCancelDecision::CancelCurrent(current_turn_id.to_string())
        }
        (None, Some(_)) => RemoteCancelDecision::AlreadyFinished,
        (None, None) => RemoteCancelDecision::NoRunningTask,
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RemoteCancelTaskRequest {
    pub session_id: String,
    pub requested_turn_id: Option<String>,
}

#[async_trait::async_trait]
pub trait RemoteCancelRuntimeHost: Send + Sync {
    async fn resolve_session_storage_dir(&self, session_id: &str) -> Option<String>;

    async fn remote_control_state(
        &self,
        session_id: &str,
    ) -> Result<Option<RemoteControlStateSnapshot>, String>;

    async fn restore_remote_session(
        &self,
        session_id: &str,
        restore_path_hint: &str,
    ) -> Result<(), String>;

    async fn cancel_remote_turn(&self, session_id: &str, turn_id: &str) -> Result<(), String>;
}

pub async fn cancel_remote_task<H>(host: &H, request: RemoteCancelTaskRequest) -> Result<(), String>
where
    H: RemoteCancelRuntimeHost + ?Sized,
{
    let RemoteCancelTaskRequest {
        session_id,
        requested_turn_id,
    } = request;

    let mut state = host.remote_control_state(&session_id).await?;
    if state.is_none() {
        let session_storage_dir = host
            .resolve_session_storage_dir(&session_id)
            .await
            .ok_or_else(|| {
                format!(
                    "Session storage directory not available for session: {}",
                    session_id
                )
            })?;
        host.restore_remote_session(&session_id, &session_storage_dir)
            .await
            .map_err(|error| format!("Session not found: {error}"))?;
        state = host.remote_control_state(&session_id).await?;
    }

    let running_turn_id = state.and_then(|state| state.active_turn_id);
    match resolve_remote_cancel_decision(running_turn_id.as_deref(), requested_turn_id.as_deref()) {
        RemoteCancelDecision::StaleRequestedTurn => {
            Err("This task is no longer running.".to_string())
        }
        RemoteCancelDecision::CancelCurrent(current_turn_id) => {
            host.cancel_remote_turn(&session_id, &current_turn_id).await
        }
        RemoteCancelDecision::AlreadyFinished => Err("This task is already finished.".to_string()),
        RemoteCancelDecision::NoRunningTask => Err(format!(
            "No running task to cancel for session: {}",
            session_id
        )),
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RemoteDialogQueuePriority {
    Low,
    Normal,
    High,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RemoteDialogSubmissionPolicy {
    pub source: RemoteConnectSubmissionSource,
    pub queue_priority: RemoteDialogQueuePriority,
    pub skip_tool_confirmation: bool,
}

impl RemoteDialogSubmissionPolicy {
    pub const fn for_source(source: RemoteConnectSubmissionSource) -> Self {
        Self {
            source,
            queue_priority: RemoteDialogQueuePriority::Normal,
            skip_tool_confirmation: true,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct RemoteDialogSubmissionRequest<ImageContext> {
    pub session_id: String,
    pub content: String,
    pub agent_type: Option<String>,
    pub image_contexts: Vec<ImageContext>,
    pub policy: RemoteDialogSubmissionPolicy,
    pub turn_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RemoteTerminalPrewarmRequest {
    pub session_id: String,
    pub binding_workspace: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RemoteDialogWorkspaceBinding {
    pub workspace_path: String,
    pub remote_connection_id: Option<String>,
    pub remote_ssh_host: Option<String>,
}

impl RemoteDialogWorkspaceBinding {
    pub fn local(workspace_path: impl Into<String>) -> Self {
        Self {
            workspace_path: workspace_path.into(),
            remote_connection_id: None,
            remote_ssh_host: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct RemoteDialogResolvedSubmission<ImageContext> {
    pub session_id: String,
    pub content: String,
    pub resolved_agent_type: String,
    pub binding_workspace: Option<RemoteDialogWorkspaceBinding>,
    pub image_contexts: Vec<ImageContext>,
    pub policy: RemoteDialogSubmissionPolicy,
    pub turn_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RemoteDialogSubmitOutcome {
    Started { session_id: String, turn_id: String },
    Queued { session_id: String, turn_id: String },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RemoteDialogSchedulerOutcomeFact {
    Started { session_id: String, turn_id: String },
    Queued { session_id: String, turn_id: String },
}

pub fn remote_dialog_submit_outcome_from_scheduler(
    fact: RemoteDialogSchedulerOutcomeFact,
) -> RemoteDialogSubmitOutcome {
    match fact {
        RemoteDialogSchedulerOutcomeFact::Started {
            session_id,
            turn_id,
        } => RemoteDialogSubmitOutcome::Started {
            session_id,
            turn_id,
        },
        RemoteDialogSchedulerOutcomeFact::Queued {
            session_id,
            turn_id,
        } => RemoteDialogSubmitOutcome::Queued {
            session_id,
            turn_id,
        },
    }
}

/// Host callbacks required by remote-connect dialog execution.
///
/// The owner crate keeps the remote dialog orchestration order stable, while
/// concrete session restore, terminal warmup, and scheduler execution stay in
/// the product runtime adapter.
#[async_trait::async_trait]
pub trait RemoteDialogRuntimeHost: Send + Sync {
    type ImageContext: Send + Sync + 'static;

    fn ensure_tracker(&self, session_id: &str);

    async fn resolve_binding_workspace(
        &self,
        session_id: &str,
    ) -> Option<RemoteDialogWorkspaceBinding>;

    async fn remote_session_exists(&self, session_id: &str) -> Result<bool, String>;

    async fn restore_remote_session(
        &self,
        session_id: &str,
        workspace: RemoteDialogWorkspaceBinding,
    ) -> Result<(), String>;

    fn prewarm_remote_terminal(&self, request: RemoteTerminalPrewarmRequest);

    fn generate_turn_id(&self) -> String;

    async fn submit_dialog(
        &self,
        submission: RemoteDialogResolvedSubmission<Self::ImageContext>,
    ) -> Result<RemoteDialogSubmitOutcome, String>;
}

pub async fn submit_remote_dialog<H>(
    host: &H,
    request: RemoteDialogSubmissionRequest<H::ImageContext>,
) -> Result<RemoteDialogSubmitOutcome, String>
where
    H: RemoteDialogRuntimeHost + ?Sized,
{
    let RemoteDialogSubmissionRequest {
        session_id,
        content,
        agent_type,
        image_contexts,
        policy,
        turn_id,
    } = request;

    host.ensure_tracker(&session_id);

    let binding_workspace = host.resolve_binding_workspace(&session_id).await;
    let session_exists = host.remote_session_exists(&session_id).await?;

    if let Some(workspace) =
        remote_session_restore_target(session_exists, binding_workspace.as_ref())
    {
        let _ = host.restore_remote_session(&session_id, workspace).await;
    }

    host.prewarm_remote_terminal(RemoteTerminalPrewarmRequest {
        session_id: session_id.clone(),
        binding_workspace: binding_workspace
            .as_ref()
            .map(|binding| binding.workspace_path.clone()),
    });

    let resolved_agent_type = resolve_remote_agent_type(agent_type.as_deref()).to_string();
    let turn_id = turn_id.unwrap_or_else(|| host.generate_turn_id());

    host.submit_dialog(RemoteDialogResolvedSubmission {
        session_id,
        content,
        resolved_agent_type,
        binding_workspace,
        image_contexts,
        policy,
        turn_id,
    })
    .await
}

pub const REMOTE_FILE_MAX_READ_BYTES: u64 = 30 * 1024 * 1024;
pub const REMOTE_FILE_MAX_CHUNK_BYTES: u64 = 3 * 1024 * 1024;

pub fn resolve_remote_file_chunk_range(
    file_len: usize,
    offset: u64,
    limit: u64,
) -> RemoteFileChunkRange {
    let actual_limit = limit.min(REMOTE_FILE_MAX_CHUNK_BYTES);
    let start = (offset as usize).min(file_len);
    let end = start.saturating_add(actual_limit as usize).min(file_len);

    RemoteFileChunkRange {
        start,
        end,
        chunk_size: (end - start) as u64,
    }
}

pub fn remote_file_display_name(name: Option<&str>) -> String {
    match name {
        Some(name) if !name.is_empty() => name.to_string(),
        _ => "file".to_string(),
    }
}

fn strip_remote_workspace_path_prefix(raw: &str) -> &str {
    raw.strip_prefix("computer://")
        .or_else(|| raw.strip_prefix("file://"))
        .unwrap_or(raw)
}

fn is_remote_absolute_workspace_path(path: &str) -> bool {
    path.starts_with('/') || (path.len() >= 3 && path.as_bytes()[1] == b':')
}

pub fn resolve_remote_workspace_path(raw: &str, workspace_root: Option<&Path>) -> Option<PathBuf> {
    let stripped = strip_remote_workspace_path_prefix(raw);

    if is_remote_absolute_workspace_path(stripped) {
        return Some(PathBuf::from(stripped));
    }

    let workspace_root = workspace_root?;
    let canonical_root = std::fs::canonicalize(workspace_root).ok()?;
    let candidate = canonical_root.join(stripped);
    let canonical_candidate = std::fs::canonicalize(candidate).ok()?;

    if canonical_candidate.starts_with(&canonical_root) {
        Some(canonical_candidate)
    } else {
        None
    }
}

pub fn detect_remote_mime_type(path: &Path) -> &'static str {
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_lowercase();

    match ext.as_str() {
        "txt" | "log" => "text/plain",
        "md" => "text/markdown",
        "html" | "htm" => "text/html",
        "css" => "text/css",
        "js" | "mjs" => "text/javascript",
        "ts" | "tsx" | "jsx" | "rs" | "py" | "go" | "java" | "c" | "cpp" | "h" | "sh" | "toml"
        | "yaml" | "yml" => "text/plain",
        "json" => "application/json",
        "xml" => "application/xml",
        "csv" => "text/csv",
        "pdf" => "application/pdf",
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "svg" => "image/svg+xml",
        "zip" => "application/zip",
        "docx" => "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
        "xlsx" => "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet",
        "pptx" => "application/vnd.openxmlformats-officedocument.presentationml.presentation",
        "mp4" => "video/mp4",
        "opus" => "audio/opus",
        _ => "application/octet-stream",
    }
}

pub async fn read_remote_workspace_file(
    raw_path: &str,
    max_size: u64,
    workspace_root: Option<&Path>,
) -> Result<RemoteWorkspaceFileContent, String> {
    let abs_path = resolve_remote_workspace_path(raw_path, workspace_root)
        .ok_or_else(|| format!("Remote file path could not be resolved: {raw_path}"))?;

    if !abs_path.exists() {
        return Err(format!("File not found: {}", abs_path.display()));
    }
    if !abs_path.is_file() {
        return Err(format!(
            "Path is not a regular file: {}",
            abs_path.display()
        ));
    }

    let metadata = tokio::fs::metadata(&abs_path)
        .await
        .map_err(|e| format!("Cannot read file metadata for {}: {e}", abs_path.display()))?;

    if metadata.len() > max_size {
        return Err(format!(
            "File too large ({} bytes, limit {max_size} bytes): {}",
            metadata.len(),
            abs_path.display()
        ));
    }

    let bytes = tokio::fs::read(&abs_path)
        .await
        .map_err(|e| format!("Cannot read file {}: {e}", abs_path.display()))?;

    Ok(RemoteWorkspaceFileContent {
        name: remote_file_display_name(abs_path.file_name().and_then(|n| n.to_str())),
        bytes,
        mime_type: detect_remote_mime_type(&abs_path),
        size: metadata.len(),
    })
}

pub async fn read_remote_workspace_file_chunk(
    raw_path: &str,
    workspace_root: Option<&Path>,
    offset: u64,
    limit: u64,
) -> Result<RemoteWorkspaceFileChunk, String> {
    let abs_path = resolve_remote_workspace_path(raw_path, workspace_root)
        .ok_or_else(|| format!("Remote file path could not be resolved: {raw_path}"))?;

    if !abs_path.exists() || !abs_path.is_file() {
        return Err(format!(
            "File not found or not a regular file: {}",
            abs_path.display()
        ));
    }

    let total_size = tokio::fs::metadata(&abs_path)
        .await
        .map_err(|e| format!("Cannot read file metadata: {e}"))?
        .len();

    let range = resolve_remote_file_chunk_range(
        usize::try_from(total_size).unwrap_or(usize::MAX),
        offset,
        limit,
    );
    let chunk_size = usize::try_from(range.chunk_size)
        .map_err(|_| "Remote file chunk is too large to allocate".to_string())?;
    let mut file = tokio::fs::File::open(&abs_path)
        .await
        .map_err(|e| format!("Cannot open file: {e}"))?;
    file.seek(std::io::SeekFrom::Start(range.start as u64))
        .await
        .map_err(|e| format!("Cannot seek file: {e}"))?;
    let mut chunk = Vec::with_capacity(chunk_size);
    let mut limited_file = file.take(range.chunk_size);
    limited_file
        .read_to_end(&mut chunk)
        .await
        .map_err(|e| format!("Cannot read file chunk: {e}"))?;
    let actual_chunk_size = u64::try_from(chunk.len()).unwrap_or(u64::MAX);
    let total_size = tokio::fs::metadata(&abs_path)
        .await
        .map(|metadata| metadata.len())
        .unwrap_or(total_size);

    Ok(RemoteWorkspaceFileChunk {
        name: remote_file_display_name(abs_path.file_name().and_then(|n| n.to_str())),
        bytes: chunk,
        offset,
        chunk_size: actual_chunk_size,
        total_size,
        mime_type: detect_remote_mime_type(&abs_path),
    })
}

pub async fn read_remote_workspace_file_info(
    raw_path: &str,
    workspace_root: Option<&Path>,
) -> Result<RemoteWorkspaceFileInfo, String> {
    let abs_path = resolve_remote_workspace_path(raw_path, workspace_root)
        .ok_or_else(|| format!("Remote file path could not be resolved: {raw_path}"))?;

    if !abs_path.exists() {
        return Err(format!("File not found: {}", abs_path.display()));
    }
    if !abs_path.is_file() {
        return Err(format!(
            "Path is not a regular file: {}",
            abs_path.display()
        ));
    }

    let size = tokio::fs::metadata(&abs_path)
        .await
        .map_err(|e| format!("Cannot read file metadata: {e}"))?
        .len();

    Ok(RemoteWorkspaceFileInfo {
        name: remote_file_display_name(abs_path.file_name().and_then(|n| n.to_str())),
        size,
        mime_type: detect_remote_mime_type(&abs_path),
    })
}

pub fn remote_file_content_response(
    result: Result<RemoteWorkspaceFileContent, String>,
) -> RemoteResponse {
    match result {
        Ok(content) => {
            use base64::Engine as _;
            RemoteResponse::FileContent {
                name: content.name,
                content_base64: base64::engine::general_purpose::STANDARD.encode(&content.bytes),
                mime_type: content.mime_type.to_string(),
                size: content.size,
            }
        }
        Err(message) => RemoteResponse::Error { message },
    }
}

pub fn remote_file_chunk_response(
    result: Result<RemoteWorkspaceFileChunk, String>,
) -> RemoteResponse {
    match result {
        Ok(chunk) => {
            use base64::Engine as _;
            RemoteResponse::FileChunk {
                name: chunk.name,
                chunk_base64: base64::engine::general_purpose::STANDARD.encode(&chunk.bytes),
                offset: chunk.offset,
                chunk_size: chunk.chunk_size,
                total_size: chunk.total_size,
                mime_type: chunk.mime_type.to_string(),
            }
        }
        Err(message) => RemoteResponse::Error { message },
    }
}

pub fn remote_file_info_response(
    result: Result<RemoteWorkspaceFileInfo, String>,
) -> RemoteResponse {
    match result {
        Ok(info) => RemoteResponse::FileInfo {
            name: info.name,
            size: info.size,
            mime_type: info.mime_type.to_string(),
        },
        Err(message) => RemoteResponse::Error { message },
    }
}

pub async fn handle_remote_workspace_file_command<H>(
    host: &H,
    command: &RemoteCommand,
) -> RemoteResponse
where
    H: RemoteWorkspaceFileRuntimeHost + ?Sized,
{
    match command {
        RemoteCommand::ReadFile { path, session_id } => {
            let workspace_root = host
                .resolve_remote_file_workspace_root(session_id.as_deref())
                .await;
            remote_file_content_response(
                read_remote_workspace_file(
                    path,
                    REMOTE_FILE_MAX_READ_BYTES,
                    workspace_root.as_deref(),
                )
                .await,
            )
        }
        RemoteCommand::ReadFileChunk {
            path,
            session_id,
            offset,
            limit,
        } => {
            let workspace_root = host
                .resolve_remote_file_workspace_root(session_id.as_deref())
                .await;
            remote_file_chunk_response(
                read_remote_workspace_file_chunk(path, workspace_root.as_deref(), *offset, *limit)
                    .await,
            )
        }
        RemoteCommand::GetFileInfo { path, session_id } => {
            let workspace_root = host
                .resolve_remote_file_workspace_root(session_id.as_deref())
                .await;
            remote_file_info_response(
                read_remote_workspace_file_info(path, workspace_root.as_deref()).await,
            )
        }
        _ => RemoteResponse::Error {
            message: "Unsupported remote workspace file command".to_string(),
        },
    }
}

pub fn remote_dialog_submit_response(
    result: Result<RemoteDialogSubmitOutcome, String>,
) -> RemoteResponse {
    match result {
        Ok(RemoteDialogSubmitOutcome::Started {
            session_id,
            turn_id,
        })
        | Ok(RemoteDialogSubmitOutcome::Queued {
            session_id,
            turn_id,
        }) => RemoteResponse::MessageSent {
            session_id,
            turn_id,
        },
        Err(message) => RemoteResponse::Error { message },
    }
}

pub fn remote_task_cancel_response(
    session_id: impl Into<String>,
    result: Result<(), String>,
) -> RemoteResponse {
    match result {
        Ok(()) => RemoteResponse::TaskCancelled {
            session_id: session_id.into(),
        },
        Err(message) => RemoteResponse::Error { message },
    }
}

pub fn remote_interaction_accepted_response(
    action: impl Into<String>,
    target_id: impl Into<String>,
    result: Result<(), String>,
) -> RemoteResponse {
    match result {
        Ok(()) => RemoteResponse::InteractionAccepted {
            action: action.into(),
            target_id: target_id.into(),
        },
        Err(message) => RemoteResponse::Error { message },
    }
}

pub fn remote_answer_question_response(result: Result<(), String>) -> RemoteResponse {
    match result {
        Ok(()) => RemoteResponse::AnswerAccepted,
        Err(message) => RemoteResponse::Error { message },
    }
}

pub fn remote_workspace_info_response(workspace: Option<RemoteWorkspaceFacts>) -> RemoteResponse {
    match workspace {
        Some(workspace) => RemoteResponse::WorkspaceInfo {
            has_workspace: true,
            path: Some(workspace.path),
            project_name: Some(workspace.name),
            git_branch: workspace.git_branch,
            workspace_kind: Some(workspace.kind.as_wire_str().to_string()),
            assistant_id: workspace.assistant_id,
            remote_connection_id: workspace.remote_connection_id,
            remote_ssh_host: workspace.remote_ssh_host,
        },
        None => RemoteResponse::WorkspaceInfo {
            has_workspace: false,
            path: None,
            project_name: None,
            git_branch: None,
            workspace_kind: None,
            assistant_id: None,
            remote_connection_id: None,
            remote_ssh_host: None,
        },
    }
}

pub fn remote_recent_workspaces_response(
    workspaces: Vec<RemoteRecentWorkspaceFacts>,
) -> RemoteResponse {
    RemoteResponse::RecentWorkspaces {
        workspaces: workspaces
            .into_iter()
            .map(|workspace| RecentWorkspaceEntry {
                path: workspace.path,
                name: workspace.name,
                last_opened: workspace.last_opened,
                workspace_kind: Some(workspace.kind.as_wire_str().to_string()),
            })
            .collect(),
    }
}

pub fn remote_assistant_list_response(
    assistants: Vec<RemoteAssistantWorkspaceFacts>,
) -> RemoteResponse {
    RemoteResponse::AssistantList {
        assistants: assistants
            .into_iter()
            .map(|assistant| AssistantEntry {
                path: assistant.path,
                name: assistant.name,
                assistant_id: assistant.assistant_id,
            })
            .collect(),
    }
}

pub fn remote_workspace_updated_response(
    result: Result<RemoteWorkspaceUpdate, String>,
) -> RemoteResponse {
    match result {
        Ok(update) => RemoteResponse::WorkspaceUpdated {
            success: true,
            path: Some(update.path),
            project_name: Some(update.name),
            error: None,
        },
        Err(message) => RemoteResponse::WorkspaceUpdated {
            success: false,
            path: None,
            project_name: None,
            error: Some(message),
        },
    }
}

pub fn remote_assistant_updated_response(
    result: Result<RemoteWorkspaceUpdate, String>,
) -> RemoteResponse {
    match result {
        Ok(update) => RemoteResponse::AssistantUpdated {
            success: true,
            path: Some(update.path),
            name: Some(update.name),
            error: None,
        },
        Err(message) => RemoteResponse::AssistantUpdated {
            success: false,
            path: None,
            name: None,
            error: Some(message),
        },
    }
}

pub fn remote_session_info(
    metadata: &RemoteSessionMetadata,
    workspace_path: Option<&str>,
    workspace_name: Option<&str>,
) -> SessionInfo {
    SessionInfo {
        session_id: metadata.session_id.clone(),
        name: metadata.name.clone(),
        agent_type: metadata.agent_type.clone(),
        created_at: (metadata.created_at_ms / 1000).to_string(),
        updated_at: (metadata.last_active_at_ms / 1000).to_string(),
        message_count: metadata.turn_count,
        workspace_path: workspace_path.map(ToOwned::to_owned),
        workspace_name: workspace_name.map(ToOwned::to_owned),
        state: None,
        active_turn_id: None,
        queue_depth: None,
    }
}

pub fn remote_session_list_response(
    metadata: Vec<RemoteSessionMetadata>,
    workspace_path: Option<&str>,
    workspace_name: Option<&str>,
    limit: usize,
    offset: usize,
) -> RemoteResponse {
    let page_size = limit.min(100);
    let total = metadata.len();
    let has_more = offset.saturating_add(page_size) < total;
    let sessions = metadata
        .iter()
        .skip(offset)
        .take(page_size)
        .map(|session| remote_session_info(session, workspace_path, workspace_name))
        .collect();

    RemoteResponse::SessionList { sessions, has_more }
}

pub fn remote_initial_sync_response(
    workspace: Option<RemoteWorkspaceFacts>,
    metadata: Vec<RemoteSessionMetadata>,
    session_workspace_name: Option<&str>,
    has_more_sessions: bool,
    authenticated_user_id: Option<String>,
) -> RemoteResponse {
    let (
        has_workspace,
        path,
        project_name,
        git_branch,
        workspace_kind,
        assistant_id,
        remote_connection_id,
        remote_ssh_host,
    ) = match workspace {
        Some(workspace) => (
            true,
            Some(workspace.path.clone()),
            Some(workspace.name.clone()),
            workspace.git_branch.clone(),
            Some(workspace.kind.as_wire_str().to_string()),
            workspace.assistant_id.clone(),
            workspace.remote_connection_id.clone(),
            workspace.remote_ssh_host.clone(),
        ),
        None => (false, None, None, None, None, None, None, None),
    };
    let workspace_path = path.as_deref();
    let sessions = metadata
        .iter()
        .map(|session| remote_session_info(session, workspace_path, session_workspace_name))
        .collect();

    RemoteResponse::InitialSync {
        has_workspace,
        path,
        project_name,
        git_branch,
        workspace_kind,
        assistant_id,
        remote_connection_id,
        remote_ssh_host,
        sessions,
        has_more_sessions,
        authenticated_user_id,
    }
}

pub async fn handle_remote_workspace_command<H>(host: &H, command: &RemoteCommand) -> RemoteResponse
where
    H: RemoteWorkspaceRuntimeHost + ?Sized,
{
    match command {
        RemoteCommand::GetWorkspaceInfo => {
            remote_workspace_info_response(host.current_workspace().await)
        }
        RemoteCommand::ListRecentWorkspaces => {
            remote_recent_workspaces_response(host.recent_workspaces().await)
        }
        RemoteCommand::SetWorkspace { path } => {
            remote_workspace_updated_response(host.open_workspace(path).await)
        }
        RemoteCommand::ListAssistants => {
            remote_assistant_list_response(host.assistant_workspaces().await)
        }
        RemoteCommand::SetAssistant { path } => {
            remote_assistant_updated_response(host.open_assistant_workspace(path).await)
        }
        _ => RemoteResponse::Error {
            message: "Unknown workspace command".into(),
        },
    }
}

pub async fn generate_remote_initial_sync<H>(
    host: &H,
    authenticated_user_id: Option<String>,
) -> RemoteResponse
where
    H: RemoteInitialSyncRuntimeHost + ?Sized,
{
    let workspace = host.current_workspace().await;
    let workspace_path = workspace
        .as_ref()
        .map(|workspace| PathBuf::from(&workspace.path));
    let workspace_name = workspace_path
        .as_ref()
        .and_then(|path| path.file_name())
        .map(|name| name.to_string_lossy().to_string());

    let workspace_identity = workspace
        .as_ref()
        .map(RemoteSessionWorkspaceIdentity::from_workspace)
        .unwrap_or_default();

    let (sessions, has_more) = if let Some(path) = workspace_path.as_deref() {
        match host.list_session_metadata(path, workspace_identity).await {
            Ok(metadata) => {
                let total = metadata.len();
                let page_size = 100usize;
                (
                    metadata.into_iter().take(page_size).collect(),
                    total > page_size,
                )
            }
            Err(_) => (Vec::new(), false),
        }
    } else {
        (Vec::new(), false)
    };

    remote_initial_sync_response(
        workspace,
        sessions,
        workspace_name.as_deref(),
        has_more,
        authenticated_user_id,
    )
}

pub fn remote_session_created_response(session_id: impl Into<String>) -> RemoteResponse {
    RemoteResponse::SessionCreated {
        session_id: session_id.into(),
    }
}

pub fn remote_session_model_updated_response(
    session_id: impl Into<String>,
    model_id: impl Into<String>,
) -> RemoteResponse {
    RemoteResponse::SessionModelUpdated {
        session_id: session_id.into(),
        model_id: model_id.into(),
    }
}

pub fn remote_messages_response(
    session_id: impl Into<String>,
    messages: Vec<ChatMessage>,
    has_more: bool,
) -> RemoteResponse {
    RemoteResponse::Messages {
        session_id: session_id.into(),
        messages,
        has_more,
    }
}

pub fn remote_session_deleted_response(session_id: impl Into<String>) -> RemoteResponse {
    RemoteResponse::SessionDeleted {
        session_id: session_id.into(),
    }
}

#[async_trait::async_trait]
pub trait RemoteSessionRuntimeHost: Send + Sync {
    async fn list_session_metadata(
        &self,
        workspace_path: &Path,
        workspace_identity: RemoteSessionWorkspaceIdentity,
    ) -> Result<Vec<RemoteSessionMetadata>, String>;
    async fn resolve_default_assistant_workspace_path(&self) -> Result<String, String>;
    async fn create_session(&self, request: AgentSessionCreateRequest) -> Result<String, String>;
    async fn load_model_catalog(
        &self,
        session_id: Option<&str>,
    ) -> Result<RemoteModelCatalog, String>;
    async fn update_session_model(
        &self,
        session_id: &str,
        model_id: &str,
    ) -> Result<String, String>;
    async fn ensure_session_loaded(&self, session_id: &str) -> Result<(), String>;
    async fn update_session_title(&self, session_id: &str, title: &str) -> Result<String, String>;
    async fn resolve_session_storage_dir(&self, session_id: &str) -> Option<PathBuf>;
    async fn load_remote_chat_messages(
        &self,
        session_storage_dir: &Path,
        session_id: &str,
    ) -> (Vec<ChatMessage>, bool);
    async fn delete_session(
        &self,
        session_storage_dir: &Path,
        session_id: &str,
    ) -> Result<(), String>;
    fn remove_tracker(&self, session_id: &str);
}

pub async fn handle_remote_session_command<H>(host: &H, command: &RemoteCommand) -> RemoteResponse
where
    H: RemoteSessionRuntimeHost + ?Sized,
{
    match command {
        RemoteCommand::ListSessions {
            workspace_path,
            remote_connection_id,
            remote_ssh_host,
            limit,
            offset,
            query,
        } => {
            let page_size = limit.unwrap_or(30).min(100);
            let page_offset = offset.unwrap_or(0);

            let Some(workspace_path) = workspace_path
                .as_deref()
                .map(str::trim)
                .filter(|path| !path.is_empty() && *path != "/")
                .map(PathBuf::from)
            else {
                return RemoteResponse::Error {
                    message: "No workspace is open on the remote device; select a recent workspace or create one first".to_string(),
                };
            };

            let workspace_path_str = workspace_path.to_string_lossy().to_string();
            let workspace_name = workspace_path
                .file_name()
                .map(|name| name.to_string_lossy().to_string());

            let workspace_identity = RemoteSessionWorkspaceIdentity::new(
                remote_connection_id.clone(),
                remote_ssh_host.clone(),
            );

            match host
                .list_session_metadata(&workspace_path, workspace_identity)
                .await
            {
                Ok(metadata) => {
                    let query = query
                        .as_deref()
                        .map(str::trim)
                        .filter(|value| !value.is_empty())
                        .map(str::to_lowercase);
                    let sessions = metadata
                        .into_iter()
                        .filter(|session| {
                            query
                                .as_ref()
                                .is_none_or(|query| session.name.to_lowercase().contains(query))
                        })
                        .collect();
                    remote_session_list_response(
                        sessions,
                        Some(workspace_path_str.as_str()),
                        workspace_name.as_deref(),
                        page_size,
                        page_offset,
                    )
                }
                Err(message) => RemoteResponse::Error { message },
            }
        }
        RemoteCommand::CreateSession {
            agent_type,
            session_name,
            workspace_path,
            remote_connection_id,
            remote_ssh_host,
        } => {
            let agent = resolve_remote_agent_type(agent_type.as_deref());
            let is_claw = agent == "Claw";
            let session_name = session_name
                .as_deref()
                .filter(|name| !name.is_empty())
                .unwrap_or(match agent {
                    "Cowork" => "Remote Cowork Session",
                    "Claw" => "Remote Claw Session",
                    _ => "Remote Code Session",
                });

            let binding_workspace = if is_claw {
                match host.resolve_default_assistant_workspace_path().await {
                    Ok(path) => Some(path),
                    Err(message) => return RemoteResponse::Error { message },
                }
            } else {
                workspace_path
                    .as_deref()
                    .map(str::trim)
                    .filter(|path| !path.is_empty() && *path != "/")
                    .map(ToOwned::to_owned)
            };

            let Some(binding_workspace) = binding_workspace else {
                return RemoteResponse::Error {
                    message: if is_claw {
                        "Failed to get or create assistant workspace".to_string()
                    } else {
                        "No workspace is open on the remote device; select a recent workspace or create one first".to_string()
                    },
                };
            };

            let request = build_remote_session_create_request(
                session_name,
                agent,
                Some(binding_workspace),
                RemoteSessionWorkspaceIdentity::new(
                    remote_connection_id.clone(),
                    remote_ssh_host.clone(),
                ),
                RemoteConnectSubmissionSource::Relay,
            );
            match host.create_session(request).await {
                Ok(session_id) => remote_session_created_response(session_id),
                Err(message) => RemoteResponse::Error { message },
            }
        }
        RemoteCommand::GetModelCatalog { session_id } => {
            match host.load_model_catalog(session_id.as_deref()).await {
                Ok(catalog) => RemoteResponse::ModelCatalog { catalog },
                Err(message) => RemoteResponse::Error { message },
            }
        }
        RemoteCommand::SetSessionModel {
            session_id,
            model_id,
        } => match host.update_session_model(session_id, model_id).await {
            Ok(normalized_model_id) => {
                remote_session_model_updated_response(session_id.clone(), normalized_model_id)
            }
            Err(message) => RemoteResponse::Error { message },
        },
        RemoteCommand::UpdateSessionTitle { session_id, title } => {
            if let Err(message) = host.ensure_session_loaded(session_id).await {
                return RemoteResponse::Error { message };
            }

            match host.update_session_title(session_id, title).await {
                Ok(normalized_title) => RemoteResponse::SessionTitleUpdated {
                    session_id: session_id.clone(),
                    title: normalized_title,
                },
                Err(message) => RemoteResponse::Error { message },
            }
        }
        RemoteCommand::GetSessionMessages {
            session_id,
            limit: _,
            before_message_id: _,
        } => {
            let Some(session_storage_dir) = host.resolve_session_storage_dir(session_id).await
            else {
                return RemoteResponse::Error {
                    message: format!(
                        "Session storage directory not available for session: {}",
                        session_id
                    ),
                };
            };
            let (chat_messages, has_more) = host
                .load_remote_chat_messages(&session_storage_dir, session_id)
                .await;
            remote_messages_response(session_id.clone(), chat_messages, has_more)
        }
        RemoteCommand::DeleteSession { session_id } => {
            let Some(session_storage_dir) = host.resolve_session_storage_dir(session_id).await
            else {
                return RemoteResponse::Error {
                    message: format!(
                        "Session storage directory not available for session: {}",
                        session_id
                    ),
                };
            };

            match host.delete_session(&session_storage_dir, session_id).await {
                Ok(()) => {
                    host.remove_tracker(session_id);
                    remote_session_deleted_response(session_id.clone())
                }
                Err(message) => RemoteResponse::Error { message },
            }
        }
        _ => RemoteResponse::Error {
            message: "Unknown session command".into(),
        },
    }
}

#[async_trait::async_trait]
pub trait RemotePollRuntimeHost: Send + Sync {
    fn ensure_tracker(&self, session_id: &str) -> Arc<RemoteSessionStateTracker>;
    async fn load_model_catalog(&self, session_id: &str) -> Option<RemoteModelCatalog>;
    async fn resolve_session_storage_dir(&self, session_id: &str) -> Option<PathBuf>;
    async fn load_remote_chat_messages(
        &self,
        session_storage_dir: &Path,
        session_id: &str,
    ) -> (Vec<ChatMessage>, bool);
}

pub async fn handle_remote_poll_command<H>(host: &H, command: &RemoteCommand) -> RemoteResponse
where
    H: RemotePollRuntimeHost + ?Sized,
{
    let RemoteCommand::PollSession {
        session_id,
        since_version,
        known_msg_count,
        known_model_catalog_version,
    } = command
    else {
        return RemoteResponse::Error {
            message: "expected poll_session".into(),
        };
    };

    let tracker = host.ensure_tracker(session_id);
    let current_version = tracker.version();
    let current_model_catalog = host.load_model_catalog(session_id).await;
    let model_catalog_delta =
        remote_model_catalog_poll_delta(current_model_catalog, *known_model_catalog_version);

    if *since_version == current_version && *since_version > 0 && !model_catalog_delta.changed {
        return remote_no_change_poll_response(current_version);
    }

    let needs_persistence = *since_version == 0 || tracker.is_persistence_dirty();
    if !needs_persistence {
        return remote_snapshot_poll_response(
            &tracker,
            current_version,
            model_catalog_delta.catalog,
        );
    }

    let Some(session_storage_dir) = host.resolve_session_storage_dir(session_id).await else {
        return RemoteResponse::Error {
            message: format!(
                "Session storage directory not available for session: {}",
                session_id
            ),
        };
    };
    let (all_chat_messages, _) = host
        .load_remote_chat_messages(&session_storage_dir, session_id)
        .await;
    let total_msg_count = all_chat_messages.len();
    let new_messages = all_chat_messages
        .into_iter()
        .skip(*known_msg_count)
        .collect();

    remote_persisted_poll_response(
        &tracker,
        current_version,
        new_messages,
        total_msg_count,
        model_catalog_delta.catalog,
    )
}

#[async_trait::async_trait]
pub trait RemoteInteractionRuntimeHost: Send + Sync {
    async fn confirm_tool(
        &self,
        tool_id: &str,
        updated_input: Option<serde_json::Value>,
    ) -> Result<(), String>;
    async fn reject_tool(&self, tool_id: &str, reason: String) -> Result<(), String>;
    async fn cancel_tool(&self, tool_id: &str, reason: String) -> Result<(), String>;
    fn answer_question(&self, tool_id: &str, answers: serde_json::Value) -> Result<(), String>;
}

pub async fn handle_remote_interaction_command<H>(
    host: &H,
    command: &RemoteCommand,
) -> RemoteResponse
where
    H: RemoteInteractionRuntimeHost + ?Sized,
{
    match command {
        RemoteCommand::ConfirmTool {
            tool_id,
            updated_input,
        } => remote_interaction_accepted_response(
            "confirm_tool",
            tool_id.clone(),
            host.confirm_tool(tool_id, updated_input.clone()).await,
        ),
        RemoteCommand::RejectTool { tool_id, reason } => {
            let reject_reason = reason
                .clone()
                .unwrap_or_else(|| "User rejected".to_string());
            remote_interaction_accepted_response(
                "reject_tool",
                tool_id.clone(),
                host.reject_tool(tool_id, reject_reason).await,
            )
        }
        RemoteCommand::CancelTool { tool_id, reason } => {
            let cancel_reason = reason
                .clone()
                .unwrap_or_else(|| "User cancelled".to_string());
            remote_interaction_accepted_response(
                "cancel_tool",
                tool_id.clone(),
                host.cancel_tool(tool_id, cancel_reason).await,
            )
        }
        RemoteCommand::AnswerQuestion { tool_id, answers } => {
            remote_answer_question_response(host.answer_question(tool_id, answers.clone()))
        }
        _ => RemoteResponse::Error {
            message: "Unknown execution command".into(),
        },
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct RemoteDefaultModelsConfig {
    pub primary: Option<String>,
    pub fast: Option<String>,
    pub search: Option<String>,
    pub image_understanding: Option<String>,
    pub image_generation: Option<String>,
    pub speech_recognition: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RemoteModelConfig {
    pub id: String,
    pub name: String,
    pub provider: String,
    pub base_url: String,
    pub model_name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub context_window: Option<u32>,
    pub enabled: bool,
    pub capabilities: Vec<String>,
    pub enable_thinking_process: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reasoning_mode: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reasoning_effort: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub thinking_budget_tokens: Option<u32>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RemoteModelCatalog {
    pub version: u64,
    pub models: Vec<RemoteModelConfig>,
    pub default_models: RemoteDefaultModelsConfig,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub session_model_id: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RemoteModelCapabilityFact {
    TextChat,
    ImageUnderstanding,
    ImageGeneration,
    Embedding,
    Search,
    CodeSpecialized,
    FunctionCalling,
    SpeechRecognition,
}

impl RemoteModelCapabilityFact {
    const fn wire_value(self) -> &'static str {
        match self {
            RemoteModelCapabilityFact::TextChat => "text_chat",
            RemoteModelCapabilityFact::ImageUnderstanding => "image_understanding",
            RemoteModelCapabilityFact::ImageGeneration => "image_generation",
            RemoteModelCapabilityFact::Embedding => "embedding",
            RemoteModelCapabilityFact::Search => "search",
            RemoteModelCapabilityFact::CodeSpecialized => "code_specialized",
            RemoteModelCapabilityFact::FunctionCalling => "function_calling",
            RemoteModelCapabilityFact::SpeechRecognition => "speech_recognition",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RemoteReasoningModeFact {
    Default,
    Enabled,
    Disabled,
    Adaptive,
}

impl RemoteReasoningModeFact {
    const fn wire_value(self) -> &'static str {
        match self {
            RemoteReasoningModeFact::Default => "default",
            RemoteReasoningModeFact::Enabled => "enabled",
            RemoteReasoningModeFact::Disabled => "disabled",
            RemoteReasoningModeFact::Adaptive => "adaptive",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RemoteModelFacts {
    pub id: String,
    pub name: String,
    pub provider: String,
    pub base_url: String,
    pub model_name: String,
    pub context_window: Option<u32>,
    pub enabled: bool,
    pub capabilities: Vec<RemoteModelCapabilityFact>,
    pub enable_thinking_process: bool,
    pub reasoning_mode: Option<RemoteReasoningModeFact>,
    pub reasoning_effort: Option<String>,
    pub thinking_budget_tokens: Option<u32>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RemoteModelCatalogFacts {
    pub last_modified_ms: i64,
    pub models: Vec<RemoteModelFacts>,
    pub default_models: RemoteDefaultModelsConfig,
    pub session_model_id: Option<String>,
}

pub fn build_remote_model_catalog(facts: RemoteModelCatalogFacts) -> RemoteModelCatalog {
    RemoteModelCatalog {
        version: facts.last_modified_ms.max(0) as u64,
        models: facts
            .models
            .into_iter()
            .map(|model| RemoteModelConfig {
                id: model.id,
                name: model.name,
                provider: model.provider,
                base_url: model.base_url,
                model_name: model.model_name,
                context_window: model.context_window,
                enabled: model.enabled,
                capabilities: model
                    .capabilities
                    .into_iter()
                    .map(|capability| capability.wire_value().to_string())
                    .collect(),
                enable_thinking_process: model.enable_thinking_process,
                reasoning_mode: model
                    .reasoning_mode
                    .map(|reasoning_mode| reasoning_mode.wire_value().to_string()),
                reasoning_effort: model.reasoning_effort,
                thinking_budget_tokens: model.thinking_budget_tokens,
            })
            .collect(),
        default_models: facts.default_models,
        session_model_id: facts.session_model_id,
    }
}

pub fn normalize_remote_session_model_id(model_id: Option<&str>) -> Option<String> {
    match model_id {
        Some(value) => {
            let trimmed = value.trim();
            if trimmed.is_empty() || trimmed == "default" {
                Some("auto".to_string())
            } else {
                Some(trimmed.to_string())
            }
        }
        None => Some("auto".to_string()),
    }
}

pub fn remote_model_selection_needs_config(requested_model_id: &str) -> bool {
    let requested_model_id = requested_model_id.trim();
    !requested_model_id.is_empty()
        && !matches!(requested_model_id, "auto" | "default" | "primary" | "fast")
}

pub fn normalize_remote_model_selection(
    requested_model_id: &str,
    resolve_model_reference: impl FnOnce(&str) -> Option<String>,
) -> Result<String, String> {
    let requested_model_id = requested_model_id.trim();
    if requested_model_id.is_empty() {
        return Err("model_id is required".to_string());
    }

    if matches!(requested_model_id, "auto" | "default" | "primary" | "fast") {
        return Ok(if requested_model_id == "default" {
            "auto".to_string()
        } else {
            requested_model_id.to_string()
        });
    }

    resolve_model_reference(requested_model_id)
        .ok_or_else(|| format!("Unknown model selection: {requested_model_id}"))
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RemoteModelCatalogPollDelta {
    pub changed: bool,
    pub catalog: Option<RemoteModelCatalog>,
}

pub fn resolve_remote_agent_type(mobile_type: Option<&str>) -> &'static str {
    match mobile_type {
        Some("code") | Some("agentic") | Some("Agentic") => "agentic",
        Some("multitask") | Some("Multitask") => "Multitask",
        Some("cowork") | Some("Cowork") => "Cowork",
        Some("plan") | Some("Plan") => "Plan",
        Some("debug") | Some("Debug") => "debug",
        _ => "agentic",
    }
}

/// Image sent from a remote client as a base64 data URL.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ImageAttachment {
    pub name: String,
    pub data_url: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SessionInfo {
    pub session_id: String,
    pub name: String,
    pub agent_type: String,
    pub created_at: String,
    pub updated_at: String,
    pub message_count: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub workspace_path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub workspace_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub state: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub active_turn_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub queue_depth: Option<usize>,
}

pub const LAN_MONITOR_DEFAULT_TURN_LIMIT: usize = 50;
pub const LAN_MONITOR_MAX_TURN_LIMIT: usize = 100;
pub const LAN_MONITOR_RESULT_STRING_CHAR_LIMIT: usize = 16 * 1024;
pub const LAN_MONITOR_RESULT_TOTAL_CHAR_BUDGET: usize = 512 * 1024;
pub const LAN_MONITOR_RESULT_CHUNK_CHAR_LIMIT: usize = 64 * 1024;
pub const LAN_MONITOR_COMMAND_MAX_AGE_MS: u64 = 5 * 60 * 1000;
pub const LAN_MONITOR_COMMAND_FUTURE_SKEW_MS: u64 = 30 * 1000;
const LAN_MONITOR_MAX_TRACKED_COMMANDS: usize = 2_048;

const LAN_MONITOR_TRUNCATED_MARKER: &str = "\n... Output truncated for LAN monitor preview";
const LAN_MONITOR_OMITTED_MARKER: &str = "Output omitted from LAN monitor preview";

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LanMonitorTranscriptPage {
    pub session_id: String,
    pub turns: Vec<LanMonitorTurn>,
    pub total_turn_count: usize,
    pub has_more: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub next_before_turn_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LanMonitorTurn {
    pub turn_id: String,
    pub turn_index: usize,
    pub kind: String,
    pub status: String,
    pub timestamp: u64,
    pub start_time: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub end_time: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub duration_ms: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub finish_reason: Option<String>,
    pub user_message: LanMonitorUserMessage,
    pub rounds: Vec<LanMonitorRound>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LanMonitorUserMessage {
    pub id: String,
    pub content: String,
    pub timestamp: u64,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub images: Vec<ChatImageAttachment>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LanMonitorRound {
    pub id: String,
    pub round_index: usize,
    pub status: String,
    pub start_time: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub end_time: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub duration_ms: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model_alias: Option<String>,
    pub items: Vec<LanMonitorItem>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(
    tag = "type",
    rename_all = "snake_case",
    rename_all_fields = "camelCase"
)]
pub enum LanMonitorItem {
    Text {
        id: String,
        content: String,
        is_markdown: bool,
        timestamp: u64,
        #[serde(skip_serializing_if = "Option::is_none")]
        status: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        order_index: Option<usize>,
        #[serde(skip_serializing_if = "Option::is_none")]
        subagent_session_id: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        parent_task_tool_id: Option<String>,
    },
    Thinking {
        id: String,
        content: String,
        is_collapsed: bool,
        timestamp: u64,
        #[serde(skip_serializing_if = "Option::is_none")]
        status: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        order_index: Option<usize>,
        #[serde(skip_serializing_if = "Option::is_none")]
        subagent_session_id: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        parent_task_tool_id: Option<String>,
    },
    Tool {
        id: String,
        name: String,
        status: String,
        input: serde_json::Value,
        #[serde(skip_serializing_if = "Option::is_none")]
        result: Option<serde_json::Value>,
        #[serde(skip_serializing_if = "Option::is_none")]
        success: Option<bool>,
        #[serde(skip_serializing_if = "Option::is_none")]
        error: Option<String>,
        start_time: u64,
        #[serde(skip_serializing_if = "Option::is_none")]
        end_time: Option<u64>,
        #[serde(skip_serializing_if = "Option::is_none")]
        duration_ms: Option<u64>,
        #[serde(skip_serializing_if = "Option::is_none")]
        order_index: Option<usize>,
        #[serde(skip_serializing_if = "Option::is_none")]
        subagent_session_id: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        subagent_dialog_turn_id: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        parent_task_tool_id: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        subagent_model_id: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        subagent_model_display_name: Option<String>,
        #[serde(default)]
        result_truncated: bool,
        #[serde(skip_serializing_if = "Option::is_none")]
        result_ref: Option<String>,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LanMonitorActiveTurn {
    pub turn_id: String,
    pub status: String,
    pub round_index: usize,
    pub text: String,
    pub thinking: String,
    pub tools: Vec<LanMonitorActiveTool>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub items: Vec<ChatMessageItem>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LanMonitorActiveTool {
    pub id: String,
    pub name: String,
    pub status: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub input: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub duration_ms: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub start_ms: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LanMonitorPollSnapshot {
    pub version: u64,
    pub changed: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub session_state: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub active_turn: Option<LanMonitorActiveTurn>,
    #[serde(default)]
    pub transcript_changed: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub total_turn_count: Option<usize>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LanMonitorToolResultChunk {
    pub result_ref: String,
    pub cursor: usize,
    pub chunk: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub next_cursor: Option<usize>,
    pub has_more: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "action", rename_all = "snake_case")]
pub enum LanMonitorCommand {
    GetWorkspaceInfo,
    ListSessions {
        limit: Option<usize>,
        offset: Option<usize>,
        query: Option<String>,
    },
    GetTranscriptPage {
        session_id: String,
        limit: Option<usize>,
        before_turn_id: Option<String>,
    },
    GetToolResultChunk {
        session_id: String,
        turn_id: String,
        tool_id: String,
        result_ref: String,
        cursor: usize,
        limit: Option<usize>,
    },
    PollSession {
        session_id: String,
        since_version: u64,
        known_turn_count: usize,
    },
    CancelTask {
        session_id: String,
        turn_id: Option<String>,
    },
    ConfirmTool {
        session_id: String,
        tool_id: String,
    },
    RejectTool {
        session_id: String,
        tool_id: String,
        reason: Option<String>,
    },
    Ping,
}

#[derive(Debug, Default)]
pub struct LanMonitorMessageGuard {
    seen_nonces: HashMap<String, u64>,
    seen_request_ids: HashMap<String, u64>,
}

impl LanMonitorMessageGuard {
    pub fn validate_envelope(
        &mut self,
        value: &serde_json::Value,
        nonce: &str,
        now_ms: u64,
    ) -> Result<String, String> {
        let version = value
            .get("_protocol_version")
            .and_then(serde_json::Value::as_u64)
            .ok_or_else(|| "Missing LAN monitor protocol version".to_string())?;
        if version != 1 {
            return Err("Unsupported LAN monitor protocol version".to_string());
        }

        let issued_at_ms = value
            .get("_issued_at_ms")
            .and_then(serde_json::Value::as_u64)
            .ok_or_else(|| "Missing LAN monitor command timestamp".to_string())?;
        if issued_at_ms > now_ms.saturating_add(LAN_MONITOR_COMMAND_FUTURE_SKEW_MS)
            || now_ms.saturating_sub(issued_at_ms) > LAN_MONITOR_COMMAND_MAX_AGE_MS
        {
            return Err("Expired LAN monitor command".to_string());
        }

        let request_id = value
            .get("_request_id")
            .and_then(serde_json::Value::as_str)
            .map(str::trim)
            .filter(|request_id| {
                (8..=128).contains(&request_id.len())
                    && request_id.chars().all(|character| {
                        character.is_ascii_alphanumeric() || matches!(character, '-' | '_')
                    })
            })
            .ok_or_else(|| "Invalid LAN monitor request ID".to_string())?;
        if nonce.is_empty() || nonce.len() > 128 {
            return Err("Invalid LAN monitor nonce".to_string());
        }

        let cutoff = now_ms.saturating_sub(
            LAN_MONITOR_COMMAND_MAX_AGE_MS.saturating_add(LAN_MONITOR_COMMAND_FUTURE_SKEW_MS),
        );
        self.seen_nonces.retain(|_, seen_at| *seen_at >= cutoff);
        self.seen_request_ids
            .retain(|_, seen_at| *seen_at >= cutoff);

        if self.seen_nonces.contains_key(nonce) || self.seen_request_ids.contains_key(request_id) {
            return Err("Replayed LAN monitor command".to_string());
        }
        if self.seen_nonces.len() >= LAN_MONITOR_MAX_TRACKED_COMMANDS
            || self.seen_request_ids.len() >= LAN_MONITOR_MAX_TRACKED_COMMANDS
        {
            return Err("Too many pending LAN monitor commands".to_string());
        }

        self.seen_nonces.insert(nonce.to_string(), now_ms);
        self.seen_request_ids.insert(request_id.to_string(), now_ms);
        Ok(request_id.to_string())
    }
}

pub fn lan_monitor_result_ref(session_id: &str, turn_id: &str, tool_id: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(session_id.as_bytes());
    hasher.update([0]);
    hasher.update(turn_id.as_bytes());
    hasher.update([0]);
    hasher.update(tool_id.as_bytes());
    hex::encode(hasher.finalize())
}

fn sensitive_monitor_key(key: &str) -> bool {
    let normalized = key
        .chars()
        .map(|character| match character {
            '-' | ' ' | '.' => '_',
            other => other.to_ascii_lowercase(),
        })
        .collect::<String>();
    matches!(
        normalized.as_str(),
        "api_key"
            | "apikey"
            | "authorization"
            | "cookie"
            | "set_cookie"
            | "password"
            | "passwd"
            | "secret"
            | "client_secret"
            | "access_token"
            | "refresh_token"
            | "id_token"
            | "private_key"
            | "credential"
            | "credentials"
    ) || normalized.ends_with("_password")
        || normalized.ends_with("_secret")
        || normalized.ends_with("_token")
        || normalized.ends_with("_credential")
}

fn redact_monitor_marker(value: &mut String, marker: &str) {
    let mut search_from = 0usize;
    loop {
        let lowercase = value.to_ascii_lowercase();
        let Some(relative_start) = lowercase[search_from..].find(&marker.to_ascii_lowercase())
        else {
            break;
        };
        let value_start = search_from + relative_start + marker.len();
        let suffix = &value[value_start..];
        let whitespace = suffix.len() - suffix.trim_start().len();
        let secret_start = value_start + whitespace;
        let secret_end = value[secret_start..]
            .find(|character: char| {
                character.is_whitespace()
                    || character == '&'
                    || character == ','
                    || character == '\''
                    || character == '"'
            })
            .map(|offset| secret_start + offset)
            .unwrap_or(value.len());
        if secret_end <= secret_start {
            search_from = value_start;
            continue;
        }
        value.replace_range(secret_start..secret_end, "[redacted]");
        search_from = secret_start + "[redacted]".len();
    }
}

pub fn sanitize_lan_monitor_value(value: &serde_json::Value) -> serde_json::Value {
    match value {
        serde_json::Value::Object(map) => serde_json::Value::Object(
            map.iter()
                .map(|(key, child)| {
                    if sensitive_monitor_key(key) {
                        (
                            key.clone(),
                            serde_json::Value::String("[redacted]".to_string()),
                        )
                    } else {
                        (key.clone(), sanitize_lan_monitor_value(child))
                    }
                })
                .collect(),
        ),
        serde_json::Value::Array(items) => {
            serde_json::Value::Array(items.iter().map(sanitize_lan_monitor_value).collect())
        }
        serde_json::Value::String(text) => {
            let mut redacted = text.clone();
            for marker in [
                "Bearer ",
                "--api-key=",
                "--api-key ",
                "--token=",
                "--token ",
                "--password=",
                "--password ",
                "api_key=",
                "access_token=",
                "refresh_token=",
                "password=",
            ] {
                redact_monitor_marker(&mut redacted, marker);
            }
            serde_json::Value::String(redacted)
        }
        other => other.clone(),
    }
}

fn truncate_lan_monitor_string(text: &str, remaining_budget: &mut usize) -> Option<String> {
    let character_count = text.chars().count();
    let available = (*remaining_budget).min(LAN_MONITOR_RESULT_STRING_CHAR_LIMIT);
    if character_count <= available {
        *remaining_budget = remaining_budget.saturating_sub(character_count);
        return None;
    }
    let marker_count = LAN_MONITOR_TRUNCATED_MARKER.chars().count();
    if available <= marker_count {
        *remaining_budget = remaining_budget.saturating_sub(
            LAN_MONITOR_OMITTED_MARKER
                .chars()
                .count()
                .min(*remaining_budget),
        );
        return Some(LAN_MONITOR_OMITTED_MARKER.to_string());
    }
    let mut preview = text
        .chars()
        .take(available - marker_count)
        .collect::<String>();
    preview.push_str(LAN_MONITOR_TRUNCATED_MARKER);
    *remaining_budget = remaining_budget.saturating_sub(preview.chars().count());
    Some(preview)
}

fn compact_lan_monitor_value(value: &mut serde_json::Value, remaining_budget: &mut usize) -> bool {
    match value {
        serde_json::Value::String(text) => {
            if let Some(preview) = truncate_lan_monitor_string(text, remaining_budget) {
                *text = preview;
                true
            } else {
                false
            }
        }
        serde_json::Value::Array(items) => items.iter_mut().fold(false, |truncated, item| {
            compact_lan_monitor_value(item, remaining_budget) || truncated
        }),
        serde_json::Value::Object(map) => map.values_mut().fold(false, |truncated, item| {
            compact_lan_monitor_value(item, remaining_budget) || truncated
        }),
        _ => false,
    }
}

pub fn compact_lan_monitor_transcript_page(page: &mut LanMonitorTranscriptPage) {
    let mut remaining_budget = LAN_MONITOR_RESULT_TOTAL_CHAR_BUDGET;
    for turn in &mut page.turns {
        for round in &mut turn.rounds {
            for item in &mut round.items {
                let LanMonitorItem::Tool {
                    id,
                    result,
                    result_truncated,
                    result_ref,
                    ..
                } = item
                else {
                    continue;
                };
                let Some(result) = result.as_mut() else {
                    continue;
                };
                if compact_lan_monitor_value(result, &mut remaining_budget) {
                    *result_truncated = true;
                    *result_ref = Some(lan_monitor_result_ref(&page.session_id, &turn.turn_id, id));
                }
            }
        }
    }
}

pub fn lan_monitor_tool_result_chunk(
    result_ref: String,
    value: &serde_json::Value,
    cursor: usize,
    requested_limit: Option<usize>,
) -> Result<LanMonitorToolResultChunk, String> {
    let serialized = serde_json::to_string_pretty(&sanitize_lan_monitor_value(value))
        .map_err(|error| format!("Failed to serialize tool result: {error}"))?;
    let characters = serialized.chars().collect::<Vec<_>>();
    if cursor > characters.len() {
        return Err("Invalid tool result cursor".to_string());
    }
    let limit = requested_limit
        .unwrap_or(LAN_MONITOR_RESULT_CHUNK_CHAR_LIMIT)
        .clamp(1, LAN_MONITOR_RESULT_CHUNK_CHAR_LIMIT);
    let end = cursor.saturating_add(limit).min(characters.len());
    let chunk = characters[cursor..end].iter().collect::<String>();
    Ok(LanMonitorToolResultChunk {
        result_ref,
        cursor,
        chunk,
        next_cursor: (end < characters.len()).then_some(end),
        has_more: end < characters.len(),
    })
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChatImageAttachment {
    pub name: String,
    pub data_url: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ChatMessage {
    pub id: String,
    pub role: String,
    pub content: String,
    pub timestamp: String,
    pub metadata: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tools: Option<Vec<RemoteToolStatus>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub thinking: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub items: Option<Vec<ChatMessageItem>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub images: Option<Vec<ChatImageAttachment>>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ChatMessageItem {
    #[serde(rename = "type")]
    pub item_type: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool: Option<RemoteToolStatus>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub is_subagent: Option<bool>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct RemoteChatHistoryTurn {
    pub turn_id: String,
    pub user_message_id: String,
    pub user_display_content: String,
    pub user_timestamp_ms: u64,
    pub user_images: Vec<ChatImageAttachment>,
    pub is_in_progress: bool,
    pub start_time_ms: u64,
    pub rounds: Vec<RemoteChatHistoryRound>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct RemoteChatHistoryRound {
    pub start_time_ms: u64,
    pub end_time_ms: Option<u64>,
    pub text_items: Vec<RemoteChatHistoryTextItem>,
    pub thinking_items: Vec<RemoteChatHistoryThinkingItem>,
    pub tool_items: Vec<RemoteChatHistoryToolItem>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RemoteChatHistoryTextItem {
    pub content: String,
    pub order_index: Option<usize>,
    pub is_subagent: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RemoteChatHistoryThinkingItem {
    pub content: String,
    pub order_index: Option<usize>,
    pub is_subagent: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct RemoteChatHistoryToolItem {
    pub id: String,
    pub name: String,
    pub call: RemoteChatHistoryToolCall,
    pub has_result: bool,
    pub status: Option<String>,
    pub duration_ms: Option<u64>,
    pub start_ms: u64,
    pub order_index: Option<usize>,
    pub is_subagent: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct RemoteChatHistoryToolCall {
    pub id: String,
    pub input: serde_json::Value,
}

pub fn build_remote_chat_messages(turns: Vec<RemoteChatHistoryTurn>) -> Vec<ChatMessage> {
    let mut result = Vec::new();

    for turn in turns {
        result.push(ChatMessage {
            id: turn.user_message_id,
            role: "user".to_string(),
            content: turn.user_display_content,
            timestamp: (turn.user_timestamp_ms / 1000).to_string(),
            metadata: None,
            tools: None,
            thinking: None,
            items: None,
            images: if turn.user_images.is_empty() {
                None
            } else {
                Some(turn.user_images)
            },
        });

        if turn.is_in_progress {
            continue;
        }

        struct OrderedEntry {
            order_index: Option<usize>,
            sequence: usize,
            round_idx: usize,
            item: ChatMessageItem,
        }

        let mut ordered = Vec::new();
        let mut tools_flat = Vec::new();
        let mut thinking_parts = Vec::new();
        let mut text_parts = Vec::new();
        let mut sequence = 0usize;
        let assistant_ts = turn
            .rounds
            .last()
            .map(|round| round.end_time_ms.unwrap_or(round.start_time_ms))
            .unwrap_or(turn.start_time_ms);

        for (round_idx, round) in turn.rounds.into_iter().enumerate() {
            for item in round.thinking_items {
                if item.is_subagent || item.content.is_empty() {
                    continue;
                }
                thinking_parts.push(item.content.clone());
                ordered.push(OrderedEntry {
                    order_index: item.order_index,
                    sequence,
                    round_idx,
                    item: ChatMessageItem {
                        item_type: "thinking".to_string(),
                        content: Some(item.content.clone()),
                        tool: None,
                        is_subagent: None,
                    },
                });
                sequence += 1;
            }

            for item in round.text_items {
                if item.is_subagent || item.content.is_empty() {
                    continue;
                }
                text_parts.push(item.content.clone());
                ordered.push(OrderedEntry {
                    order_index: item.order_index,
                    sequence,
                    round_idx,
                    item: ChatMessageItem {
                        item_type: "text".to_string(),
                        content: Some(item.content.clone()),
                        tool: None,
                        is_subagent: None,
                    },
                });
                sequence += 1;
            }

            for item in round.tool_items {
                if item.is_subagent {
                    continue;
                }
                let status = item.status.as_deref().unwrap_or(if item.has_result {
                    "completed"
                } else {
                    "running"
                });
                let tool_status = RemoteToolStatus {
                    id: item.id,
                    name: item.name.clone(),
                    status: status.to_string(),
                    duration_ms: item.duration_ms,
                    start_ms: Some(item.start_ms),
                    input_preview: make_slim_tool_params(&item.call.input),
                    tool_input: if item.name == "AskUserQuestion"
                        || item.name == "Task"
                        || item.name == "TodoWrite"
                    {
                        Some(item.call.input.clone())
                    } else {
                        None
                    },
                    monitor_input: Some(item.call.input.clone()),
                };
                tools_flat.push(tool_status.clone());
                ordered.push(OrderedEntry {
                    order_index: item.order_index,
                    sequence,
                    round_idx,
                    item: ChatMessageItem {
                        item_type: "tool".to_string(),
                        content: None,
                        tool: Some(tool_status),
                        is_subagent: None,
                    },
                });
                sequence += 1;
            }
        }

        ordered.sort_by(|a, b| {
            let round_cmp = a.round_idx.cmp(&b.round_idx);
            if round_cmp != std::cmp::Ordering::Equal {
                return round_cmp;
            }
            match (a.order_index, b.order_index) {
                (Some(a_idx), Some(b_idx)) => a_idx.cmp(&b_idx),
                (Some(_), None) => std::cmp::Ordering::Less,
                (None, Some(_)) => std::cmp::Ordering::Greater,
                (None, None) => a.sequence.cmp(&b.sequence),
            }
        });

        let items: Vec<ChatMessageItem> = ordered.into_iter().map(|entry| entry.item).collect();

        result.push(ChatMessage {
            id: format!("{}_assistant", turn.turn_id),
            role: "assistant".to_string(),
            content: text_parts.join("\n\n"),
            timestamp: (assistant_ts / 1000).to_string(),
            metadata: None,
            tools: if tools_flat.is_empty() {
                None
            } else {
                Some(tools_flat)
            },
            thinking: if thinking_parts.is_empty() {
                None
            } else {
                Some(thinking_parts.join("\n\n"))
            },
            items: if items.is_empty() { None } else { Some(items) },
            images: None,
        });
    }

    result
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RecentWorkspaceEntry {
    pub path: String,
    pub name: String,
    pub last_opened: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub workspace_kind: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AssistantEntry {
    pub path: String,
    pub name: String,
    pub assistant_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ActiveTurnSnapshot {
    pub turn_id: String,
    pub status: String,
    pub text: String,
    pub thinking: String,
    pub tools: Vec<RemoteToolStatus>,
    pub round_index: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub items: Option<Vec<ChatMessageItem>>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RemoteToolStatus {
    pub id: String,
    pub name: String,
    pub status: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub duration_ms: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub start_ms: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub input_preview: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_input: Option<serde_json::Value>,
    #[serde(skip)]
    pub monitor_input: Option<serde_json::Value>,
}

/// Commands that remote clients can send to the desktop runtime.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "cmd", rename_all = "snake_case")]
pub enum RemoteCommand {
    LanMonitor {
        request: LanMonitorCommand,
    },
    GetWorkspaceInfo,
    ListRecentWorkspaces,
    SetWorkspace {
        path: String,
    },
    ListAssistants,
    SetAssistant {
        path: String,
    },
    ListSessions {
        workspace_path: Option<String>,
        #[serde(default)]
        remote_connection_id: Option<String>,
        #[serde(default)]
        remote_ssh_host: Option<String>,
        limit: Option<usize>,
        offset: Option<usize>,
        query: Option<String>,
    },
    CreateSession {
        agent_type: Option<String>,
        session_name: Option<String>,
        workspace_path: Option<String>,
        #[serde(default)]
        remote_connection_id: Option<String>,
        #[serde(default)]
        remote_ssh_host: Option<String>,
    },
    GetModelCatalog {
        session_id: Option<String>,
    },
    SetSessionModel {
        session_id: String,
        model_id: String,
    },
    UpdateSessionTitle {
        session_id: String,
        title: String,
    },
    GetSessionMessages {
        session_id: String,
        limit: Option<usize>,
        before_message_id: Option<String>,
    },
    SendMessage {
        session_id: String,
        content: String,
        agent_type: Option<String>,
        images: Option<Vec<ImageAttachment>>,
        image_contexts: Option<Vec<RemoteImageContext>>,
    },
    CancelTask {
        session_id: String,
        turn_id: Option<String>,
    },
    DeleteSession {
        session_id: String,
    },
    ConfirmTool {
        tool_id: String,
        updated_input: Option<serde_json::Value>,
    },
    RejectTool {
        tool_id: String,
        reason: Option<String>,
    },
    CancelTool {
        tool_id: String,
        reason: Option<String>,
    },
    AnswerQuestion {
        tool_id: String,
        answers: serde_json::Value,
    },
    PollSession {
        session_id: String,
        since_version: u64,
        known_msg_count: usize,
        known_model_catalog_version: Option<u64>,
    },
    ReadFile {
        path: String,
        session_id: Option<String>,
    },
    ReadFileChunk {
        path: String,
        session_id: Option<String>,
        offset: u64,
        limit: u64,
    },
    GetFileInfo {
        path: String,
        session_id: Option<String>,
    },
    Ping,

    // ── Device-to-device distributed control ──────────────────────────────
    //
    // These variants are carried *inside* an encrypted device-to-device
    // payload (see `RelayMessage::DeviceMessage`). The relay never sees them
    // in cleartext; the receiving device decrypts the outer envelope with the
    // account master_key, then deserializes the inner JSON into `RemoteCommand`.
    //
    // Currently sent over the HTTP /api/sync/* path (encrypted with master_key)
    // or the WS device-messaging path. The relay routes by device_id only.
    /// Push a serialized chat session to a peer device so it can import it.
    SendSessionToDevice {
        /// The opaque session blob (exported session JSON, encrypted by the
        /// caller before it reaches this layer if sent over WS).
        session_data: String,
        session_id: String,
        session_name: Option<String>,
    },
    /// Ask a peer device to execute a prompt in an existing or new session.
    ExecuteOnDevice {
        session_id: Option<String>,
        content: String,
        agent_type: Option<String>,
        workspace_path: Option<String>,
    },
    /// Query a peer device for workspace / session info (read-only).
    DeviceQueryInfo,
    /// Create a new workspace directory on the peer device.
    CreateWorkspace {
        path: String,
    },
    /// Proxy a desktop product Tauri command onto the peer host.
    /// Used by Peer Device Mode so the controller UI can reuse the same invoke surface.
    HostInvoke {
        command: String,
        #[serde(default)]
        args: serde_json::Value,
    },
    /// Push a UI event from peer to controller (same event name as local Tauri emit).
    DeviceEvent {
        event: String,
        #[serde(default)]
        payload: serde_json::Value,
    },
}

/// Responses sent from desktop back to remote clients.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "resp", rename_all = "snake_case")]
pub enum RemoteResponse {
    LanMonitorWorkspace {
        workspace: Option<RemoteWorkspaceFacts>,
    },
    LanMonitorSessions {
        sessions: Vec<SessionInfo>,
        has_more: bool,
    },
    LanMonitorTranscript {
        page: LanMonitorTranscriptPage,
    },
    LanMonitorToolResult {
        result: LanMonitorToolResultChunk,
    },
    LanMonitorPoll {
        snapshot: LanMonitorPollSnapshot,
    },
    LanMonitorActionAccepted {
        action: String,
        target_id: String,
    },
    WorkspaceInfo {
        has_workspace: bool,
        path: Option<String>,
        project_name: Option<String>,
        git_branch: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        workspace_kind: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        assistant_id: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        remote_connection_id: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        remote_ssh_host: Option<String>,
    },
    RecentWorkspaces {
        workspaces: Vec<RecentWorkspaceEntry>,
    },
    WorkspaceUpdated {
        success: bool,
        path: Option<String>,
        project_name: Option<String>,
        error: Option<String>,
    },
    AssistantList {
        assistants: Vec<AssistantEntry>,
    },
    AssistantUpdated {
        success: bool,
        path: Option<String>,
        name: Option<String>,
        error: Option<String>,
    },
    SessionList {
        sessions: Vec<SessionInfo>,
        has_more: bool,
    },
    SessionCreated {
        session_id: String,
    },
    ModelCatalog {
        catalog: RemoteModelCatalog,
    },
    SessionModelUpdated {
        session_id: String,
        model_id: String,
    },
    SessionTitleUpdated {
        session_id: String,
        title: String,
    },
    Messages {
        session_id: String,
        messages: Vec<ChatMessage>,
        has_more: bool,
    },
    MessageSent {
        session_id: String,
        turn_id: String,
    },
    TaskCancelled {
        session_id: String,
    },
    SessionDeleted {
        session_id: String,
    },
    InitialSync {
        has_workspace: bool,
        #[serde(skip_serializing_if = "Option::is_none")]
        path: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        project_name: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        git_branch: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        workspace_kind: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        assistant_id: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        remote_connection_id: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        remote_ssh_host: Option<String>,
        sessions: Vec<SessionInfo>,
        has_more_sessions: bool,
        #[serde(skip_serializing_if = "Option::is_none")]
        authenticated_user_id: Option<String>,
    },
    SessionPoll {
        version: u64,
        changed: bool,
        #[serde(skip_serializing_if = "Option::is_none")]
        session_state: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        title: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        new_messages: Option<Vec<ChatMessage>>,
        #[serde(skip_serializing_if = "Option::is_none")]
        total_msg_count: Option<usize>,
        #[serde(skip_serializing_if = "Option::is_none")]
        active_turn: Option<ActiveTurnSnapshot>,
        #[serde(skip_serializing_if = "Option::is_none")]
        model_catalog: Box<Option<RemoteModelCatalog>>,
    },
    AnswerAccepted,
    InteractionAccepted {
        action: String,
        target_id: String,
    },
    FileContent {
        name: String,
        content_base64: String,
        mime_type: String,
        size: u64,
    },
    FileChunk {
        name: String,
        chunk_base64: String,
        offset: u64,
        chunk_size: u64,
        total_size: u64,
        mime_type: String,
    },
    FileInfo {
        name: String,
        size: u64,
        mime_type: String,
    },
    Pong,
    /// Device-to-device: a session was received and imported.
    SessionReceived {
        session_id: String,
    },
    /// Device-to-device: a command was accepted by the peer device.
    DeviceAccepted {
        message: String,
    },
    /// Device-to-device: info response from a peer device.
    DeviceInfo {
        device_name: Option<String>,
        workspace_path: Option<String>,
        session_count: Option<usize>,
    },
    /// Result of a HostInvoke proxy call (JSON-compatible with local invoke).
    HostInvokeResult {
        ok: bool,
        #[serde(skip_serializing_if = "Option::is_none")]
        value: Option<serde_json::Value>,
        #[serde(skip_serializing_if = "Option::is_none")]
        error: Option<String>,
    },
    /// Event already delivered out-of-band; ack only.
    DeviceEventAccepted,
    Error {
        message: String,
    },
}

#[async_trait::async_trait]
pub trait LanMonitorRuntimeHost: Send + Sync {
    async fn workspace_info(&self) -> Result<Option<RemoteWorkspaceFacts>, String>;
    async fn list_sessions(
        &self,
        limit: usize,
        offset: usize,
        query: Option<&str>,
    ) -> Result<(Vec<SessionInfo>, bool), String>;
    async fn transcript_page(
        &self,
        session_id: &str,
        limit: usize,
        before_turn_id: Option<&str>,
    ) -> Result<LanMonitorTranscriptPage, String>;
    async fn tool_result_chunk(
        &self,
        session_id: &str,
        turn_id: &str,
        tool_id: &str,
        result_ref: &str,
        cursor: usize,
        limit: Option<usize>,
    ) -> Result<LanMonitorToolResultChunk, String>;
    async fn poll_session(
        &self,
        session_id: &str,
        since_version: u64,
        known_turn_count: usize,
    ) -> Result<LanMonitorPollSnapshot, String>;
    async fn cancel_task(&self, session_id: &str, turn_id: Option<&str>) -> Result<(), String>;
    async fn confirm_tool(&self, session_id: &str, tool_id: &str) -> Result<(), String>;
    async fn reject_tool(
        &self,
        session_id: &str,
        tool_id: &str,
        reason: String,
    ) -> Result<(), String>;
}

fn normalize_lan_monitor_rejection_reason(reason: Option<&str>) -> String {
    let normalized = reason
        .unwrap_or("Rejected from LAN monitor")
        .chars()
        .filter(|character| !character.is_control() || *character == ' ')
        .take(240)
        .collect::<String>();
    let trimmed = normalized.trim();
    if trimmed.is_empty() {
        "Rejected from LAN monitor".to_string()
    } else {
        trimmed.to_string()
    }
}

pub async fn handle_lan_monitor_command<H>(host: &H, command: &LanMonitorCommand) -> RemoteResponse
where
    H: LanMonitorRuntimeHost + ?Sized,
{
    match command {
        LanMonitorCommand::GetWorkspaceInfo => match host.workspace_info().await {
            Ok(workspace) => RemoteResponse::LanMonitorWorkspace { workspace },
            Err(message) => RemoteResponse::Error { message },
        },
        LanMonitorCommand::ListSessions {
            limit,
            offset,
            query,
        } => {
            let limit = limit.unwrap_or(30).clamp(1, LAN_MONITOR_MAX_TURN_LIMIT);
            match host
                .list_sessions(limit, offset.unwrap_or(0), query.as_deref())
                .await
            {
                Ok((sessions, has_more)) => {
                    RemoteResponse::LanMonitorSessions { sessions, has_more }
                }
                Err(message) => RemoteResponse::Error { message },
            }
        }
        LanMonitorCommand::GetTranscriptPage {
            session_id,
            limit,
            before_turn_id,
        } => {
            let limit = limit
                .unwrap_or(LAN_MONITOR_DEFAULT_TURN_LIMIT)
                .clamp(1, LAN_MONITOR_MAX_TURN_LIMIT);
            match host
                .transcript_page(session_id, limit, before_turn_id.as_deref())
                .await
            {
                Ok(page) => RemoteResponse::LanMonitorTranscript { page },
                Err(message) => RemoteResponse::Error { message },
            }
        }
        LanMonitorCommand::GetToolResultChunk {
            session_id,
            turn_id,
            tool_id,
            result_ref,
            cursor,
            limit,
        } => match host
            .tool_result_chunk(session_id, turn_id, tool_id, result_ref, *cursor, *limit)
            .await
        {
            Ok(result) => RemoteResponse::LanMonitorToolResult { result },
            Err(message) => RemoteResponse::Error { message },
        },
        LanMonitorCommand::PollSession {
            session_id,
            since_version,
            known_turn_count,
        } => match host
            .poll_session(session_id, *since_version, *known_turn_count)
            .await
        {
            Ok(snapshot) => RemoteResponse::LanMonitorPoll { snapshot },
            Err(message) => RemoteResponse::Error { message },
        },
        LanMonitorCommand::CancelTask {
            session_id,
            turn_id,
        } => match host.cancel_task(session_id, turn_id.as_deref()).await {
            Ok(()) => RemoteResponse::LanMonitorActionAccepted {
                action: "cancel_task".to_string(),
                target_id: session_id.clone(),
            },
            Err(message) => RemoteResponse::Error { message },
        },
        LanMonitorCommand::ConfirmTool {
            session_id,
            tool_id,
        } => match host.confirm_tool(session_id, tool_id).await {
            Ok(()) => RemoteResponse::LanMonitorActionAccepted {
                action: "confirm_tool".to_string(),
                target_id: tool_id.clone(),
            },
            Err(message) => RemoteResponse::Error { message },
        },
        LanMonitorCommand::RejectTool {
            session_id,
            tool_id,
            reason,
        } => match host
            .reject_tool(
                session_id,
                tool_id,
                normalize_lan_monitor_rejection_reason(reason.as_deref()),
            )
            .await
        {
            Ok(()) => RemoteResponse::LanMonitorActionAccepted {
                action: "reject_tool".to_string(),
                target_id: tool_id.clone(),
            },
            Err(message) => RemoteResponse::Error { message },
        },
        LanMonitorCommand::Ping => RemoteResponse::Pong,
    }
}

/// Host callbacks required by full remote-connect command routing.
///
/// This owner crate decides how wire commands are grouped and translated into
/// remote responses. Product runtimes only provide concrete service adapters.
#[async_trait::async_trait]
pub trait RemoteCommandRuntimeHost: Send + Sync {
    type ImageContext: Send + Sync + 'static;

    async fn handle_workspace_command(&self, command: &RemoteCommand) -> RemoteResponse;
    async fn handle_session_command(&self, command: &RemoteCommand) -> RemoteResponse;
    async fn handle_poll_command(&self, command: &RemoteCommand) -> RemoteResponse;
    async fn handle_workspace_file_command(&self, command: &RemoteCommand) -> RemoteResponse;
    async fn handle_interaction_command(&self, command: &RemoteCommand) -> RemoteResponse;

    async fn handle_lan_monitor_command(&self, _command: &LanMonitorCommand) -> RemoteResponse {
        RemoteResponse::Error {
            message: "LAN monitor is not supported on this device".to_string(),
        }
    }

    /// Handle a device-to-device command arriving from a peer in the same
    /// account.  Default implementation returns an error so existing
    /// implementors don't break.
    async fn handle_device_command(&self, _command: &RemoteCommand) -> RemoteResponse {
        RemoteResponse::Error {
            message: "Device-to-device commands are not supported on this device".to_string(),
        }
    }

    async fn submit_dialog(
        &self,
        request: RemoteDialogSubmissionRequest<Self::ImageContext>,
    ) -> Result<RemoteDialogSubmitOutcome, String>;

    async fn cancel_task(&self, request: RemoteCancelTaskRequest) -> Result<(), String>;

    fn legacy_image_contexts(&self, images: Option<&[ImageAttachment]>) -> Vec<Self::ImageContext>;

    fn explicit_image_contexts(&self, contexts: Vec<RemoteImageContext>)
        -> Vec<Self::ImageContext>;
}

pub async fn handle_remote_command<H>(
    host: &H,
    command: &RemoteCommand,
    source: RemoteConnectSubmissionSource,
) -> RemoteResponse
where
    H: RemoteCommandRuntimeHost + ?Sized,
{
    if source == RemoteConnectSubmissionSource::LanMonitor {
        return match command {
            RemoteCommand::LanMonitor { request } => host.handle_lan_monitor_command(request).await,
            _ => RemoteResponse::Error {
                message: "Command is not allowed in LAN monitor mode".to_string(),
            },
        };
    }

    match command {
        RemoteCommand::LanMonitor { .. } => RemoteResponse::Error {
            message: "LAN monitor commands require a LAN monitor connection".to_string(),
        },
        RemoteCommand::Ping => RemoteResponse::Pong,

        RemoteCommand::GetWorkspaceInfo
        | RemoteCommand::ListRecentWorkspaces
        | RemoteCommand::SetWorkspace { .. }
        | RemoteCommand::ListAssistants
        | RemoteCommand::SetAssistant { .. } => host.handle_workspace_command(command).await,

        RemoteCommand::ListSessions { .. }
        | RemoteCommand::CreateSession { .. }
        | RemoteCommand::GetModelCatalog { .. }
        | RemoteCommand::SetSessionModel { .. }
        | RemoteCommand::UpdateSessionTitle { .. }
        | RemoteCommand::GetSessionMessages { .. }
        | RemoteCommand::DeleteSession { .. } => host.handle_session_command(command).await,

        RemoteCommand::PollSession { .. } => host.handle_poll_command(command).await,

        RemoteCommand::ReadFile { .. }
        | RemoteCommand::ReadFileChunk { .. }
        | RemoteCommand::GetFileInfo { .. } => host.handle_workspace_file_command(command).await,

        RemoteCommand::ConfirmTool { .. }
        | RemoteCommand::RejectTool { .. }
        | RemoteCommand::CancelTool { .. }
        | RemoteCommand::AnswerQuestion { .. } => host.handle_interaction_command(command).await,

        RemoteCommand::SendMessage {
            session_id,
            content,
            agent_type,
            images,
            image_contexts,
        } => {
            let resolved_contexts = resolve_remote_execution_image_contexts(
                images.as_ref().map(Vec::as_slice),
                image_contexts
                    .clone()
                    .map(|contexts| host.explicit_image_contexts(contexts)),
                |images| host.legacy_image_contexts(images),
            );
            info!(
                "Remote send_message: session={session_id}, agent_type={}, image_contexts={}",
                agent_type.as_deref().unwrap_or("agentic"),
                resolved_contexts.len()
            );
            remote_dialog_submit_response(
                host.submit_dialog(RemoteDialogSubmissionRequest {
                    session_id: session_id.clone(),
                    content: content.clone(),
                    agent_type: agent_type.clone(),
                    image_contexts: resolved_contexts,
                    policy: RemoteDialogSubmissionPolicy::for_source(source),
                    turn_id: None,
                })
                .await,
            )
        }

        RemoteCommand::CancelTask {
            session_id,
            turn_id,
        } => remote_task_cancel_response(
            session_id.clone(),
            host.cancel_task(RemoteCancelTaskRequest {
                session_id: session_id.clone(),
                requested_turn_id: turn_id.clone(),
            })
            .await,
        ),

        RemoteCommand::SendSessionToDevice { .. }
        | RemoteCommand::ExecuteOnDevice { .. }
        | RemoteCommand::DeviceQueryInfo
        | RemoteCommand::CreateWorkspace { .. }
        | RemoteCommand::HostInvoke { .. }
        | RemoteCommand::DeviceEvent { .. } => host.handle_device_command(command).await,
    }
}

/// Build a slim version of tool params for remote preview payloads.
///
/// Large string values such as file contents and diffs are omitted, while
/// short structured fields stay available for remote clients that need to
/// render tool details.
pub fn make_slim_tool_params(params: &serde_json::Value) -> Option<String> {
    match params {
        serde_json::Value::Object(obj) => {
            let slim: serde_json::Map<String, serde_json::Value> = obj
                .iter()
                .filter_map(|(key, value)| match value {
                    serde_json::Value::String(text) if text.len() > 200 => None,
                    _ => Some((key.clone(), value.clone())),
                })
                .collect();
            if slim.is_empty() {
                return None;
            }
            serde_json::to_string(&serde_json::Value::Object(slim)).ok()
        }
        serde_json::Value::String(text) => Some(text.chars().take(200).collect()),
        _ => None,
    }
}

#[derive(Debug)]
struct TrackerState {
    session_state: String,
    title: String,
    turn_id: Option<String>,
    turn_status: String,
    accumulated_text: String,
    accumulated_thinking: String,
    active_tools: Vec<RemoteToolStatus>,
    round_index: usize,
    active_items: Vec<ChatMessageItem>,
    persistence_dirty: bool,
    linked_subagent_sessions: HashMap<String, String>,
}

/// Lightweight event broadcast by the tracker for real-time consumers.
#[derive(Debug, Clone, PartialEq)]
pub enum TrackerEvent {
    TextChunk(String),
    ThinkingChunk(String),
    ThinkingEnd,
    ToolStarted {
        tool_id: String,
        tool_name: String,
        params: Option<serde_json::Value>,
    },
    ToolCompleted {
        tool_id: String,
        tool_name: String,
        duration_ms: Option<u64>,
        success: bool,
    },
    TurnCompleted {
        turn_id: String,
    },
    TurnFailed {
        turn_id: String,
        error: String,
    },
    TurnCancelled {
        turn_id: String,
    },
}

/// Tracks the real-time state of a session for remote polling and bot streams.
pub struct RemoteSessionStateTracker {
    target_session_id: String,
    version: AtomicU64,
    state: RwLock<TrackerState>,
    event_tx: tokio::sync::broadcast::Sender<TrackerEvent>,
}

impl RemoteSessionStateTracker {
    pub fn new(session_id: String) -> Self {
        let (event_tx, _) = tokio::sync::broadcast::channel(1024);
        Self {
            target_session_id: session_id,
            version: AtomicU64::new(0),
            state: RwLock::new(TrackerState {
                session_state: "idle".to_string(),
                title: String::new(),
                turn_id: None,
                turn_status: String::new(),
                accumulated_text: String::new(),
                accumulated_thinking: String::new(),
                active_tools: Vec::new(),
                round_index: 0,
                active_items: Vec::new(),
                persistence_dirty: true,
                linked_subagent_sessions: HashMap::new(),
            }),
            event_tx,
        }
    }

    pub fn subscribe(&self) -> tokio::sync::broadcast::Receiver<TrackerEvent> {
        self.event_tx.subscribe()
    }

    pub fn version(&self) -> u64 {
        self.version.load(Ordering::Relaxed)
    }

    fn bump_version(&self) {
        self.version.fetch_add(1, Ordering::Relaxed);
    }

    pub fn snapshot_active_turn(&self) -> Option<ActiveTurnSnapshot> {
        let state = self.state.read().unwrap();
        let has_items = !state.active_items.is_empty();
        state.turn_id.as_ref().map(|turn_id| ActiveTurnSnapshot {
            turn_id: turn_id.clone(),
            status: state.turn_status.clone(),
            text: if has_items {
                String::new()
            } else {
                state.accumulated_text.clone()
            },
            thinking: if has_items {
                String::new()
            } else {
                state.accumulated_thinking.clone()
            },
            tools: state.active_tools.clone(),
            round_index: state.round_index,
            items: if has_items {
                Some(state.active_items.clone())
            } else {
                None
            },
        })
    }

    pub fn session_state(&self) -> String {
        self.state.read().unwrap().session_state.clone()
    }

    pub fn title(&self) -> String {
        self.state.read().unwrap().title.clone()
    }

    pub fn turn_status(&self) -> String {
        self.state.read().unwrap().turn_status.clone()
    }

    pub fn accumulated_text(&self) -> String {
        self.state.read().unwrap().accumulated_text.clone()
    }

    pub fn accumulated_thinking(&self) -> String {
        self.state.read().unwrap().accumulated_thinking.clone()
    }

    pub fn is_turn_finished(&self) -> bool {
        let state = self.state.read().unwrap();
        state.turn_id.is_some()
            && matches!(
                state.turn_status.as_str(),
                "completed" | "failed" | "cancelled"
            )
    }

    pub fn initialize_active_turn(&self, turn_id: String) {
        let mut state = self.state.write().unwrap();
        if state.turn_id.is_none() {
            state.turn_id = Some(turn_id);
            state.turn_status = "active".to_string();
            state.session_state = "running".to_string();
        }
        drop(state);
        self.bump_version();
    }

    pub fn finalize_completed_turn(&self) {
        let mut state = self.state.write().unwrap();
        if matches!(
            state.turn_status.as_str(),
            "completed" | "failed" | "cancelled"
        ) {
            state.turn_id = None;
            state.accumulated_text.clear();
            state.accumulated_thinking.clear();
            state.active_tools.clear();
            state.active_items.clear();
        }
    }

    pub fn is_persistence_dirty(&self) -> bool {
        self.state.read().unwrap().persistence_dirty
    }

    pub fn mark_persistence_clean(&self) {
        self.state.write().unwrap().persistence_dirty = false;
    }

    fn find_mergeable_item(
        items: &[ChatMessageItem],
        target_type: &str,
        subagent_marker: &Option<bool>,
    ) -> Option<usize> {
        for index in (0..items.len()).rev() {
            let item = &items[index];
            if item.item_type == "tool" {
                return None;
            }
            if item.item_type == target_type && &item.is_subagent == subagent_marker {
                return Some(index);
            }
        }
        None
    }

    fn upsert_active_tool(
        state: &mut TrackerState,
        tool_id: &str,
        tool_name: &str,
        status: &str,
        input_preview: Option<String>,
        tool_input: Option<serde_json::Value>,
        monitor_input: Option<serde_json::Value>,
        is_subagent: bool,
    ) {
        let resolved_id = if tool_id.is_empty() {
            format!("{}-{}", tool_name, state.active_tools.len())
        } else {
            tool_id.to_string()
        };
        let allow_name_fallback = tool_id.is_empty() && !tool_name.is_empty();
        let subagent_marker = if is_subagent { Some(true) } else { None };

        if let Some(tool) =
            state.active_tools.iter_mut().rev().find(|tool| {
                tool.id == resolved_id || (allow_name_fallback && tool.name == tool_name)
            })
        {
            tool.status = status.to_string();
            if input_preview.is_some() {
                tool.input_preview = input_preview.clone();
            }
            if tool_input.is_some() {
                tool.tool_input = tool_input.clone();
            }
            if monitor_input.is_some() {
                tool.monitor_input = monitor_input.clone();
            }
        } else {
            let tool_status = RemoteToolStatus {
                id: resolved_id.clone(),
                name: tool_name.to_string(),
                status: status.to_string(),
                duration_ms: None,
                start_ms: Some(
                    std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap_or_default()
                        .as_millis() as u64,
                ),
                input_preview,
                tool_input,
                monitor_input,
            };
            state.active_tools.push(tool_status.clone());
            state.active_items.push(ChatMessageItem {
                item_type: "tool".to_string(),
                content: None,
                tool: Some(tool_status),
                is_subagent: subagent_marker,
            });
            return;
        }

        if let Some(item) = state.active_items.iter_mut().rev().find(|item| {
            item.item_type == "tool"
                && item.tool.as_ref().is_some_and(|tool| {
                    tool.id == resolved_id || (allow_name_fallback && tool.name == tool_name)
                })
        }) {
            if let Some(tool) = item.tool.as_mut() {
                tool.status = status.to_string();
                if input_preview.is_some() {
                    tool.input_preview = input_preview;
                }
                if tool_input.is_some() {
                    tool.tool_input = tool_input;
                }
                if monitor_input.is_some() {
                    tool.monitor_input = monitor_input;
                }
            }
        }
    }

    pub fn handle_agentic_event(&self, event: &AgenticEvent) {
        use bitfun_events::AgenticEvent as AE;

        if let AE::SubagentSessionLinked {
            session_id,
            parent_session_id,
            ..
        } = event
        {
            if parent_session_id != &self.target_session_id {
                return;
            }

            let mut state = self.state.write().unwrap();
            state
                .linked_subagent_sessions
                .insert(session_id.clone(), parent_session_id.clone());
            drop(state);
            self.bump_version();
            return;
        }

        let is_direct = event.session_id() == Some(self.target_session_id.as_str());
        let is_subagent = if !is_direct {
            match event {
                AE::TextChunk { session_id, .. }
                | AE::ThinkingChunk { session_id, .. }
                | AE::ToolEvent { session_id, .. } => self
                    .state
                    .read()
                    .unwrap()
                    .linked_subagent_sessions
                    .get(session_id)
                    .is_some_and(|parent_session_id| parent_session_id == &self.target_session_id),
                _ => false,
            }
        } else {
            false
        };

        if !is_direct && !is_subagent {
            return;
        }

        match event {
            AE::TextChunk { text, .. } => {
                let subagent_marker = if is_subagent { Some(true) } else { None };
                let mut state = self.state.write().unwrap();
                if !is_subagent {
                    state.accumulated_text.push_str(text);
                }
                if let Some(index) =
                    Self::find_mergeable_item(&state.active_items, "text", &subagent_marker)
                {
                    let item = &mut state.active_items[index];
                    item.content.get_or_insert_with(String::new).push_str(text);
                } else {
                    state.active_items.push(ChatMessageItem {
                        item_type: "text".to_string(),
                        content: Some(text.clone()),
                        tool: None,
                        is_subagent: subagent_marker,
                    });
                }
                drop(state);
                self.bump_version();
                let _ = self.event_tx.send(TrackerEvent::TextChunk(text.clone()));
            }
            AE::ThinkingChunk {
                content, is_end, ..
            } => {
                let clean = content.replace("</thinking>", "").replace("<thinking>", "");
                let subagent_marker = if is_subagent { Some(true) } else { None };
                let mut state = self.state.write().unwrap();
                if !is_subagent {
                    state.accumulated_thinking.push_str(&clean);
                }
                if let Some(index) =
                    Self::find_mergeable_item(&state.active_items, "thinking", &subagent_marker)
                {
                    let item = &mut state.active_items[index];
                    item.content
                        .get_or_insert_with(String::new)
                        .push_str(&clean);
                } else {
                    state.active_items.push(ChatMessageItem {
                        item_type: "thinking".to_string(),
                        content: Some(clean),
                        tool: None,
                        is_subagent: subagent_marker,
                    });
                }
                drop(state);
                self.bump_version();
                if *is_end {
                    let _ = self.event_tx.send(TrackerEvent::ThinkingEnd);
                } else if !content.is_empty() {
                    let _ = self
                        .event_tx
                        .send(TrackerEvent::ThinkingChunk(content.clone()));
                }
            }
            AE::ToolEvent { tool_event, .. } => {
                if let Ok(value) = serde_json::to_value(tool_event) {
                    let event_type = value
                        .get("event_type")
                        .and_then(|value| value.as_str())
                        .unwrap_or("");
                    let tool_id = value
                        .get("tool_id")
                        .and_then(|value| value.as_str())
                        .unwrap_or("")
                        .to_string();
                    let tool_name = value
                        .get("tool_name")
                        .and_then(|value| value.as_str())
                        .unwrap_or("")
                        .to_string();

                    let mut state = self.state.write().unwrap();
                    let allow_name_fallback = tool_id.is_empty() && !tool_name.is_empty();
                    let mut pending_tool_event: Option<TrackerEvent> = None;
                    match event_type {
                        "EarlyDetected" => {
                            Self::upsert_active_tool(
                                &mut state,
                                &tool_id,
                                &tool_name,
                                "preparing",
                                None,
                                None,
                                None,
                                is_subagent,
                            );
                        }
                        "ConfirmationNeeded" => {
                            let params = value.get("params").cloned();
                            let input_preview = params.as_ref().and_then(make_slim_tool_params);
                            Self::upsert_active_tool(
                                &mut state,
                                &tool_id,
                                &tool_name,
                                "pending_confirmation",
                                input_preview,
                                params.clone(),
                                params,
                                is_subagent,
                            );
                        }
                        "Started" => {
                            let params = value.get("params").cloned();
                            let input_preview = params.as_ref().and_then(make_slim_tool_params);
                            let tool_input = if tool_name == "AskUserQuestion"
                                || tool_name == "Task"
                                || tool_name == "TodoWrite"
                            {
                                params.clone()
                            } else {
                                None
                            };
                            Self::upsert_active_tool(
                                &mut state,
                                &tool_id,
                                &tool_name,
                                "running",
                                input_preview,
                                tool_input,
                                params.clone(),
                                is_subagent,
                            );
                            let _ = self.event_tx.send(TrackerEvent::ToolStarted {
                                tool_id: tool_id.clone(),
                                tool_name: tool_name.clone(),
                                params,
                            });
                        }
                        "Confirmed" => {
                            Self::upsert_active_tool(
                                &mut state,
                                &tool_id,
                                &tool_name,
                                "confirmed",
                                None,
                                None,
                                None,
                                is_subagent,
                            );
                        }
                        "Rejected" => {
                            Self::upsert_active_tool(
                                &mut state,
                                &tool_id,
                                &tool_name,
                                "rejected",
                                None,
                                None,
                                None,
                                is_subagent,
                            );
                        }
                        "Completed" | "Succeeded" => {
                            let duration =
                                value.get("duration_ms").and_then(|value| value.as_u64());
                            if let Some(tool) = state.active_tools.iter_mut().rev().find(|tool| {
                                (tool.id == tool_id
                                    || (allow_name_fallback && tool.name == tool_name))
                                    && tool.status == "running"
                            }) {
                                tool.status = "completed".to_string();
                                tool.duration_ms = duration;
                            }
                            if let Some(item) = state.active_items.iter_mut().rev().find(|item| {
                                item.item_type == "tool"
                                    && item.tool.as_ref().is_some_and(|tool| {
                                        (tool.id == tool_id
                                            || (allow_name_fallback && tool.name == tool_name))
                                            && tool.status == "running"
                                    })
                            }) {
                                if let Some(tool) = item.tool.as_mut() {
                                    tool.status = "completed".to_string();
                                    tool.duration_ms = duration;
                                }
                            }
                            pending_tool_event = Some(TrackerEvent::ToolCompleted {
                                tool_id: tool_id.clone(),
                                tool_name: tool_name.clone(),
                                duration_ms: duration,
                                success: true,
                            });
                            state.persistence_dirty = true;
                        }
                        "Failed" => {
                            if let Some(tool) = state.active_tools.iter_mut().rev().find(|tool| {
                                (tool.id == tool_id
                                    || (allow_name_fallback && tool.name == tool_name))
                                    && tool.status == "running"
                            }) {
                                tool.status = "failed".to_string();
                            }
                            if let Some(item) = state.active_items.iter_mut().rev().find(|item| {
                                item.item_type == "tool"
                                    && item.tool.as_ref().is_some_and(|tool| {
                                        (tool.id == tool_id
                                            || (allow_name_fallback && tool.name == tool_name))
                                            && tool.status == "running"
                                    })
                            }) {
                                if let Some(tool) = item.tool.as_mut() {
                                    tool.status = "failed".to_string();
                                }
                            }
                            pending_tool_event = Some(TrackerEvent::ToolCompleted {
                                tool_id: tool_id.clone(),
                                tool_name: tool_name.clone(),
                                duration_ms: None,
                                success: false,
                            });
                            state.persistence_dirty = true;
                        }
                        "Cancelled" => {
                            if let Some(tool) = state.active_tools.iter_mut().rev().find(|tool| {
                                (tool.id == tool_id
                                    || (allow_name_fallback && tool.name == tool_name))
                                    && matches!(
                                        tool.status.as_str(),
                                        "running" | "pending_confirmation" | "confirmed"
                                    )
                            }) {
                                tool.status = "cancelled".to_string();
                            }
                            if let Some(item) = state.active_items.iter_mut().rev().find(|item| {
                                item.item_type == "tool"
                                    && item.tool.as_ref().is_some_and(|tool| {
                                        (tool.id == tool_id
                                            || (allow_name_fallback && tool.name == tool_name))
                                            && matches!(
                                                tool.status.as_str(),
                                                "running" | "pending_confirmation" | "confirmed"
                                            )
                                    })
                            }) {
                                if let Some(tool) = item.tool.as_mut() {
                                    tool.status = "cancelled".to_string();
                                }
                            }
                        }
                        _ => {}
                    }
                    drop(state);
                    self.bump_version();
                    if let Some(event) = pending_tool_event {
                        let _ = self.event_tx.send(event);
                    }
                }
            }
            AE::DialogTurnStarted { turn_id, .. } if is_direct => {
                let mut state = self.state.write().unwrap();
                state.turn_id = Some(turn_id.clone());
                state.turn_status = "active".to_string();
                state.accumulated_text.clear();
                state.accumulated_thinking.clear();
                state.active_tools.clear();
                state.active_items.clear();
                state.round_index = 0;
                state.session_state = "running".to_string();
                state.persistence_dirty = true;
                drop(state);
                self.bump_version();
            }
            AE::DialogTurnCompleted { turn_id, .. } if is_direct => {
                let mut state = self.state.write().unwrap();
                state.turn_status = "completed".to_string();
                state.session_state = "idle".to_string();
                state.persistence_dirty = true;
                drop(state);
                self.bump_version();
                let _ = self.event_tx.send(TrackerEvent::TurnCompleted {
                    turn_id: turn_id.clone(),
                });
            }
            AE::DialogTurnFailed { turn_id, error, .. } if is_direct => {
                let mut state = self.state.write().unwrap();
                state.turn_status = "failed".to_string();
                state.session_state = "idle".to_string();
                state.persistence_dirty = true;
                drop(state);
                self.bump_version();
                let _ = self.event_tx.send(TrackerEvent::TurnFailed {
                    turn_id: turn_id.clone(),
                    error: error.clone(),
                });
            }
            AE::DialogTurnCancelled { turn_id, .. } if is_direct => {
                let mut state = self.state.write().unwrap();
                state.turn_status = "cancelled".to_string();
                state.session_state = "idle".to_string();
                state.persistence_dirty = true;
                drop(state);
                self.bump_version();
                let _ = self.event_tx.send(TrackerEvent::TurnCancelled {
                    turn_id: turn_id.clone(),
                });
            }
            AE::ModelRoundStarted { round_index, .. } if is_direct => {
                let mut state = self.state.write().unwrap();
                state.round_index = *round_index;
                drop(state);
                self.bump_version();
            }
            AE::SessionStateChanged { new_state, .. } if is_direct => {
                let mut state = self.state.write().unwrap();
                state.session_state = new_state.clone();
                drop(state);
                self.bump_version();
            }
            AE::SessionTitleGenerated { title, .. } if is_direct => {
                let mut state = self.state.write().unwrap();
                state.title = title.clone();
                drop(state);
                self.bump_version();
            }
            _ => {}
        }
    }
}

/// Host callbacks required to bind tracker lifecycle to the owning product runtime.
///
/// `services-integrations` owns tracker storage and lifecycle shape, while core
/// remains responsible for subscribing to concrete agent events and reading
/// in-memory active-turn state.
pub trait RemoteSessionTrackerHost {
    fn subscribe_tracker(&self, session_id: &str, tracker: Arc<RemoteSessionStateTracker>);
    fn unsubscribe_tracker(&self, session_id: &str);
    fn active_turn_id(&self, session_id: &str) -> Option<String>;
}

#[derive(Default)]
pub struct RemoteSessionTrackerRegistry {
    state_trackers: RwLock<HashMap<String, Arc<RemoteSessionStateTracker>>>,
}

impl RemoteSessionTrackerRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn ensure_tracker_with_host<H: RemoteSessionTrackerHost>(
        &self,
        session_id: &str,
        host: &H,
    ) -> Arc<RemoteSessionStateTracker> {
        if let Some(tracker) = self.get_tracker(session_id) {
            return tracker;
        }

        let tracker = {
            let mut trackers = self.state_trackers.write().unwrap();
            if let Some(tracker) = trackers.get(session_id) {
                return tracker.clone();
            }
            let tracker = Arc::new(RemoteSessionStateTracker::new(session_id.to_string()));
            trackers.insert(session_id.to_string(), tracker.clone());
            tracker
        };

        host.subscribe_tracker(session_id, tracker.clone());
        if let Some(active_turn_id) = host.active_turn_id(session_id) {
            tracker.initialize_active_turn(active_turn_id);
        }

        tracker
    }

    pub fn get_tracker(&self, session_id: &str) -> Option<Arc<RemoteSessionStateTracker>> {
        self.state_trackers.read().unwrap().get(session_id).cloned()
    }

    pub fn remove_tracker_with_host<H: RemoteSessionTrackerHost>(
        &self,
        session_id: &str,
        host: &H,
    ) -> Option<Arc<RemoteSessionStateTracker>> {
        let removed = self.state_trackers.write().unwrap().remove(session_id);
        if removed.is_some() {
            host.unsubscribe_tracker(session_id);
        }
        removed
    }
}

pub fn should_send_remote_model_catalog(
    current_model_catalog: Option<&RemoteModelCatalog>,
    known_model_catalog_version: Option<u64>,
) -> bool {
    let current_version = current_model_catalog
        .map(|catalog| catalog.version)
        .unwrap_or(0);
    known_model_catalog_version.unwrap_or(0) != current_version
}

pub fn remote_model_catalog_poll_delta(
    current_model_catalog: Option<RemoteModelCatalog>,
    known_model_catalog_version: Option<u64>,
) -> RemoteModelCatalogPollDelta {
    let changed = should_send_remote_model_catalog(
        current_model_catalog.as_ref(),
        known_model_catalog_version,
    );
    let catalog = if changed { current_model_catalog } else { None };

    RemoteModelCatalogPollDelta { changed, catalog }
}

pub fn remote_no_change_poll_response(version: u64) -> RemoteResponse {
    RemoteResponse::SessionPoll {
        version,
        changed: false,
        session_state: None,
        title: None,
        new_messages: None,
        total_msg_count: None,
        active_turn: None,
        model_catalog: Box::new(None),
    }
}

pub fn remote_snapshot_poll_response(
    tracker: &RemoteSessionStateTracker,
    version: u64,
    model_catalog: Option<RemoteModelCatalog>,
) -> RemoteResponse {
    let active_turn = tracker.snapshot_active_turn();
    let session_state = tracker.session_state();
    let title = tracker.title();
    RemoteResponse::SessionPoll {
        version,
        changed: true,
        session_state: Some(session_state),
        title: non_empty_title(title),
        new_messages: None,
        total_msg_count: None,
        active_turn,
        model_catalog: Box::new(model_catalog),
    }
}

pub fn remote_persisted_poll_response(
    tracker: &RemoteSessionStateTracker,
    version: u64,
    new_messages: Vec<ChatMessage>,
    total_msg_count: usize,
    model_catalog: Option<RemoteModelCatalog>,
) -> RemoteResponse {
    let turn_finished = tracker.is_turn_finished();
    let has_assistant_msg = new_messages
        .iter()
        .any(|message| message.role == "assistant");

    let active_turn = if turn_finished && has_assistant_msg {
        tracker.finalize_completed_turn();
        None
    } else if turn_finished {
        let status = tracker.turn_status();
        if status == "completed" {
            tracker.snapshot_active_turn()
        } else {
            tracker.finalize_completed_turn();
            tracker.mark_persistence_clean();
            None
        }
    } else {
        tracker.snapshot_active_turn()
    };

    let (send_messages, send_total) = if turn_finished && !has_assistant_msg {
        (None, None)
    } else {
        if !new_messages.is_empty() {
            tracker.mark_persistence_clean();
        }
        (Some(new_messages), Some(total_msg_count))
    };

    let session_state = tracker.session_state();
    let title = tracker.title();
    RemoteResponse::SessionPoll {
        version,
        changed: true,
        session_state: Some(session_state),
        title: non_empty_title(title),
        new_messages: send_messages,
        total_msg_count: send_total,
        active_turn,
        model_catalog: Box::new(model_catalog),
    }
}

fn non_empty_title(title: String) -> Option<String> {
    if title.is_empty() {
        None
    } else {
        Some(title)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    struct FakeWorkspaceHost;

    #[async_trait::async_trait]
    impl RemoteWorkspaceRuntimeHost for FakeWorkspaceHost {
        async fn current_workspace(&self) -> Option<RemoteWorkspaceFacts> {
            Some(RemoteWorkspaceFacts {
                path: "/workspace/project".to_string(),
                name: "project".to_string(),
                git_branch: Some("main".to_string()),
                kind: RemoteWorkspaceKind::Normal,
                assistant_id: None,
                remote_connection_id: None,
                remote_ssh_host: None,
            })
        }

        async fn recent_workspaces(&self) -> Vec<RemoteRecentWorkspaceFacts> {
            vec![RemoteRecentWorkspaceFacts {
                path: "/workspace/project".to_string(),
                name: "project".to_string(),
                last_opened: "2026-05-29T00:00:00Z".to_string(),
                kind: RemoteWorkspaceKind::Normal,
            }]
        }

        async fn open_workspace(&self, path: &str) -> Result<RemoteWorkspaceUpdate, String> {
            Ok(RemoteWorkspaceUpdate {
                path: path.to_string(),
                name: "opened".to_string(),
            })
        }

        async fn assistant_workspaces(&self) -> Vec<RemoteAssistantWorkspaceFacts> {
            vec![RemoteAssistantWorkspaceFacts {
                path: "/workspace/assistant".to_string(),
                name: "assistant".to_string(),
                assistant_id: None,
            }]
        }

        async fn open_assistant_workspace(
            &self,
            path: &str,
        ) -> Result<RemoteWorkspaceUpdate, String> {
            Ok(RemoteWorkspaceUpdate {
                path: path.to_string(),
                name: "assistant".to_string(),
            })
        }
    }

    #[tokio::test]
    async fn remote_workspace_handler_preserves_response_shapes() {
        let host = FakeWorkspaceHost;

        assert_eq!(
            handle_remote_workspace_command(&host, &RemoteCommand::GetWorkspaceInfo).await,
            RemoteResponse::WorkspaceInfo {
                has_workspace: true,
                path: Some("/workspace/project".to_string()),
                project_name: Some("project".to_string()),
                git_branch: Some("main".to_string()),
                workspace_kind: Some("normal".to_string()),
                assistant_id: None,
                remote_connection_id: None,
                remote_ssh_host: None,
            }
        );

        assert_eq!(
            handle_remote_workspace_command(
                &host,
                &RemoteCommand::SetWorkspace {
                    path: "/workspace/next".to_string(),
                },
            )
            .await,
            RemoteResponse::WorkspaceUpdated {
                success: true,
                path: Some("/workspace/next".to_string()),
                project_name: Some("opened".to_string()),
                error: None,
            }
        );
    }

    #[derive(Default)]
    struct FakeSessionHost {
        created_requests: Mutex<Vec<AgentSessionCreateRequest>>,
        list_identities: Mutex<Vec<RemoteSessionWorkspaceIdentity>>,
        removed_trackers: Mutex<Vec<String>>,
    }

    #[async_trait::async_trait]
    impl RemoteSessionRuntimeHost for FakeSessionHost {
        async fn list_session_metadata(
            &self,
            _workspace_path: &Path,
            workspace_identity: RemoteSessionWorkspaceIdentity,
        ) -> Result<Vec<RemoteSessionMetadata>, String> {
            self.list_identities
                .lock()
                .unwrap()
                .push(workspace_identity);
            Ok(vec![
                RemoteSessionMetadata {
                    session_id: "session-a".to_string(),
                    name: "keep me".to_string(),
                    agent_type: "agentic".to_string(),
                    created_at_ms: 1_000,
                    last_active_at_ms: 2_000,
                    turn_count: 3,
                },
                RemoteSessionMetadata {
                    session_id: "session-b".to_string(),
                    name: "other".to_string(),
                    agent_type: "agentic".to_string(),
                    created_at_ms: 1_000,
                    last_active_at_ms: 2_000,
                    turn_count: 1,
                },
            ])
        }

        async fn resolve_default_assistant_workspace_path(&self) -> Result<String, String> {
            Ok("/workspace/assistant".to_string())
        }

        async fn create_session(
            &self,
            request: AgentSessionCreateRequest,
        ) -> Result<String, String> {
            self.created_requests.lock().unwrap().push(request);
            Ok("created-session".to_string())
        }

        async fn load_model_catalog(
            &self,
            _session_id: Option<&str>,
        ) -> Result<RemoteModelCatalog, String> {
            Ok(RemoteModelCatalog {
                version: 1,
                models: Vec::new(),
                default_models: RemoteDefaultModelsConfig::default(),
                session_model_id: None,
            })
        }

        async fn update_session_model(
            &self,
            _session_id: &str,
            model_id: &str,
        ) -> Result<String, String> {
            Ok(model_id.to_string())
        }

        async fn ensure_session_loaded(&self, _session_id: &str) -> Result<(), String> {
            Ok(())
        }

        async fn update_session_title(
            &self,
            _session_id: &str,
            title: &str,
        ) -> Result<String, String> {
            Ok(title.trim().to_string())
        }

        async fn resolve_session_storage_dir(&self, _session_id: &str) -> Option<PathBuf> {
            Some(PathBuf::from("/workspace/project"))
        }

        async fn load_remote_chat_messages(
            &self,
            _session_storage_dir: &Path,
            _session_id: &str,
        ) -> (Vec<ChatMessage>, bool) {
            (
                vec![ChatMessage {
                    id: "message-1".to_string(),
                    role: "user".to_string(),
                    content: "hello".to_string(),
                    timestamp: "1".to_string(),
                    metadata: None,
                    images: None,
                    thinking: None,
                    tools: None,
                    items: None,
                }],
                false,
            )
        }

        async fn delete_session(
            &self,
            _session_storage_dir: &Path,
            _session_id: &str,
        ) -> Result<(), String> {
            Ok(())
        }

        fn remove_tracker(&self, session_id: &str) {
            self.removed_trackers
                .lock()
                .unwrap()
                .push(session_id.to_string());
        }
    }

    #[tokio::test]
    async fn remote_session_handler_preserves_list_and_create_policy() {
        let host = FakeSessionHost::default();

        let list = handle_remote_session_command(
            &host,
            &RemoteCommand::ListSessions {
                workspace_path: Some("/workspace/project".to_string()),
                remote_connection_id: Some("conn-1".to_string()),
                remote_ssh_host: Some("host-1".to_string()),
                limit: Some(20),
                offset: Some(0),
                query: Some("keep".to_string()),
            },
        )
        .await;
        let RemoteResponse::SessionList { sessions, has_more } = list else {
            panic!("expected session list");
        };
        assert!(!has_more);
        assert_eq!(sessions.len(), 1);
        assert_eq!(sessions[0].session_id, "session-a");
        assert_eq!(
            sessions[0].workspace_path.as_deref(),
            Some("/workspace/project")
        );
        {
            let list_identities = host.list_identities.lock().unwrap();
            assert_eq!(
                list_identities[0].remote_connection_id.as_deref(),
                Some("conn-1")
            );
            assert_eq!(
                list_identities[0].remote_ssh_host.as_deref(),
                Some("host-1")
            );
        }

        let created = handle_remote_session_command(
            &host,
            &RemoteCommand::CreateSession {
                agent_type: Some("Cowork".to_string()),
                session_name: None,
                workspace_path: Some("/workspace/project".to_string()),
                remote_connection_id: Some("conn-1".to_string()),
                remote_ssh_host: Some("host-1".to_string()),
            },
        )
        .await;
        assert_eq!(
            created,
            RemoteResponse::SessionCreated {
                session_id: "created-session".to_string(),
            }
        );
        let created_requests = host.created_requests.lock().unwrap();
        assert_eq!(created_requests[0].session_name, "Remote Cowork Session");
        assert_eq!(created_requests[0].agent_type, "Cowork");
        assert_eq!(
            created_requests[0].workspace_path.as_deref(),
            Some("/workspace/project")
        );
        assert_eq!(
            created_requests[0].remote_connection_id.as_deref(),
            Some("conn-1")
        );
        assert_eq!(
            created_requests[0].remote_ssh_host.as_deref(),
            Some("host-1")
        );
    }

    #[tokio::test]
    async fn remote_session_handler_removes_tracker_after_delete_success() {
        let host = FakeSessionHost::default();

        let deleted = handle_remote_session_command(
            &host,
            &RemoteCommand::DeleteSession {
                session_id: "session-a".to_string(),
            },
        )
        .await;

        assert_eq!(
            deleted,
            RemoteResponse::SessionDeleted {
                session_id: "session-a".to_string(),
            }
        );
        assert_eq!(
            host.removed_trackers.lock().unwrap().as_slice(),
            ["session-a"]
        );
    }

    struct FakePollHost {
        tracker: Arc<RemoteSessionStateTracker>,
    }

    #[async_trait::async_trait]
    impl RemotePollRuntimeHost for FakePollHost {
        fn ensure_tracker(&self, _session_id: &str) -> Arc<RemoteSessionStateTracker> {
            self.tracker.clone()
        }

        async fn load_model_catalog(&self, _session_id: &str) -> Option<RemoteModelCatalog> {
            None
        }

        async fn resolve_session_storage_dir(&self, _session_id: &str) -> Option<PathBuf> {
            None
        }

        async fn load_remote_chat_messages(
            &self,
            _session_storage_dir: &Path,
            _session_id: &str,
        ) -> (Vec<ChatMessage>, bool) {
            (Vec::new(), false)
        }
    }

    #[tokio::test]
    async fn remote_poll_handler_preserves_missing_workspace_error() {
        let host = FakePollHost {
            tracker: Arc::new(RemoteSessionStateTracker::new("session-a".to_string())),
        };

        let response = handle_remote_poll_command(
            &host,
            &RemoteCommand::PollSession {
                session_id: "session-a".to_string(),
                since_version: 0,
                known_msg_count: 0,
                known_model_catalog_version: None,
            },
        )
        .await;

        assert_eq!(
            response,
            RemoteResponse::Error {
                message: "Session storage directory not available for session: session-a"
                    .to_string(),
            }
        );
    }

    #[derive(Default)]
    struct FakeInteractionHost {
        rejected: Mutex<Vec<(String, String)>>,
    }

    #[async_trait::async_trait]
    impl RemoteInteractionRuntimeHost for FakeInteractionHost {
        async fn confirm_tool(
            &self,
            _tool_id: &str,
            _updated_input: Option<serde_json::Value>,
        ) -> Result<(), String> {
            Ok(())
        }

        async fn reject_tool(&self, tool_id: &str, reason: String) -> Result<(), String> {
            self.rejected
                .lock()
                .unwrap()
                .push((tool_id.to_string(), reason));
            Ok(())
        }

        async fn cancel_tool(&self, _tool_id: &str, _reason: String) -> Result<(), String> {
            Ok(())
        }

        fn answer_question(
            &self,
            _tool_id: &str,
            _answers: serde_json::Value,
        ) -> Result<(), String> {
            Ok(())
        }
    }

    #[tokio::test]
    async fn remote_interaction_handler_preserves_default_reject_reason() {
        let host = FakeInteractionHost::default();

        let response = handle_remote_interaction_command(
            &host,
            &RemoteCommand::RejectTool {
                tool_id: "tool-1".to_string(),
                reason: None,
            },
        )
        .await;

        assert_eq!(
            response,
            RemoteResponse::InteractionAccepted {
                action: "reject_tool".to_string(),
                target_id: "tool-1".to_string(),
            }
        );
        assert_eq!(
            host.rejected.lock().unwrap().as_slice(),
            [("tool-1".to_string(), "User rejected".to_string())]
        );
    }

    fn lan_monitor_envelope(now_ms: u64, request_id: &str) -> serde_json::Value {
        serde_json::json!({
            "cmd": "lan_monitor",
            "request": { "action": "ping" },
            "_protocol_version": 1,
            "_issued_at_ms": now_ms,
            "_request_id": request_id,
        })
    }

    #[test]
    fn lan_monitor_message_guard_rejects_replay_and_expired_envelopes() {
        let now_ms = 10_000_000;
        let mut guard = LanMonitorMessageGuard::default();
        let first = lan_monitor_envelope(now_ms, "request-0001");

        assert_eq!(
            guard.validate_envelope(&first, "nonce-0001", now_ms),
            Ok("request-0001".to_string())
        );
        assert_eq!(
            guard.validate_envelope(&first, "nonce-0001", now_ms),
            Err("Replayed LAN monitor command".to_string())
        );
        assert_eq!(
            guard.validate_envelope(&first, "nonce-0002", now_ms),
            Err("Replayed LAN monitor command".to_string())
        );

        let expired =
            lan_monitor_envelope(now_ms - LAN_MONITOR_COMMAND_MAX_AGE_MS - 1, "request-0002");
        assert_eq!(
            guard.validate_envelope(&expired, "nonce-0003", now_ms),
            Err("Expired LAN monitor command".to_string())
        );

        let future = lan_monitor_envelope(
            now_ms + LAN_MONITOR_COMMAND_FUTURE_SKEW_MS + 1,
            "request-0003",
        );
        assert_eq!(
            guard.validate_envelope(&future, "nonce-0004", now_ms),
            Err("Expired LAN monitor command".to_string())
        );
    }

    #[test]
    fn lan_monitor_redaction_and_result_chunks_exclude_credentials() {
        let value = serde_json::json!({
            "authorization": "secret header",
            "nested": {
                "password": "plain-password",
                "output": "curl -H 'Authorization: Bearer abcdef' --token=token-value"
            }
        });
        let sanitized = sanitize_lan_monitor_value(&value);
        let serialized = serde_json::to_string(&sanitized).unwrap();
        assert_eq!(sanitized["authorization"], "[redacted]");
        assert_eq!(sanitized["nested"]["password"], "[redacted]");
        assert!(!serialized.contains("abcdef"));
        assert!(!serialized.contains("token-value"));

        let chunk =
            lan_monitor_tool_result_chunk("result-ref".to_string(), &value, 0, Some(24)).unwrap();
        assert_eq!(chunk.chunk.chars().count(), 24);
        assert!(chunk.has_more);
        assert_eq!(chunk.next_cursor, Some(24));
        assert!(
            lan_monitor_tool_result_chunk("result-ref".to_string(), &value, 100_000, None,)
                .is_err()
        );
    }

    #[test]
    fn lan_monitor_transcript_preview_marks_large_results_for_chunking() {
        let mut page = LanMonitorTranscriptPage {
            session_id: "session-1".to_string(),
            turns: vec![LanMonitorTurn {
                turn_id: "turn-1".to_string(),
                turn_index: 0,
                kind: "user_dialog".to_string(),
                status: "completed".to_string(),
                timestamp: 1,
                start_time: 1,
                end_time: Some(2),
                duration_ms: Some(1),
                finish_reason: None,
                user_message: LanMonitorUserMessage {
                    id: "user-1".to_string(),
                    content: "question".to_string(),
                    timestamp: 1,
                    images: Vec::new(),
                },
                rounds: vec![LanMonitorRound {
                    id: "round-1".to_string(),
                    round_index: 0,
                    status: "completed".to_string(),
                    start_time: 1,
                    end_time: Some(2),
                    duration_ms: Some(1),
                    model_id: None,
                    model_alias: None,
                    items: vec![LanMonitorItem::Tool {
                        id: "tool-1".to_string(),
                        name: "Read".to_string(),
                        status: "completed".to_string(),
                        input: serde_json::json!({}),
                        result: Some(serde_json::Value::String(
                            "x".repeat(LAN_MONITOR_RESULT_STRING_CHAR_LIMIT + 100),
                        )),
                        success: Some(true),
                        error: None,
                        start_time: 1,
                        end_time: Some(2),
                        duration_ms: Some(1),
                        order_index: Some(0),
                        subagent_session_id: None,
                        subagent_dialog_turn_id: None,
                        parent_task_tool_id: None,
                        subagent_model_id: None,
                        subagent_model_display_name: None,
                        result_truncated: false,
                        result_ref: None,
                    }],
                }],
            }],
            total_turn_count: 1,
            has_more: false,
            next_before_turn_id: None,
        };

        compact_lan_monitor_transcript_page(&mut page);
        let LanMonitorItem::Tool {
            result,
            result_truncated,
            result_ref,
            ..
        } = &page.turns[0].rounds[0].items[0]
        else {
            panic!("expected tool item");
        };
        assert!(*result_truncated);
        assert_eq!(
            result_ref.as_deref(),
            Some(lan_monitor_result_ref("session-1", "turn-1", "tool-1").as_str())
        );
        assert!(
            result
                .as_ref()
                .and_then(serde_json::Value::as_str)
                .unwrap()
                .chars()
                .count()
                <= LAN_MONITOR_RESULT_STRING_CHAR_LIMIT
        );
    }
}
