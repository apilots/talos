use sqlx::{Row, SqlitePool};
use talos_error::{TalosError, TalosResult};

use super::usage::{accumulate_usage, storage_error, usage_from_row};
use super::TurnUsageUpdate;

pub(super) async fn record_turn_usage(
    pool: &SqlitePool,
    update: TurnUsageUpdate,
) -> TalosResult<()> {
    let mut transaction = pool
        .begin()
        .await
        .map_err(storage_error("begin profile usage update"))?;
    let existing = sqlx::query(
        "SELECT user_id, last_request_id, model_provider, model_id, input_tokens, output_tokens, total_tokens, cache_creation_input_tokens, cache_read_input_tokens, usage_available, finalized, timezone FROM profile_turn_usage WHERE session_id = ? AND turn_id = ?",
    )
    .bind(&update.session_id)
    .bind(&update.turn_id)
    .fetch_optional(&mut *transaction)
    .await
    .map_err(storage_error("read profile turn usage"))?;

    if update.is_discarded() {
        let finalized = match existing.as_ref() {
            Some(row) => {
                row.try_get::<i64, _>("finalized")
                    .map_err(storage_error("read profile finalization state"))?
                    != 0
            }
            None => false,
        };
        if !finalized {
            sqlx::query("DELETE FROM profile_turn_usage WHERE session_id = ? AND turn_id = ?")
                .bind(&update.session_id)
                .bind(&update.turn_id)
                .execute(&mut *transaction)
                .await
                .map_err(storage_error("discard cancelled profile turn usage"))?;
        }
        return transaction
            .commit()
            .await
            .map_err(storage_error("commit cancelled profile usage discard"));
    }

    if let Some(row) = existing {
        validate_existing_turn(&row, &update)?;
        if row
            .try_get::<String, _>("last_request_id")
            .map_err(storage_error("read profile request id"))?
            == update.request_id
        {
            transaction
                .commit()
                .await
                .map_err(storage_error("commit duplicate profile usage"))?;
            return Ok(());
        }
        if row
            .try_get::<i64, _>("finalized")
            .map_err(storage_error("read profile finalization state"))?
            != 0
        {
            transaction
                .commit()
                .await
                .map_err(storage_error("commit finalized profile replay"))?;
            return Ok(());
        }

        let usage = accumulate_usage(usage_from_row(&row)?, &update.usage)?;
        let usage_available = row
            .try_get::<i64, _>("usage_available")
            .map_err(storage_error("read profile usage availability"))?
            != 0
            && update.usage_available;
        sqlx::query(
            r#"
            UPDATE profile_turn_usage
            SET last_request_id = ?, input_tokens = ?, output_tokens = ?, total_tokens = ?,
                cache_creation_input_tokens = ?, cache_read_input_tokens = ?,
                usage_available = ?, finalized = ?,
                usage_date = CASE WHEN ? THEN ? ELSE usage_date END,
                completed_at_ms = CASE WHEN ? THEN ? ELSE completed_at_ms END
            WHERE session_id = ? AND turn_id = ?
            "#,
        )
        .bind(&update.request_id)
        .bind(i64::from(usage.input_tokens))
        .bind(i64::from(usage.output_tokens))
        .bind(i64::from(usage.total_tokens))
        .bind(usage.cache_creation_input_tokens.map(i64::from))
        .bind(usage.cache_read_input_tokens.map(i64::from))
        .bind(i64::from(u8::from(usage_available)))
        .bind(i64::from(u8::from(update.is_final())))
        .bind(update.is_final())
        .bind(&update.usage_date)
        .bind(update.is_final())
        .bind(update.completed_at_ms)
        .bind(&update.session_id)
        .bind(&update.turn_id)
        .execute(&mut *transaction)
        .await
        .map_err(storage_error("update profile turn usage"))?;
    } else {
        sqlx::query(
            r#"
            INSERT INTO profile_turn_usage (
                user_id, session_id, turn_id, last_request_id, model_provider, model_id,
                input_tokens, output_tokens, total_tokens,
                cache_creation_input_tokens, cache_read_input_tokens, usage_available,
                finalized, usage_date, timezone, completed_at_ms
            ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
            "#,
        )
        .bind(&update.user_id)
        .bind(&update.session_id)
        .bind(&update.turn_id)
        .bind(&update.request_id)
        .bind(&update.model_ref.provider)
        .bind(&update.model_ref.id)
        .bind(i64::from(update.usage.input_tokens))
        .bind(i64::from(update.usage.output_tokens))
        .bind(i64::from(update.usage.total_tokens))
        .bind(update.usage.cache_creation_input_tokens.map(i64::from))
        .bind(update.usage.cache_read_input_tokens.map(i64::from))
        .bind(i64::from(u8::from(update.usage_available)))
        .bind(i64::from(u8::from(update.is_final())))
        .bind(update.is_final().then_some(&update.usage_date))
        .bind(&update.timezone)
        .bind(update.is_final().then_some(update.completed_at_ms))
        .execute(&mut *transaction)
        .await
        .map_err(storage_error("insert profile turn usage"))?;
    }

    transaction
        .commit()
        .await
        .map_err(storage_error("commit profile usage update"))
}

pub(super) async fn discard_turn_usage(
    pool: &SqlitePool,
    session_id: &str,
    turn_id: &str,
) -> TalosResult<()> {
    sqlx::query("DELETE FROM profile_turn_usage WHERE session_id = ? AND turn_id = ?")
        .bind(session_id)
        .bind(turn_id)
        .execute(pool)
        .await
        .map_err(storage_error("discard failed profile turn usage"))?;
    Ok(())
}

pub(super) async fn delete_finalized_before(
    pool: &SqlitePool,
    completed_before_ms: i64,
    limit: usize,
) -> TalosResult<u64> {
    if limit == 0 {
        return Err(TalosError::ValidationError(
            "profile usage retention batch size must be > 0".to_string(),
        ));
    }
    let limit = i64::try_from(limit).map_err(|_| {
        TalosError::ValidationError("profile usage retention batch size is too large".to_string())
    })?;
    let result = sqlx::query(
        r#"
        DELETE FROM profile_turn_usage
        WHERE id IN (
            SELECT id
            FROM profile_turn_usage
            WHERE finalized = 1 AND completed_at_ms < ?
            ORDER BY completed_at_ms ASC, id ASC
            LIMIT ?
        )
        "#,
    )
    .bind(completed_before_ms)
    .bind(limit)
    .execute(pool)
    .await
    .map_err(storage_error("delete expired profile usage"))?;
    Ok(result.rows_affected())
}

fn validate_existing_turn(
    row: &sqlx::sqlite::SqliteRow,
    update: &TurnUsageUpdate,
) -> TalosResult<()> {
    let matches = row
        .try_get::<String, _>("user_id")
        .map_err(storage_error("read profile turn owner"))?
        == update.user_id
        && row
            .try_get::<String, _>("model_provider")
            .map_err(storage_error("read profile turn provider"))?
            == update.model_ref.provider
        && row
            .try_get::<String, _>("model_id")
            .map_err(storage_error("read profile turn model"))?
            == update.model_ref.id
        && row
            .try_get::<String, _>("timezone")
            .map_err(storage_error("read profile turn timezone"))?
            == update.timezone;
    if matches {
        Ok(())
    } else {
        Err(TalosError::Conflict(
            "profile turn usage context changed during a logical turn".to_string(),
        ))
    }
}
