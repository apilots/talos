use async_trait::async_trait;
use serde_json::Value;
use talos_error::{TalosError, TalosResult};
use talos_types::talos::TalosContentItem;

use crate::runtimes::web_search::{run_with_config, WebSearchRequest};
use crate::spec::{ToolBehaviorFlags, ToolResultPolicy};
use crate::{Tool, ToolContext, ToolOutputPayload};

pub struct WebSearchTool;

#[async_trait]
impl Tool for WebSearchTool {
    fn name(&self) -> &str {
        "WebSearch"
    }

    fn internal_name(&self) -> &str {
        "web_search"
    }

    fn aliases(&self) -> &'static [&'static str] {
        &["web_search"]
    }

    fn description(&self) -> &str {
        "Search the public web for fresh information and return structured result hits."
    }

    fn behavior_flags(&self) -> ToolBehaviorFlags {
        ToolBehaviorFlags {
            read_only: true,
            destructive: false,
            concurrency_safe: true,
            requires_user_interaction: false,
        }
    }

    fn result_policy(&self) -> ToolResultPolicy {
        ToolResultPolicy {
            max_inline_bytes: 48 * 1024,
            persist_large_output: false,
            preview_lines: 50,
        }
    }

    fn parameters_schema(&self) -> Option<Value> {
        Some(serde_json::json!({
            "type": "object",
            "required": ["query"],
            "properties": {
                "query": {
                    "type": "string",
                    "description": "Search query text."
                },
                "allowed_domains": {
                    "type": "array",
                    "items": { "type": "string" },
                    "description": "Optional domain allowlist."
                },
                "blocked_domains": {
                    "type": "array",
                    "items": { "type": "string" },
                    "description": "Optional domain blocklist."
                },
                "domains": {
                    "type": "array",
                    "items": { "type": "string" },
                    "description": "Alias for allowed_domains."
                },
                "recency_days": {
                    "type": "integer",
                    "minimum": 1,
                    "description": "Requested freshness window in days. Provider support is exposed in the structured result."
                },
                "limit": {
                    "type": "integer",
                    "minimum": 1,
                    "description": "Maximum number of hits to return."
                }
            }
        }))
    }

    async fn execute_payload(
        &self,
        input: Value,
        context: ToolContext,
    ) -> TalosResult<ToolOutputPayload> {
        let request: WebSearchRequest = serde_json::from_value(input)
            .map_err(|err| TalosError::ToolError(format!("invalid web_search arguments: {err}")))?;
        let default_config;
        let config = if let Some(kernel_config) = context.kernel_config() {
            &kernel_config.tools.web_search
        } else {
            default_config = talos_config::WebSearchToolConfig::default();
            &default_config
        };
        let response = run_with_config(&request, config).await?;
        let structured = serde_json::to_value(&response).map_err(|err| {
            TalosError::ToolError(format!("serialize web_search response: {err}"))
        })?;
        let mut content_items = response
            .hits
            .iter()
            .enumerate()
            .map(|(index, hit)| TalosContentItem::WebReference {
                url: hit.url.clone(),
                title: Some(hit.title.clone()),
                citation_id: Some(format!("web-{}", index + 1)),
                snippet: Some(hit.snippet.clone()).filter(|value| !value.trim().is_empty()),
                source_domain: hit.source_domain.clone(),
                published_at: hit.published_at.clone(),
                provider_rank: hit.provider_rank,
            })
            .collect::<Vec<_>>();
        if content_items.is_empty() {
            content_items.push(TalosContentItem::Text {
                text: response.summary.clone(),
            });
        }
        Ok(ToolOutputPayload::success_with_content_items(
            structured,
            content_items,
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use talos_config::AgentKernelConfig;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;

    async fn spawn_json_server(body: &'static str) -> String {
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("test server should bind");
        let addr = listener
            .local_addr()
            .expect("test server addr should exist");
        tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.expect("request should arrive");
            let mut buf = vec![0; 4096];
            let _ = socket.read(&mut buf).await.expect("request should read");
            let response = format!(
                "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{}",
                body.len(),
                body
            );
            socket
                .write_all(response.as_bytes())
                .await
                .expect("response should write");
        });
        format!("http://{addr}")
    }

    #[tokio::test]
    async fn execute_payload_reads_web_search_provider_config_from_context() {
        let base_url = spawn_json_server(
            r#"{
                "Results": [
                    {
                        "FirstURL": "https://www.rust-lang.org/tools",
                        "Text": "Rust Tools - Official tools page"
                    }
                ]
            }"#,
        )
        .await;
        let mut kernel_config = AgentKernelConfig::default();
        kernel_config.tools.web_search.duckduckgo.base_url = Some(base_url);
        let context = ToolContext::new_with_kernel_config(
            "web-search-session",
            "user-1",
            Some(Arc::new(kernel_config)),
        );

        let payload = WebSearchTool
            .execute_payload(
                serde_json::json!({
                    "query": "rust tools",
                    "allowed_domains": ["rust-lang.org"],
                    "limit": 1
                }),
                context,
            )
            .await
            .expect("web search should execute");

        let ToolOutputPayload::Function {
            success,
            structured_content,
            content_items,
            ..
        } = payload
        else {
            panic!("web search should return a function payload");
        };
        assert!(success);
        assert_eq!(
            structured_content
                .as_ref()
                .and_then(|value| value.get("provider"))
                .and_then(Value::as_str),
            Some("duckduckgo")
        );
        assert!(matches!(
            content_items.first(),
            Some(TalosContentItem::WebReference { url, .. })
                if url == "https://www.rust-lang.org/tools"
        ));
    }
}
