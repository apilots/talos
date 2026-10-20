use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
pub enum BrowserTabAction {
    Open,
    Close,
    Focus,
}

/// Internal closed action, parsed before acquiring any browser authority.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, ts_rs::TS)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
#[serde(deny_unknown_fields)]
pub enum BrowserAction {
    State,
    Navigate {
        url: String,
    },
    Click {
        element: String,
    },
    Fill {
        element: String,
        text: String,
    },
    Key {
        key: String,
    },
    Scroll {
        delta_x: i32,
        delta_y: i32,
    },
    Tab {
        action: BrowserTabAction,
        tab_id: Option<String>,
        url: Option<String>,
    },
    Screenshot,
    Download,
    Upload {
        element: String,
        file_id: String,
    },
}
