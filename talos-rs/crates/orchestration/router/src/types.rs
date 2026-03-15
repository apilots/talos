use serde::{Deserialize, Serialize};

// 路由匹配规则
#[derive(Debug, Clone, PartialEq)]
pub enum MatchRule {
    Peer(String),    // 精确匹配用户
    Channel(String), // 匹配渠道
    Default,         // 默认路由
}

// 路由绑定
#[derive(Debug, Clone)]
pub struct Binding {
    pub id: String,
    pub agent_id: String,
    pub rule: MatchRule,
    pub priority: u32,
}

// 路由器配置
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RouterConfig {
    pub bindings: Vec<BindingConfig>,
}

// 绑定配置
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BindingConfig {
    pub id: String,
    pub agent_id: String,
    pub rule_type: String, // "peer", "channel", "default"
    pub rule_value: Option<String>,
    pub priority: u32,
}
