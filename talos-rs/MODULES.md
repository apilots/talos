# Talos-RS 模块职责详解

**版本**: v2.0  
**最后更新**: 2026-03-14  
**总模块数**: 44 个 crate

---

## 架构分层总览

```
┌─────────────────────────────────────────────────────────────────┐
│  Common 层 - 通用库（跨层使用，有业务语义）7 个 crate              │
├─────────────────────────────────────────────────────────────────┤
│  Infra 层 - 基础设施（纯技术）6 个 crate                          │
├─────────────────────────────────────────────────────────────────┤
│  Perception 层 - 感知 4 个 crate                                  │
├─────────────────────────────────────────────────────────────────┤
│  Cognition 层 - 认知 5 个 crate                                   │
├─────────────────────────────────────────────────────────────────┤
│  Action 层 - 执行 5 个 crate                                      │
├─────────────────────────────────────────────────────────────────┤
│  Orchestration 层 - 编排 9 个 crate                               │
├─────────────────────────────────────────────────────────────────┤
│  Gateway 层 - 网关 4 个 crate                                     │
├─────────────────────────────────────────────────────────────────┤
│  Meta 层 - 元认知（横向切面）4 个 crate                           │
└─────────────────────────────────────────────────────────────────┘
```

---

## Common 层（7 个 crate）

**定位**：跨层通用的、有业务语义的模块。所有层都可以依赖 Common，但 Common 只依赖 Infra。

---

### 1. `talos-llm-provider`
**路径**: `crates/common/llm/provider/`

**职责**：LLM 抽象层的核心 trait 定义

**核心功能**：
- `LlmProvider` trait - 所有模型提供者的统一接口
- `ChatMessage` - 聊天消息类型（User/Assistant/System）
- `LlmResponse` - 模型响应类型
- `ModelConfig` - 模型配置（temperature/max_tokens 等）

**关键类型**：
```rust
pub trait LlmProvider: Send + Sync {
    async fn chat(&self, messages: Vec<ChatMessage>) -> Result<LlmResponse>;
    async fn stream(&self, messages: Vec<ChatMessage>) -> Result<Stream<LlmResponse>>;
    fn name(&self) -> &str;
}

pub enum Role {
    System,
    User,
    Assistant,
}

pub struct ChatMessage {
    pub role: Role,
    pub content: String,
}
```

**依赖**：`talos-error`, `talos-types`

**使用者**：`talos-memory`, `talos-knowledge`, `talos-agent`, `talos-anthropic`, `talos-openai`, `talos-ollama`

---

### 2. `talos-llm-anthropic`
**路径**: `crates/common/llm/anthropic/`

**职责**：Anthropic API（Claude）实现

**核心功能**：
- 实现 `LlmProvider` trait
- 支持 Claude 系列模型（claude-3-opus, claude-3-sonnet 等）
- 处理 Anthropic 特有的 API 格式

**依赖**：`talos-llm-provider`, `talos-error`

---

### 3. `talos-llm-openai`
**路径**: `crates/common/llm/openai/`

**职责**：OpenAI API 实现

**核心功能**：
- 实现 `LlmProvider` trait
- 支持 GPT 系列模型（gpt-4, gpt-3.5-turbo 等）
- 处理 OpenAI 特有的 API 格式

**依赖**：`talos-llm-provider`, `talos-error`

---

### 4. `talos-llm-ollama`
**路径**: `crates/common/llm/ollama/`

**职责**：Ollama API 实现（本地模型）

**核心功能**：
- 实现 `LlmProvider` trait
- 支持本地运行的开源模型（llama3, mistral 等）
- 处理 Ollama REST API

**依赖**：`talos-llm-provider`, `talos-error`

---

### 5. `talos-llm-router`
**路径**: `crates/common/llm/router/`

**职责**：多模型提供者路由/故障转移

**核心功能**：
- 自动故障转移（Anthropic 失败 → OpenAI）
- 负载均衡（按请求量/延迟分配）
- 成本优化（根据 token 价格路由）

**使用场景**：
```rust
let router = LlmRouter::new()
    .with_primary(AnthropicProvider::new())
    .with_fallback(OpenAiProvider::new())
    .with_strategy(RoutingStrategy::Failover);
```

**依赖**：`talos-llm-provider`, `talos-error`

---

### 6. `talos-prompt`
**路径**: `crates/common/prompt/`

**职责**：Prompt 模板引擎

**核心功能**：
- Prompt 模板渲染（变量替换）
- 上下文组装（系统提示 + 对话历史 + 用户输入）
- 预设模板（Agent 系统提示/工具调用提示）

**使用场景**：
```rust
let template = PromptTemplate::new("你是{{role}}，擅长{{skill}}");
let prompt = template.render(&[("role", "助手"), ("skill", "编程")]);
```

**依赖**：`talos-error`, `serde`

---

### 7. `talos-types`
**路径**: `crates/common/types/`

**职责**：通用类型定义

**核心功能**：
- 跨层共享的类型定义
- ID 类型（SessionId, AgentId, ToolId 等）
- 时间戳/持续时间封装

**关键类型**：
```rust
pub struct SessionId(String);
pub struct AgentId(String);
pub struct ToolId(String);
pub type Timestamp = u64;
```

**依赖**：`serde`, `thiserror`

---

## Infra 层（6 个 crate）

**定位**：纯技术基础设施，无业务语义。所有层都可以依赖 Infra。

---

### 8. `talos-error`
**路径**: `crates/infra/error/`

**职责**：统一错误处理

**核心功能**：
- `TalosError` 枚举（所有错误类型）
- `TalosResult<T>` 类型别名
- 错误转换 trait（From/Into）

**关键类型**：
```rust
pub enum TalosError {
    InitError(String),
    ConfigError(String),
    StorageError(String),
    AuthError(String),
    SessionError(String),
    AgentError(String),
    ToolError(String),
    ChannelError(String),
    GatewayError(String),
    InternalError(String),
}

pub type TalosResult<T> = std::result::Result<T, TalosError>;
```

**依赖**：`thiserror`, `serde`

**使用者**：**所有 crate**

---

### 9. `talos-logging`
**路径**: `crates/infra/logging/`

**职责**：日志/追踪集成

**核心功能**：
- tracing 初始化
- 结构化日志输出
- 日志级别控制（DEBUG/INFO/WARN/ERROR）
- 日志文件轮转

**使用场景**：
```rust
talos_logging::init()?;
tracing::info!("Talos started");
tracing::error!(error = %e, "Failed to load config");
```

**依赖**：`talos-error`, `tracing`, `tracing-subscriber`

---

### 10. `talos-config`
**路径**: `crates/infra/config/`

**职责**：配置管理

**核心功能**：
- 配置文件加载（JSON/TOML）
- 环境变量覆盖
- 配置热更新
- 配置验证

**配置结构**：
```json
{
  "agent": {
    "name": "jarvis",
    "model": "anthropic/claude-3-opus"
  },
  "gateway": {
    "port": 18789,
    "host": "127.0.0.1"
  },
  "channels": {
    "whatsapp": { ... }
  }
}
```

**依赖**：`talos-error`, `talos-logging`, `serde`, `config`

---

### 11. `talos-runtime`
**路径**: `crates/infra/runtime/`

**职责**：异步运行时封装

**核心功能**：
- Tokio 运行时封装
- 任务调度基础
- 超时/重试机制

**依赖**：`tokio`, `talos-error`

---

### 12. `talos-storage`
**路径**: `crates/infra/storage/`

**职责**：存储抽象

**核心功能**：
- KV 存储 trait（Redis/内存）
- SQL 存储 trait（SQLite/PostgreSQL）
- 向量存储 trait（Qdrant/Pinecone）
- 备份/恢复

**关键 trait**：
```rust
pub trait KvStore: Send + Sync {
    async fn get(&self, key: &str) -> Result<Option<String>>;
    async fn set(&self, key: &str, value: &str) -> Result<()>;
}

pub trait VectorStore: Send + Sync {
    async fn search(&self, query: &[f32], limit: usize) -> Result<Vec<SearchResult>>;
}
```

**依赖**：`talos-error`, `talos-config`

---

### 13. `talos-auth`
**路径**: `crates/infra/auth/`

**职责**：认证/配对

**核心功能**：
- 设备配对（类似 OpenClaw pairing）
- Token 管理
- 会话认证

**依赖**：`talos-error`, `talos-storage`

---

## Perception 层（4 个 crate）

**定位**：外部输入处理，将各种渠道的消息转换为统一格式。

---

### 14. `talos-input`
**路径**: `crates/perception/input/`

**职责**：多模态输入处理

**核心功能**：
- 输入类型定义（文本/语音/图像/事件）
- 输入解析器
- 输入验证

**关键类型**：
```rust
pub enum Input {
    Text(String),
    Voice(Vec<u8>),
    Image(Vec<u8>),
    Event(EventType),
}
```

**依赖**：`talos-types`, `talos-error`

---

### 15. `talos-channel`
**路径**: `crates/perception/channels/`

**职责**：IM channel 接入与 ChannelServer 运行时

**核心功能**：
- `ChannelServer`（统一管理和运行各 IM channel）
- `Channel` trait（平台适配接口）
- Telegram channel 实现与出站/流式适配器

**关键 trait**：
```rust
pub struct ChannelServer;

pub trait Channel: Send + Sync {
    fn definition(&self) -> &ChannelDefinition;
    fn enabled_account_ids(&self, config: &Config) -> Vec<String>;
    async fn start_account(&self, ctx: ChannelRuntimeContext) -> TalosResult<()>;
    async fn stop_account(&self, account_id: &str) -> TalosResult<()>;
}
```

**依赖**：`talos-types`, `talos-error`, `talos-config`

---

### 16. `talos-listener`
**路径**: `crates/perception/listener/`

**职责**：事件监听

**核心功能**：
- 日历事件监听
- 邮件监听
- Webhook 处理

**依赖**：`talos-error`, `talos-runtime`

---

### 17. `talos-transport`
**路径**: `crates/perception/transport/`

**职责**：传输适配

**核心功能**：
- HTTP 适配器
- WebSocket 适配器
- CLI 适配器

**依赖**：`talos-error`, `talos-runtime`

---

## Cognition 层（5 个 crate）

**定位**：记忆、知识、推理、决策——AI 的"大脑"。

---

### 18. `talos-memory`
**路径**: `crates/cognition/memory/`

**职责**：记忆系统

**核心功能**：
- 短期记忆（对话上下文，固定容量）
- 长期记忆（向量存储，持久化）
- 程序性记忆（技能/工具使用记录）

**关键类型**：
```rust
pub struct ShortTermMemory {
    capacity: usize,
    items: VecDeque<MemoryItem>,
}

pub trait LongTermMemory: Send + Sync {
    async fn store(&self, item: MemoryItem) -> Result<()>;
    async fn search(&self, query: &str) -> Result<Vec<MemoryItem>>;
}
```

**依赖**：`talos-error`, `talos-types`, `talos-llm-provider`

---

### 19. `talos-memory-store`
**路径**: `crates/cognition/memory-store/`

**职责**：记忆持久化

**核心功能**：
- 记忆存储到磁盘/数据库
- 记忆缓存层
- 记忆过期策略

**依赖**：`talos-memory`, `talos-storage`

---

### 20. `talos-knowledge`
**路径**: `crates/cognition/knowledge/`

**职责**：知识图谱/RAG

**核心功能**：
- 知识图谱构建
- RAG 检索（检索增强生成）
- 嵌入生成

**依赖**：`talos-memory`, `talos-llm-provider`, `talos-storage`

---

### 21. `talos-reasoning`
**路径**: `crates/cognition/reasoning/`

**职责**：推理引擎

**核心功能**：
- 任务规划（分解复杂任务）
- 因果推理
- 逻辑推导

**依赖**：`talos-memory`, `talos-llm-provider`

---

### 22. `talos-decision`
**路径**: `crates/cognition/decision/`

**职责**：决策系统

**核心功能**：
- 优先级判断（哪个任务更重要）
- 风险评估（执行某操作的风险）
- 资源分配（CPU/内存/时间）

**依赖**：`talos-reasoning`

---

## Action 层（5 个 crate）

**定位**：工具调用、输出——AI 的"手和嘴"。

---

### 23. `talos-tools`
**路径**: `crates/action/tools/`

**职责**：工具系统

**核心功能**：
- `Tool` trait（所有工具的统一接口）
- 工具注册表
- 内置工具（read/exec/edit/write）

**关键 trait**：
```rust
pub trait Tool: Send + Sync {
    fn name(&self) -> &str;
    fn description(&self) -> &str;
    async fn execute(&self, params: &str) -> Result<String>;
}

// 内置工具
pub struct ReadTool;
pub struct ExecTool;
pub struct EditTool;
pub struct WriteTool;
```

**依赖**：`talos-error`, `talos-types`

---

### 24. `talos-tools-policy`
**路径**: `crates/action/tools-policy/`

**职责**：工具策略

**核心功能**：
- Allowlist（允许的工具）
- Denylist（禁止的工具）
- Elevated（需要特权的工具）

**依赖**：`talos-tools`

---

### 25. `talos-tools-sandbox`
**路径**: `crates/action/tools-sandbox/`

**职责**：沙箱执行

**核心功能**：
- Docker 沙箱
- 本地沙箱（文件系统隔离）
- 资源限制（CPU/内存/时间）

**依赖**：`talos-tools`, `talos-error`

---

### 26. `talos-output`
**路径**: `crates/action/output/`

**职责**：输出通道

**核心功能**：
- 文本输出
- 语音输出（TTS 集成）
- 设备控制输出

**依赖**：`talos-error`, `talos-types`

---

### 27. `talos-skill`
**路径**: `crates/action/skill/`

**职责**：技能系统

**核心功能**：
- `Skill` trait
- 技能加载（从文件/网络）
- 技能注册表

**依赖**：`talos-tools`

---

## Orchestration 层（9 个 crate）

**定位**：Agent 调度、会话管理、任务规划——系统的"指挥中心"。

---

### 28. `talos-agent`
**路径**: `crates/orchestration/agent/`

**职责**：Agent 核心

**核心功能**：
- `Agent` trait
- Agent 生命周期管理（init/run/shutdown）
- Agent 上下文

**关键 trait**：
```rust
pub trait Agent: Send + Sync {
    fn name(&self) -> &str;
    async fn init(&mut self) -> Result<()>;
    async fn process(&self, input: &str) -> Result<String>;
    async fn shutdown(&self) -> Result<()>;
}
```

**依赖**：`talos-llm-provider`, `talos-memory`, `talos-tools`

---

### 29. `talos-agent-workspace`
**路径**: `crates/orchestration/agent-workspace/`

**职责**：工作空间管理

**核心功能**：
- AGENTS.md/SOUL.md/USER.md 加载
- 文件注入到 Agent 上下文
- 工作空间验证

**依赖**：`talos-agent`, `talos-storage`

---

### 30. `talos-session`
**路径**: `crates/orchestration/session/`

**职责**：会话管理

**核心功能**：
- SessionKey 生成（`agent:main:telegram:dm:123`）
- dmScope 策略（main/per-peer/per-channel-peer）
- 上下文管理

**关键类型**：
```rust
pub struct SessionKey(String);
pub enum DmScope {
    Main,
    PerPeer,
    PerChannelPeer,
}
```

**依赖**：`talos-types`, `talos-error`

---

### 31. `talos-session-store`
**路径**: `crates/orchestration/session-store/`

**职责**：会话存储

**核心功能**：
- sessions.json 管理
- JSONL 转录存储
- 会话维护（prune/rotate/archive）

**依赖**：`talos-session`, `talos-storage`

---

### 32. `talos-routing`
**路径**: `crates/orchestration/routing/`

**职责**：路由规则

**核心功能**：
- Binding 定义（匹配规则）
- 路由匹配（最具体优先）
- 路由策略

**关键类型**：
```rust
pub struct Binding {
    pub agent_id: AgentId,
    pub match: MatchRule,
}

pub enum MatchRule {
    Peer { kind: String, id: String },
    Channel { channel: String },
    Guild { guild_id: String },
}
```

**依赖**：`talos-session`

---

### 33. `talos-scheduler`
**路径**: `crates/orchestration/scheduler/`

**职责**：任务调度

**核心功能**：
- Cron 调度
- Heartbeat 机制
- 定时任务管理

**依赖**：`talos-agent`, `talos-runtime`

---

### 34. `talos-planner`
**路径**: `crates/orchestration/planner/`

**职责**：任务规划

**核心功能**：
- 工作流引擎
- 任务分解（复杂任务 → 可执行步骤）

**依赖**：`talos-agent`, `talos-reasoning`

---

### 35. `talos-coordinator`
**路径**: `crates/orchestration/coordinator/`

**职责**：多 Agent 协调

**核心功能**：
- 任务分配（给哪个 Agent）
- 结果聚合（合并多个 Agent 的输出）
- 冲突解决

**依赖**：`talos-agent`, `talos-planner`

---

### 36. `talos-queue`
**路径**: `crates/orchestration/queue/`

**职责**：消息队列

**核心功能**：
- steer 模式（立即插入当前对话）
- followup 模式（等待当前对话结束）
- collect 模式（收集后批量处理）

**依赖**：`talos-session`

---

## Gateway 层（4 个 crate）

**定位**：统一入口/出口——系统的"大门"。

---

### 37. `talos-gateway`
**路径**: `crates/gateway/core/`

**职责**：Gateway 核心

**核心功能**：
- `Gateway` trait
- Gateway 构建器
- 请求/响应处理

**关键 trait**：
```rust
pub trait Gateway: Send + Sync {
    async fn start(&self) -> Result<()>;
    async fn stop(&self) -> Result<()>;
    fn emit(&self, event: Event) -> Result<()>;
}
```

**依赖**：`talos-error`, `talos-routing`

---

### 38. `talos-ws`
**路径**: `crates/gateway/ws/`

**职责**：WebSocket 服务器

**核心功能**：
- WebSocket 服务器
- 协议解析（帧处理）
- 认证握手

**依赖**：`talos-gateway`, `talos-runtime`

---

### 39. `talos-events`
**路径**: `crates/gateway/events/`

**职责**：事件系统

**核心功能**：
- 事件总线
- 发布/订阅
- 事件类型定义（agent/chat/presence/heartbeat）

**依赖**：`talos-gateway`

---

### 40. `talos-health`
**路径**: `crates/gateway/health/`

**职责**：健康检查

**核心功能**：
- 指标收集
- 状态报告
- 告警触发

**依赖**：`talos-gateway`, `talos-monitor`

---

## Meta 层（4 个 crate）

**定位**：横向切面，贯穿所有层——系统的"自我意识"。

---

### 41. `talos-monitor`
**路径**: `crates/meta/monitor/`

**职责**：系统监控

**核心功能**：
- 性能监控（CPU/内存/延迟）
- 健康检查
- 指标导出（Prometheus 格式）

**实现方式**：
```rust
pub trait Monitorable {
    fn metrics(&self) -> Metrics;
    fn health(&self) -> HealthStatus;
}

// 各模块实现这个 trait
impl Monitorable for Agent { ... }
impl Monitorable for Session { ... }
```

**依赖**：`talos-types`, `talos-error`

---

### 42. `talos-learner`
**路径**: `crates/meta/learner/`

**职责**：持续学习

**核心功能**：
- 反馈收集（用户评分/纠正）
- 策略优化（调整响应策略）
- 事件订阅（从所有层学习）

**实现方式**：
```rust
pub struct Learner {
    events: EventStream,
}

impl Learner {
    async fn run(&mut self) {
        while let Some(event) = self.events.next().await {
            match event {
                Event::ToolCall(result) => self.learn_from_tool(result),
                Event::UserFeedback(fb) => self.learn_from_feedback(fb),
                ...
            }
        }
    }
}
```

**依赖**：`talos-events`

---

### 43. `talos-personality`
**路径**: `crates/meta/personality/`

**职责**：人格管理

**核心功能**：
- IDENTITY.md/SOUL.md 解析
- 响应风格调整（语气/用词）
- 中间件模式（拦截并调整响应）

**实现方式**：
```rust
pub struct PersonalityMiddleware<S> {
    inner: S,
    personality: Personality,
}

impl<S: Service> Service for PersonalityMiddleware<S> {
    async fn call(&self, req: Request) -> Response {
        let req = self.personality.apply(req);
        let resp = self.inner.call(req).await;
        self.personality.adjust(resp)
    }
}
```

**依赖**：`talos-agent-workspace`

---

### 44. `talos-maintenance`
**路径**: `crates/meta/maintenance/`

**职责**：维护任务

**核心功能**：
- 会话剪枝（pruneAfter/maxEntries）
- 归档处理
- 文件轮转

**依赖**：`talos-session-store`, `talos-scheduler`

---

## 依赖关系图

```
                    ┌─────────────────┐
                    │     Common      │
                    │  (llm/prompt)   │
                    └────────┬────────┘
                             │
┌────────────────────────────┼────────────────────────────┐
│                            │                            │
│  ┌───────────┐  ┌──────────▼──────────┐  ┌───────────┐ │
│  │ Gateway   │→ │  Orchestration      │  │   Meta    │ │
│  └───────────┘  └──────────┬──────────┘  └───────────┘ │
│                            │                            │
│  ┌───────────┐  ┌──────────▼──────────┐  ┌───────────┐ │
│  │Perception │→ │    Cognition        │  │  Action   │ │
│  └───────────┘  └──────────┬──────────┘  └───────────┘ │
│                            │                            │
│  ┌─────────────────────────▼─────────────────────────┐ │
│  │                    Infra                          │ │
│  │         (error/config/logging/runtime)            │ │
│  └───────────────────────────────────────────────────┘ │
└─────────────────────────────────────────────────────────┘
```

---

## 使用示例

### 1. 使用 LLM

```rust
use talos_llm_provider::{LlmProvider, ChatMessage, Role};
use talos_llm_anthropic::AnthropicProvider;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let provider = AnthropicProvider::new("your-api-key");
    
    let messages = vec![
        ChatMessage { role: Role::System, content: "你是助手".into() },
        ChatMessage { role: Role::User, content: "你好".into() },
    ];
    
    let response = provider.chat(messages).await?;
    println!("{}", response.content);
    
    Ok(())
}
```

### 2. 使用工具

```rust
use talos_tools::{Tool, ToolRegistry};
use talos_tools::builtin::{ReadTool, ExecTool};

let mut registry = ToolRegistry::new();
registry.register(ReadTool);
registry.register(ExecTool);

let result = registry.execute("read", "/path/to/file").await?;
println!("{}", result);
```

### 3. 使用会话

```rust
use talos_session::{SessionKey, DmScope};

let key = SessionKey::new("main", DmScope::PerChannelPeer, "telegram", "123");
println!("{}", key); // agent:main:telegram:dm:123
```

---

*最后更新：2026-03-14*
