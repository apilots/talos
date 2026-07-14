# Provider Reasoning 与模型选择边界重构

Status: archived
Archived: 2026-07-14
Owner: agent
Created: 2026-07-10
Updated: 2026-07-11
Related: `archive/2026/refactor-026-provider-protocol-abstraction`, `archive/2026/refactor-022-provider-neutral-agent-stream`

## 背景与问题

Talos 已经形成 `chat`、`responses`、`messages` 三类 wire protocol，并通过 `LlmProvider` 向 Agent 提供统一调用边界。但 Provider、模型选择和 Reasoning 仍存在以下断层：

1. `OpenRouter` 与 `Ollama` 通过修改 identity 复用 `OpenAIProvider`，Provider 身份和 OpenAI 原厂语义仍然混在一起。
2. 缺少一等 `DeepSeekProvider`，直接 DeepSeek endpoint 只能借用 OpenAI-compatible 路径。
3. TUI 模型选择器可以选择 Reasoning effort，但会话更新只提交 `ModelRef`，选中的 effort 只保存在 TUI 本地状态。
4. `SessionConf` 和 `StartTurnRequest` 只能表达模型身份，不能表达模型默认 effort 或会话级 effort 覆盖。
5. Kernel 从模型目录读取默认 `reasoning_effort`，因此客户端显示的 effort 与实际 Provider 请求可能不一致。
6. 通用 `ChatRequest` 只有 `reasoning_effort`，没有区分模型默认选择、解析后的有效 Reasoning 和 Provider continuation。
7. 工具循环会保存 assistant text 与 tool calls，但不会保存模型返回的 Reasoning continuation。DeepSeek 工具调用要求完整回传 `reasoning_content`，OpenRouter 要求保持 `reasoning_details` 的原始顺序。
8. `compat.thinking_format` 已进入配置、schema 和 TS 类型，但当前运行时不消费该字段；它不能解决 Provider 特有的请求映射和工具回放问题。
9. Provider-specific effort 转换当前放在 Provider-neutral API 中，导致不同 Messages Provider 被迫共享同一种请求策略。

参考文档：

- [DeepSeek 思考模式](https://api-docs.deepseek.com/zh-cn/guides/thinking_mode/)
- [DeepSeek Anthropic API](https://api-docs.deepseek.com/zh-cn/guides/anthropic_api/)
- [OpenRouter Reasoning Tokens](https://openrouter.ai/docs/guides/best-practices/reasoning-tokens)
- [OpenRouter Responses API](https://openrouter.ai/docs/api/reference/responses/overview)
- [OpenRouter Anthropic Messages API](https://openrouter.ai/docs/api/api-reference/anthropic-messages/create-messages)

## 目标

1. 用 `ModelSelection` 统一模型身份与 Reasoning 选择，供 Session、Turn、SDK、TUI、Web 和 CLI 共同使用。
2. 保留现有模型级 `reasoning` 与 `reasoning_effort` 配置：前者表示模型是否支持 Reasoning，后者表示模型默认 effort。
3. Reasoning 选择统一为 `ModelDefault` 和显式 `Effort`。
4. 让客户端选择的 effort 真实进入 Session、Agent、Kernel 和 Provider 请求，不再维护仅 UI 可见的本地真相。
5. 新增一等 `DeepSeekProvider` 与 `OpenRouterProvider`；Provider 复用共享 protocol driver，只实现厂商/网关差异。
6. 将 OpenAI 原厂语义收回 `OpenAIProvider`，不再通过 identity 重命名承载其他 Provider。
7. 分离 Reasoning 的用户可见文本和工具调用 continuation，保证流式展示、历史恢复和工具回放一致。
8. 删除 `thinking_format`、无效 compat 对象、旧 Provider identity 兼容路径及相关反向测试。
9. 同步更新根目录三份 `conf`、Desktop 内置配置、Rust 配置类型、schema、TS SDK 和所有客户端。

## 非目标

- 不引入动态 dialect registry、任意 JSON extension 或万能 Provider hook。
- 不保留 `model_ref` 与 `model_selection` 两套长期公共 API。
- 不为旧配置、旧 schema 或旧 Provider identity 路径增加 fallback。
- 不改变 Agent 的工具选择策略、stop decision 或 TUI transcript 渲染架构。

## 当前实现事实

### 模型配置

当前每个模型已经拥有模型级默认配置：

```yaml
- id: "gpt-5.5"
  name: "GPT-5.5"
  context_window: 1000000
  reasoning: true
  reasoning_effort: "medium"
```

语义固定为：

- `reasoning`: 模型是否支持 Reasoning。
- `reasoning_effort`: 该模型在没有 Session/Turn 覆盖时使用的默认 effort。
- `context_window`: 模型总上下文窗口。

Kernel 输出上限继续由 `agent.kernel.output.max_tokens` 和 `upper_limit` 控制。模型级 `max_tokens` 已被配置校验拒绝，本设计不恢复该字段。

### 模型选择断层

当前 `ModelRef` 只有 `provider` 与 `id`。`SessionConf`、`StartTurnRequest` 和 `SessionInfo` 围绕 `ModelRef` 建模；TUI effort 选择没有稳定 API 承载，因此无法成为 Agent 请求事实源。

### Provider 与 protocol

当前 protocol 已有：

```text
chat       OpenAI Chat Completions-compatible
responses  OpenAI Responses-compatible
messages   Anthropic Messages-compatible
```

`BailianProvider` 已证明正确复用方式：Provider 构造厂商字段并复用共享 Chat protocol。`OpenRouter` 和 `Ollama` 当前仍通过 `OpenAIProvider` identity 重命名构造，未达到相同边界。

### Reasoning 输出与回放

当前 Provider 输出把 Reasoning 收敛为展示字符串。Agent 工具循环追加 assistant tool-call message 时没有携带 Reasoning continuation。这会造成：

- DeepSeek `reasoning_content` 在 tool result 后丢失，后续请求可能返回 400。
- OpenRouter `reasoning_details` 的 text/summary/encrypted block 无法按原顺序回传。
- Anthropic thinking signature 或 OpenAI Responses reasoning item 无稳定承载边界。
- UI 能看到 Reasoning 不等于 Agent 能正确继续模型推理。

## 核心设计决策

### 1. Provider、Protocol 与 Model 分离

```text
Provider  = 服务商或网关身份与差异策略
Protocol  = wire request/response/stream 结构
Model     = 模型能力、默认 effort、上下文和成本元数据
```

约束：

```text
provider != protocol
protocol != reasoning capability
model default != session selection
visible reasoning != provider continuation
```

### 2. 使用简洁的 ModelSelection

`ModelRef` 继续只表示模型身份。新增统一核心对象：

```rust
#[serde(rename_all = "camelCase")]
pub struct ModelSelection {
    pub model_ref: ModelRef,
    pub reasoning: ReasoningSelection,
}

#[serde(tag = "type", rename_all = "camelCase")]
pub enum ReasoningSelection {
    ModelDefault,
    Effort { level: ReasoningEffort },
}
```

`ModelSelection` 是稳定产品概念，不属于具体客户端或执行器。

解析规则：

```text
reasoning=false + ModelDefault   -> EffectiveReasoning::Unsupported
reasoning=false + Effort(level)  -> validation error
reasoning=true  + ModelDefault   -> model.reasoning_effort
reasoning=true  + Effort(level)  -> level
```

无效组合必须在 Service 边界显式失败；Provider 不读取 UI 状态，也不自行查找 Session 配置。

### 3. ModelSelection 是唯一跨层选择对象

以下边界统一使用 `ModelSelection`：

- `SessionConf`
- `SessionInfo`
- `StartTurnRequest`
- Session patch/update API
- Agent 默认配置与 `AgentDefinition`
- `AgentHarness` 到 Coordinator 的 Session 初始化默认值
- Service 到 Coordinator/Agent 的 prepared session
- TUI、Web、CLI 与 SDK 的模型选择

目标字段：

```rust
pub struct SessionConf {
    pub model_selection: Option<ModelSelection>,
}
```

配置层使用同构的 `ModelSelectionConfig`，仅保留配置文件所需的 snake_case wire 形式：

```rust
pub struct ModelSelectionConfig {
    pub model_ref: ModelRefConfig,
    pub reasoning: ReasoningSelectionConfig,
}
```

`AgentConfig` 与 `AgentDefinitionConfig` 只暴露 `model_selection`。解析后的
`AgentDefinition`、`AgentHarness` 和 `SessionRuntime` 传递完整 `ModelSelection`；不保留
`agent.model_ref`、`default_model_ref` 或 `resolve_target_agent_model_ref` 入口。

全局 `llm.provider` 与 `llm.model` 构造默认 `ModelSelection`，其 Reasoning 为 `ModelDefault`。Session 或 Turn 提供 selection 时覆盖全局默认。

优先级保持单一且显式：

```text
turn ModelSelection
  > session ModelSelection
  > agent/global ModelSelection
```

`SessionInfo` 返回后端确认的 `ModelSelection`。客户端根据该 selection 与后端模型目录中的默认 effort 展示当前有效值，不维护第二份本地选择状态。

公共 wire 字段按当前主版本 API 规则使用 camelCase。旧 `modelRef` 单独选择字段在迁移完成后删除，不保留 alias。

### 4. Provider 只接收解析后的 EffectiveReasoning

Kernel 在构造 LLM 请求前解析模型元数据和 `ReasoningSelection`：

```rust
pub enum EffectiveReasoning {
    Unsupported,
    Effort(ReasoningEffort),
}
```

共享 `ChatRequest` 使用 `EffectiveReasoning`，不再用含义不清的 `Option<ReasoningEffort>`。Provider 只做 wire 映射。

### 5. Provider 拥有厂商字段映射

客户端、Session API、模型配置和 `ModelSelection` 只表达 Talos effort。具体 Provider 在内部完成厂商字段转换：

- Anthropic Messages 可以将 effort 映射为 thinking budget。
- Bailian 可以将 effort 映射为 `thinking_budget`。
- DeepSeek Messages 使用 `output_config.effort`，不得复用 Anthropic budget 映射。
- OpenRouter 使用统一 `reasoning.effort`，由 OpenRouter 转换到底层 Provider。

Provider-specific effort 转换应迁入真正使用它的 Provider policy，并按请求输出上限和 Provider 约束做校验。共享 Messages driver 不再内置单一厂商策略。

### 6. 保留当前四级 Talos effort

Talos 继续使用：

```text
Low | Medium | High | XHigh
```

Provider 映射：

| Provider | Talos effort 映射 |
| --- | --- |
| OpenAI | 按 Chat/Responses 原生 effort 字段映射 |
| DeepSeek | `Low/Medium -> high`, `High -> high`, `XHigh -> max` |
| OpenRouter | 映射到统一 `reasoning.effort` |
| Anthropic | Provider policy 映射到 Messages thinking 配置 |
| Bailian | Provider policy 映射到 `enable_thinking` / `thinking_budget` |

TUI 继续使用当前四个 effort 选项，具体 wire 值由 Provider 映射。

## Provider 目标结构

### Provider 支持矩阵

| Provider | `chat` | `responses` | `messages` |
| --- | ---: | ---: | ---: |
| OpenAI | yes | yes | no |
| Anthropic | no | no | yes |
| DeepSeek | yes | no | yes |
| OpenRouter | yes | yes | yes |
| Bailian | yes | no | no |
| Ollama | yes | no | no |

OpenRouter `responses` 当前为 Beta，必须由配置显式选择；默认仍为 `chat`。

### Concrete Provider

目标 concrete provider：

```text
talos-llm-openai
talos-llm-anthropic
talos-llm-deepseek
talos-llm-openrouter
talos-llm-bailian
talos-llm-ollama
```

每个 Provider 负责：

- API key、base URL、headers 和 endpoint 选择。
- 允许的 protocol 集合。
- 模型默认值和 Provider capability 校验。
- effort 到厂商字段的映射。
- Reasoning response/stream decode profile。
- Provider error 上下文。
- Reasoning continuation 编码和回放。

每个 Provider 不负责：

- 重复实现 HTTP SSE framing。
- 重复实现 Chat/Responses/Messages 基础 DTO 映射。
- 读取 TUI/Web 本地状态。
- 修改 Agent timeline 顺序。

共享 protocol driver 继续负责稳定 wire 结构与 SSE 事件解码。Provider 通过强类型 request options/decoder profile 组合 protocol driver，不新增动态 dialect registry。

`OpenAIConfig::with_identity` 和对应 `openai_compatible_config` 主路径删除。OpenAI Provider 只代表 OpenAI。

## Reasoning 请求映射

| Provider / Protocol | 请求字段 |
| --- | --- |
| OpenAI / Chat | `reasoning_effort` |
| OpenAI / Responses | `reasoning.effort` |
| DeepSeek / Chat | `thinking.type=enabled` + `reasoning_effort` |
| DeepSeek / Messages | `thinking.type=enabled` + `output_config.effort` |
| OpenRouter / Chat | `reasoning.effort` |
| OpenRouter / Responses | `reasoning.effort` |
| OpenRouter / Messages | OpenRouter Messages reasoning mapping |
| Anthropic / Messages | Provider-specific thinking config |
| Bailian / Chat | `enable_thinking` + `thinking_budget` |

`EffectiveReasoning::Unsupported` 不序列化任何 Reasoning 控制字段。

## Reasoning 输出与工具回放

### 两类输出必须分离

```text
Reasoning display projection
  -> Timeline / TUI / Web / history

Reasoning continuation
  -> Agent conversation state / tool-loop replay
```

Provider 输出统一为：

```rust
pub struct ReasoningOutput {
    pub display_text: Vec<String>,
    pub continuation: Option<ReasoningContinuation>,
}

pub enum ReasoningContinuation {
    Text(String),
    Blocks(Vec<ReasoningBlock>),
}

pub enum ReasoningBlock {
    Text(ReasoningTextBlock),
    Summary(ReasoningSummaryBlock),
    Encrypted(ReasoningEncryptedBlock),
}
```

Block 类型必须保留 Provider 返回的稳定标识、顺序、format、signature 或 encrypted data；不得转成任意 JSON，也不得在工具回放前重排、拼接或重新生成。

### Tool loop 规则

1. Provider response 同时产生 display projection 和 continuation。
2. Timeline 只消费 display projection。
3. assistant tool-call message 必须携带 continuation。
4. tool result 后的下一次请求由当前 Provider/Protocol encoder 原样回放 continuation。
5. Dynamic tool pause/resume 必须连同 continuation 恢复，不能只恢复 tool call。
6. Provider 文档允许丢弃 continuation 时，只能由对应 Provider policy 决定；Agent 公共逻辑不猜测。
7. 需要跨 turn 保留的 DeepSeek continuation 必须进入内部 Session conversation state，但不得泄漏到 Gateway/SDK 公共 DTO。

## 配置设计

### 保留的模型字段

```yaml
context_window: 1000000
reasoning: true
reasoning_effort: "medium"
```

### 新增 DeepSeek Provider 配置

`LlmConfig` 新增 `deepseek: ProviderConfig`，配置示例：

```yaml
llm:
  deepseek:
    api_key: ${TALOS_LLM_DEEPSEEK_API_KEY}
    base_url: "https://api.deepseek.com"
    protocol: "chat"
    models:
      - id: "deepseek-v4-pro"
        name: "DeepSeek V4 Pro"
        context_window: 1000000
        reasoning: true
        reasoning_effort: "high"
        speed: "standard"
        input: ["text"]
        cost:
          input: 0
          output: 0
          cache_read: 0
          cache_write: 0
```

DeepSeek endpoint 由 Provider 根据 protocol 解析：

```text
chat      {base_url}/chat/completions
messages  {base_url}/anthropic/v1/messages
```

### 配置文件同步范围

以下文件必须同步修改：

- `conf/talos.yaml`
- `conf/talos-dev.yaml`
- `conf/talos-prod.yaml`
- `talos-rs/crates/infra/config` 配置类型、校验、store、脱敏和 env key 状态。
- `talos-rs/crates/common/types` 公共配置对象和转换。
- app protocol schema 与生成的 TS SDK。
- Web Settings、TUI model catalog 与 CLI 配置读取。

`conf/talos.yaml`、`conf/talos-dev.yaml` 与 `conf/talos-prod.yaml` 是产品配置维护源。
Desktop resource 不单独维护；打包脚本按 profile 复制 dev 或 prod 配置。运行时加载后的
`Config` 必须保留实际 source path，配置 API 和 Web Settings 写回同一文件。通过
`TALOS_CONFIG` 指定配置时，禁止回退写入 `$TALOS_HOME/conf/talos.yaml`。

新增 env key：

```text
TALOS_LLM_DEEPSEEK_API_KEY
```

### 删除无效 compat 配置

`ModelCompatConfig` 当前只有 `thinking_format`，目标态删除：

- `ProviderModelConfig.compat`
- `ModelCompatConfig`
- `ThinkingFormat`
- `LlmModelCompat`
- `LlmThinkingFormat`
- schema / TS 对应类型。
- 所有产品配置中的 `compat` 和 `thinking_format`。
- 仅验证旧字段的测试。

Provider-specific Reasoning mapping 进入 concrete Provider，不再由模型配置声明 wire format。

## 客户端行为

### TUI / Web

- `reasoning: false` 的模型不进入 effort 选择页。
- `reasoning: true` 的模型默认选中模型配置的 `reasoning_effort`。
- 用户未修改 effort 时提交 `ModelDefault`。
- 用户明确选择 effort 时提交 `Effort(level)`。
- Model picker 成功后以 `SessionInfo.modelSelection` 为事实源更新 UI。
- 状态栏显示后端确认的有效 effort，不显示仅本地状态。

### CLI

- CLI 继续与 TUI/Web 共用 `ModelSelection`。
- 未显式提供 effort 时使用 `ModelDefault`。
- 如已有 effort 参数，必须映射到 `ReasoningSelection::Effort`，不能只修改 CLI 本地变量。
- exec mode 与交互 mode 使用同一 Service/Agent 路径。

### HTTP / SSE

- Start Turn 返回的 `turn_id` 是单轮事件订阅的唯一过滤键。
- `/v1/sessions/{session_id}/events` 接受 `turn_id` query，并在该轮 terminal 事件后关闭。
- 未指定 `turn_id` 时按 session 连续订阅，不得被任一历史轮次的 terminal 事件提前关闭。

## 错误处理

以下情况显式返回 typed error，不做 fallback：

- Provider 不存在或未配置。
- Provider 不支持所选 protocol。
- ModelRef 不属于所选 Provider。
- 非 Reasoning 模型收到显式 effort。
- Provider 无法映射有效 effort。
- 工具调用后缺少 Provider 要求的 continuation。
- continuation Provider/Protocol 与当前请求不匹配。
- DeepSeek/OpenRouter response block 无法按协议解析。

错误文案只用于展示，不参与 Provider、protocol 或 Reasoning 类型判断。

## 影响范围

| 区域 | 主要变化 |
| --- | --- |
| `common/types` | `ModelSelection`、`ReasoningSelection`、Session/Turn API 收敛 |
| `common/llm/provider` | `EffectiveReasoning`、Reasoning output/continuation、protocol options |
| `common/llm/*` | DeepSeek/OpenRouter/Ollama concrete provider 与原厂 Provider 清理 |
| `infra/config` | DeepSeek 配置、protocol matrix、删除 compat/thinking format |
| `orchestration/service` | ModelSelection 解析、校验、Session patch/persistence |
| `agent-kernel` | effective effort、Reasoning continuation、tool-loop replay |
| `gateway/app-protocol` | Session/Turn schema 与 generated TS 更新 |
| `apps/sdk` | 新 ModelSelection API |
| `apps/tui` | picker 选择真实提交并以后端 snapshot 为事实源 |
| `talos-app` | Web model/effort 选择与生成类型同步 |
| `conf` | 三份环境配置是单一维护源；Desktop 构建时按 profile 复制 |

## Breaking Change 审查

本设计有意产生以下 breaking changes，不保留兼容逻辑：

- Session/Turn 公共 API 从单独 `modelRef` 收敛为 `modelSelection`。
- Session 持久化 conf 从 `model_ref` 收敛为 `model_selection`。
- 删除 `compat`、`thinking_format` 及相关 schema/TS 类型。
- OpenRouter 不再由 `OpenAIProvider` identity 重命名构造。
- Provider protocol 支持矩阵更新，OpenRouter 增加显式 `responses/messages`，DeepSeek 增加 `chat/messages`。
- Reasoning tool-loop message shape 增加内部 continuation，但该对象不进入公共 SDK。

所有使用方必须一次性迁移；不增加旧字段 alias、dual read、dual write 或反向测试。

## 验证策略

### 核心对象

- 完整对象断言 `ModelSelection` 的默认与显式 effort 解析。
- Session create/patch/read 后返回同一 selection。
- Turn override 优先于 Session，Session 优先于全局默认。
- 非 Reasoning 模型不会产生 Provider Reasoning 参数。

### Provider 映射

- DeepSeek Chat/stream request 与 `reasoning_content` decode。
- DeepSeek Messages request 与 thinking block decode。
- OpenRouter Chat/Responses/Messages request 与 reasoning decode。
- OpenAI、Anthropic、Bailian 现有正向行为保持。
- Provider/protocol 不支持组合返回明确错误。

### Tool loop

- DeepSeek reasoning -> tool call -> tool result -> continuation replay -> final answer。
- OpenRouter reasoning_details 顺序、signature/encrypted data 不变。
- Dynamic tool pause/resume 后 continuation 仍存在。
- Timeline Reasoning 在 final answer 前可见，Provider continuation 不暴露给客户端。

### 客户端与生成产物

- TUI model picker 选择 effort 后，mock Provider 捕获到相同有效 effort。
- TUI 真实验证使用 `TALOS_CONFIG=$REPO/conf/talos-dev.yaml` 启动，沿用现有 picker UI，仅验证 `ModelSelection` 接入与生效。
- Web/SDK 使用生成的 `ModelSelection` 类型。
- CLI 未指定 effort 使用模型默认，指定后真实进入 Provider request。
- schema 和 generated TS 与 Rust 类型一致。
- 全部产品配置解析和 validation 通过。

## 完成约束

- 只有一套 `ModelSelection` 与 `ReasoningSelection`。
- 只有一套 Provider-neutral Reasoning output/continuation。
- OpenAI Provider 不再代表 OpenRouter/Ollama/DeepSeek。
- 无 `thinking_format`、无无效 compat、无旧 `model_ref` 兼容入口。
- 无 Provider wire 字段泄漏到 TUI/Web/CLI。
- 所有 UI 选择都能通过正向集成测试证明已进入真实 Provider request。
