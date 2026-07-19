use bitfun_product_domains::tool_permissions::{
    merge_permission_rule_layers, resolve_permission_policy, wildcard_matches, PermissionEffect,
    PermissionEvaluator, PermissionPolicyConfig, PermissionPolicyLayers, PermissionPolicyPreset,
    PermissionReply, PermissionRequest, PermissionRequestSource, PermissionRequestSourceKind,
    PermissionResourceCaseSensitivity, PermissionRule, ToolPermissionConfig,
};
use serde_json::json;
use serde_json::Map;

fn rule(action: &str, resource: &str, effect: PermissionEffect) -> PermissionRule {
    PermissionRule::new(action, resource, effect)
}

fn policy(preset: PermissionPolicyPreset, rules: Vec<PermissionRule>) -> PermissionPolicyConfig {
    PermissionPolicyConfig { preset, rules }
}

#[test]
fn tool_permission_config_defaults_to_ask_with_auto_approve_disabled() {
    let config = ToolPermissionConfig::default();

    assert_eq!(config.policy.preset, PermissionPolicyPreset::Ask);
    assert!(config.policy.rules.is_empty());
    assert!(!config.interaction.auto_approve_ask);
    assert_eq!(
        serde_json::to_value(config).expect("serialize tool permission config"),
        json!({
            "policy": {
                "preset": "ask",
                "rules": [],
            },
            "interaction": {
                "auto_approve_ask": false,
            },
        })
    );
}

#[test]
fn policy_presets_expand_into_ordinary_baseline_rules() {
    let ask = policy(PermissionPolicyPreset::Ask, Vec::new());
    let full_access = policy(PermissionPolicyPreset::FullAccess, Vec::new());
    let evaluator = PermissionEvaluator::case_sensitive();

    let ask_rules = resolve_permission_policy(PermissionPolicyLayers {
        product_defaults: &[],
        global: &ask,
        project: &[],
        agent: &[],
        enforced: &[],
    });
    let full_access_rules = resolve_permission_policy(PermissionPolicyLayers {
        product_defaults: &[],
        global: &full_access,
        project: &[],
        agent: &[],
        enforced: &[],
    });

    assert_eq!(ask_rules, vec![rule("*", "*", PermissionEffect::Ask)]);
    assert_eq!(
        full_access_rules,
        vec![rule("*", "*", PermissionEffect::Allow)]
    );
    assert_eq!(
        evaluator.evaluate_resource("edit", "src/main.rs", &ask_rules),
        PermissionEffect::Ask
    );
    assert_eq!(
        evaluator.evaluate_resource("edit", "src/main.rs", &full_access_rules),
        PermissionEffect::Allow
    );
}

#[test]
fn resolved_policy_preserves_layer_order_and_enforced_limits() {
    let product_defaults = vec![rule("read", "*", PermissionEffect::Allow)];
    let global = policy(
        PermissionPolicyPreset::FullAccess,
        vec![rule("bash", "rm *", PermissionEffect::Ask)],
    );
    let project = vec![rule("edit", "generated/*", PermissionEffect::Deny)];
    let agent = vec![rule("edit", "generated/review.md", PermissionEffect::Allow)];
    let enforced = vec![rule("edit", "generated/*", PermissionEffect::Deny)];

    let resolved = resolve_permission_policy(PermissionPolicyLayers {
        product_defaults: &product_defaults,
        global: &global,
        project: &project,
        agent: &agent,
        enforced: &enforced,
    });

    assert_eq!(
        resolved,
        [
            product_defaults,
            vec![rule("*", "*", PermissionEffect::Allow)],
            global.rules,
            project,
            agent,
            enforced,
        ]
        .concat()
    );

    let evaluator = PermissionEvaluator::case_sensitive();
    assert_eq!(
        evaluator.evaluate_resource("bash", "rm -rf target", &resolved),
        PermissionEffect::Ask
    );
    assert_eq!(
        evaluator.evaluate_resource("edit", "generated/review.md", &resolved),
        PermissionEffect::Deny
    );
    assert_eq!(
        evaluator.evaluate_resource("webfetch", "https://example.com", &resolved),
        PermissionEffect::Allow
    );
}

#[test]
fn legacy_skip_confirmation_field_does_not_enable_access_or_auto_approve() {
    let config: ToolPermissionConfig = serde_json::from_value(json!({
        "skip_tool_confirmation": true,
    }))
    .expect("deserialize legacy-shaped permission config");

    assert_eq!(config, ToolPermissionConfig::default());
}

#[test]
fn permission_rule_uses_stable_wire_values() {
    let value = serde_json::to_value(rule("read", "src/*", PermissionEffect::Ask))
        .expect("serialize permission rule");

    assert_eq!(
        value,
        json!({
            "action": "read",
            "resource": "src/*",
            "effect": "ask",
        })
    );
    assert_eq!(
        serde_json::from_value::<PermissionRule>(value).expect("deserialize permission rule"),
        rule("read", "src/*", PermissionEffect::Ask)
    );
}

#[test]
fn permission_reply_uses_stable_tagged_wire_values() {
    assert_eq!(
        serde_json::to_value(PermissionReply::Once).expect("serialize once reply"),
        json!({ "reply": "once" })
    );
    assert_eq!(
        serde_json::to_value(PermissionReply::Always).expect("serialize always reply"),
        json!({ "reply": "always" })
    );
    assert_eq!(
        serde_json::to_value(PermissionReply::Reject {
            feedback: Some("Use a read-only path".to_string()),
        })
        .expect("serialize reject reply"),
        json!({
            "reply": "reject",
            "feedback": "Use a read-only path",
        })
    );
}

#[test]
fn permission_request_call_id_is_optional_and_camel_cased() {
    let request = PermissionRequest {
        request_id: "request-1".to_string(),
        tool_call_id: Some("call-1".to_string()),
        project_id: "project-1".to_string(),
        session_id: "session-1".to_string(),
        agent_id: "agentic".to_string(),
        action: "read".to_string(),
        resources: vec!["README.md".to_string()],
        save_resources: Vec::new(),
        source: PermissionRequestSource {
            kind: PermissionRequestSourceKind::ToolCall,
            identity: "Read".to_string(),
        },
        display_metadata: Map::new(),
    };
    let value = serde_json::to_value(&request).expect("serialize permission request");
    assert_eq!(value["toolCallId"], "call-1");

    let legacy = json!({
        "requestId": "request-legacy",
        "projectId": "project-1",
        "sessionId": "session-1",
        "agentId": "agentic",
        "action": "read",
        "resources": ["README.md"],
        "source": { "kind": "tool_call", "identity": "Read" },
    });
    let decoded: PermissionRequest = serde_json::from_value(legacy).expect("decode legacy request");
    assert_eq!(decoded.tool_call_id, None);
}

#[test]
fn wildcard_matching_supports_star_question_and_normalized_separators() {
    let sensitive = PermissionResourceCaseSensitivity::Sensitive;

    assert!(wildcard_matches("src/main.rs", "src/*.rs", sensitive));
    assert!(wildcard_matches("src/main.rs", "src/mai?.rs", sensitive));
    assert!(wildcard_matches(
        r"src\nested\main.rs",
        "src/*/main.rs",
        sensitive
    ));
    assert!(wildcard_matches("git", "git *", sensitive));
    assert!(wildcard_matches("git status", "git *", sensitive));
    assert!(!wildcard_matches("src/main.ts", "src/*.rs", sensitive));
    assert!(!wildcard_matches(
        "src/deep/main.rs",
        "src/????.rs",
        sensitive
    ));
}

#[test]
fn windows_compatible_matching_is_case_insensitive_for_resources() {
    let evaluator = PermissionEvaluator::windows_compatible();
    let rules = vec![rule(
        "read",
        r"C:\Users\Developer\Project\*",
        PermissionEffect::Allow,
    )];

    assert_eq!(
        evaluator.evaluate_resource("read", r"c:\users\developer\project\SRC\main.rs", &rules,),
        PermissionEffect::Allow
    );
    assert_eq!(
        PermissionEvaluator::case_sensitive().evaluate_resource(
            "read",
            r"c:\users\developer\project\SRC\main.rs",
            &rules,
        ),
        PermissionEffect::Ask
    );
}

#[test]
fn last_matching_action_and_resource_rule_wins() {
    let evaluator = PermissionEvaluator::case_sensitive();
    let rules = vec![
        rule("*", "*", PermissionEffect::Ask),
        rule("read", "src/*", PermissionEffect::Allow),
        rule("read", "src/private/*", PermissionEffect::Deny),
        rule("read", "src/private/public.txt", PermissionEffect::Allow),
    ];

    assert_eq!(
        evaluator.evaluate_resource("read", "src/lib.rs", &rules),
        PermissionEffect::Allow
    );
    assert_eq!(
        evaluator.evaluate_resource("read", "src/private/key.txt", &rules),
        PermissionEffect::Deny
    );
    assert_eq!(
        evaluator.evaluate_resource("read", "src/private/public.txt", &rules),
        PermissionEffect::Allow
    );
    assert_eq!(
        evaluator.evaluate_resource("edit", "src/lib.rs", &rules),
        PermissionEffect::Ask
    );
}

#[test]
fn merged_layers_preserve_global_project_agent_override_order() {
    let global = vec![rule("*", "*", PermissionEffect::Ask)];
    let project = vec![rule("read", "*", PermissionEffect::Allow)];
    let agent = vec![rule("read", "secrets/*", PermissionEffect::Deny)];
    let merged = merge_permission_rule_layers(&[&global, &project, &agent]);
    let evaluator = PermissionEvaluator::case_sensitive();

    assert_eq!(merged, [global, project, agent].concat());
    assert_eq!(
        evaluator.evaluate_resource("read", "README.md", &merged),
        PermissionEffect::Allow
    );
    assert_eq!(
        evaluator.evaluate_resource("read", "secrets/token.txt", &merged),
        PermissionEffect::Deny
    );
}

#[test]
fn unmatched_and_empty_resource_requests_default_to_ask() {
    let evaluator = PermissionEvaluator::case_sensitive();
    let rules = vec![rule("read", "src/*", PermissionEffect::Allow)];

    assert_eq!(
        evaluator.evaluate_resource("edit", "src/lib.rs", &rules),
        PermissionEffect::Ask
    );
    assert_eq!(
        evaluator.evaluate_resources("read", &[], &rules),
        PermissionEffect::Ask
    );
}

#[test]
fn multi_resource_decision_is_atomic_with_deny_then_ask_precedence() {
    let evaluator = PermissionEvaluator::case_sensitive();
    let rules = vec![
        rule("edit", "src/*", PermissionEffect::Allow),
        rule("edit", "src/generated/*", PermissionEffect::Ask),
        rule("edit", "src/secrets/*", PermissionEffect::Deny),
    ];

    assert_eq!(
        evaluator.evaluate_resources("edit", &["src/lib.rs".into(), "src/main.rs".into()], &rules,),
        PermissionEffect::Allow
    );
    assert_eq!(
        evaluator.evaluate_resources(
            "edit",
            &["src/lib.rs".into(), "src/generated/api.rs".into()],
            &rules,
        ),
        PermissionEffect::Ask
    );
    assert_eq!(
        evaluator.evaluate_resources(
            "edit",
            &["src/generated/api.rs".into(), "src/secrets/key.rs".into(),],
            &rules,
        ),
        PermissionEffect::Deny
    );
}
