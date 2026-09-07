use crate::{ReasoningBlock, ReasoningContinuation};
use serde_json::Value;
use talos_error::{TalosError, TalosResult};

#[derive(Debug)]
struct ResponsesReasoningItem {
    id: String,
    summary: Vec<Value>,
    content: Vec<Value>,
    encrypted_content: Option<String>,
}

impl ResponsesReasoningItem {
    fn new(id: String) -> Self {
        Self {
            id,
            summary: Vec::new(),
            content: Vec::new(),
            encrypted_content: None,
        }
    }

    fn into_value(self) -> Value {
        let mut value = serde_json::json!({
            "type": "reasoning",
            "id": self.id,
        });
        let object = value
            .as_object_mut()
            .expect("reasoning item should serialize as object");
        if !self.summary.is_empty() {
            object.insert("summary".to_string(), Value::Array(self.summary));
        }
        if !self.content.is_empty() {
            object.insert("content".to_string(), Value::Array(self.content));
        }
        if let Some(encrypted_content) = self.encrypted_content {
            object.insert(
                "encrypted_content".to_string(),
                Value::String(encrypted_content),
            );
        }
        value
    }
}

pub(super) fn items(continuation: Option<ReasoningContinuation>) -> TalosResult<Vec<Value>> {
    let Some(continuation) = continuation else {
        return Ok(Vec::new());
    };
    let ReasoningContinuation::Blocks(blocks) = continuation else {
        return Err(TalosError::new(
            talos_error::ErrorCode::InvalidRequest,
            "Responses protocol requires structured reasoning continuation".to_string(),
        ));
    };

    let mut items: Vec<ResponsesReasoningItem> = Vec::new();
    for block in blocks {
        let provider_item_id = match &block {
            ReasoningBlock::Text(block) => block.provider_item_id.as_ref(),
            ReasoningBlock::Summary(block) => block.provider_item_id.as_ref(),
            ReasoningBlock::Encrypted(block) => block.provider_item_id.as_ref(),
        }
        .cloned()
        .ok_or_else(|| {
            TalosError::new(
                talos_error::ErrorCode::InvalidRequest,
                "Responses reasoning continuation requires a provider item id".to_string(),
            )
        })?;

        if items.last().is_none_or(|item| item.id != provider_item_id) {
            items.push(ResponsesReasoningItem::new(provider_item_id));
        }
        let item = items
            .last_mut()
            .expect("reasoning item should exist after insertion");
        match block {
            ReasoningBlock::Text(block) => item.content.push(serde_json::json!({
                "type": "reasoning_text",
                "text": block.text,
            })),
            ReasoningBlock::Summary(block) => item.summary.push(serde_json::json!({
                "type": "summary_text",
                "text": block.text,
            })),
            ReasoningBlock::Encrypted(block) => {
                if item.encrypted_content.replace(block.data).is_some() {
                    return Err(TalosError::new(
                        talos_error::ErrorCode::InvalidRequest,
                        format!(
                            "Responses reasoning item '{}' contains multiple encrypted blocks",
                            item.id
                        ),
                    ));
                }
            }
        }
    }

    Ok(items
        .into_iter()
        .map(ResponsesReasoningItem::into_value)
        .collect())
}
