use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use talos_error::TalosResult;

/// 工具策略类型
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum PolicyType {
    Allow, // 白名单模式 - 只允许列出的工具
    Deny,  // 黑名单模式 - 禁止列出的工具
}

/// 工具策略配置
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolPolicy {
    /// 策略类型
    pub policy_type: PolicyType,
    /// 工具列表
    pub tools: HashSet<String>,
    /// 是否启用策略
    pub enabled: bool,
}

impl Default for ToolPolicy {
    fn default() -> Self {
        Self {
            policy_type: PolicyType::Allow,
            tools: vec![
                "read".to_string(),
                "write".to_string(),
                "edit".to_string(),
                "exec".to_string(),
                "memory_search".to_string(),
                "memory_get".to_string(),
                "web_search".to_string(),
                "web_fetch".to_string(),
                "http_request".to_string(),
                "echo_test".to_string(),
            ]
            .into_iter()
            .collect(),
            enabled: true,
        }
    }
}

impl ToolPolicy {
    /// 创建一个新的白名单策略
    pub fn allow_list(tools: Vec<&str>) -> Self {
        Self {
            policy_type: PolicyType::Allow,
            tools: tools.into_iter().map(|s| s.to_string()).collect(),
            enabled: true,
        }
    }

    /// 创建一个新的黑名单策略
    pub fn deny_list(tools: Vec<&str>) -> Self {
        Self {
            policy_type: PolicyType::Deny,
            tools: tools.into_iter().map(|s| s.to_string()).collect(),
            enabled: true,
        }
    }

    /// 检查工具是否被允许
    pub fn is_allowed(&self, tool_name: &str) -> bool {
        if !self.enabled {
            return true; // 策略未启用，所有工具都被允许
        }

        match self.policy_type {
            PolicyType::Allow => self.tools.contains(tool_name),
            PolicyType::Deny => !self.tools.contains(tool_name),
        }
    }

    /// 添加工具到策略
    pub fn add_tool(&mut self, tool_name: &str) {
        self.tools.insert(tool_name.to_string());
    }

    /// 移除工具从策略
    pub fn remove_tool(&mut self, tool_name: &str) {
        self.tools.remove(tool_name);
    }

    /// 设置策略类型
    pub fn set_policy_type(&mut self, policy_type: PolicyType) {
        self.policy_type = policy_type;
    }

    /// 启用策略
    pub fn enable(&mut self) {
        self.enabled = true;
    }

    /// 禁用策略
    pub fn disable(&mut self) {
        self.enabled = false;
    }
}

/// 工具策略检查器
pub struct ToolPolicyChecker {
    pub policy: ToolPolicy,
}

impl ToolPolicyChecker {
    /// 创建一个新的策略检查器
    pub fn new(policy: ToolPolicy) -> Self {
        Self { policy }
    }

    /// 检查工具执行是否被允许
    pub fn check_tool_access(&self, tool_name: &str) -> TalosResult<()> {
        if self.policy.is_allowed(tool_name) {
            Ok(())
        } else {
            Err(talos_error::TalosError::AuthorizationDenied)
        }
    }

    /// 批量检查多个工具
    pub fn check_tools_access(&self, tool_names: &[String]) -> TalosResult<()> {
        for tool_name in tool_names {
            self.check_tool_access(tool_name)?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_allow_policy() {
        let policy = ToolPolicy::allow_list(vec!["read", "write"]);

        assert!(policy.is_allowed("read"));
        assert!(policy.is_allowed("write"));
        assert!(!policy.is_allowed("exec"));
        assert!(!policy.is_allowed("delete"));
    }

    #[test]
    fn test_deny_policy() {
        let policy = ToolPolicy::deny_list(vec!["exec", "delete"]);

        assert!(policy.is_allowed("read"));
        assert!(policy.is_allowed("write"));
        assert!(!policy.is_allowed("exec"));
        assert!(!policy.is_allowed("delete"));
    }

    #[test]
    fn test_disabled_policy() {
        let mut policy = ToolPolicy::allow_list(vec!["read"]);
        policy.disable();

        assert!(policy.is_allowed("read")); // 读取被允许，因为策略已禁用
        assert!(policy.is_allowed("exec")); // 执行也被允许，因为策略已禁用
    }

    #[test]
    fn test_policy_modification() {
        let mut policy = ToolPolicy::allow_list(vec![]);

        // 添加工具
        policy.add_tool("read");
        assert!(policy.is_allowed("read"));

        // 移除工具
        policy.remove_tool("read");
        assert!(!policy.is_allowed("read"));
    }

    #[test]
    fn test_policy_checker() {
        let policy = ToolPolicy::allow_list(vec!["read", "write"]);
        let checker = ToolPolicyChecker::new(policy);

        // 允许的工具
        assert!(checker.check_tool_access("read").is_ok());
        assert!(checker.check_tool_access("write").is_ok());

        // 不允许的工具
        assert!(checker.check_tool_access("exec").is_err());
    }

    #[test]
    fn test_batch_check() {
        let policy = ToolPolicy::allow_list(vec!["read", "write"]);
        let checker = ToolPolicyChecker::new(policy);

        // 全部允许的工具
        assert!(checker
            .check_tools_access(&vec!["read".to_string(), "write".to_string()])
            .is_ok());

        // 包含不允许的工具
        assert!(checker
            .check_tools_access(&vec!["read".to_string(), "exec".to_string()])
            .is_err());
    }

    #[test]
    fn test_default_policy_allows_memory_tools() {
        let policy = ToolPolicy::default();

        assert!(policy.is_allowed("memory_search"));
        assert!(policy.is_allowed("memory_get"));
    }
}
