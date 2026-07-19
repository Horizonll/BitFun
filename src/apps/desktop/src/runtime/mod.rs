use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use bitfun_agent_runtime::sdk::{
    AgentDialogTurnPort, AgentInteractionResponsePort, AgentRuntime, AgentRuntimeBuilder,
    AgentSessionModelPort, AgentSubmissionPort, AgentTurnCancellationPort, PermissionRequestEvent,
    RuntimeBuildError,
};
use bitfun_core::agentic::coordination::{ConversationCoordinator, DialogScheduler};

/// Desktop-owned access to the Agent Runtime SDK interaction facade.
///
/// Core remains the sole owner of the coordinator, scheduler, sessions, tool
/// pipeline, and Agentic event queue. This context exposes only the interaction
/// ports used by current Tauri commands; it does not claim that the complete
/// Desktop delivery profile or its product services have been assembled.
pub struct DesktopRuntimeContext {
    agent_runtime: AgentRuntime,
    permission_events_started: AtomicBool,
}

impl DesktopRuntimeContext {
    pub(crate) fn build(
        coordinator: Arc<ConversationCoordinator>,
        scheduler: Arc<DialogScheduler>,
    ) -> Result<Self, RuntimeBuildError> {
        let submission: Arc<dyn AgentSubmissionPort> = coordinator.clone();
        let session_model: Arc<dyn AgentSessionModelPort> = coordinator.clone();
        let interaction_response: Arc<dyn AgentInteractionResponsePort> = coordinator;
        let dialog_turn: Arc<dyn AgentDialogTurnPort> = scheduler.clone();
        let cancellation: Arc<dyn AgentTurnCancellationPort> = scheduler;
        let agent_runtime = AgentRuntimeBuilder::new()
            .with_submission_port(submission)
            .with_session_model_port(session_model)
            .with_dialog_turn_port(dialog_turn)
            .with_cancellation_port(cancellation)
            .with_interaction_response_port(interaction_response)
            .with_permission_request_manager(
                bitfun_core::product_runtime::core_permission_request_manager()
                    .map_err(RuntimeBuildError::PermissionRequestManagerUnavailable)?,
            )
            .build()?;

        Ok(Self {
            agent_runtime,
            permission_events_started: AtomicBool::new(false),
        })
    }

    pub(crate) fn agent_runtime(&self) -> &AgentRuntime {
        &self.agent_runtime
    }

    pub(crate) fn start_permission_event_forwarding(
        &self,
        app: tauri::AppHandle,
    ) -> Result<(), bitfun_agent_runtime::sdk::RuntimeError> {
        if self.permission_events_started.swap(true, Ordering::AcqRel) {
            return Ok(());
        }

        let mut receiver = match self.agent_runtime.subscribe_permission_requests() {
            Ok(receiver) => receiver,
            Err(error) => {
                self.permission_events_started
                    .store(false, Ordering::Release);
                return Err(error);
            }
        };
        let runtime = self.agent_runtime.clone();
        tauri::async_runtime::spawn(async move {
            use tauri::Emitter;

            loop {
                match receiver.recv().await {
                    Ok(event) => {
                        let fanout = crate::api::peer_host_invoke::track_permission_event(&event);
                        if fanout {
                            if let Ok(payload) = serde_json::to_value(&event) {
                                crate::api::remote_connect_api::maybe_fanout_peer_ui_event(
                                    "permission://event",
                                    payload,
                                );
                            }
                        }
                        let _ = app.emit("permission://event", event);
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => {
                        if let Ok(requests) = runtime.pending_permission_requests() {
                            for request in requests {
                                let event = PermissionRequestEvent::Asked { request };
                                let fanout =
                                    crate::api::peer_host_invoke::track_permission_event(&event);
                                if fanout {
                                    if let Ok(payload) = serde_json::to_value(&event) {
                                        crate::api::remote_connect_api::maybe_fanout_peer_ui_event(
                                            "permission://event",
                                            payload,
                                        );
                                    }
                                }
                                let _ = app.emit("permission://event", event);
                            }
                        }
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Closed) => {
                        let request_ids =
                            crate::api::peer_host_invoke::take_tracked_permission_requests();
                        if let Err(error) =
                            crate::api::peer_host_invoke::fail_closed_permission_requests(
                                request_ids,
                                "Peer permission event stream closed",
                            )
                            .await
                        {
                            log::warn!(
                                "Peer permission requests were not fully cancelled: {error}"
                            );
                        }
                        break;
                    }
                }
            }
        });
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn desktop_runtime_wiring_reuses_existing_core_owners() {
        let runtime_source = include_str!("mod.rs");
        let coordinator_constructor = ["ConversationCoordinator", "::new"].concat();
        let scheduler_constructor = ["DialogScheduler", "::new"].concat();
        assert!(!runtime_source.contains(&coordinator_constructor));
        assert!(!runtime_source.contains(&scheduler_constructor));

        let app_source = include_str!("../lib.rs");
        assert!(app_source.contains("DesktopRuntimeContext::build("));
        assert!(app_source.contains(".manage(desktop_runtime)"));

        assert!(runtime_source.contains("with_dialog_turn_port"));
        assert!(runtime_source.contains("with_cancellation_port"));
        assert!(runtime_source.contains("with_interaction_response_port"));
        assert!(runtime_source.contains("with_session_model_port"));
    }

    #[test]
    fn desktop_interaction_runtime_does_not_claim_unimplemented_product_services() {
        let runtime_source = include_str!("mod.rs");
        let product_assembler = ["Product", "Assembler"].concat();
        let runtime_services = ["Runtime", "Services"].concat();
        let desktop_services_provider = ["DesktopRuntime", "ServicesProvider"].concat();

        assert!(!runtime_source.contains(&product_assembler));
        assert!(!runtime_source.contains(&runtime_services));
        assert!(!runtime_source.contains(&desktop_services_provider));
    }
}
