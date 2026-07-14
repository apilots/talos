# LLM 模型配置与会话选择设计

Status: active
Owner: agent
Created: 2026-07-14
Updated: 2026-07-14
Related: `archive/2026/refactor-036-provider-reasoning-selection`, `archive/2026/refactor-027-core-api-object-boundary`

## 背景与问题

Talos 已经在 `llm` 配置树中保存了选择和运行模型所需的产品元数据：每个 Provider 拥有一组强类型模型配置，顶层 `provider` 和 `model` 指定默认模型。但是 Session、模型目录、SDK、Web、TUI 和 Provider 当前仍在不同层重复定义对象或重新推导默认值，造成以下问题：

1. 模型目录只暴露顶层默认 Provider 的模型，但 Session 路由实际上已经支持完整的 Provider 与模型组合。
2. Web 把模型元数据降级成模型 ID 和展示名称，用户选择前已经丢失 Provider 身份和 Reasoning 配置。
3. 配置层、共享类型、协议类型和 SDK 存在结构相似的模型、成本、输入类型、速度与 Reasoning effort 对象，并维护多层转换。
4. Session 没有稳定保存选中模型配置解析出的明确 Reasoning effort，运行时仍可能再次依赖当前配置推导。
5. Web 和 TUI 没有始终依据选中模型配置决定 Reasoning 控件的默认值和可选状态。
6. Ollama 没有默认模型目录配置，无法和远程 Provider 使用同一套模型选择链路。

本设计将配置作为模型元数据和默认选择行为的唯一事实来源。在概念一致时，Service、协议、SDK 和客户端直接复用配置对象，只保留边界上必要的轻量包装。

## 目标

1. 保持现有 `llm` YAML 结构，并将其作为 Provider、模型和默认 Reasoning effort 的权威来源。
2. 复用配置层核心对象，删除重复的模型、成本、输入类型、速度和 Reasoning effort 类型。
3. 通过统一模型目录返回所有已配置 Provider 的模型。
4. 所有客户端使用完整 `(provider, model id)` 选择模型。
5. 创建 Session 或切换模型时，把模型配置中的 Reasoning effort 明确写入 Session。
6. Web 和 TUI 使用相同的模型与 Reasoning 语义。
7. Talos 维护稳定的 effort 档位，具体厂商参数只在 Provider adapter 中映射。
8. Ollama 与其他 Provider 使用相同的静态模型目录和 dispatch 链路。

## 非目标

1. 不引入动态能力注册表或基于模型名称的能力推断。
2. 现有模型字段能够表达产品语义时，不新增 YAML 配置对象。
3. 不向客户端暴露 `budget_tokens`、`reasoning.effort`、`thinking` 等 Provider wire 参数。
4. 不在本轮引入远程 Provider 模型发现机制。
5. 不自动 pull Ollama 模型，也不在模型缺失时 fallback 到其他模型。
6. 不修改 Agent routing、timeline 渲染或工具循环 continuation 设计。

## 配置对象是唯一事实来源

配置层拥有以下权威类型：

```text
LlmConfig
ProviderConfig
ProviderModelConfig
ReasoningEffort
ModelSpeed
ModelInputKind
ModelCostConfig
```

Service、协议、schema 生成、Rust SDK 和 TypeScript SDK 在表达相同概念时必须复用或重新导出这些类型，不能仅为了跨 crate 或 transport 边界再定义一套平行的 `Llm*` 对象。复用按最小公开面进行：模型目录只公开 `ProviderModelConfig` 及其字段类型，不得因为复用配置对象而把 `ProviderConfig.api_key`、`base_url` 或完整 `LlmConfig` 带入模型目录、Session 或 Turn API。完整 Provider 配置只属于现有配置管理边界，并沿用其凭证脱敏规则。

Provider-specific request/response DTO 具有不同 wire 语义，继续保留在具体 Provider adapter 内部，不作为产品配置对象复用。

### API Key 与配置分离

`ProviderConfig.api_key` 可以继续作为配置更新对象中的输入字段，但真实密钥与 YAML、模型目录和前端读取结果分离：

- YAML 只保存 `${TALOS_LLM_<PROVIDER>_API_KEY}` 环境变量引用，真实值保存到 Talos runtime `.env`。
- 只有 Provider runtime 在执行请求时解析真实值；模型目录、Session 和 Turn 对象不包含 `api_key`。
- `config.get` 通过现有 `LlmConfigRead` 返回 UI 配置。已配置的密钥显示为 `********`，并通过 `provider_keys[provider].api_key_configured` 暴露非敏感状态；不得返回明文或可逆密文。
- 配置更新收到 `********` 或环境变量引用时保留现有密钥，收到空字符串时清除密钥，收到新值时只更新 runtime `.env` 并保持 YAML 为环境变量引用。

因此，复用 `ProviderConfig` 不代表扩大秘密值的公开范围；配置读取、配置更新和 Provider runtime 仍使用各自既有的安全边界。

### 模型 YAML

当前 YAML 结构保持不变：

```yaml
llm:
  provider: "anthropic"
  model: "claude-opus-4-8"
  anthropic:
    api_key: ${TALOS_LLM_ANTHROPIC_API_KEY}
    base_url: "https://api.anthropic.com/v1"
    protocol: "messages"
    models:
      - id: "claude-opus-4-8"
        name: "Claude Opus 4.8"
        context_window: 1000000
        reasoning: true
        reasoning_effort: "medium"
        speed: "standard"
        input: ["text", "image"]
        cost:
          input: 0
          output: 0
          cache_read: 0
          cache_write: 0
```

所有层统一使用以下字段语义：

- `llm.provider` 和 `llm.model` 指定新 Session 的默认模型。
- `reasoning` 表示 Talos 是否为该模型启用 Reasoning effort 选择。
- `reasoning_effort` 的类型是非可选 `ReasoningEffort`，缺省值为 `Medium`；它是 `reasoning: true` 时使用的 Talos 默认档位。
- `reasoning: false` 时，`reasoning_effort` 不产生运行时效果。
- Provider、protocol、context、input、speed 和 cost 都是配置事实，客户端不能通过模型 ID 推断。

## 统一模型目录

`ProviderModelConfig` 已经包含客户端需要的模型元数据。模型目录只需要补充 Provider 身份，因此 `ModelInfo` 是轻量包装，而不是第二套模型定义：

```rust
pub struct ModelInfo {
    pub provider: String,

    #[serde(flatten)]
    pub model: ProviderModelConfig,
}
```

序列化结果继续保持扁平，避免客户端增加无意义的嵌套访问：

```json
{
  "provider": "anthropic",
  "id": "claude-opus-4-8",
  "name": "Claude Opus 4.8",
  "context_window": 1000000,
  "reasoning": true,
  "reasoning_effort": "medium",
  "speed": "standard",
  "input": ["text", "image"],
  "cost": {
    "input": 0,
    "output": 0,
    "cache_read": 0,
    "cache_write": 0
  }
}
```

模型目录按 `anthropic`、`openai`、`bailian`、`deepseek`、`ollama`、`openrouter` 的配置字段顺序，以及各 Provider 内的模型配置顺序，遍历所有非空模型列表。Provider 凭证或 endpoint 的实际运行状态不影响配置目录枚举。它不得：

- 只返回 `llm.provider` 对应的模型。
- 跨 Provider 按模型 ID 去重。
- 通过模型 ID 推断 Provider。

模型目录响应继续使用同时包含 Provider 和模型 ID 的 `ModelRef` 标识配置中的默认模型。

## Session 模型选择

Session 只保存选中的模型身份和明确的配置 effort，不复制完整模型元数据：

```rust
pub struct ModelSelection {
    pub model_ref: ModelRef,
    pub reasoning_effort: Option<ReasoningEffort>,
}
```

`ReasoningEffort` 直接复用配置层的权威枚举，不新增 Session、SDK 或协议专用的 effort 类型。

### 默认值解析

创建新 Session 时：

1. 创建请求未提供 `ModelSelection` 时，使用 `llm.provider` 和 `llm.model` 查找对应的 `ProviderModelConfig`；请求提供了完整选择时，以请求值为准并执行同样校验。
2. 将完整 `ModelRef` 写入 Session。
3. 模型 `reasoning: true` 时，写入 `Some(model.reasoning_effort)`。
4. 模型 `reasoning: false` 时，写入 `None`。
5. 默认 Provider 或模型无法解析时，配置校验必须显式失败。

用户切换模型时，对目标模型执行相同解析，并原子更新 Provider、模型 ID 和 Reasoning effort。旧模型的 effort 不继承到新模型。

用户只修改 effort 时，Session 保存选中的权威 `ReasoningEffort`。YAML 后续发生变化时，已存在 Session 继续使用自身保存的明确值。

Web 和 TUI 从模型目录初始化选择后，在创建请求中提交完整 `ModelSelection`；CLI、SDK 或其他调用方可以省略该字段并使用服务端配置默认值。若目录加载后配置发生变化，服务端拒绝已经失效的选择，客户端刷新目录后重新选择，不静默改用新的默认模型。

### 校验边界

Service 在持久化或 dispatch 前校验完整选择：

- Provider 存在于 `LlmConfig`。
- 模型存在于该 Provider 的模型列表。
- 只有 `reasoning: true` 的模型才能接受 `Some(effort)`。
- `reasoning: false` 的模型保存 `None`，并且不发送 Reasoning 控制字段。

Provider 名称和模型 ID 均按配置中的原始字符串精确匹配，不执行大小写折叠、trim 后猜测或别名推断。YAML 删除 Provider/模型，或把已有 Session 所选模型从 `reasoning: true` 改为 `false` 后，若 Session 保存的选择不再通过校验，dispatch 返回包含 Provider 和模型 ID 的明确错误。用户通过现有模型选择 mutation 重新选择即可恢复；不新增隐式修复状态或 fallback。

Provider adapter 只接收校验后的模型身份和可选权威 effort，不读取 Session 状态、UI 状态或配置默认值。

## API 契约

本轮不新增一套模型管理 API，只调整现有契约：

- `model.list` 返回 `ModelListResponse { models, default_model_selection }`；`models` 包含所有 Provider-qualified `ModelInfo`，默认选择已经解析成完整 `ModelSelection`。
- Session 创建请求中的 `model_selection` 保持可选。省略时由 Session service 使用 `default_model_selection`，提供时由 Service 校验后原样持久化。
- Session 模型更新继续使用现有 Session update/patch mutation，但 payload 必须是完整 `ModelSelection`。Provider、模型 ID 和 effort 在同一次 store update 中提交，不能分步写入。
- mutation 响应返回更新后的 Session。客户端以该响应覆盖本地选择；本轮不增加客户端版本号或另一套并发状态对象。
- 未知 Provider、未知模型、Reasoning 不匹配和模型已失效统一返回现有 validation error，并在错误消息中包含 Provider 与模型 ID；readiness 错误继续由具体 Provider 边界返回。

## Talos Reasoning Effort

Talos 产品档位统一为：

```text
Light | Medium | High | XHigh
```

现有 `Low` 在 Rust、YAML、schema、生成的 TypeScript、Web 和 TUI 中统一改名为 `Light`。Provider wire 值由 Provider adapter 负责，仍可使用 `low`。

当前 Provider adapter 的确定性映射如下：

| Talos | OpenAI | Anthropic budget tokens | Bailian/Qwen budget tokens | DeepSeek | OpenRouter | Ollama effort |
| --- | --- | ---: | ---: | --- | --- | --- |
| `Light` | `low` | `1024` | `1024` | `high` | `low` | `low` |
| `Medium` | `medium` | `2048` | `2048` | `high` | `medium` | `medium` |
| `High` | `high` | `4096` | `4096` | `high` | `high` | `high` |
| `XHigh` | `xhigh` | `8192` | `8192` | `max` | `xhigh` | `high` |

补充约束：

- Session 字段为 `None` 时，不序列化 Reasoning 控制参数。
- OpenRouter 使用其标准 Reasoning effort 请求对象。
- Anthropic 和 Bailian/Qwen 当前使用表中的 budget-token 映射；相关模型只有在四档映射均有 wire request 测试后才能配置为 `reasoning: true`。
- Ollama 对接受 effort 字符串的模型使用表中映射。只接受布尔 `think` 的模型在 adapter 内将四档统一映射为开启；Session 中仍保存原始 Talos effort。
- 新 Provider 或新的模型特例必须在 adapter 内增加四档穷尽映射和对应 wire request 测试。不得在运行时猜测“最近档位”。

## Web 与 TUI 行为

Web 和 TUI 使用相同的完整 `ModelInfo`，不能再维护只包含模型 ID 的目录。

模型选择器必须：

- 按 Provider 分组展示模型。
- 使用 `(provider, id)` 作为选择值。
- 展示配置中的模型名称。
- 新 Session 默认选中 `llm.provider` 和 `llm.model`。
- 已存在 Session 选中其保存的 `ModelRef`。

Reasoning 控件必须：

- 选中模型为 `reasoning: true` 时，展示 `Light`、`Medium`、`High` 和 `XHigh`。
- 新 Session 或刚切换模型时，选中模型配置中的 `reasoning_effort`。
- 已存在 Session 展示其保存的 effort。
- 选中模型为 `reasoning: false` 时，展示 `None` 并禁用选择。
- 不展示内部默认标记或 Provider-specific effort 名称。

创建 Session、切换模型或修改 effort 时，两端都提交完整 `ModelSelection`。mutation 完成后，以后端确认的 Session 为事实来源。

## Ollama 预置模型配置

根目录配置为 Ollama 声明少量预置模型，继续复用与其他 Provider 相同的 `ProviderModelConfig`。这些条目只代表 Talos 的模型配置，不代表本地已经安装对应模型：

```yaml
ollama:
  api_key: null
  base_url: "http://localhost:11434"
  protocol: "chat"
  models:
    - id: "qwen3:8b"
      name: "Qwen3 8B"
      context_window: 40960
      reasoning: true
      reasoning_effort: "medium"
      speed: "standard"
      input: ["text"]
      cost: { input: 0, output: 0, cache_read: 0, cache_write: 0 }

    - id: "deepseek-r1:8b"
      name: "DeepSeek R1 8B"
      context_window: 131072
      reasoning: true
      reasoning_effort: "medium"
      speed: "standard"
      input: ["text"]
      cost: { input: 0, output: 0, cache_read: 0, cache_write: 0 }

    - id: "gpt-oss:20b"
      name: "GPT OSS 20B"
      context_window: 131072
      reasoning: true
      reasoning_effort: "medium"
      speed: "standard"
      input: ["text"]
      cost: { input: 0, output: 0, cache_read: 0, cache_write: 0 }
```

Ollama 模型目录与其他 Provider 一样完全来自配置。用户需要自行保证配置的模型在目标 Ollama 服务中可用。Talos 不调用模型发现接口，也不推断本地安装状态；dispatch 直接请求配置的 endpoint，并保留实际响应中的连接或模型错误。Talos 不自动安装模型，也不 fallback。

## 职责边界

| 层级 | 职责 |
| --- | --- |
| Configuration | 权威 Provider/模型元数据和配置默认值 |
| Model catalog service | 枚举包含 Provider 身份的配置模型 |
| Session service | 解析并保存明确的模型身份和 effort |
| Gateway/App Protocol/SDK | 传输共享对象，不重复定义 |
| Web/TUI | 渲染目录元数据并提交完整选择 |
| Provider adapter | 将权威 effort 映射到 Provider wire 字段 |
| Ollama adapter | 执行 effort 映射并请求配置的 endpoint |

## 不兼容处理

本设计会修改当前主版本协议和持久化 Session schema，按 breaking change 处理：

1. 配置、公共 API 和持久化 Session 只接受当前 `ModelSelection` 结构以及 `light/medium/high/xhigh` effort。
2. Session 必须明确保存 Provider、模型 ID 和 Reasoning effort；缺少 `modelSelection` 的 Session 直接返回不兼容格式错误。
3. 不读取旧 tagged `reasoning`、`modelDefault` 或 `low`，不按裸模型 ID 猜测 Provider，也不执行迁移、写回、alias 或 fallback parser。
4. 同步更新 Rust/TypeScript schema、SDK 调用方、根目录配置、Desktop、E2E 和测试 fixture。
5. 针对不兼容 Session 只验证当前错误边界，不保留针对已删除旧结构的反向测试和 fixture。

## 验证策略

### 配置与共享类型

- 使用权威配置类型解析所有随仓库发布的 YAML。
- 校验每个默认 Provider/模型组合都真实存在。
- 校验模型目录序列化保持扁平并包含 Provider 身份。
- 校验公共 API 中不存在重复模型或 effort 定义。

### Service 与 Provider

- 使用每个已配置 Provider 创建 Session，断言保存的 effort 等于模型配置。
- 跨 Provider 切换 Session 模型，断言 Provider、模型和 effort 同时更新。
- 拒绝为 `reasoning: false` 的模型设置 effort。
- 分别验证每个 Provider adapter 的 Light/Medium/High/XHigh 请求映射。
- 验证 Ollama 直接请求配置的 endpoint，且不执行模型发现、fallback 或安装。

### Web 与 TUI

- 为按 Provider 分组的模型选择器增加 snapshot。
- 验证不同 Provider 下相同模型 ID 可以独立选择。
- 验证切换模型后 effort 使用目标模型配置。
- 验证 `reasoning: false` 展示禁用的 `None`。
- 验证已存在 Session 展示自身保存的 effort。
- 验证不兼容 Session 返回明确错误且不修改持久化数据。
- 为动态 Provider、模型和 effort 更新增加 Web/TUI E2E。

### Breaking change 审查

- 重新生成 App Protocol schema 和 TypeScript binding。
- 搜索旧 effort 名称、重复模型 DTO 和仅使用模型 ID 的选择 helper。
- 验证 Session restore、clear、compact、retry 和 resume 都保留明确选择。

## 风险与后续

1. Provider 文档可能独立变化。映射测试必须断言实际 wire request，确保变化只影响 Provider adapter。
2. 配置模型可能因为凭证、endpoint 下线或服务端模型状态而不可用。实际 dispatch 错误不应改变配置对象模型。
3. 如果未来需要远程模型发现，应通过相同的 Provider-qualified 模型目录合并，不能替代配置元数据。
