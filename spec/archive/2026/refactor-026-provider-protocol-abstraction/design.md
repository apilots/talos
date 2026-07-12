# Provider Protocol 抽象重构设计

Status: archived
Owner: agent
Created: 2026-06-15
Updated: 2026-07-10
Related: `refactor-022-provider-neutral-agent-stream`, `feat-001-admin-model-management`

归档说明：Chat/Responses/Messages 共享协议抽取保留为历史背景；Concrete Provider 构造、`ModelSelection`、Reasoning 映射、continuation 回放和当前 protocol matrix 统一由 `refactor-036-provider-reasoning-selection` 定义。

## 背景

Talos 当前 LLM provider 实现已经有统一的内部 trait：

```rust
pub trait LlmProvider: Send + Sync {
    fn name(&self) -> &str;
    async fn chat(&self, request: ChatRequest) -> TalosResult<ChatResponse>;
    async fn stream(&self, request: ChatRequest) -> TalosResult<ChatStream>;
    async fn stream_model_blocks(&self, request: ChatRequest) -> TalosResult<ModelBlockStream>;
}
```

但这个 trait 只是调用边界，不是可复用的 base provider。当前 `openai`、`anthropic`、`bailian` 都各自实现 HTTP request、response decode、SSE decode、tool call decode、model block stream。尤其 `bailian` 与 `openai` 都是 Chat Completions-compatible，却复制了大量协议通用逻辑。

这带来几个问题：

1. 新增 OpenRouter、OneAPI、LiteLLM、国产模型网关时，容易继续复制一套 provider。
2. Provider 和 protocol 概念混在一起，代码里容易把 `provider == openai` 等同于 `protocol == chat`。
3. OpenAI 原厂同时有 Chat Completions 和 Responses 两种协议，不能再用单个 `OpenAIProvider` 名称承载所有语义。
4. Anthropic 原厂是 Messages 协议，第三方也可能提供 Anthropic-compatible endpoint。
5. Settings 当前只暴露 provider/model/base_url/api_key/models，没有 protocol 选择，无法表达“同一 provider 支持多协议”。

现有配置命名必须保留：

```yaml
llm:
  provider: "bailian"
  model: "qwen3.7-plus"
```

本设计不引入新的当前选择字段名，继续使用 `llm.provider` 与 `llm.model` 表达当前默认 provider 和默认 model。

## 当前代码盘点

| 区域 | 当前实现 | 影响 |
| --- | --- | --- |
| `talos_llm_provider` | 只有统一 trait、请求/响应/stream 类型、SSE line decoder | 缺少可复用 base provider |
| `talos_llm_openai` | 完整实现 Chat Completions request/response/SSE/tool/model block | 可作为 `chat` protocol base 的主要来源 |
| `talos_llm_bailian` | 基本复制 OpenAI-compatible 逻辑，并加入 thinking/cache/usage 差异 | 应收敛为 `chat` protocol + Bailian dialect |
| `talos_llm_anthropic` | 完整实现 Messages request/response/SSE/tool/model block | 可作为 `messages` protocol base 的主要来源 |
| `talos_config::LlmConfig` | 固定字段：`provider/model/anthropic/openai/bailian/ollama` | 第一阶段保留，不做 registry 迁移 |
| `YamlLlmConfigStore` | provider key env、脱敏、placeholder 都按固定 provider 写死 | 新 provider 需要补 env key 与 key status |
| Settings Models UI | provider union 和 provider tabs 写死四家 | 需要补 protocol 控件与 OpenRouter tab |
| TS SDK types | `LlmConfig` 固定四家 provider | 需要同步生成/更新 protocol 字段 |

本次重构的第一步不是新增更多平级 provider，而是把重复协议逻辑收敛出来。

## 目标

1. 建立 provider-neutral 的 Talos 内部 LLM 请求/响应/stream block 接口。
2. 将 wire protocol 抽象为 `chat`、`responses`、`messages` 三类。
3. 将 provider 收敛为厂商/网关差异：默认地址、headers、能力、reasoning/cache/usage/error 等少量差异。
4. 让 `llm.<provider>.protocol` 可配置，并由 provider 限制可选范围。
5. 原厂 Anthropic 默认固定 `messages`，UI 不要求用户选择。
6. 原厂 OpenAI 允许选择 `responses` 或 `chat`；第一阶段保持现有 `chat` 行为，是否切默认到 `responses` 单独决策。
7. Bailian、OpenRouter、Ollama 等 OpenAI-compatible provider 默认使用 `chat`。
8. 新增 provider 时优先新增 provider dialect，不复制协议实现。
9. 保持 HTTP、JSON-RPC、Desktop、SDK 共享同一 application/service 层和同一 config 类型。

## 非目标

- 不在本 spec 中立即重写所有 provider 实现。
- 不在第一阶段强制把 `llm.anthropic/openai/bailian/ollama` 固定字段改成 map registry。
- 不改变 `llm.provider` / `llm.model` 字段名。
- 不改变 agent-kernel 的 general_turn stop decision。
- 不设计 embedding、rerank、image generation、realtime speech 协议。
- 不把 OpenRouter plugins、routing preferences、web search 等扩展能力纳入首版。
- 不为旧 provider bug 写 fallback 逻辑掩盖错误。

## 核心概念

| 概念 | 含义 | 示例 |
| --- | --- | --- |
| Provider | 厂商或网关身份，负责密钥、base_url、默认协议、少量差异 | `openai`, `anthropic`, `bailian`, `openrouter`, `ollama` |
| Protocol | wire protocol，请求和响应的结构形态 | `chat`, `responses`, `messages` |
| Model | 具体模型 ID 与能力元数据 | `gpt-5.5`, `qwen3.7-plus`, `anthropic/claude-sonnet-4.5` |
| Dialect | provider 在某个 protocol 下的差异点 | Bailian thinking 字段、OpenRouter attribution header |

约束：

```text
provider != protocol
protocol != capability
capability comes from model metadata + provider dialect
```

## 已确认决策

1. 配置继续使用 `llm.provider` 和 `llm.model`。
2. `protocol` 字段放在 provider 配置下，即 `llm.<provider>.protocol`。
3. 外部 protocol 名称只用 `chat`、`responses`、`messages`。
4. 第一阶段保留固定 provider 字段，不引入 `llm.providers` registry。
5. Provider 只关注差异部分，不复制协议通用 request/response/stream 逻辑。
6. 原厂 Anthropic 固定 `messages`，Settings 可以隐藏或只读展示。
7. 原厂 OpenAI 可选 `chat` / `responses`，第一阶段不改变现有默认行为。
8. Bailian/OpenRouter/Ollama 默认 `chat`，除非未来确认支持其他协议。

## 当前落地状态

截至 2026-06-15，已完成阶段 1-5：

1. `ProviderConfig.protocol` 已接入后端配置、TS SDK 与 Settings Models UI。
2. OpenRouter 已作为一等 provider 接入配置、密钥存储、model catalog、runtime factory 与 UI。
3. OpenAI-compatible `chat` protocol base 已抽出到 `talos_llm_provider::protocols::chat`，OpenAI、Bailian、OpenRouter、Ollama 共享 Chat Completions request/response/SSE/model block 基础逻辑。
4. Anthropic-compatible `messages` protocol base 已抽出到 `talos_llm_provider::protocols::messages`，Anthropic provider 只保留 headers、endpoint kind、default model、model capability 等厂商差异。
5. OpenAI `responses` protocol adapter 已接入为可选协议，覆盖 `/responses` request/response/stream、reasoning、function call 与 model block 映射。
6. OpenAI 默认仍保持 `chat`，不会自动切换到 `responses`。
7. Runtime factory 已增加 provider/protocol helper，OpenAI-compatible provider 通过统一路径生成 runtime config；常规 Rust/TS/WebUI 回归通过。
8. LLM 相关 e2e case/group 已使用临时 `TALOS_E2E_TMP_ROOT` 跑通，覆盖 session lifecycle、HTTP SSE、CLI、tools、memory 与 app-server protocol。
9. Desktop packaged smoke 尚未执行。

## 协议命名

外部配置使用短名称：

```rust
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum LlmProtocol {
    Chat,
    Responses,
    Messages,
}
```

语义：

| 配置值 | 协议含义 | 典型 endpoint |
| --- | --- | --- |
| `chat` | OpenAI Chat Completions-compatible | `/chat/completions` |
| `responses` | OpenAI Responses API | `/responses` |
| `messages` | Anthropic Messages-compatible | `/messages` |

文档中必须说明：`chat` 不是泛指聊天能力，而是 Chat Completions-compatible wire protocol。

`base_url` 统一表示 provider 的 API 根地址，不包含 protocol endpoint。`chat` 和 `responses` 都由 protocol adapter 追加 endpoint：

```text
protocol=chat
base_url=https://dashscope.aliyuncs.com/compatible-mode/v1
request_url=https://dashscope.aliyuncs.com/compatible-mode/v1/chat/completions

protocol=responses
base_url=https://dashscope.aliyuncs.com/compatible-mode/v1
request_url=https://dashscope.aliyuncs.com/compatible-mode/v1/responses
```

因此 Bailian 文档中的 `POST https://dashscope.aliyuncs.com/compatible-mode/v1/responses` 应拆为 `llm.openai.base_url=https://dashscope.aliyuncs.com/compatible-mode/v1` 与 `llm.openai.protocol=responses`。这样 `base_url` 字段在 `chat` / `responses` 下保持一致语义。

## 配置设计

第一阶段保留现有固定 provider 字段，只给 `ProviderConfig` 增加 `protocol`。

```rust
pub struct LlmConfig {
    pub provider: String,
    pub model: String,
    pub anthropic: ProviderConfig,
    pub openai: ProviderConfig,
    pub bailian: ProviderConfig,
    pub ollama: ProviderConfig,
    pub openrouter: ProviderConfig,
}

pub struct ProviderConfig {
    pub api_key: Option<String>,
    pub base_url: Option<String>,
    pub enabled: bool,
    pub protocol: Option<LlmProtocol>,
    pub models: Vec<ProviderModelConfig>,
}
```

示例：

```yaml
llm:
  provider: "openrouter"
  model: "anthropic/claude-sonnet-4.5"

  anthropic:
    protocol: "messages"
    api_key: ${TALOS_LLM_ANTHROPIC_API_KEY}
    base_url: "https://api.anthropic.com/v1"
    enabled: false
    models: []

  openai:
    protocol: "responses"
    api_key: ${TALOS_LLM_OPENAI_API_KEY}
    base_url: "https://api.openai.com/v1"
    enabled: false
    models: []

  bailian:
    protocol: "chat"
    api_key: ${TALOS_LLM_BAILIAN_API_KEY}
    base_url: "https://dashscope.aliyuncs.com/compatible-mode/v1"
    enabled: false
    models: []

  openrouter:
    protocol: "chat"
    api_key: ${TALOS_LLM_OPENROUTER_API_KEY}
    base_url: "https://openrouter.ai/api/v1"
    enabled: true
    models:
      - id: "anthropic/claude-sonnet-4.5"
        name: "Claude Sonnet 4.5 via OpenRouter"
        context_window: 200000
        reasoning: true
        input: ["text", "image"]
        cost:
          input: 0
          output: 0
          cache_read: 0
          cache_write: 0
        compat: {}
```

默认协议由 provider 决定：

| Provider | 默认 protocol | 可选 protocol | UI |
| --- | --- | --- | --- |
| `anthropic` | `messages` | `messages` | 隐藏或只读 |
| `openai` | `chat` | `chat`, `responses` | 可选 |
| `bailian` | `chat` | `chat` | 隐藏或只读 |
| `openrouter` | `chat` | `chat` | 隐藏或只读 |
| `ollama` | `chat` | `chat` | 隐藏或只读 |

若未来某个 provider 同时支持 `chat` 与 `messages`，只扩展该 provider 的可选协议列表。

有效协议解析规则：

```rust
fn protocol_for(provider: &str, config: &ProviderConfig) -> TalosResult<LlmProtocol> {
    let protocol = config
        .protocol
        .unwrap_or_else(|| default_protocol(provider));

    if !allowed_protocols(provider).contains(&protocol) {
        return Err(TalosError::InvalidConfiguration(format!(
            "llm.{provider}.protocol does not support {protocol}"
        )));
    }

    Ok(protocol)
}
```

OpenAI 后续若要把默认协议从 `chat` 切到 `responses`，必须另起迁移步骤，并完成 CLI、WebUI、Desktop packaged smoke 与 e2e 验证。

## 总体架构

```text
agent-kernel/general_turn
        |
        v
Talos LLM internal request
        |
        v
+-------------------------------+
| GenericLlmProvider            |
| - selected provider dialect   |
| - selected protocol adapter   |
| - shared transport            |
+-------------------------------+
        |
        +-------------------+--------------------+
        |                   |                    |
        v                   v                    v
   Protocol chat       Protocol responses    Protocol messages
        |                   |                    |
        v                   v                    v
  provider dialect    provider dialect       provider dialect
        |                   |                    |
        v                   v                    v
   HTTP / SSE          HTTP / SSE             HTTP / SSE
```

Provider 只关注自己的差异：

```text
OpenRouter:
  - default base_url
  - Authorization header
  - optional HTTP-Referer / X-OpenRouter-Title

Bailian:
  - default base_url
  - enable_thinking / thinking_budget
  - reasoning_content / thinking / reasoning stream fields
  - cache_control content block
  - cached_tokens / cache_creation usage fields

Anthropic:
  - x-api-key / anthropic-version headers
  - messages protocol only
  - thinking object
  - cache_control content block
```

## 核心 API 伪代码

接口命名保持直接，不使用 `map_*` / `build_*` 这类实现味较重的名字。

### 内部请求/响应

```rust
pub struct ModelRequest {
    pub model: String,
    pub messages: Vec<ModelMessage>,
    pub temperature: Option<f32>,
    pub max_tokens: Option<u32>,
    pub stop: Vec<String>,
    pub top_p: Option<f32>,
    pub tools: Vec<ToolDefinition>,
    pub tool_choice: Option<ToolChoice>,
    pub parallel_tool_calls: Option<bool>,
    pub reasoning: Option<ReasoningConfig>,
    pub response_format: Option<ResponseFormat>,
}

pub struct ModelMessage {
    pub role: ModelRole,
    pub content: Vec<ModelContent>,
    pub name: Option<String>,
    pub tool_call_id: Option<String>,
    pub tool_calls: Vec<FunctionCall>,
}

pub enum ModelContent {
    Text {
        text: String,
        cache: Option<CacheControl>,
    },
    Image {
        source: ImageSource,
    },
    File {
        source: FileSource,
    },
}

pub struct ModelResponse {
    pub content: Vec<ModelContent>,
    pub reasoning: Vec<String>,
    pub model: String,
    pub usage: Usage,
    pub finish_reason: Option<String>,
    pub tool_calls: Vec<FunctionCall>,
}
```

### Provider trait

```rust
#[async_trait]
pub trait LlmProvider: Send + Sync {
    fn id(&self) -> &str;
    fn protocol(&self) -> LlmProtocol;
    fn capabilities(&self, model: &ModelInfo) -> ModelCapabilities;

    async fn chat(&self, request: ModelRequest) -> TalosResult<ModelResponse>;
    async fn stream_model_blocks(&self, request: ModelRequest) -> TalosResult<ModelBlockStream>;
}
```

### Protocol adapter

```rust
pub trait LlmProtocolAdapter: Send + Sync {
    fn id(&self) -> LlmProtocol;
    fn path(&self) -> &'static str;

    fn request(
        &self,
        input: ModelRequest,
        provider: &dyn ProviderDialect,
    ) -> TalosResult<serde_json::Value>;

    fn response(
        &self,
        raw: serde_json::Value,
        provider: &dyn ProviderDialect,
    ) -> TalosResult<ModelResponse>;

    fn event(
        &self,
        raw: serde_json::Value,
        state: &mut StreamState,
        provider: &dyn ProviderDialect,
    ) -> TalosResult<Vec<ModelBlockStreamEvent>>;

    async fn stream(
        &self,
        input: SseStream,
        provider: &dyn ProviderDialect,
    ) -> TalosResult<ModelBlockStream>;
}
```

### Provider dialect

```rust
pub trait ProviderDialect: Send + Sync {
    fn id(&self) -> &str;
    fn label(&self) -> &str;
    fn default_protocol(&self) -> LlmProtocol;
    fn allowed_protocols(&self) -> &'static [LlmProtocol];
    fn default_base_url(&self) -> &'static str;

    fn endpoint(&self, base_url: &str, protocol: LlmProtocol) -> String;
    fn headers(&self, api_key: Option<&str>) -> TalosResult<header::HeaderMap>;
    fn features(&self, model: &ModelInfo) -> ModelCapabilities;

    fn reasoning(&self, reasoning: Option<&ReasoningConfig>) -> serde_json::Value {
        serde_json::Value::Null
    }

    fn cache(&self, cache: Option<&CacheControl>) -> serde_json::Value {
        serde_json::Value::Null
    }

    fn usage(&self, raw: &serde_json::Value) -> TalosResult<Usage>;
    fn error(&self, status: reqwest::StatusCode, body: String) -> TalosError;

    fn delta(&self, raw_delta: &serde_json::Value) -> ProviderDelta {
        ProviderDelta::default()
    }
}
```

### Generic provider

```rust
pub struct GenericLlmProvider {
    protocol: Arc<dyn LlmProtocolAdapter>,
    dialect: Arc<dyn ProviderDialect>,
    transport: Arc<dyn LlmTransport>,
    config: ProviderRuntimeConfig,
}

#[async_trait]
impl LlmProvider for GenericLlmProvider {
    fn id(&self) -> &str {
        self.dialect.id()
    }

    fn protocol(&self) -> LlmProtocol {
        self.protocol.id()
    }

    fn capabilities(&self, model: &ModelInfo) -> ModelCapabilities {
        self.dialect.features(model)
    }

    async fn chat(&self, request: ModelRequest) -> TalosResult<ModelResponse> {
        let body = self.protocol.request(request, self.dialect.as_ref())?;
        let raw = self.transport.json(
            self.config.endpoint.clone(),
            self.config.headers.clone(),
            body,
        ).await?;
        self.protocol.response(raw, self.dialect.as_ref())
    }

    async fn stream_model_blocks(&self, request: ModelRequest) -> TalosResult<ModelBlockStream> {
        let body = self.protocol.request(request, self.dialect.as_ref())?;
        let stream = self.transport.sse(
            self.config.endpoint.clone(),
            self.config.headers.clone(),
            body,
        ).await?;
        self.protocol.stream(stream, self.dialect.as_ref()).await
    }
}
```

## Protocol 责任边界

### `chat`

负责 Chat Completions-compatible 通用逻辑：

- role/message/tool schema；
- `/chat/completions` endpoint path；
- non-stream response `choices[0].message`；
- stream `choices[].delta.content`；
- stream `choices[].delta.tool_calls`；
- `stream_options.include_usage`；
- tool call arguments 字符串与 JSON 互转；
- no phase hint 的 assistant text 映射为 `AssistantText`，不伪造 final phase。

Provider dialect 只补：

- reasoning request 字段；
- reasoning delta 字段；
- prompt cache content block；
- usage cache 字段；
- provider-specific headers/error。

首个实现应从现有 `OpenAIProvider` 提取 `chat` base，而不是新写一套：

```text
OpenAIProvider current code
  -> ChatProtocol
       - request shape
       - tool schema
       - SSE decode
       - model block stream state
  -> OpenAiDialect
       - default base_url
       - auth/header
       - reasoning_effort
       - OpenAI usage
```

Bailian 迁移到同一个 `ChatProtocol`：

```text
BailianProvider current code
  -> ChatProtocol reuse
  -> BailianDialect
       - default base_url
       - enable_thinking / thinking_budget
       - reasoning_content / thinking / reasoning
       - cache_control block
       - cache usage fields
```

OpenRouter 首版也复用 `ChatProtocol`：

```text
OpenRouterDialect
  - default base_url = https://openrouter.ai/api/v1
  - bearer auth
  - optional attribution headers
  - no custom cache behavior in first phase
```

### `responses`

负责 OpenAI Responses API 逻辑：

- `/responses` endpoint path；
- input item / output item lifecycle；
- response item stream event；
- reasoning item；
- function call item；
- explicit item done / completed event；
- Responses-native usage。

该协议不能复用 Chat Completions 的 delta parser。

### `messages`

负责 Anthropic Messages-compatible 通用逻辑：

- `/messages` endpoint path；
- top-level system；
- content block；
- tool_use / tool_result block；
- cache_control block；
- thinking block；
- content_block_start/delta/stop stream；
- message_delta / message_stop usage。

Provider dialect 只补：

- headers；
- base_url；
- version header；
- 兼容 endpoint 的非标准字段差异。

## Settings UI 设计

现有 UI 文案可以保留：

```text
Provider selector -> 写 llm.provider
Model selector    -> 写 llm.model
```

新增 Protocol 控件：

```text
Provider: OpenAI
Protocol: Responses | Chat

Provider: Anthropic
Protocol: Messages  # hidden or read-only

Provider: Bailian
Protocol: Chat      # hidden or read-only

Provider: OpenRouter
Protocol: Chat      # hidden or read-only
```

规则：

1. 若 `allowed_protocols.len() == 1`，隐藏或只读展示 protocol。
2. 若 provider 支持多个协议，展示 select。
3. 保存时验证 `llm.<provider>.protocol` 在 allowed list 中。
4. UI 不根据 protocol 推断 provider 是否可用；readiness 仍由配置层统一返回。
5. UI 不新增新的当前 provider/model 字段，只编辑现有 `provider` 与 `model`。

## 模块接口清单

| 模块 | 新增/调整 | 职责 |
| --- | --- | --- |
| `talos_config::LlmProtocol` | 新增 enum | 表达 `chat/responses/messages` |
| `talos_config::ProviderConfig` | 新增 `protocol` 字段 | provider 级协议选择 |
| `talos_config::provider_config_for_llm` | 调整 | 返回 provider 配置并校验 protocol |
| `talos_types::resolve_model_catalog` | 调整 | 使用 `llm.provider` 和 provider models，不推断 protocol |
| `talos_llm_provider` | 调整 | 承载 provider-neutral `ModelRequest/ModelMessage/ModelContent` |
| `talos_llm_protocol_chat` | 新增或模块化 | Chat Completions-compatible protocol adapter |
| `talos_llm_protocol_responses` | 新增或模块化 | OpenAI Responses protocol adapter |
| `talos_llm_protocol_messages` | 新增或模块化 | Anthropic Messages protocol adapter |
| `talos_llm_dialect_*` | 新增或模块化 | Provider dialect：OpenAI/Bailian/OpenRouter/Ollama/Anthropic |
| `talos_llm_transport` | 新增或模块化 | 共享 JSON/SSE HTTP client、headers、timeout、error body |
| `talos-base::create_llm_provider` | 重构 | config -> dialect + protocol + generic provider |
| Settings Models UI | 调整 | 展示/保存 provider protocol |
| TS SDK types | 生成/调整 | 暴露 `protocol?: "chat" | "responses" | "messages"` |

## 分阶段计划

### 阶段 1: 配置协议字段

**目标**: 在不重写 provider 的情况下，为现有 `ProviderConfig` 增加 `protocol` 字段和校验。

**成功标准**:

- `llm.provider` / `llm.model` 保持不变。
- `llm.<provider>.protocol` 可反序列化、序列化、通过 Settings 保存。
- Readiness 能报告 unsupported protocol。
- 未显式配置 protocol 时保持现有 provider 行为；OpenAI 仍走当前 Chat Completions 路径。

**测试**:

- `talos_config` 反序列化默认值测试。
- `model_readiness_for_llm` unsupported protocol 测试。
- TS SDK typecheck。

**状态**: 未开始

### 阶段 2: 抽 Chat protocol base

**目标**: 抽出 Chat Completions-compatible 通用 request/response/stream 逻辑，让 OpenAI、Bailian、Ollama、OpenRouter 复用。

**成功标准**:

- OpenAI chat 现有测试通过。
- Bailian 只保留 dialect 差异，不复制通用 Chat parser。
- OpenRouter 可通过 dialect 接入。

**测试**:

- Chat request serialization。
- Tool call request/response roundtrip。
- SSE split line decode。
- Reasoning delta dialect tests。
- Usage dialect tests。

**状态**: 未开始

### 阶段 3: 抽 Messages protocol base

**目标**: 将 Anthropic Messages-compatible request/response/stream 与 Anthropic dialect 分离。

**成功标准**:

- Anthropic 原厂功能不变。
- Prompt cache、thinking、tool_use/tool_result 测试覆盖。
- 后续 Anthropic-compatible provider 不需要复制 messages protocol。

**测试**:

- Messages request serialization。
- Content block/cache control serialization。
- Tool use stream decode。
- Thinking stream decode。

**状态**: 未开始

### 阶段 4: 新增 Responses protocol

**目标**: 支持 OpenAI Responses API，作为 OpenAI provider 可选 protocol。

**成功标准**:

- OpenAI provider 可配置 `protocol: "responses"`。
- Responses stream 映射到 Talos model block。
- Chat 与 Responses 可在 Settings 中切换。
- 默认 provider 行为不在本阶段自动切到 Responses。

**测试**:

- Responses request serialization。
- Responses item lifecycle stream decode。
- Function call item decode。
- Reasoning item decode。

**状态**: 未开始

### 阶段 5: 清理 provider duplication

**目标**: 删除不再需要的重复 provider 代码，统一 runtime factory。

**成功标准**:

- `create_llm_provider` 不再按 provider 手写大段 HTTP client 构造。
- 新 provider 只新增 dialect 和配置默认值。
- OpenRouter 作为一等 provider 出现在 Settings。

**测试**:

- 相关 Rust provider crate tests。
- Gateway `/v1/llm` GET/PUT tests。
- Settings Models editor tests。
- `talos-e2e/run_e2e.sh` 中 provider smoke。

**状态**: 未开始

## 迁移策略

第一阶段保持向后兼容：

- 旧配置没有 `protocol` 时按 provider 默认补齐。
- `llm.provider` 和 `llm.model` 不改名。
- 固定 provider 字段先保留。
- `openrouter` 可作为新增固定字段加入，后续再评估 provider map registry。

长期可以另起 spec 讨论：

```yaml
llm:
  provider: "openrouter"
  model: "anthropic/claude-sonnet-4.5"
  providers:
    openrouter:
      kind: "openrouter"
      protocol: "chat"
```

但这属于配置结构迁移，不纳入本 spec 第一阶段。

## 风险与开放问题

1. `ModelRequest` 从 `content: String` 升级为 content block 会影响较多测试，需要分阶段适配。
2. OpenAI Responses 与 Chat Completions stream event 差异较大，不能低估单测覆盖成本。
3. Bailian 当前 prompt cache 和 usage 字段需要保留行为，重构时必须先补足 fixture。
4. OpenRouter 对 reasoning 参数、tool call、usage 字段在不同下游模型上可能不完全一致，需要 e2e smoke。
5. `protocol` UI 是否隐藏还是只读展示，需要产品确认；技术上两者都可。
6. OpenAI 默认 protocol 是否立即切到 `responses` 需要单独迁移决策；保守做法是新增支持但保持现有 `chat` 默认，待 e2e 稳定后再切默认。

## Definition of Done

- [ ] `llm.provider` / `llm.model` 保持原名。
- [ ] `ProviderConfig.protocol` 支持默认值、校验、Settings 保存。
- [ ] Provider factory 基于 provider + protocol 选择 adapter。
- [ ] Chat protocol 通用逻辑不再复制到 Bailian/OpenRouter/Ollama。
- [ ] Anthropic Messages protocol 与 Anthropic provider 差异分离。
- [ ] OpenAI Responses protocol 有独立 adapter。
- [ ] Rust 相关 crate tests 通过。
- [ ] TS SDK typecheck 通过。
- [ ] Settings Models editor tests 通过。
- [ ] 相关 e2e provider smoke 通过。
