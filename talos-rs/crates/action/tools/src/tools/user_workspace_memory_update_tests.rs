use std::sync::{Arc, Mutex};

use talos_memory::{
    MemoryScope, UserWorkspaceMemoryDocument, UserWorkspaceMemoryEntry,
    UserWorkspaceMemoryEntryUpdate, UserWorkspaceMemorySnapshot, UserWorkspaceMemoryWriter,
};
use talos_types::ExpectedFileVersion;

use super::*;

#[derive(Default)]
struct RecordingWriter {
    upserts: Mutex<Vec<(MemoryScope, UserWorkspaceMemoryEntry)>>,
    updates: Mutex<Vec<(MemoryScope, String, UserWorkspaceMemoryEntryUpdate)>>,
}

fn valid_snapshot() -> UserWorkspaceMemorySnapshot {
    let document = UserWorkspaceMemoryDocument::template().unwrap();
    UserWorkspaceMemorySnapshot::Valid {
        raw_content: document.render().unwrap(),
        updated_at: None,
        version_token: "v1".to_string(),
        document,
    }
}

macro_rules! unused_write {
    ($name:ident($($arg:ident: $ty:ty),*)) => {
        fn $name<'a>(
            &'a self,
            _scope: &'a MemoryScope,
            $($arg: $ty),*
        ) -> std::pin::Pin<Box<dyn std::future::Future<Output = TalosResult<UserWorkspaceMemorySnapshot>> + Send + 'a>> {
            Box::pin(async { unreachable!("unexpected User Workspace Memory operation") })
        }
    };
}

impl UserWorkspaceMemoryWriter for RecordingWriter {
    unused_write!(create_entry(_entry: UserWorkspaceMemoryEntry));
    fn update_entry<'a>(
        &'a self,
        scope: &'a MemoryScope,
        key: &'a str,
        update: UserWorkspaceMemoryEntryUpdate,
    ) -> std::pin::Pin<
        Box<dyn std::future::Future<Output = TalosResult<UserWorkspaceMemorySnapshot>> + Send + 'a>,
    > {
        Box::pin(async move {
            self.updates
                .lock()
                .expect("recorded updates should lock")
                .push((scope.clone(), key.to_string(), update));
            Ok(valid_snapshot())
        })
    }

    fn upsert_entry<'a>(
        &'a self,
        scope: &'a MemoryScope,
        entry: UserWorkspaceMemoryEntry,
    ) -> std::pin::Pin<
        Box<dyn std::future::Future<Output = TalosResult<UserWorkspaceMemorySnapshot>> + Send + 'a>,
    > {
        Box::pin(async move {
            self.upserts
                .lock()
                .expect("recorded upserts should lock")
                .push((scope.clone(), entry));
            Ok(valid_snapshot())
        })
    }

    unused_write!(replace_entry(_entry: UserWorkspaceMemoryEntry));
    unused_write!(remove_entry(_key: &'a str));
    unused_write!(replace_document(_content: String, _base_version: ExpectedFileVersion));
}

#[tokio::test]
async fn update_tool_uses_only_the_trusted_context_scope() {
    let writer = Arc::new(RecordingWriter::default());
    let scope = MemoryScope {
        user_id: "trusted-user".to_string(),
        workspace_root: std::path::PathBuf::from("/trusted/workspace"),
    };
    let context = ToolContext::new("session-1", "trusted-user")
        .with_memory_scope(scope.clone())
        .with_user_workspace_memory_writer(writer.clone());

    let output = UserWorkspaceMemoryUpdateTool
        .execute_payload(
            serde_json::json!({
                "operation": "upsert_entry",
                "entry": {
                    "key": "response-style",
                    "section": "interaction_guidance",
                    "title": "Response style",
                    "kind": "preference",
                    "summary": "Keep responses concise."
                }
            }),
            context,
        )
        .await
        .expect("memory update should succeed");

    assert!(!output.is_error());
    let upserts = writer.upserts.lock().expect("recorded upserts should lock");
    assert_eq!(upserts.len(), 1);
    assert_eq!(upserts[0].0, scope);
    assert_eq!(upserts[0].1.key, "response-style");
}

#[test]
fn update_tool_schema_exposes_operation_specific_contracts() {
    let schema = UserWorkspaceMemoryUpdateTool
        .parameters_schema()
        .expect("tool schema");

    assert_eq!(schema["oneOf"].as_array().map(Vec::len), Some(6));
    assert!(schema.to_string().contains("base_version"));
    assert!(schema.to_string().contains("additionalProperties"));
    assert!(schema.to_string().contains("clear_fields"));
    assert!(!schema.to_string().contains("\"action\""));
}

#[tokio::test]
async fn update_tool_maps_direct_fields_and_explicit_clears_to_domain_update() {
    let writer = Arc::new(RecordingWriter::default());
    let scope = MemoryScope {
        user_id: "trusted-user".to_string(),
        workspace_root: std::path::PathBuf::from("/trusted/workspace"),
    };
    let context = ToolContext::new("session-1", "trusted-user")
        .with_memory_scope(scope.clone())
        .with_user_workspace_memory_writer(writer.clone());

    UserWorkspaceMemoryUpdateTool
        .execute_payload(
            serde_json::json!({
                "operation": "update_entry",
                "key": "response-style",
                "update": {
                    "section": "interaction_guidance",
                    "kind": "preference",
                    "summary": "Prefer concise answers.",
                    "clear_fields": ["why"]
                }
            }),
            context,
        )
        .await
        .expect("memory update should succeed");

    let updates = writer.updates.lock().expect("recorded updates should lock");
    assert_eq!(updates.len(), 1);
    assert_eq!(updates[0].0, scope);
    assert_eq!(updates[0].1, "response-style");
    assert_eq!(
        updates[0].2,
        UserWorkspaceMemoryEntryUpdate::builder()
            .section(UserWorkspaceMemorySection::InteractionGuidance)
            .kind(talos_types::MemoryKind::Preference)
            .summary("Prefer concise answers.")
            .why(OptionalField::Clear)
            .build()
            .expect("domain update")
    );
}

#[test]
fn update_tool_rejects_setting_and_clearing_the_same_field() {
    let error = UserWorkspaceMemoryUpdateTool
        .validate_input(&serde_json::json!({
            "operation": "update_entry",
            "key": "response-style",
            "update": {
                "why": "Because the user requested it.",
                "clear_fields": ["why"]
            }
        }))
        .expect_err("ambiguous update must be rejected");

    assert!(error.to_string().contains("cannot be set and cleared"));
}

#[test]
fn update_tool_rejects_unknown_model_fields() {
    let error = UserWorkspaceMemoryUpdateTool
        .validate_input(&serde_json::json!({
            "operation": "remove_entry",
            "key": "response-style",
            "workspace_root": "/untrusted"
        }))
        .expect_err("unknown scope fields must be rejected");

    assert!(error.to_string().contains("unknown field"));
}

#[test]
fn update_tool_rejects_structurally_invalid_domain_fields() {
    let error = UserWorkspaceMemoryUpdateTool
        .validate_input(&serde_json::json!({
            "operation": "upsert_entry",
            "entry": {
                "key": "response-style",
                "section": "interaction_guidance",
                "title": "Response style",
                "kind": "preference",
                "summary": "first line\nsecond line"
            }
        }))
        .expect_err("multiline domain field must be rejected at the Tool boundary");

    assert!(error.to_string().contains("single-line"));
}
