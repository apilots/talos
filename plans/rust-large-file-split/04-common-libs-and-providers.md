# 阶段 5：`common` 与 `llm` 拆分执行计划

> 本文件只定义 `cognition/memory`、`common/prompt`、`common/types`、`common/llm/provider` 以及三个 provider crate 的迁移边界。
> 与后续独立的 streaming interleaved execution 迭代直接相关的 `general_turn`、`KernelStreamEvent*`、`TalosEvent*`、`agent.rs`、`ui-tui/stream.rs` 不在本文件中实现，只能在这里冻结接口和说明风险。

## 0. 当前基线

本阶段的拆分对象与当前行数基线如下：

- `talos-rs/crates/cognition/memory/src/store.rs`: `1027`
- `talos-rs/crates/common/prompt/src/lib.rs`: `1027`
- `talos-rs/crates/common/types/src/lib.rs`: `1135`
- `talos-rs/crates/common/llm/provider/src/lib.rs`: `731`
- `talos-rs/crates/common/llm/ollama/src/lib.rs`: `1132`
- `talos-rs/crates/common/llm/anthropic/src/lib.rs`: `1087`
- `talos-rs/crates/common/llm/bailian/src/lib.rs`: `1084`

当前已经在阈值内、原则上不作为本阶段主拆分目标的文件：

- `talos-rs/crates/cognition/memory/src/lib.rs`: `25`
- `talos-rs/crates/common/prompt/src/renderer.rs`: 已独立
- `talos-rs/crates/common/prompt/src/budget.rs`: 已独立
- `talos-rs/crates/common/prompt/src/bootstrap.rs`: 已独立
- `talos-rs/crates/common/prompt/src/diagnostics.rs`: 已独立
- `talos-rs/crates/common/prompt/src/types.rs`: 已独立

## 0.1 与后续独立 streaming 迭代的边界

本阶段需要为后续独立的 streaming interleaved execution 迭代冻结共享契约，但不允许在这里提前改动以下语义：

- `talos-rs/crates/common/types/src/lib.rs` 中未来会归入 `talos.rs` 的事件协议层
- `talos-rs/crates/common/llm/provider/src/lib.rs` 中 `ChatRequest`、`ChatResponse`、`ToolStreamEvent` 等流式契约

执行顺序约束如下：

1. 阶段 5 可以先完成 `memory`、`prompt` 与三个 provider 的结构拆分。
2. 阶段 5 的 `types` 拆分只能先冻结模块位置、导出面和文件边界，不能在本阶段顺手改事件语义。
3. 后续 streaming 迭代若开始实现，必须基于已经冻结的位置去扩展事件与流式执行语义，而不是反向改写阶段 5 的模块划分。
4. 若后续 streaming 迭代修改 `TalosEvent*` 或 `ToolStreamEvent`，阶段 5 的 `types/provider` 拆分应保持稳定，不再改名或重排 payload 组织。

## 1. 阶段 1：冻结共同边界

**目标**: 先把本阶段所有会动的目录、公开导出和依赖方向冻结下来，避免后续拆分过程中频繁改边界。

**成功标准**:
- 目标目录结构在本阶段开始前固定。
- `cognition/memory`、`common/prompt`、`common/types`、`common/llm/provider`、`common/llm/{ollama,anthropic,bailian}` 的公共导出面先锁定，不在后续阶段随意改名。
- 明确哪些内容属于后续 streaming 迭代，哪些内容只允许“透传接口”而不能提前实现。

**目标目录结构**:

```text
cognition/memory/src/
  lib.rs
  store/
    mod.rs
    record.rs
    query.rs
    transcript.rs
    summary.rs
  path_policy.rs
  retrieval.rs
  session_store.rs
  workspace_store.rs
  memory_impl.rs
  mapper.rs
  trait.rs

common/prompt/src/
  lib.rs
  template.rs
  builder.rs
  assembler.rs
  bootstrap.rs
  budget.rs
  diagnostics.rs
  renderer.rs
  types.rs
  presets.rs
  tests.rs

common/types/src/
  lib.rs
  ids.rs
  model.rs
  gateway.rs
  session.rs
  runtime.rs
  talos.rs
  tests.rs

common/llm/provider/src/
  lib.rs
  types.rs
  request.rs
  stream.rs
  tool.rs
  tests.rs

common/llm/{ollama,anthropic,bailian}/src/
  lib.rs
  config.rs
  client.rs
  request.rs
  response.rs
  stream.rs
  mapper.rs
  tests.rs
```

**依赖方向**:
- `cognition/memory` 只向外依赖基础库，不反向依赖 `prompt` 或 provider 实现。
- `common/prompt` 可以依赖 `talos_memory`、`talos_tools`、`talos_llm_provider`、`talos_types`，但不能依赖任一 provider 实现 crate。
- `common/types` 保持为纯数据与协议类型层，不依赖 `prompt` 或 provider 实现层。
- `common/llm/provider` 只定义抽象契约和共享基础类型，三个 provider crate 只能依赖它，不互相依赖。

**推荐迁移顺序**:
1. 先冻结目录树和导出面，列出每个 crate 的“允许改动”和“禁止改动”。
2. 再冻结 `common/types/src/talos.rs` 与 `common/llm/provider/src/lib.rs` 的公共契约，因为后续 streaming 迭代会消费这两处。
3. 最后确认本阶段不碰 `general_turn`、`KernelStreamEvent*`、`TalosEvent*` 的语义实现。

**后续 streaming 迭代风险**:
- `common/types/src/talos.rs` 后续会被 streaming 迭代扩展事件语义，但本阶段只能冻结位置和导出，不提前加新事件语义。
- `common/llm/provider/src/lib.rs` 的 `ChatRequest`、`ChatResponse`、`ToolStreamEvent` 是 streaming 迭代的输入/输出契约，本阶段只能做稳定化，不能为了 provider 去改调用方协议。

**验证门禁**:
- 基线测试全部通过后，才能进入后续迁移：
  - `cargo test -p talos-memory`
  - `cargo test -p talos-prompt`
  - `cargo test -p talos-types`
  - `cargo test -p talos-llm-provider`
  - `cargo test -p talos-llm-ollama`
  - `cargo test -p talos-llm-anthropic`
  - `cargo test -p talos-llm-bailian`

**状态**: 已完成

## 2. 阶段 2：拆分 `cognition/memory` 与 `common/prompt`

**目标**: 把 `cognition/memory/src/store.rs` 和 `common/prompt/src/lib.rs` 的职责下沉到稳定子模块，保留 crate root 为薄门面。

**成功标准**:
- `cognition/memory/src/store.rs` 不再承载全部记录、查询、摘要、会话相关类型。
- `common/prompt/src/lib.rs` 不再承载模板、构建、装配、预算、诊断的主要逻辑。
- 两个 crate 的公共 API 仍然稳定，调用方不需要大规模改 import 路径。

**目标目录结构**:

```text
cognition/memory/src/
  lib.rs              仅保留 facade、re-export 和少量入口逻辑
  store/
    mod.rs            store 入口与 re-export
    record.rs         记录与索引相关类型
    query.rs          查询与检索请求/结果
    transcript.rs     transcript / event / payload 相关类型
    summary.rs        摘要与压缩结果
  path_policy.rs      路径与 workspace policy
  retrieval.rs        检索流程
  session_store.rs    session 级别存取
  workspace_store.rs  workspace 级别存取
  memory_impl.rs      `Memory` trait 实现
  mapper.rs           结构转换辅助
  trait.rs            `Memory` trait

common/prompt/src/
  lib.rs              facade + re-export
  template.rs         `PromptTemplate`、`PromptManager`
  builder.rs          `PromptBuilder` 与装配入口
  assembler.rs        prompt 拼装的细粒度 helper
  bootstrap.rs        bootstrap 文件与上下文加载
  budget.rs           budget / truncation / tier 计算
  diagnostics.rs      diagnostics / warning 生成
  renderer.rs         template 渲染与 cache control
  types.rs            mode / section / config / diagnostic types
  presets.rs          内置 prompt presets
  tests.rs            crate 单测
```

**依赖方向**:
- `cognition/memory` 先拆，不依赖 `common/prompt`。
- `common/prompt` 可以依赖 `cognition/memory`、`talos_llm_provider`、`talos_tools`、`talos_types`，但不能反向依赖具体 provider crate。
- `template.rs` 和 `builder.rs` 之间只允许单向依赖，避免互相调用造成循环。

**推荐迁移顺序**:
1. 先拆 `cognition/memory/src/store.rs`，因为它是最明确的超长文件，收益最大。
2. 再收敛 `memory_impl.rs`、`retrieval.rs`、`session_store.rs`、`workspace_store.rs` 的公共 helper，避免重复逻辑继续散落在 `store.rs`。
3. 然后拆 `common/prompt/src/lib.rs` 中的模板定义与装配逻辑，优先把 `PromptTemplate` / `PromptManager` 独立出去。
4. 再拆 `bootstrap.rs`、`budget.rs`、`diagnostics.rs`、`renderer.rs`，让 `lib.rs` 只负责门面。
5. 最后整理 `tests.rs`，保证测试不再依赖内部实现细节。

**后续 streaming 迭代风险**:
- `common/prompt` 会被后续 streaming 迭代间接消费，但它只需要稳定的 prompt 组装能力，不需要 prompt 内部重新设计。
- 这一步不能提前引入任何 `general_turn` 相关语义，也不能把 streaming 事件模型塞进 prompt crate。

**验证门禁**:
- 每次移动一个子模块后，先跑对应 crate 的单测。
- 阶段收口时至少执行：
  - `cargo test -p talos-memory`
  - `cargo test -p talos-prompt`

**状态**: 已完成

## 3. 阶段 3：拆分 `common/types` 的协议面

**目标**: 把 `common/types/src/lib.rs` 按协议边界拆成独立模块，减少 `talos` 协议、session、gateway、ID 类型、运行时辅助之间的耦合。

**成功标准**:
- `lib.rs` 只保留门面、re-export 和少量公共别名。
- `talos.rs`、`gateway.rs`、`session.rs`、`ids.rs`、`model.rs`、`runtime.rs` 的职责清晰，不再互相夹杂。
- 调用方仍然通过 `talos_types::...` 的稳定路径访问核心类型。

**目标目录结构**:

```text
common/types/src/
  lib.rs        facade + re-export
  ids.rs        `AgentId`、`SessionId`、`UserId`、`ChannelId`、`MessageId`
  model.rs      `ModelInfo`
  gateway.rs    `GatewayRequest` / `GatewayResponse`
  session.rs    `SessionRecord`、`SessionContext`、`TalosSession`、`SessionService`
  runtime.rs    `now()`、`DEFAULT_SESSION_TITLE`、`SessionConfigOverridePolicy`
  talos.rs      `TalosConnectionMode`、`StopReason`、`TalosContext`、`TalosInputItem`、
                `TalosOutputItem`、`TalosEventType`、`TalosEventPayload`、`TalosEvent`
  tests.rs      crate tests
```

**依赖方向**:
- `ids.rs` 不依赖其他本地模块。
- `model.rs` 只依赖 `talos_config` 的 re-export，不依赖 session 或 gateway。
- `gateway.rs` 只依赖 `talos.rs`。
- `session.rs` 只依赖 `talos.rs`、`talos_config`、`serde_json` 和基础容器类型。
- `runtime.rs` 可以依赖 `talos.rs` 和 `session.rs`，但不反向依赖 `gateway.rs`。
- `talos.rs` 是后续 streaming 迭代最敏感的共享契约层，不能把执行时编排逻辑塞进来。

**推荐迁移顺序**:
1. 先拆 `ids.rs` 和 `model.rs`，因为它们是最独立的基础类型。
2. 再拆 `talos.rs`，把协议层和事件层先冻结下来。
3. 然后拆 `gateway.rs` 和 `session.rs`，让数据面与运行时辅助解耦。
4. 最后拆 `runtime.rs` 和 `tests.rs`，收尾门面导出。
5. `lib.rs` 最后只保留 re-export 和少量 alias。

**后续 streaming 迭代风险**:
- 后续 streaming 迭代会继续扩展 `TalosEventType`、`TalosEventPayload` 以及相关事件数据结构，因此 `talos.rs` 必须先冻结模块位置与命名，再谈新增字段。
- 如果本阶段顺手改掉事件命名或 payload 组织方式，后续 streaming 迭代的 TUI 和 kernel 代码都会被迫跟着返工，所以这里只能做结构拆分，不能做语义重命名。

**验证门禁**:
- 每移动一个子模块后先跑 `cargo test -p talos-types`。
- 阶段收口时至少执行 `cargo test -p talos-types`。

**状态**: 已完成

## 4. 阶段 4：一次性拆分三个 provider crate

**目标**: 把 `ollama`、`anthropic`、`bailian` 统一拆成相同的内部模板，确保三者目录层级一致、对比成本一致、后续 review 成本一致。

**成功标准**:
- 三个 provider crate 的目录结构尽量同构。
- 每个 provider crate 都不再把配置、请求、响应、流式解析、工具映射全部堆在 `lib.rs`。
- provider 之间没有横向依赖，公共行为只走 `common/llm/provider`。

**目标目录结构**:

```text
common/llm/{ollama,anthropic,bailian}/src/
  lib.rs
  config.rs      provider config / default / builder
  client.rs      HTTP client / endpoint / auth
  request.rs     provider-specific request shape
  response.rs    provider-specific response shape
  stream.rs      streaming parser
  mapper.rs      tool / usage / message mapping
  tests.rs       crate tests
```

**依赖方向**:
- `common/llm/provider` 提供统一抽象，三个 provider crate 只能依赖它，不互相依赖。
- `ollama`、`anthropic`、`bailian` 只保留 provider 自己的 endpoint、auth、协议差异，不把共享 helper 留在各自 crate 里。
- 任何共享的 `ToolStreamEvent` 处理、usage 归一化、tool call 聚合，都优先抽到 `common/llm/provider`，不要复制三份。

**推荐迁移顺序**:
1. 先确认 `common/llm/provider` 的公共抽象不变。
2. 先拆 `ollama`，因为它的兼容层最容易作为同构模板起点。
3. 再拆 `bailian`，把 OpenAI-compatible 分支和 thinking 相关逻辑分开。
4. 最后拆 `anthropic`，保留它自己的 thinking/cache 变体。
5. 三个 provider crate 的结构定下来后，再统一对齐测试命名和 helper 命名。

**后续 streaming 迭代风险**:
- 后续 streaming 迭代会消费 provider 的 streaming 行为，但不应该反向驱动本阶段去改 provider trait 的语义。
- 如果本阶段改动 `ChatRequest`、`ChatResponse`、`ToolStreamEvent` 的公共含义，后续 streaming 迭代会立刻受影响，所以这里只允许抽离实现细节，不允许改公共契约。

**验证门禁**:
- 每拆完一个 provider 子模块，先跑对应 crate 测试。
- 阶段收口时至少执行：
  - `cargo test -p talos-llm-provider`
  - `cargo test -p talos-llm-ollama`
  - `cargo test -p talos-llm-anthropic`
  - `cargo test -p talos-llm-bailian`

**状态**: 已完成

## 5. 阶段 5：清理 provider 之间的重复逻辑

**目标**: 在三套 provider 目录结构都稳定之后，再把已经证明稳定的重复逻辑抽到 `common/llm/provider`，避免一开始就做过早抽象。

**成功标准**:
- `common/llm/provider/src/lib.rs` 变成真正的公共门面。
- `FunctionCall`、`FunctionResponse`、`ToolDefinition`、`ToolChoice`、`ChatRequest`、`ToolStream`、`ToolStreamEvent` 这些共享类型的实现边界清晰。
- 三个 provider crate 里只保留真正的 provider 差异，不再复制流式拼装、usage 映射、tool call 合并、错误转换等稳定逻辑。

**目标目录结构**:

```text
common/llm/provider/src/
  lib.rs        facade + trait re-export
  types.rs      shared function / tool / error types
  request.rs    `ChatRequest` / builder / message helpers
  stream.rs     `ChatStream` / `ToolStream` adapter
  tool.rs       tool call normalization / validation helpers
  tests.rs      shared tests
```

**依赖方向**:
- `common/llm/provider` 向外提供共享契约，provider 实现 crate 只使用这些共享契约，不反向引入 provider 实现细节。
- 重复逻辑如果是“稳定通用行为”，就抽进 `common/llm/provider`；如果是“单 provider 的 API 差异”，就留在各自 crate。
- 这一步不能为了后续 streaming 迭代直接加新事件模型，新增语义应该在独立 streaming 方案里定义。

**推荐迁移顺序**:
1. 先收紧 `common/llm/provider/src/lib.rs` 的公共 surface，确认哪些类型是稳定契约。
2. 先抽 `stream.rs`，优先收敛流式事件适配和工具调用的公共行为。
3. 再抽 `tool.rs` 和 `types.rs`，把 tool call / response / error 处理统一起来。
4. 如果仍有重复，再补 `request.rs`，把 request builder 和参数验证也统一掉。
5. 最后回到三个 provider crate，删掉已被上移的重复 helper。

**后续 streaming 迭代风险**:
- 后续 streaming 迭代会直接消费 `ToolStreamEvent` 和 `ChatStream`，所以这里最怕的是“顺手改协议”；协议一变，后续执行流就会重写。
- 这里允许的是“抽 helper，不改语义”；不允许的是“为了更好抽象，改掉调用方已经依赖的事件顺序或字段语义”。

**验证门禁**:
- 每次抽公共 helper 后，至少跑一次：
  - `cargo test -p talos-llm-provider`
  - `cargo test -p talos-llm-ollama`
  - `cargo test -p talos-llm-anthropic`
  - `cargo test -p talos-llm-bailian`
- 阶段收口时再跑一次完整回归：
  - `tests/integration/run_e2e.sh`

**状态**: 已完成

## 6. 最终收口标准

当且仅当以下条件全部满足时，阶段 5 才算完成：

- `cognition/memory`、`common/prompt`、`common/types`、`common/llm/provider`、`common/llm/{ollama,anthropic,bailian}` 的边界都已稳定。
- 超长文件都被拆到 1000 行以下，剩余 facade 文件不再承载核心业务逻辑。
- 共享契约没有为了后续 streaming 迭代预埋实现，所需的语义变更只保留接口冻结，不在本阶段落地。
- 所有相关 crate 的单测和集成回归都通过。

## 7. 最终验收矩阵

在提交阶段 5 实现前，至少要满足以下验收矩阵：

- 结构门禁
  - `common/prompt`、`cognition/memory`、`common/types`、三个 provider `lib.rs` 全部降到 1000 行以下
  - facade 文件只保留 re-export、入口函数和少量胶水逻辑
  - 没有为了搬运参数新增低价值 `*Options` / `*Config` 中转结构
- 契约门禁
  - `talos_types` 的外部导出路径保持兼容
  - `common/llm/provider` 的 trait 与共享流式事件语义不因拆分而改变
  - 不新增 fallback 逻辑，不把 provider 特例伪装成公共抽象
- 验证门禁
  - `cargo test -p talos-memory`
  - `cargo test -p talos-prompt`
  - `cargo test -p talos-types`
  - `cargo test -p talos-llm-provider`
  - `cargo test -p talos-llm-ollama`
  - `cargo test -p talos-llm-anthropic`
  - `cargo test -p talos-llm-bailian`
  - 若共享契约有移动，再补 `tests/integration/run_e2e.sh`
