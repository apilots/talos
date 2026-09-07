use sqlx::{Row, SqlitePool};
use talos_error::{TalosError, TalosResult};
use talos_types::{ModelRef, TokenUsage};

use super::usage::{storage_error, usage_from_aggregate_row};
use super::{ProfileDailyUsageSummary, ProfileModelUsageSummary, ProfileUsageSummary};

pub(super) async fn usage_summary(
    pool: &SqlitePool,
    user_id: &str,
) -> TalosResult<ProfileUsageSummary> {
    let row = sqlx::query(
        r#"
        SELECT COUNT(*) AS usage_count,
               COUNT(DISTINCT CASE WHEN usage_kind = 'turn' THEN session_id END) AS usage_session_count,
               SUM(input_tokens) AS input_tokens,
               SUM(output_tokens) AS output_tokens, SUM(total_tokens) AS total_tokens,
               CASE WHEN COUNT(cache_creation_input_tokens) = COUNT(*)
                    THEN SUM(cache_creation_input_tokens) END AS cache_creation_input_tokens,
               CASE WHEN COUNT(cache_read_input_tokens) = COUNT(*)
                    THEN SUM(cache_read_input_tokens) END AS cache_read_input_tokens,
               MIN(usage_available) AS usage_available,
               MIN(completed_at_ms) AS usage_started_at
        FROM profile_turn_usage
        WHERE user_id = ? AND finalized = 1
        "#,
    )
    .bind(user_id)
    .fetch_one(pool)
    .await
    .map_err(storage_error("read profile usage summary"))?;
    let usage_count = row
        .try_get::<i64, _>("usage_count")
        .map_err(storage_error("read profile usage count"))?;
    if usage_count == 0 {
        return Ok(ProfileUsageSummary {
            usage: TokenUsage::default(),
            usage_available: false,
            usage_session_count: 0,
            usage_started_at_ms: None,
        });
    }
    Ok(ProfileUsageSummary {
        usage: usage_from_aggregate_row(&row)?,
        usage_available: row
            .try_get::<Option<i64>, _>("usage_available")
            .map_err(storage_error("read profile usage availability"))?
            .unwrap_or_default()
            != 0,
        usage_session_count: u64::try_from(
            row.try_get::<i64, _>("usage_session_count")
                .map_err(storage_error("read profile usage session count"))?,
        )
        .map_err(|_| TalosError::internal("profile usage session count is negative"))?,
        usage_started_at_ms: row
            .try_get("usage_started_at")
            .map_err(storage_error("read profile usage start"))?,
    })
}

pub(super) async fn daily_usage(
    pool: &SqlitePool,
    user_id: &str,
    start_date: &str,
    end_date: &str,
) -> TalosResult<Vec<ProfileDailyUsageSummary>> {
    let rows = sqlx::query(
        r#"
        SELECT usage_date, SUM(input_tokens) AS input_tokens,
               SUM(output_tokens) AS output_tokens, SUM(total_tokens) AS total_tokens,
               CASE WHEN COUNT(cache_creation_input_tokens) = COUNT(*)
                    THEN SUM(cache_creation_input_tokens) END AS cache_creation_input_tokens,
               CASE WHEN COUNT(cache_read_input_tokens) = COUNT(*)
                    THEN SUM(cache_read_input_tokens) END AS cache_read_input_tokens,
               MIN(usage_available) AS usage_available
        FROM profile_turn_usage
        WHERE user_id = ? AND finalized = 1 AND usage_date BETWEEN ? AND ?
        GROUP BY usage_date
        ORDER BY usage_date ASC
        "#,
    )
    .bind(user_id)
    .bind(start_date)
    .bind(end_date)
    .fetch_all(pool)
    .await
    .map_err(storage_error("read profile daily usage"))?;
    rows.iter()
        .map(|row| {
            Ok(ProfileDailyUsageSummary {
                date: row
                    .try_get("usage_date")
                    .map_err(storage_error("read profile usage date"))?,
                usage: usage_from_aggregate_row(row)?,
                usage_available: row
                    .try_get::<Option<i64>, _>("usage_available")
                    .map_err(storage_error("read profile day availability"))?
                    .unwrap_or_default()
                    != 0,
            })
        })
        .collect()
}

pub(super) async fn model_usage(
    pool: &SqlitePool,
    user_id: &str,
    limit: usize,
) -> TalosResult<Vec<ProfileModelUsageSummary>> {
    let limit = i64::try_from(limit.max(1)).map_err(|_| {
        TalosError::new(
            talos_error::ErrorCode::InvalidRequest,
            "profile model limit is too large".to_string(),
        )
    })?;
    let rows = sqlx::query(
        r#"
        SELECT model_provider, model_id,
               COUNT(*) AS usage_count,
               SUM(CASE WHEN usage_kind = 'turn' THEN 1 ELSE 0 END) AS turn_count,
               SUM(input_tokens) AS input_tokens, SUM(output_tokens) AS output_tokens,
               SUM(total_tokens) AS total_tokens,
               CASE WHEN COUNT(cache_creation_input_tokens) = COUNT(*)
                    THEN SUM(cache_creation_input_tokens) END AS cache_creation_input_tokens,
               CASE WHEN COUNT(cache_read_input_tokens) = COUNT(*)
                    THEN SUM(cache_read_input_tokens) END AS cache_read_input_tokens,
               MIN(usage_available) AS usage_available
        FROM profile_turn_usage
        WHERE user_id = ? AND finalized = 1
        GROUP BY model_provider, model_id
        ORDER BY total_tokens DESC, usage_count DESC, model_provider ASC, model_id ASC
        LIMIT ?
        "#,
    )
    .bind(user_id)
    .bind(limit)
    .fetch_all(pool)
    .await
    .map_err(storage_error("read profile model usage"))?;
    rows.iter()
        .map(|row| {
            let turn_count = row
                .try_get::<i64, _>("turn_count")
                .map_err(storage_error("read profile model turn count"))?;
            Ok(ProfileModelUsageSummary {
                model_ref: ModelRef {
                    provider: row
                        .try_get("model_provider")
                        .map_err(storage_error("read profile model provider"))?,
                    id: row
                        .try_get("model_id")
                        .map_err(storage_error("read profile model id"))?,
                },
                turn_count: u64::try_from(turn_count)
                    .map_err(|_| TalosError::internal("profile model turn count is negative"))?,
                usage: usage_from_aggregate_row(row)?,
                usage_available: row
                    .try_get::<Option<i64>, _>("usage_available")
                    .map_err(storage_error("read profile model availability"))?
                    .unwrap_or_default()
                    != 0,
            })
        })
        .collect()
}
