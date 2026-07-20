use async_trait::async_trait;
use bitfun_agent_runtime::permission_v2::{
    PermissionRequestManager, PermissionRequestManagerError, PermissionWaitOutcome,
};
use bitfun_runtime_ports::{
    ClockPort, PermissionAuditRecord, PermissionAuditStorePort, PermissionGrant,
    PermissionGrantKey, PermissionGrantStorePort, PermissionReply, PermissionReplySource,
    PermissionReplyStorePort, PermissionRequestSource, PermissionRequestSourceKind,
    PermissionV2Request, PortError, PortErrorKind, PortResult, RuntimeServiceCapability,
    RuntimeServicePort,
};
use serde_json::Map;
use std::sync::{Arc, Mutex};

#[derive(Debug, Default)]
struct RecordingPermissionStore {
    grants: Mutex<Vec<PermissionGrant>>,
    audit: Mutex<Vec<PermissionAuditRecord>>,
    fail_grants: Mutex<bool>,
    fail_audit: Mutex<bool>,
}

impl RuntimeServicePort for RecordingPermissionStore {
    fn capability(&self) -> RuntimeServiceCapability {
        RuntimeServiceCapability::Permission
    }
}

#[async_trait]
impl PermissionGrantStorePort for RecordingPermissionStore {
    async fn list_project_grants(&self, project_id: &str) -> PortResult<Vec<PermissionGrant>> {
        Ok(self
            .grants
            .lock()
            .unwrap()
            .iter()
            .filter(|grant| grant.project_id == project_id)
            .cloned()
            .collect())
    }

    async fn add_project_grants(&self, grants: Vec<PermissionGrant>) -> PortResult<()> {
        if *self.fail_grants.lock().unwrap() {
            return Err(PortError::new(
                PortErrorKind::Backend,
                "grant store unavailable",
            ));
        }
        let mut stored = self.grants.lock().unwrap();
        for grant in grants {
            if !stored.iter().any(|existing| existing.key() == grant.key()) {
                stored.push(grant);
            }
        }
        Ok(())
    }

    async fn remove_project_grant(&self, key: PermissionGrantKey) -> PortResult<bool> {
        let mut stored = self.grants.lock().unwrap();
        let previous_len = stored.len();
        stored.retain(|grant| grant.key() != key);
        Ok(stored.len() != previous_len)
    }

    async fn clear_project_grants(&self, project_id: &str) -> PortResult<usize> {
        let mut stored = self.grants.lock().unwrap();
        let previous_len = stored.len();
        stored.retain(|grant| grant.project_id != project_id);
        Ok(previous_len - stored.len())
    }
}

#[async_trait]
impl PermissionAuditStorePort for RecordingPermissionStore {
    async fn append_permission_audit(&self, record: PermissionAuditRecord) -> PortResult<()> {
        if *self.fail_audit.lock().unwrap() {
            return Err(PortError::new(
                PortErrorKind::Backend,
                "audit store unavailable",
            ));
        }
        let mut stored = self.audit.lock().unwrap();
        if !stored
            .iter()
            .any(|existing| existing.audit_id == record.audit_id)
        {
            stored.push(record);
        }
        Ok(())
    }

    async fn list_project_permission_audit(
        &self,
        project_id: &str,
    ) -> PortResult<Vec<PermissionAuditRecord>> {
        Ok(self
            .audit
            .lock()
            .unwrap()
            .iter()
            .filter(|record| record.request.project_id == project_id)
            .cloned()
            .collect())
    }
}

#[async_trait]
impl PermissionReplyStorePort for RecordingPermissionStore {
    async fn commit_permission_reply(
        &self,
        grants: Vec<PermissionGrant>,
        audit: Vec<PermissionAuditRecord>,
    ) -> PortResult<()> {
        if *self.fail_grants.lock().unwrap() {
            return Err(PortError::new(
                PortErrorKind::Backend,
                "reply store unavailable",
            ));
        }
        if *self.fail_audit.lock().unwrap() {
            return Err(PortError::new(
                PortErrorKind::Backend,
                "audit store unavailable",
            ));
        }
        self.add_project_grants(grants).await?;
        for record in audit {
            self.append_permission_audit(record).await?;
        }
        Ok(())
    }
}

#[derive(Debug)]
struct FixedClock(i64);

impl RuntimeServicePort for FixedClock {
    fn capability(&self) -> RuntimeServiceCapability {
        RuntimeServiceCapability::Clock
    }
}

impl ClockPort for FixedClock {
    fn now_unix_millis(&self) -> i64 {
        self.0
    }
}

fn request(request_id: &str, session_id: &str) -> PermissionV2Request {
    PermissionV2Request {
        request_id: request_id.to_string(),
        tool_call_id: None,
        project_id: "project-a".to_string(),
        session_id: session_id.to_string(),
        agent_id: "agentic".to_string(),
        action: "edit".to_string(),
        resources: vec!["src/lib.rs".to_string()],
        save_resources: vec!["src/*".to_string(), "src/*".to_string()],
        source: PermissionRequestSource {
            kind: PermissionRequestSourceKind::ToolCall,
            identity: format!("tool-{request_id}"),
        },
        delegation: None,
        display_metadata: Map::new(),
    }
}

fn manager() -> (PermissionRequestManager, Arc<RecordingPermissionStore>) {
    let store = Arc::new(RecordingPermissionStore::default());
    (
        PermissionRequestManager::new(store.clone(), store.clone(), Arc::new(FixedClock(123)))
            .with_grant_store(store.clone()),
        store,
    )
}

#[tokio::test]
async fn once_releases_only_the_selected_request_and_records_audit() {
    let (manager, store) = manager();
    let receiver = manager
        .register(request("request-1", "session-a"))
        .await
        .expect("register request");

    let resolution = manager
        .reply(
            "request-1",
            PermissionReply::Once,
            PermissionReplySource::User,
        )
        .await
        .expect("reply once");

    assert_eq!(
        receiver.wait().await,
        PermissionWaitOutcome::Replied(PermissionReply::Once)
    );
    assert_eq!(resolution.resolved_request_ids, vec!["request-1"]);
    assert!(resolution.saved_grants.is_empty());
    assert!(manager.pending_requests().is_empty());
    assert_eq!(store.audit.lock().unwrap().len(), 2);
}

#[tokio::test]
async fn always_persists_unique_project_grants_without_releasing_other_pending_requests() {
    let (manager, store) = manager();
    let receiver = manager
        .register(request("request-1", "session-a"))
        .await
        .expect("register request");
    let other = manager
        .register(request("request-2", "session-b"))
        .await
        .expect("register other request");

    let resolution = manager
        .reply(
            "request-1",
            PermissionReply::Always,
            PermissionReplySource::User,
        )
        .await
        .expect("reply always");

    assert_eq!(
        receiver.wait().await,
        PermissionWaitOutcome::Replied(PermissionReply::Always)
    );
    assert_eq!(resolution.saved_grants.len(), 1);
    assert_eq!(store.grants.lock().unwrap().len(), 1);
    assert_eq!(
        manager
            .pending_requests()
            .iter()
            .map(|request| request.request_id.as_str())
            .collect::<Vec<_>>(),
        vec!["request-2"]
    );

    manager
        .cancel_request("request-2", "test cleanup")
        .await
        .expect("cancel other request");
    assert_eq!(
        other.wait().await,
        PermissionWaitOutcome::Cancelled {
            reason: "test cleanup".to_string()
        }
    );
}

#[tokio::test]
async fn reject_releases_only_the_selected_request() {
    let (manager, _) = manager();
    let target = manager
        .register(request("request-1", "session-a"))
        .await
        .expect("register target");
    let sibling = manager
        .register(request("request-2", "session-a"))
        .await
        .expect("register sibling");
    let other_session = manager
        .register(request("request-3", "session-b"))
        .await
        .expect("register other session");

    let reply = PermissionReply::Reject {
        feedback: Some("Use a read-only path".to_string()),
    };
    let resolution = manager
        .reply("request-1", reply.clone(), PermissionReplySource::User)
        .await
        .expect("reject request");

    assert_eq!(target.wait().await, PermissionWaitOutcome::Replied(reply));
    assert_eq!(resolution.resolved_request_ids, vec!["request-1"]);
    assert_eq!(
        manager
            .pending_requests()
            .iter()
            .map(|request| request.request_id.as_str())
            .collect::<Vec<_>>(),
        vec!["request-2", "request-3"]
    );

    assert!(
        tokio::time::timeout(std::time::Duration::from_millis(20), sibling.wait())
            .await
            .is_err(),
        "a sibling request must keep waiting after the target is rejected"
    );

    manager
        .cancel_session("session-b", "disconnected")
        .await
        .expect("cancel other session");
    assert_eq!(
        other_session.wait().await,
        PermissionWaitOutcome::Cancelled {
            reason: "disconnected".to_string()
        }
    );
    manager
        .cancel_session("session-a", "test cleanup")
        .await
        .expect("cancel sibling request");
}

#[tokio::test]
async fn pending_snapshots_and_session_cancellation_preserve_registration_order() {
    let (manager, _) = manager();
    let first = manager
        .register(request("request-z", "session-a"))
        .await
        .expect("register first request");
    let second = manager
        .register_non_interactive(request("request-a", "session-a"))
        .await
        .expect("register second request");
    let third = manager
        .register(request("request-m", "session-a"))
        .await
        .expect("register third request");

    let request_ids = |requests: Vec<PermissionV2Request>| {
        requests
            .into_iter()
            .map(|request| request.request_id)
            .collect::<Vec<_>>()
    };
    assert_eq!(
        request_ids(manager.pending_requests()),
        vec!["request-z", "request-a", "request-m"]
    );
    assert_eq!(
        request_ids(manager.interactive_pending_requests()),
        vec!["request-z", "request-m"]
    );

    assert_eq!(
        manager
            .cancel_session("session-a", "session closed")
            .await
            .expect("cancel session"),
        vec!["request-z", "request-a", "request-m"]
    );
    for receiver in [first, second, third] {
        assert_eq!(
            receiver.wait().await,
            PermissionWaitOutcome::Cancelled {
                reason: "session closed".to_string()
            }
        );
    }
}

#[tokio::test]
async fn grant_persistence_failure_keeps_the_request_pending_and_waiting() {
    let (manager, store) = manager();
    let _receiver = manager
        .register(request("request-1", "session-a"))
        .await
        .expect("register request");
    *store.fail_grants.lock().unwrap() = true;

    let error = manager
        .reply(
            "request-1",
            PermissionReply::Always,
            PermissionReplySource::User,
        )
        .await
        .expect_err("grant failure must fail closed");

    assert!(matches!(
        error,
        PermissionRequestManagerError::ReplyStore(_)
    ));
    assert_eq!(manager.pending_requests().len(), 1);
}

#[tokio::test]
async fn audit_persistence_failure_keeps_the_request_pending_and_waiting() {
    let (manager, store) = manager();
    let _receiver = manager
        .register(request("request-1", "session-a"))
        .await
        .expect("register request");
    *store.fail_audit.lock().unwrap() = true;

    let error = manager
        .reply(
            "request-1",
            PermissionReply::Once,
            PermissionReplySource::User,
        )
        .await
        .expect_err("audit failure must fail closed");

    assert!(matches!(
        error,
        PermissionRequestManagerError::ReplyStore(_)
    ));
    assert_eq!(manager.pending_requests().len(), 1);
}

#[tokio::test]
async fn pending_requests_are_process_local_and_not_restored_by_a_new_manager() {
    let (manager, store) = manager();
    let _receiver = manager
        .register(request("request-1", "session-a"))
        .await
        .expect("register request");

    let restarted = PermissionRequestManager::new(store.clone(), store, Arc::new(FixedClock(456)));
    assert!(restarted.pending_requests().is_empty());
}

#[tokio::test]
async fn grant_management_is_project_scoped_and_audit_remains_append_only() {
    let (manager, store) = manager();
    store
        .add_project_grants(vec![
            PermissionGrant {
                project_id: "project-a".to_string(),
                action: "edit".to_string(),
                resource: "src/main.rs".to_string(),
                created_at_ms: 1,
            },
            PermissionGrant {
                project_id: "project-b".to_string(),
                action: "read".to_string(),
                resource: "README.md".to_string(),
                created_at_ms: 2,
            },
        ])
        .await
        .expect("seed grants");
    let pending = manager
        .register(request("request-audit", "session-a"))
        .await
        .expect("register audited request");

    assert_eq!(
        manager
            .list_project_grants("project-a")
            .await
            .expect("list project grants")
            .len(),
        1
    );
    assert!(manager
        .remove_project_grant(PermissionGrantKey {
            project_id: "project-a".to_string(),
            action: "edit".to_string(),
            resource: "src/main.rs".to_string(),
        })
        .await
        .expect("remove project grant"));
    assert_eq!(
        manager
            .clear_project_grants("project-b")
            .await
            .expect("clear project grants"),
        1
    );
    assert_eq!(
        manager
            .list_project_permission_audit("project-a")
            .await
            .expect("list project audit")
            .len(),
        1
    );

    manager
        .cancel_request("request-audit", "test cleanup")
        .await
        .expect("cancel request");
    assert!(matches!(
        pending.wait().await,
        PermissionWaitOutcome::Cancelled { .. }
    ));
}
