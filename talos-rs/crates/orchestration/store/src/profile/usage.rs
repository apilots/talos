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

pub(super) fn accumulate_usage(left: TokenUsage, right: &TokenUsage) -> TalosResult<TokenUsage> {
    let input_tokens = left
        .input_tokens
        .checked_add(right.input_tokens)
        .ok_or_else(|| TalosError::StorageError("profile input token overflow".to_string()))?;
    let output_tokens = left
        .output_tokens
        .checked_add(right.output_tokens)
        .ok_or_else(|| TalosError::StorageError("profile output token overflow".to_string()))?;
    let total_tokens = input_tokens
        .checked_add(output_tokens)
        .ok_or_else(|| TalosError::StorageError("profile total token overflow".to_string()))?;
    Ok(TokenUsage {
        input_tokens,
        output_tokens,
        total_tokens,
        cache_creation_input_tokens: accumulate_optional(
            left.cache_creation_input_tokens,
            right.cache_creation_input_tokens,
        )?,
        cache_read_input_tokens: accumulate_optional(
            left.cache_read_input_tokens,
            right.cache_read_input_tokens,
        )?,
    })
}

pub(super) fn storage_error(action: &'static str) -> impl FnOnce(sqlx::Error) -> TalosError {
    move |error| TalosError::StorageError(format!("{action}: {error}"))
}

fn accumulate_optional(left: Option<u32>, right: Option<u32>) -> TalosResult<Option<u32>> {
    match (left, right) {
        (Some(left), Some(right)) => left
            .checked_add(right)
            .map(Some)
            .ok_or_else(|| TalosError::StorageError("profile cache token overflow".to_string())),
        _ => Ok(None),
    }
}

fn token_value(value: i64, name: &str) -> TalosResult<u32> {
    u32::try_from(value).map_err(|_| {
        TalosError::StorageError(format!("profile {name} is outside the supported range"))
    })
}

fn optional_token_value(value: Option<i64>, name: &str) -> TalosResult<Option<u32>> {
    value.map(|value| token_value(value, name)).transpose()
}
