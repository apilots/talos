# Agent Inner Model 设计

Status: archived
Owner: agent
Created: 2026-08-23
Updated: 2026-08-23
Related: `refactor-038-llm-model-selection`, `refactor-039-memory-storage-retrieval`, `feat-031-profile-usage-statistics`

## 1. 结论

Talos 为 Agent 增加一个可选的 `inner_model`，用于 Title、Daily Memory Dream 整理和 managed `MEMORY.md` 维护三类 Agent 内部模型调用。

```yaml
agent:
  inner_model:
    provider: "deepseek"
    id: "deepseek-v4-flash"
```

配置存在时必须使用指定模型；配置缺省时使用 Agent 默认模型。Session 中用户选择的模型继续只驱动 Session 工作、Subagent 和上下文压缩。

`inner_model` 只保存 `provider + id`。Provider 凭证、endpoint、protocol 和模型元数据继续由现有 `llm` 配置和模型目录提供。

## 2. 目标

1. 为 Agent 内部模型调用提供单一、强类型的模型选择、Provider 路由和计量入口。
2. Title、Daily Memory Dream 整理和 managed `MEMORY.md` 维护使用同一个 `InnerModelClient`。
3. Provider 返回的 Token Usage 复用现有 Profile 计量链路，按用户、实际模型和调用用途归类。
4. 配置启动和热更新时先校验再切换，运行时不隐式 fallback 到其他模型。
5. 保持业务 Prompt、输出解析、Memory 写入和 Session 生命周期在现有 owner 中。

## 3. 非目标

1. 不修改正常 Session Turn 的模型选择。
2. 不修改 Subagent 继承父 Session 模型的行为。
3. 不修改自动或手动上下文压缩的模型和 Session 独占语义。
4. 不修改 Bash 风险评估或其他安全策略。
5. 不为 `inner_model` 复制 Provider 凭证、endpoint、protocol 或模型目录。

## 4. 当前实现事实

- Session Title 直接使用当前 `AgentRunConfig.model_selection`。
- Daily Memory 和 managed `MEMORY.md` 整理使用全局 `llm` 默认模型。
- 自动和手动上下文压缩使用 Session Provider 和 Session 模型。
- Subagent 创建时持久化父 Session 的 `model_selection`。
- `ReloadableLlmProviderRouter` 已能按 `ModelRef` 创建、缓存和刷新 Provider，不需要新 Provider 运行时。
- `ChatResponse.usage` 已包含输入、输出、总量和缓存 Token，Profile 页面已通过 `ProfileStore` 和 `ProfileService` 汇总展示用户用量。
- 现有 Profile 写入只接受 `TurnUsageUpdate`，尚不能表达 Agent 内部调用。

## 5. 配置与解析

`AgentConfig` 增加可选字段：

```rust
pub struct AgentConfig {
    pub inner_model: Option<ModelRef>,
    // Existing fields remain unchanged.
}
```

`ModelRef` 作为规范模型身份与 `LlmConfig` 位于同一低依赖配置边界，`talos-types` 继续重新导出同一类型，不改变已有公开 wire shape。

解析优先级固定为：

```text
agent.inner_model
    ?? AgentDefinition.model_selection.model_ref
```

解析结果必须在 `llm` 模型目录中存在。Provider 运行参数通过已解析的 `provider` 从 `LlmConfig` 获取；模型能力通过 `provider + id` 从 `ProviderModelConfig` 获取。

配置启动时完成目录校验和 Provider 构造校验。缺少凭证继续遵循现有 setup-required 启动语义，不视为构造失败；实际调用返回原有配置错误且不 fallback。配置热更新先解析新值，校验成功后再原子替换 `InnerModelClient` 持有的 `ModelRef`；校验失败时配置更新失败，当前有效值不变。

## 6. 模块与 API

共享 Usage 类型和 Inner Model 分别放在独立目录：

```text
talos-types/src/usage/
  mod.rs
  model.rs

talos-agent/src/inner_model/
  mod.rs
  client.rs
  resolver.rs
  types.rs
  client_tests.rs
  resolver_tests.rs
```

职责边界：

- `talos-types::usage` 是 Usage 类型的唯一 owner，保存 `TokenUsage` 和 `ModelUsageKind`。
- `usage/mod.rs` 只组织并导出公开类型；`usage/model.rs` 保存类型定义和必要方法。
- `talos-types` crate root 与现有 `talos` 模块重导出同一 `TokenUsage`，不复制定义、不改变 wire shape。
- `resolver.rs` 只解析 Agent 默认值、显式配置和模型目录。
- `client.rs` 只负责注入已解析模型、通过 Router 发起调用，并把 Provider Usage 交给现有 `ProfileStore`。
- `types.rs` 只保存稳定的内部调用参数，并复用共享的 `ModelUsageKind`。
- Title、Memory 和 Dream 的 Prompt、输出 schema 与持久化不进入该目录。

共享 Usage 类型定义在 `talos-types::usage`：

```rust
pub enum ModelUsageKind {
    Turn,
    Compact,
    Title,
    Memory,
    Dream,
}
```

`talos-agent::inner_model` 只定义调用参数和 Client：

```rust

pub struct InnerModelChatParams {
    pub user_id: String,
    pub session_id: Option<String>,
    pub usage_kind: ModelUsageKind,
    pub messages: Vec<ChatMessage>,
}

pub struct InnerModelClientBuilder;
pub struct InnerModelClient;
```

Builder 用于组装多个显式依赖，避免不透明位置参数：

```rust
let inner_model = InnerModelClient::builder()
    .configured_model(config.agent.inner_model.clone())
    .agent_default_model(agent_definition.model_selection.model_ref.clone())
    .model_catalog(resolve_model_catalog(&config.llm))
    .provider_router(llm_provider_router.clone())
    .profile_store(profile_store.clone())
    .profile_timezone(config.agent.kernel.timezone.clone())
    .build()?;
```

`build()` 解析有效 `ModelRef`、校验模型目录并确认 Router 能构造 Provider。`InnerModelClient` 不长期保存独立 Provider 实例，避免配置刷新后继续使用旧凭证或 endpoint。

运行调用：

```rust
let response = inner_model
    .chat(InnerModelChatParams {
        user_id,
        session_id: Some(session_id),
        usage_kind: ModelUsageKind::Title,
        messages,
    })
    .await?;
```

`chat()` 用当前已解析 `ModelRef` 构造 `ChatRequest`，通过 `LlmProviderRouter` 获取 Provider，把 `ChatResponse.usage` 转成现有 `TokenUsage`，再按对应的 `ModelUsageKind` 写入 Profile。调用方只提供业务 Messages，不能覆盖 Inner Model。

配置刷新只暴露一个窄方法：

```rust
inner_model.reload(validated_model_ref);
```

`reload()` 只接收上层已完成目录与 Provider 构造校验的 `ModelRef`；校验失败时上层不调用该方法，因此当前值保持不变。

## 7. 调用点

### 7.1 Title

`AgentHarnessImpl` 注入 `Arc<InnerModelClient>`。Session 首次标题生成使用 `ModelUsageKind::Title`。标题调用失败时保持现有确定性标题生成行为。

### 7.2 Daily Memory

`MemoryDailyMaintenance` 注入同一个 `Arc<InnerModelClient>`。Session tail 的 durable memory candidate 提取属于 Dream 整理，使用 `ModelUsageKind::Dream`。

Provider、解析或计量失败时，不写 daily memory，不推进 `memory_processed_until_seq`。

### 7.3 Managed MEMORY.md

managed `MEMORY.md` 维护使用 `ModelUsageKind::Memory`。调用失败、输出无效或计量失败时，不更改 `MEMORY.md`。

原子写入成功后输出一条结构化 `info` 日志，记录用户、模型、Token Usage、候选数量、managed 内容字符数、文件路径和内容是否实际变化；日志不记录 Memory 正文。

## 8. 计量与错误

`ModelUsageKind` 由 `talos-types::usage` 唯一定义，Inner Model、Turn、Compact 和 Profile Store 共同使用，不再定义 Inner 专用枚举。Profile 写入对象为：

```rust
pub enum ProfileUsageUpdate {
    Turn(TurnUsageUpdate),
    ModelCall(ModelCallUsageUpdate),
}

pub struct ModelCallUsageUpdate {
    pub user_id: String,
    pub session_id: Option<String>,
    pub request_id: String,
    pub kind: ModelUsageKind,
    pub model_ref: ModelRef,
    pub usage: TokenUsage,
    pub usage_available: bool,
    pub timezone: String,
    pub completed_at_ms: i64,
    pub usage_date: String,
}
```

`ProfileStore::record_usage(ProfileUsageUpdate)` 是唯一写入口。`Turn` 继续使用原有等待、恢复、完成和去重语义；`ModelCall` 通过共享 `ModelUsageKind` 表达 Compact、Title、Memory 和 Dream，一次 Provider 调用对应一条已完成记录，以 `request_id` 去重。

现有 Profile usage 表原地增加分类字段，不新增计量表：

```text
usage_kind: turn | compact | title | memory | dream
```

已有数据迁移后统一标记为 `turn`。所有类型复用同一事务、保留策略和用户作用域；总量、每日用量和模型用量聚合包含全部类型，现有 Profile 页面因此自动展示完整 Token。Session 数、消息数和 Turn 数只统计 `Turn`。`usage_kind` 可直接区分用户 Session 工作、压缩和 Agent 内部调用，本 Spec 不增加新的 Gateway、SDK 或 UI 接口。

`Title`、`Memory` 和 `Dream` 不更改 Session 的 `model_selection`、最后 Assistant 模型、Turn Metrics 或 Session 对话 Token。当前只对已有显式 Session Compact API 写入 `Compact` 分类；自动上下文压缩保持现有计量行为。两者都继续使用 Session 模型。

错误语义：

- `agent.inner_model` 存在但 Provider 或 Model 不在目录中：配置启动或更新失败，错误包含 `provider:id`。
- Provider 协议或构造失败：配置启动或更新失败。
- Provider 凭证缺失：服务保持现有 setup-required 启动行为，调用失败且不 fallback。
- Provider 请求失败：返回原有强类型 Provider 错误，不调用 Agent 默认模型。
- 内部计量失败：调用失败并由业务 owner 执行上述不变式，禁止静默丢失用量。

## 9. 影响范围

| 区域 | 变更 |
| --- | --- |
| `talos-config` | `AgentConfig.inner_model`、schema、配置校验 |
| `talos-types` | 新增 `usage/`，集中 `TokenUsage` 和 `ModelUsageKind`；重新导出规范 `ModelRef` |
| `talos-agent` | `inner_model/`、Title 调用、Harness 注入 |
| `talos-service` | Daily Memory、managed `MEMORY.md` 调用，复用 Profile Usage 组装 |
| `talos-store::profile` | `ProfileUsageUpdate` 用途类型、现有 usage 表分类字段和聚合 |
| `talos-base` | 构造、共享注入和配置 reload |
| `conf` | 默认配置中声明 mini Inner Model |

## 10. 验证

- 解析器完整对象测试：缺省使用 Agent 默认模型，显式配置使用指定模型，无效 `provider:id` 被拒绝。
- Client 测试：请求实际使用已解析模型，Provider 通过 Router 获取，不发生模型 fallback。
- reload 测试：有效新值从下一次调用生效，无效新值不替换当前值。
- Title 集成测试：使用 Inner Model，失败时保持确定性标题。
- Daily Memory 集成测试：使用 `Dream` 计量，失败时不写入且不推进 cursor。
- Managed Memory 集成测试：使用 `Memory` 计量，失败时不更改 `MEMORY.md`。
- 计量测试：Provider Usage 完整写入对应 `ProfileUsageUpdate`，按 `user_id + usage_kind + model_ref` 归类。
- Profile 聚合测试：总量、每日和模型统计包含 Inner；Session、消息和 Turn 数不受 Inner 记录影响。
- 回归测试：正常 Session、Subagent、自动/手动压缩和 Bash 风险评估继续使用现有模型选择。
