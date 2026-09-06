use sqlx::Row;
use talos_error::{TalosError, TalosResult};
use talos_types::TokenUsage;

pub(super) fn usage_from_row(row: &sqlx::sqlite::SqliteRow) -> TalosResult<TokenUsage> {
    Ok(TokenUsage {
        input_tokens: token_value(
            row.try_get("input_tokens")
                .map_err(storage_error("read input tokens"))?,
            "input_tokens",
        )?,
        output_tokens: token_value(
            row.try_get("output_tokens")
                .map_err(storage_error("read output tokens"))?,
            "output_tokens",
        )?,
        total_tokens: token_value(
            row.try_get("total_tokens")
                .map_err(storage_error("read total tokens"))?,
            "total_tokens",
        )?,
        cache_creation_input_tokens: optional_token_value(
            row.try_get("cache_creation_input_tokens")
                .map_err(storage_error("read cache creation tokens"))?,
            "cache_creation_input_tokens",
        )?,
        cache_read_input_tokens: optional_token_value(
            row.try_get("cache_read_input_tokens")
                .map_err(storage_error("read cache read tokens"))?,
            "cache_read_input_tokens",
        )?,
    })
}

pub(super) fn usage_from_aggregate_row(row: &sqlx::sqlite::SqliteRow) -> TalosResult<TokenUsage> {
    Ok(TokenUsage {
        input_tokens: token_value(
            row.try_get::<Option<i64>, _>("input_tokens")
                .map_err(storage_error("read aggregate input tokens"))?
                .unwrap_or_default(),
            "input_tokens",
        )?,
        output_tokens: token_value(
            row.try_get::<Option<i64>, _>("output_tokens")
                .map_err(storage_error("read aggregate output tokens"))?
                .unwrap_or_default(),
            "output_tokens",
        )?,
        total_tokens: token_value(
            row.try_get::<Option<i64>, _>("total_tokens")
                .map_err(storage_error("read aggregate total tokens"))?
                .unwrap_or_default(),
            "total_tokens",
        )?,
        cache_creation_input_tokens: optional_token_value(
            row.try_get("cache_creation_input_tokens")
                .map_err(storage_error("read aggregate cache creation tokens"))?,
            "cache_creation_input_tokens",
        )?,
        cache_read_input_tokens: optional_token_value(
            row.try_get("cache_read_input_tokens")
                .map_err(storage_error("read aggregate cache read tokens"))?,
            "cache_read_input_tokens",
        )?,
    })
}

pub(super) fn storage_error(action: &'static str) -> impl FnOnce(sqlx::Error) -> TalosError {
    move |error| TalosError::internal(format!("{action}: {error}"))
}

fn token_value(value: i64, name: &str) -> TalosResult<u32> {
    u32::try_from(value)
        .map_err(|_| TalosError::internal(format!("profile {name} is outside the supported range")))
}

fn optional_token_value(value: Option<i64>, name: &str) -> TalosResult<Option<u32>> {
    value.map(|value| token_value(value, name)).transpose()
}
