use std::str::FromStr;

use super::{
    AgentPath, AgentTarget, AgentTurnRef, CollaborationToolResult, SubagentOutcome, SubagentRef,
    SubagentSpawnResult,
};
use crate::agent_dispatch::DispatchBinding;
use crate::talos::TalosContentItem;

#[test]
fn agent_path_parses_root_and_recursive_paths() {
    let root = AgentPath::from_str("/root").expect("parse root path");
    let child = AgentPath::from_str("/root/research").expect("parse child path");
    let descendant = AgentPath::from_str("/root/research/api").expect("parse descendant path");

    assert_eq!((root.depth(), child.depth(), descendant.depth()), (0, 1, 2));
    assert_eq!(descendant.name(), "api");
    assert_eq!(descendant.parent(), Some(child.clone()));
    assert!(root.is_ancestor_of(&descendant));
    assert!(child.is_ancestor_of(&descendant));
}

#[test]
fn agent_path_joins_one_valid_segment() {
    let parent = AgentPath::from_str("/root/research").expect("parse parent path");

    assert_eq!(
        parent.join("api").expect("join child segment"),
        AgentPath::from_str("/root/research/api").expect("parse expected path")
    );
}

#[test]
fn agent_path_rejects_invalid_segments_and_bounds() {
    for invalid in [
        "",
        "root",
        ".",
        "..",
        "Upper",
        "hyphen-name",
        "contains/slash",
        "abcdefghijklmnopqrstuvwxyzabcdefghijklmnopqrstuvwxyzabcdefghijklm",
    ] {
        assert!(
            AgentPath::root().join(invalid).is_err(),
            "segment should be rejected: {invalid:?}"
        );
    }

    assert!(AgentPath::from_str("root/research").is_err());
    assert!(AgentPath::from_str("/other/research").is_err());
    assert!(AgentPath::from_str("/root//research").is_err());
    assert!(AgentPath::from_str("/root/a/b/c/d/e/f/g/h/i").is_err());
}

#[test]
fn agent_path_serde_round_trip_is_canonical() {
    let path = AgentPath::from_str("/root/research/api").expect("parse path");
    let encoded = serde_json::to_string(&path).expect("serialize path");
    let decoded: AgentPath = serde_json::from_str(&encoded).expect("deserialize path");

    assert_eq!(encoded, "\"/root/research/api\"");
    assert_eq!(decoded, path);
    assert!(serde_json::from_str::<AgentPath>("\"/root/../api\"").is_err());
}

#[test]
fn agent_target_serde_uses_an_explicit_tagged_shape() {
    let direct: AgentTarget =
        serde_json::from_str(r#"{"type":"directChild","name":"research"}"#).unwrap();
    let descendant: AgentTarget =
        serde_json::from_str(r#"{"type":"descendant","path":"/root/research"}"#).unwrap();

    assert_eq!(direct.as_str(), "research");
    assert_eq!(descendant.as_str(), "/root/research");
    assert!(serde_json::from_str::<AgentTarget>(r#""/root/research""#).is_err());
    assert!(
        serde_json::from_str::<AgentTarget>(r#"{"type":"directChild","name":"../research"}"#)
            .is_err()
    );
    assert!(
        serde_json::from_str::<AgentTarget>(r#"{"type":"descendant","name":"research"}"#).is_err()
    );
}

#[test]
fn agent_target_resolves_only_inside_the_caller_subtree() {
    let caller = AgentPath::from_str("/root/research").expect("parse caller path");
    let direct = AgentTarget::DirectChild {
        name: "api".to_string(),
    };
    let descendant =
        AgentTarget::canonical(AgentPath::from_str("/root/research/api/tests").unwrap());
    let sibling = AgentTarget::canonical(AgentPath::from_str("/root/implement").unwrap());

    assert_eq!(
        direct.resolve_descendant(&caller).unwrap(),
        Some(AgentPath::from_str("/root/research/api").unwrap())
    );
    assert_eq!(
        descendant.resolve_descendant(&caller).unwrap(),
        Some(AgentPath::from_str("/root/research/api/tests").unwrap())
    );
    assert_eq!(sibling.resolve_descendant(&caller).unwrap(), None);
}

#[test]
fn subagent_outcome_uses_camel_case_wire_fields() {
    let outcome = SubagentOutcome::Running {
        turn_id: "turn-1".to_string(),
    };

    assert_eq!(
        serde_json::to_value(outcome).expect("serialize outcome"),
        serde_json::json!({"status": "running", "turnId": "turn-1"})
    );
}

#[test]
fn spawn_result_contains_exact_child_turn_identity() {
    let child_turn = AgentTurnRef {
        session_id: "child-session".to_string(),
        turn_id: "child-turn".to_string(),
    };
    let result = SubagentSpawnResult {
        subagent: SubagentRef {
            path: AgentPath::from_str("/root/research").expect("parse path"),
            session_id: child_turn.session_id.clone(),
            parent: AgentTurnRef {
                session_id: "root-session".to_string(),
                turn_id: "root-turn".to_string(),
            },
        },
        agent_id: "reviewer".to_string(),
        turn: child_turn,
    };

    assert_eq!(
        serde_json::to_value(result).expect("serialize spawn result"),
        serde_json::json!({
            "subagent": {
                "path": "/root/research",
                "sessionId": "child-session",
                "parent": {
                    "sessionId": "root-session",
                    "turnId": "root-turn"
                }
            },
            "agentId": "reviewer",
            "turn": {
                "sessionId": "child-session",
                "turnId": "child-turn"
            }
        })
    );
}

#[test]
fn dispatch_binding_round_trips_exhaustive_variants() {
    for binding in [
        DispatchBinding::Routed,
        DispatchBinding::Exact {
            agent_id: "reviewer".to_string(),
        },
    ] {
        let encoded = serde_json::to_string(&binding).expect("serialize dispatch binding");
        let decoded: DispatchBinding =
            serde_json::from_str(&encoded).expect("deserialize dispatch binding");
        assert_eq!(decoded, binding);
    }
}

#[test]
fn collaboration_tool_result_is_explicitly_tagged() {
    let result = CollaborationToolResult::SpawnAgent(SubagentSpawnResult {
        subagent: SubagentRef {
            path: AgentPath::from_str("/root/research").unwrap(),
            session_id: "child-session".to_string(),
            parent: AgentTurnRef {
                session_id: "root-session".to_string(),
                turn_id: "root-turn".to_string(),
            },
        },
        agent_id: "reviewer".to_string(),
        turn: AgentTurnRef {
            session_id: "child-session".to_string(),
            turn_id: "child-turn".to_string(),
        },
    });

    let encoded = serde_json::to_value(&result).expect("serialize collaboration result");
    let decoded: CollaborationToolResult =
        serde_json::from_value(encoded.clone()).expect("deserialize collaboration result");

    assert_eq!(decoded, result);
    assert_eq!(encoded["operation"], "spawnAgent");
    assert_eq!(encoded["result"]["turn"]["turnId"], "child-turn");

    let content_item = TalosContentItem::SubagentResult { result };
    assert_eq!(
        serde_json::to_value(content_item).expect("serialize content item")["type"],
        "subagent_result"
    );
}
