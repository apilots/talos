use serde_json::{json, Value};
use talos_error::{ErrorCode, TalosError, TalosResult};
pub use talos_types::{BrowserAction, BrowserTabAction};

pub trait BrowserActionExt {
    fn parse(tool: &str, arguments: Value) -> TalosResult<BrowserAction>;
    fn catalog() -> Vec<Value>;
    fn is_read_only(&self) -> bool;
}

impl BrowserActionExt for BrowserAction {
    fn parse(tool: &str, mut arguments: Value) -> TalosResult<Self> {
        let kind = match tool {
            "browser_state" => "state",
            "browser_navigate" => "navigate",
            "browser_click" => "click",
            "browser_fill" => "fill",
            "browser_key" => "key",
            "browser_scroll" => "scroll",
            "browser_tab" => "tab",
            "browser_screenshot" => "screenshot",
            "browser_download" => "download",
            "browser_upload" => "upload",
            _ => return Err(invalid("Unknown Browser tool.")),
        };
        if arguments.is_null() {
            arguments = json!({});
        }
        let map = arguments
            .as_object_mut()
            .ok_or_else(|| invalid("Browser arguments must be an object."))?;
        if map.contains_key("kind") {
            return Err(invalid(
                "Browser arguments cannot select their own operation.",
            ));
        }
        map.insert("kind".into(), json!(kind));
        let action: Self = serde_json::from_value(arguments).map_err(|error| {
            invalid("Browser arguments do not match the tool schema.").with_source(error)
        })?;
        if serde_json::to_vec(&action)
            .map_err(|error| {
                TalosError::internal_with_source("Failed to encode browser action.", error)
            })?
            .len()
            > 32 * 1024
        {
            return Err(invalid("Browser tool input exceeds 32 KiB."));
        }
        if let Self::Navigate { url } | Self::Tab { url: Some(url), .. } = &action {
            if !url.starts_with("https://") && !url.starts_with("http://") && url != "about:blank" {
                return Err(invalid(
                    "Browser navigation requires HTTP, HTTPS or about:blank.",
                ));
            }
            let parsed = url::Url::parse(url)
                .map_err(|error| invalid("Browser URL is invalid.").with_source(error))?;
            if url != "about:blank"
                && (parsed.host_str().is_none()
                    || !parsed.username().is_empty()
                    || parsed.password().is_some())
            {
                return Err(invalid(
                    "Browser navigation cannot contain URL credentials.",
                ));
            }
            if url.len() > 8192 || url.chars().any(char::is_control) {
                return Err(invalid("Browser URL is invalid or too long."));
            }
        }
        Ok(action)
    }

    fn is_read_only(&self) -> bool {
        matches!(self, Self::State | Self::Screenshot | Self::Download)
    }

    fn catalog() -> Vec<Value> {
        let element = json!({"type":"string","description":"Element reference from the latest browser_state, including its document revision."});
        let string = json!({"type":"string"});
        let entries = [
            (
                "browser_state",
                "Inspect the current page and bounded element references.",
                json!({}),
                vec![],
            ),
            (
                "browser_navigate",
                "Navigate the selected browser to an HTTP(S) URL.",
                json!({"url":string}),
                vec!["url"],
            ),
            (
                "browser_click",
                "Click an element from the current document revision.",
                json!({"element":element}),
                vec!["element"],
            ),
            (
                "browser_fill",
                "Fill a text control using its current element reference.",
                json!({"element":element,"text":string}),
                vec!["element", "text"],
            ),
            (
                "browser_key",
                "Press a named key or key chord in the current page.",
                json!({"key":string}),
                vec!["key"],
            ),
            (
                "browser_scroll",
                "Scroll the current page in CSS pixels.",
                json!({"deltaX":{"type":"integer"},"deltaY":{"type":"integer"}}),
                vec!["deltaX", "deltaY"],
            ),
            (
                "browser_tab",
                "Open, focus or close a browser tab; only authorized tabs may be changed.",
                json!({"action":{"enum":["open","close","focus"]},"tabId":string,"url":string}),
                vec!["action"],
            ),
            (
                "browser_screenshot",
                "Capture the current browser page as an image.",
                json!({}),
                vec![],
            ),
            (
                "browser_download",
                "Read completed downloads through the authorized file boundary.",
                json!({}),
                vec![],
            ),
            (
                "browser_upload",
                "Upload an already authorized Talos attachment to a file input.",
                json!({"element":element,"fileId":string}),
                vec!["element", "fileId"],
            ),
        ];
        entries.into_iter().map(|(name,description,properties,required)| json!({
            "name":name,"description":description,
            "inputSchema":{"type":"object","properties":properties,"required":required,"additionalProperties":false},
            "annotations":{"readOnlyHint":matches!(name,"browser_state"|"browser_screenshot"|"browser_download"),"openWorldHint":true}
        })).collect()
    }
}

fn invalid(message: &'static str) -> TalosError {
    TalosError::new(ErrorCode::InvalidRequest, message)
}
