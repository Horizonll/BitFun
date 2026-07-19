use bitfun_agent_runtime::sdk::PermissionV2Request;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum CliApprovalPolicy {
    /// Inherit the persisted user interaction preference.
    Ask,
    /// Explicitly disable Auto mode for this invocation/session.
    DisableAuto,
    Reject,
    Auto,
}

pub(crate) fn permission_request_targets_session(
    request: &PermissionV2Request,
    session_id: &str,
) -> bool {
    request.session_id == session_id
        || request
            .delegation
            .as_ref()
            .is_some_and(|delegation| delegation.parent_session_id == session_id)
}

#[cfg(test)]
mod tests {
    use super::permission_request_targets_session;
    use bitfun_agent_runtime::sdk::{
        PermissionDelegationContext, PermissionRequestSource, PermissionRequestSourceKind,
        PermissionV2Request,
    };
    use serde_json::Map;

    fn request() -> PermissionV2Request {
        PermissionV2Request {
            request_id: "request-1".to_string(),
            tool_call_id: Some("child-tool".to_string()),
            project_id: "project-1".to_string(),
            session_id: "child-session".to_string(),
            agent_id: "Explore".to_string(),
            action: "edit".to_string(),
            resources: vec!["src/main.rs".to_string()],
            save_resources: Vec::new(),
            source: PermissionRequestSource {
                kind: PermissionRequestSourceKind::ToolCall,
                identity: "Write".to_string(),
            },
            delegation: Some(PermissionDelegationContext {
                parent_session_id: "parent-session".to_string(),
                parent_dialog_turn_id: "parent-turn".to_string(),
                parent_tool_call_id: "parent-task".to_string(),
                subagent_type: "Explore".to_string(),
            }),
            display_metadata: Map::new(),
        }
    }

    #[test]
    fn permission_requests_target_their_execution_and_parent_interaction_sessions() {
        let request = request();

        assert!(permission_request_targets_session(
            &request,
            "child-session"
        ));
        assert!(permission_request_targets_session(
            &request,
            "parent-session"
        ));
        assert!(!permission_request_targets_session(
            &request,
            "unrelated-session"
        ));
    }
}
