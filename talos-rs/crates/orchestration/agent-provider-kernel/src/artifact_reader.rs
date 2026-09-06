use super::*;
use tokio::io::{AsyncReadExt, AsyncSeekExt, SeekFrom};

pub(super) async fn load_recent_artifacts(
    path: &Path,
    session_id: &str,
    limit: usize,
    matches: fn(&SessionArtifactRecord) -> bool,
) -> TalosResult<Vec<SessionArtifactRecord>> {
    const READ_CHUNK_BYTES: u64 = 8 * 1024;

    let mut file = tokio::fs::File::open(path).await.map_err(|error| {
        let diagnostic = format!("open session artifact {}: {error}", path.display());
        TalosError::internal_with_source(
            "Open session artifact; technical details were withheld.",
            error,
        )
        .context(diagnostic)
    })?;
    let mut position = file
        .metadata()
        .await
        .map_err(|error| {
            let diagnostic = format!("read session artifact metadata {}: {error}", path.display());
            TalosError::internal_with_source(
                "Read session artifact metadata; technical details were withheld.",
                error,
            )
            .context(diagnostic)
        })?
        .len();
    let mut carry = Vec::new();
    let mut records = Vec::with_capacity(limit);
    while position > 0 && records.len() < limit {
        let chunk_len = position.min(READ_CHUNK_BYTES);
        position -= chunk_len;
        file.seek(SeekFrom::Start(position))
            .await
            .map_err(|error| {
                let diagnostic = format!("seek session artifact {}: {error}", path.display());
                TalosError::internal_with_source(
                    "Seek session artifact; technical details were withheld.",
                    error,
                )
                .context(diagnostic)
            })?;
        let mut data = vec![0; chunk_len as usize];
        file.read_exact(&mut data).await.map_err(|error| {
            let diagnostic = format!("read session artifact {}: {error}", path.display());
            TalosError::internal_with_source(
                "Read session artifact; technical details were withheld.",
                error,
            )
            .context(diagnostic)
        })?;
        data.extend_from_slice(&carry);
        let complete_from = if position == 0 {
            0
        } else if let Some(index) = data.iter().position(|byte| *byte == b'\n') {
            carry = data[..index].to_vec();
            index.saturating_add(1)
        } else {
            carry = data;
            continue;
        };
        for line in data[complete_from..].rsplit(|byte| *byte == b'\n') {
            let line = std::str::from_utf8(line).map_err(|error| {
                let diagnostic = format!(
                    "decode session artifact {} during tail scan: {error}",
                    path.display()
                );
                TalosError::internal_with_source(
                    "Decode session artifact during tail scan; technical details were withheld.",
                    error,
                )
                .context(diagnostic)
            })?;
            if line.trim().is_empty() {
                continue;
            }
            let record = parse_artifact_record(path, "tail scan", line, session_id)?;
            if matches(&record) {
                records.push(record);
                if records.len() == limit {
                    break;
                }
            }
        }
    }
    Ok(records)
}
