use super::*;

#[tokio::test]
async fn upgrade_converts_model_metrics_and_retires_only_trace_intervals() {
    let pool = sqlx::SqlitePool::connect("sqlite::memory:").await.unwrap();
    sqlx::query("CREATE TABLE agent_events(event_seq INTEGER PRIMARY KEY, execution_capture_json TEXT, payload_json TEXT)").execute(&pool).await.unwrap();
    let old = json!({
        "clockSegments": [{"segmentId":"clock", "startedAt":100}],
        "requests":[{"request":{"callRecords":[{
            "timing":{"interval":{"segmentId":"clock", "startedOffsetMs":10, "endedOffsetMs":30, "elapsedMs":20},
                      "firstOutputOffsetMs":15, "lastOutputOffsetMs":25, "ttftMs":5, "generationMs":10},
            "usage":{"total_tokens":12}, "callRecordId":"call"
        }]}, "prompt":{"payloadId":"prompt"}}],
        "tools":[{"tool":{"callId":"read", "intervals":[]}}],
        "payloads":{"prompt":{"text":"original"}}
    });
    sqlx::query("INSERT INTO agent_events VALUES (1, ?, 'original-event')")
        .bind(old.to_string())
        .execute(&pool)
        .await
        .unwrap();
    let config = talos_config::Config::default();
    let paths = config.talos_paths();
    let context = UpgradeContext {
        config: &config,
        database_pool: &pool,
        paths: &paths,
    };
    run(&context).await.unwrap();
    let row = sqlx::query("SELECT * FROM agent_events")
        .fetch_one(&pool)
        .await
        .unwrap();
    let saved: Value =
        serde_json::from_str(&row.get::<String, _>("execution_capture_json")).unwrap();
    let call = &saved["requests"][0]["request"]["callRecords"][0];
    assert_eq!(call["metrics"]["startedAtMs"], 100010.0);
    assert_eq!(call["metrics"]["finishedAtMs"], 100030.0);
    assert_eq!(
        call["metrics"]["usage"],
        old["requests"][0]["request"]["callRecords"][0]["usage"]
    );
    assert!(call.get("timing").is_none());
    assert!(saved["tools"][0]["tool"].get("intervals").is_none());
    assert_eq!(saved["payloads"], old["payloads"]);
    assert_eq!(row.get::<String, _>("payload_json"), "original-event");
}
