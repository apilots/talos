//! One-time retirement of the 0.1.14 tracing interval representation.
use crate::runner::{
    UpgradeContext, UpgradeFuture, UpgradeTask, UpgradeTaskDefinition, UpgradeTaskRegistration,
};
use semver::Version;
use serde_json::{json, Value};
use sqlx::Row;
use talos_error::TalosError;

pub(crate) fn upgrade_task() -> Box<dyn UpgradeTask> {
    Box::new(UpgradeTaskRegistration::new(
        UpgradeTaskDefinition {
            id: "202610310001_execution_metrics",
            revision: 1,
            target_version: Version::new(0, 1, 15),
            priority: 200,
        },
        run,
        |_| Box::pin(async { Ok(()) }),
    ))
}
fn error(source: impl std::error::Error + Send + Sync + 'static) -> TalosError {
    TalosError::internal_with_source("Upgrade execution metrics failed.", source)
}
fn run<'a>(context: &'a UpgradeContext<'_>) -> UpgradeFuture<'a> {
    Box::pin(async move {
        let mut tx = context.database_pool.begin().await.map_err(error)?;
        let mut cursor = 0_i64;
        loop {
            let rows = sqlx::query("SELECT event_seq, execution_capture_json FROM agent_events WHERE event_seq > ? AND execution_capture_json IS NOT NULL ORDER BY event_seq LIMIT 32")
                .bind(cursor).fetch_all(&mut *tx).await.map_err(error)?;
            if rows.is_empty() {
                break;
            }
            for row in rows {
                cursor = row.get("event_seq");
                let old: String = row.get("execution_capture_json");
                let mut value: Value = serde_json::from_str(&old).map_err(error)?;
                convert(&mut value);
                let new = serde_json::to_string(&value).map_err(error)?;
                if new != old {
                    sqlx::query(
                        "UPDATE agent_events SET execution_capture_json = ? WHERE event_seq = ?",
                    )
                    .bind(new)
                    .bind(cursor)
                    .execute(&mut *tx)
                    .await
                    .map_err(error)?;
                }
            }
        }
        tx.commit().await.map_err(error)
    })
}
fn convert(value: &mut Value) {
    let clocks: std::collections::HashMap<_, _> = value["clockSegments"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|c| {
            Some((
                c["segmentId"].as_str()?.to_owned(),
                c["startedAt"].as_f64()? * 1000.0,
            ))
        })
        .collect();
    for request in value["requests"].as_array_mut().into_iter().flatten() {
        for call in request["request"]["callRecords"]
            .as_array_mut()
            .into_iter()
            .flatten()
        {
            let Some(record) = call.as_object_mut() else {
                continue;
            };
            let Some(timing) = record.remove("timing") else {
                continue;
            };
            let usage = record.remove("usage").unwrap_or(Value::Null);
            let interval = &timing["interval"];
            let base = interval["segmentId"]
                .as_str()
                .and_then(|id| clocks.get(id))
                .copied();
            let timestamp = |offset: &Value| {
                base.zip(offset.as_f64())
                    .map(|(base, offset)| base + offset)
            };
            record.insert("metrics".into(), if timing.is_null() && usage.is_null() { Value::Null } else { json!({
                "segmentId": interval["segmentId"].as_str().unwrap_or_default(),
                "startedAtMs": timestamp(&interval["startedOffsetMs"]),
                "finishedAtMs": timestamp(&interval["endedOffsetMs"]),
                "executionDurationMs": interval["elapsedMs"],
                "firstOutputAtMs": timestamp(&timing["firstOutputOffsetMs"]),
                "lastOutputAtMs": timestamp(&timing["lastOutputOffsetMs"]),
                "ttftMs": timing["ttftMs"], "generationMs": timing["generationMs"], "usage": usage,
            }) });
        }
    }
    // Do not rewrite canonical, possibly synchronized ToolResult events. Historical
    // outputs that never carried task-owned metrics remain empty, as authorized.
    for tool in value["tools"].as_array_mut().into_iter().flatten() {
        if let Some(tool) = tool["tool"].as_object_mut() {
            tool.remove("intervals");
        }
    }
}

#[cfg(test)]
#[path = "execution_metrics_tests.rs"]
mod tests;
