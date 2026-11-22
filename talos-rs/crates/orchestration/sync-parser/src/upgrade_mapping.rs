//! Application-owned Header rules for the generic Hub startup upgrader.
use std::collections::BTreeMap;

use serde::Serialize;
use talos_error::TalosResult;

pub(super) const COMPUTER_IMMUTABLE: &[&str] = &["/data/ownerUserId"];
pub(super) const PROJECT_IMMUTABLE: &[&str] = &["/data/nodeId", "/data/directory"];
pub(super) const SESSION_IMMUTABLE: &[&str] = &[
    "/data/session/nodeId",
    "/data/binding",
    "/data/treeIdentity",
];
pub(super) const FILE_IMMUTABLE: &[&str] = &["/data/storagePeerId"];

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct UpgradeMapping {
    format_version: u32,
    source_schema_version: u32,
    target_schema_version: u32,
    namespaces: BTreeMap<&'static str, HeaderMapping>,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct HeaderMapping {
    owner_account: bool,
    related_namespace: Option<&'static str>,
    related_id_source: RelatedIdSource,
    immutable_fields: &'static [&'static str],
}
#[derive(Clone, Copy, Serialize)]
#[serde(rename_all = "camelCase")]
enum RelatedIdSource {
    ResourceId,
    StoredIndex,
}

/// Generate the declarative application profile consumed by Sync's v1 Hub upgrade.
/// The same immutable-field lists define normal Parser Header construction.
pub fn v1_upgrade_mapping_json() -> TalosResult<String> {
    serde_json::to_string_pretty(&UpgradeMapping {
        format_version: 1,
        source_schema_version: 1,
        target_schema_version: 2,
        namespaces: BTreeMap::from([
            (
                "computer",
                HeaderMapping {
                    owner_account: true,
                    related_namespace: Some("computer"),
                    related_id_source: RelatedIdSource::ResourceId,
                    immutable_fields: COMPUTER_IMMUTABLE,
                },
            ),
            (
                "project",
                HeaderMapping {
                    owner_account: false,
                    related_namespace: Some("computer"),
                    related_id_source: RelatedIdSource::StoredIndex,
                    immutable_fields: PROJECT_IMMUTABLE,
                },
            ),
            (
                "session",
                HeaderMapping {
                    owner_account: false,
                    related_namespace: Some("computer"),
                    related_id_source: RelatedIdSource::StoredIndex,
                    immutable_fields: SESSION_IMMUTABLE,
                },
            ),
            (
                "file",
                HeaderMapping {
                    owner_account: false,
                    related_namespace: None,
                    related_id_source: RelatedIdSource::StoredIndex,
                    immutable_fields: FILE_IMMUTABLE,
                },
            ),
        ]),
    })
    .map(|value| format!("{value}\n"))
    .map_err(crate::codec_error)
}

#[cfg(test)]
#[path = "upgrade_mapping_tests.rs"]
mod tests;
