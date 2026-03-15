//! Talos Event Bus
//!
//! 事件总线实现 - 发布/订阅模式

use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::mpsc;
use tokio::sync::{broadcast, RwLock};
use tracing::{debug, info, warn};

use talos_error::{TalosError, TalosResult};

// ============================================================================
// 事件总线
// ============================================================================

/// 事件总线配置
#[derive(Debug, Clone)]
pub struct EventBusConfig {
    pub buffer_size: usize,
    pub max_subscribers_per_topic: usize,
}

impl Default for EventBusConfig {
    fn default() -> Self {
        Self {
            buffer_size: 1024,
            max_subscribers_per_topic: 100,
        }
    }
}

/// 事件总线
pub struct EventBus {
    config: EventBusConfig,
    /// 全局事件广播
    global_tx: broadcast::Sender<BusEvent>,
    /// 按会话的事件广播
    session_channels: RwLock<HashMap<String, broadcast::Sender<BusEvent>>>,
    /// 按订阅者 ID 的事件广播
    subscriber_channels: RwLock<HashMap<String, broadcast::Sender<BusEvent>>>,
    /// 订阅者管理
    subscribers: Arc<SubscriberRegistry>,
}

impl EventBus {
    /// 创建新的事件总线
    pub fn new(config: EventBusConfig) -> Self {
        let (global_tx, _) = broadcast::channel(config.buffer_size);

        Self {
            config,
            global_tx,
            session_channels: RwLock::new(HashMap::new()),
            subscriber_channels: RwLock::new(HashMap::new()),
            subscribers: Arc::new(SubscriberRegistry::new()),
        }
    }

    /// 发布全局事件
    pub fn publish(&self, event: BusEvent) -> TalosResult<()> {
        self.global_tx
            .send(event)
            .map_err(|_| TalosError::GatewayError("Event bus closed".into()))?;
        Ok(())
    }

    /// 发布到特定会话
    pub async fn publish_to_session(&self, session_id: &str, event: BusEvent) -> TalosResult<()> {
        let channels = self.session_channels.read().await;
        if let Some(tx) = channels.get(session_id) {
            tx.send(event).ok();
        }
        Ok(())
    }

    /// 发布到特定订阅者
    pub async fn publish_to_subscriber(
        &self,
        subscriber_id: &str,
        event: BusEvent,
    ) -> TalosResult<()> {
        let channels = self.subscriber_channels.read().await;
        if let Some(tx) = channels.get(subscriber_id) {
            tx.send(event).ok();
        }
        Ok(())
    }

    /// 订阅全局事件
    pub fn subscribe(&self) -> broadcast::Receiver<BusEvent> {
        self.global_tx.subscribe()
    }

    /// 订阅特定会话的事件
    pub async fn subscribe_session(
        &self,
        session_id: &str,
    ) -> TalosResult<broadcast::Receiver<BusEvent>> {
        let mut channels = self.session_channels.write().await;

        let tx = channels
            .entry(session_id.to_string())
            .or_insert_with(|| broadcast::channel(self.config.buffer_size).0);

        Ok(tx.subscribe())
    }

    /// 注册订阅者
    pub async fn register_subscriber(
        &self,
        subscriber_id: &str,
    ) -> TalosResult<broadcast::Receiver<BusEvent>> {
        let mut channels = self.subscriber_channels.write().await;

        if channels.len() >= self.config.max_subscribers_per_topic {
            return Err(TalosError::GatewayError("Max subscribers reached".into()));
        }

        let tx = channels
            .entry(subscriber_id.to_string())
            .or_insert_with(|| broadcast::channel(self.config.buffer_size).0);

        self.subscribers.add(subscriber_id.to_string()).await;

        Ok(tx.subscribe())
    }

    /// 取消订阅者
    pub async fn unregister_subscriber(&self, subscriber_id: &str) -> TalosResult<()> {
        self.subscribers.remove(subscriber_id).await;

        let mut channels = self.subscriber_channels.write().await;
        channels.remove(subscriber_id);

        Ok(())
    }

    /// 清理会话频道
    pub async fn cleanup_session(&self, session_id: &str) -> TalosResult<()> {
        let mut channels = self.session_channels.write().await;
        channels.remove(session_id);
        Ok(())
    }

    /// 获取活跃会话数
    pub async fn active_session_count(&self) -> usize {
        self.session_channels.read().await.len()
    }

    /// 获取订阅者数
    pub async fn subscriber_count(&self) -> usize {
        self.subscribers.count().await
    }
}

impl Clone for EventBus {
    fn clone(&self) -> Self {
        Self {
            config: self.config.clone(),
            global_tx: self.global_tx.clone(),
            session_channels: RwLock::new(HashMap::new()),
            subscriber_channels: RwLock::new(HashMap::new()),
            subscribers: Arc::clone(&self.subscribers),
        }
    }
}

// ============================================================================
// 总线事件类型
// ============================================================================

/// 总线事件
#[derive(Debug, Clone)]
pub struct BusEvent {
    /// 事件 ID
    pub id: String,
    /// 事件类型
    pub kind: BusEventKind,
    /// 事件数据
    pub data: serde_json::Value,
    /// 时间戳
    pub timestamp: u64,
    /// 会话 ID（可选）
    pub session_id: Option<String>,
    /// 用户 ID（可选）
    pub user_id: Option<String>,
}

impl BusEvent {
    /// 创建新事件
    pub fn new(kind: BusEventKind, data: serde_json::Value) -> Self {
        Self {
            id: uuid::Uuid::new_v4().to_string(),
            kind,
            data,
            timestamp: talos_types::now(),
            session_id: None,
            user_id: None,
        }
    }

    /// 创建会话相关事件
    pub fn for_session(
        session_id: impl Into<String>,
        kind: BusEventKind,
        data: serde_json::Value,
    ) -> Self {
        Self {
            session_id: Some(session_id.into()),
            ..Self::new(kind, data)
        }
    }

    /// 创建用户相关事件
    pub fn for_user(
        user_id: impl Into<String>,
        kind: BusEventKind,
        data: serde_json::Value,
    ) -> Self {
        Self {
            user_id: Some(user_id.into()),
            ..Self::new(kind, data)
        }
    }
}

/// 事件类型
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BusEventKind {
    // 连接事件
    Connected,
    Disconnected,

    // 消息事件
    MessageReceived,
    MessageSent,
    MessageFailed,

    // Agent 事件
    AgentStarted,
    AgentStopped,
    AgentThinking,
    AgentToolCall,
    AgentToolResult,

    // 会话事件
    SessionCreated,
    SessionUpdated,
    SessionDestroyed,

    // 系统事件
    Heartbeat,
    Error,
    Metric,
}

// ============================================================================
// 订阅者注册表
// ============================================================================

struct SubscriberRegistry {
    subscribers: RwLock<HashMap<String, SubscriberInfo>>,
}

impl SubscriberRegistry {
    fn new() -> Self {
        Self {
            subscribers: RwLock::new(HashMap::new()),
        }
    }

    async fn add(&self, id: String) {
        let mut subscribers = self.subscribers.write().await;
        subscribers.insert(
            id,
            SubscriberInfo {
                registered_at: std::time::SystemTime::now(),
                message_count: 0,
            },
        );
    }

    async fn remove(&self, id: &str) {
        let mut subscribers = self.subscribers.write().await;
        subscribers.remove(id);
    }

    async fn count(&self) -> usize {
        self.subscribers.read().await.len()
    }

    async fn increment_message_count(&self, id: &str) {
        if let Some(info) = self.subscribers.write().await.get_mut(id) {
            info.message_count += 1;
        }
    }
}

#[derive(Debug)]
struct SubscriberInfo {
    registered_at: std::time::SystemTime,
    message_count: u64,
}

// ============================================================================
// 事件过滤器
// ============================================================================

/// 事件过滤器
#[derive(Debug, Clone)]
pub struct EventFilter {
    /// 过滤事件类型
    pub kinds: Vec<BusEventKind>,
    /// 过滤会话 ID
    pub session_ids: Vec<String>,
    /// 过滤用户 ID
    pub user_ids: Vec<String>,
}

impl EventFilter {
    pub fn new() -> Self {
        Self {
            kinds: vec![],
            session_ids: vec![],
            user_ids: vec![],
        }
    }

    pub fn with_kind(mut self, kind: BusEventKind) -> Self {
        self.kinds.push(kind);
        self
    }

    pub fn with_session(mut self, session_id: impl Into<String>) -> Self {
        self.session_ids.push(session_id.into());
        self
    }

    pub fn with_user(mut self, user_id: impl Into<String>) -> Self {
        self.user_ids.push(user_id.into());
        self
    }

    pub fn matches(&self, event: &BusEvent) -> bool {
        if !self.kinds.is_empty() && !self.kinds.contains(&event.kind) {
            return false;
        }

        if !self.session_ids.is_empty() {
            if let Some(ref session_id) = event.session_id {
                if !self.session_ids.contains(session_id) {
                    return false;
                }
            } else {
                return false;
            }
        }

        if !self.user_ids.is_empty() {
            if let Some(ref user_id) = event.user_id {
                if !self.user_ids.contains(user_id) {
                    return false;
                }
            } else {
                return false;
            }
        }

        true
    }
}

impl Default for EventFilter {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// 事件处理器
// ============================================================================

/// 事件处理器 trait
#[async_trait::async_trait]
pub trait EventHandler: Send + Sync {
    async fn handle(&self, event: &BusEvent) -> TalosResult<()>;
}

/// 事件处理器注册表
pub struct EventProcessor {
    handlers: RwLock<HashMap<String, Arc<dyn EventHandler>>>,
    event_bus: EventBus,
}

impl EventProcessor {
    pub fn new(event_bus: EventBus) -> Self {
        Self {
            handlers: RwLock::new(HashMap::new()),
            event_bus,
        }
    }

    pub async fn register_handler(
        &self,
        id: &str,
        handler: Arc<dyn EventHandler>,
    ) -> TalosResult<()> {
        let mut handlers = self.handlers.write().await;
        handlers.insert(id.to_string(), handler);
        Ok(())
    }

    pub async fn unregister_handler(&self, id: &str) -> TalosResult<()> {
        let mut handlers = self.handlers.write().await;
        handlers.remove(id);
        Ok(())
    }

    pub async fn process(&self, event: &BusEvent) -> TalosResult<()> {
        let handlers = self.handlers.read().await;
        for handler in handlers.values() {
            if let Err(e) = handler.handle(event).await {
                warn!("Handler error: {}", e);
            }
        }
        Ok(())
    }

    pub fn get_event_bus(&self) -> &EventBus {
        &self.event_bus
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_event_bus_publish_subscribe() {
        let bus = EventBus::new(EventBusConfig::default());

        let mut rx = bus.subscribe();

        let event = BusEvent::new(BusEventKind::Heartbeat, serde_json::json!({"tick": 1}));

        bus.publish(event.clone()).unwrap();

        let received = rx.recv().await.unwrap();
        assert_eq!(received.kind, BusEventKind::Heartbeat);
    }

    #[tokio::test]
    async fn test_event_filter() {
        let filter = EventFilter::new()
            .with_kind(BusEventKind::MessageReceived)
            .with_session("sess-1");

        let matching_event = BusEvent::for_session(
            "sess-1",
            BusEventKind::MessageReceived,
            serde_json::json!({}),
        );

        let non_matching_event = BusEvent::new(BusEventKind::Heartbeat, serde_json::json!({}));

        assert!(filter.matches(&matching_event));
        assert!(!filter.matches(&non_matching_event));
    }
}
