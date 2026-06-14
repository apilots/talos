# Rust 超大文件拆分方案

## 背景

项目约定要求源码文件单文件尽量不超过 `1000` 行。当前仓库内仍有 `13` 个 Rust 生产文件超过该阈值，且集中分布在核心编排、配置、网关和 provider 实现中。

超限文件清单：

| 行数 | 文件 |
| --- | --- |
| 2511 | `talos-rs/crates/infra/config/src/lib.rs` |
| 2180 | `talos-rs/crates/orchestration/agent-kernel/src/general_turn.rs` |
| 2013 | `talos-rs/crates/orchestration/agent-kernel/src/flow.rs` |
| 1840 | `talos-rs/crates/orchestration/agent-kernel/src/processor.rs` |
| 1832 | `talos-rs/crates/orchestration/agent/src/agent.rs` |
| 1739 | `talos-rs/crates/orchestration/coordinator/src/coordinator.rs` |
| 1569 | `talos-rs/crates/gateway/http/src/server.rs` |
| 1135 | `talos-rs/crates/common/types/src/lib.rs` |
| 1132 | `talos-rs/crates/common/llm/ollama/src/lib.rs` |
| 1087 | `talos-rs/crates/common/llm/anthropic/src/lib.rs` |
| 1084 | `talos-rs/crates/common/llm/bailian/src/lib.rs` |
| 1027 | `talos-rs/crates/common/prompt/src/lib.rs` |
| 1027 | `talos-rs/crates/cognition/memory/src/store.rs` |

按子系统聚合后，问题最集中的区域是：

- `agent-kernel`：3 个超限文件，总计 `6033` 行
- `common/llm`：3 个超限文件，总计 `3303` 行
- `infra/config`：单文件 `2511` 行

这说明当前问题不是“局部文件偶然变大”，而是多个核心子系统正在持续向单文件集中。

---

## 目标

本方案的目标不是机械地把文件切碎，而是建立一套可评审的拆分策略：

- 让模块边界与职责边界一致
- 让高频变更点和高风险路径更容易理解与测试
- 保持对外 API 稳定，不为了测试或拆分扩大暴露面
- 统一命名方式，避免出现 `misc.rs`、`helpers.rs` 这类无语义兜底文件

非目标：

- 不在一次重构中顺带修改行为语义
- 不做“每个 struct 单独一个文件”的过度拆分
- 不为追求绝对最小文件数而把 facade 层删掉

---

## 拆分原则

### 1. 按稳定职责拆，不按文本段落拆

合理拆分应该沿着“职责稳定、依赖方向单向”的边界展开，例如：

- 请求构造 vs 响应解析
- session 管理 vs event 转发
- prompt 模板定义 vs prompt 组装执行
- provider 协议模型 vs provider 运行时实现

不应按以下方式拆：

- 前 500 行放 `part1.rs`，后 500 行放 `part2.rs`
- 把多个毫无语义关系的函数塞进 `utils.rs`
- 仅因为行数超限而抽取大量薄封装函数

### 2. 允许保留 facade，但 facade 只做聚合

对于 crate 入口或核心模块入口，允许保留 `lib.rs` 或 `mod.rs` 作为 facade，但其职责应仅限于：

- `mod` 声明
- `pub use` 再导出
- 少量顶层类型别名或极薄的构造入口

不应继续把核心实现堆回 facade 文件。

### 3. 先拆高变更密度和高耦合区域

优先拆分的不是“最容易拆的文件”，而是：

- 被多个功能反复修改的文件
- 业务和协议、状态和流程混在一起的文件
- 容易引发回归、难以评审的文件

对于这类高耦合文件，执行顺序必须更保守：

- 先搬领域类型和纯函数
- 再搬单向依赖的实现模块
- 最后处理 orchestration 入口和测试归属

不能一开始就同时搬类型、主流程和测试，否则可见性与导出边界会一起失稳。

### 4. 类型定义集中，不等于所有类型都应继续塞在一个文件

`types` 模块可以保留集中式边界，但内部仍应按领域拆开，例如：

- `ids.rs`
- `session.rs`
- `events.rs`
- `model.rs`

“都是类型”不是合理的单文件理由。

### 5. 测试跟随模块职责，而不是跟随历史文件

拆分生产代码时，测试应随着职责重新归位：

- 请求构造逻辑的测试跟 `request.rs`
- 响应解析逻辑的测试跟 `response.rs`
- provider streaming 测试跟 `streaming.rs`

避免出现“生产代码已拆，测试仍全部堆在单入口”的半拆分状态。

### 6. 每移动一个模块后立即验证，不攒批量错误

每次拆分一个明确子模块后，必须立即执行最小相关验证，而不是把多个搬迁动作积累到阶段末再统一跑。

推荐顺序：

- 移动一个类型模块后，先执行 `cargo test -p <crate>`
- 移动一个流程实现模块后，立即执行 `cargo test -p <crate> <target_filter>`
- 完成一个子系统阶段后，再执行相关 integration / e2e

这样做是为了防止 import / re-export 断裂、循环依赖和行为回归在阶段末集中爆发，导致问题难以定位。

### 7. 物理拆分不等于完成，必须顺带做重复逻辑清理

对于结构高度相似的子系统，拆分不能只完成“文件搬家”，还必须检查是否存在可收敛的重复代码。

尤其是：

- 多 provider 的请求映射
- 多 provider 的 stream chunk 累积
- 多 provider 的 usage / error 转换
- 多 handler / processor 的重复样板

但清理重复逻辑的前提是“共性已经稳定”。如果抽象会掩盖 provider 差异，则应保留显式实现。

---

## 总体优先级

### P0：核心编排链路，优先处理

- `crates/orchestration/agent-kernel/src/general_turn.rs`
- `crates/orchestration/agent-kernel/src/flow.rs`
- `crates/orchestration/agent-kernel/src/processor.rs`
- `crates/orchestration/agent/src/agent.rs`
- `crates/orchestration/coordinator/src/coordinator.rs`

理由：

- 这些文件处于任务执行主链路
- 编排、状态、事件、协议和测试容易堆叠在一起
- 每次功能迭代都很容易再次失控

### P1：系统基础设施，其次处理

- `crates/infra/config/src/lib.rs`
- `crates/gateway/http/src/server.rs`
- `crates/cognition/memory/src/store.rs`
- `crates/common/prompt/src/lib.rs`

理由：

- 文件大且跨职责明显
- 拆分收益大，但需要注意对外接口兼容和配置路径稳定

### P2：通用模型与 provider，统一模式处理

- `crates/common/types/src/lib.rs`
- `crates/common/llm/{ollama,anthropic,bailian}/src/lib.rs`

理由：

- 结构相似，适合形成一套统一 provider 模板
- 但对功能行为影响较小，优先级可略低于主链路

---

## 总体拆分流程

```text
盘点超限文件
    ↓
识别稳定职责边界
    ↓
设计目标模块树与 facade 边界
    ↓
先搬类型与纯函数
    ↓
再搬流程实现与状态修改逻辑
    ↓
调整测试归属
    ↓
逐模块执行 cargo test
    ↓
逐 crate 执行 integration / e2e 回归
```

执行顺序建议：

```text
agent-kernel
    ↓
agent / coordinator
    ↓
config / gateway-http
    ↓
prompt / memory
    ↓
common-types / llm providers
```

---

## 子系统拆分方案

## 1. `infra/config`

### 现状判断

`config/src/lib.rs` 里混合了：

- 根配置对象 `Config`
- 路径推导
- YAML 加载与环境变量覆盖
- 配置校验
- 多个子系统配置类型
- 一部分 display/default 实现

这类文件最容易演化成“配置总仓库”，但配置本身天然有领域边界，不应全部堆在根文件。

### 建议结构

```text
crates/infra/config/src/
├── lib.rs
├── loader.rs
├── paths.rs
├── validate.rs
├── gateway.rs
├── storage.rs
├── agent.rs
├── llm.rs
├── tools.rs
└── channels.rs
```

### 命名与职责

| 模块 | 职责 | 典型内容 |
| --- | --- | --- |
| `lib.rs` | facade + 根 `Config` 聚合 | `Config`、`pub use`、`load()` 入口 |
| `loader.rs` | 配置文件读取与环境变量展开 | `load_from_path`、插值、merge |
| `paths.rs` | `TalosPaths` 与目录规则 | home、conf、data、workspace |
| `validate.rs` | 运行时校验 | `validate_runtime()`、边界检查 |
| `gateway.rs` | gateway/http/ws 相关配置 | host、port、auth、token |
| `storage.rs` | sqlite / backend 配置 | connection string、backend mode |
| `agent.rs` | agent/kernel/planner 配置 | `AgentKernelConfig`、planner mode |
| `llm.rs` | provider/model/cost 相关配置 | llm defaults、compat |
| `tools.rs` | tools runtime / sandbox 配置 | approval、sandbox、exec、file |
| `channels.rs` | telegram 等 channel 配置 | channel 开关和凭证 |

### 关键决策

- `Config` 保留在 `lib.rs`，因为它是 crate 入口类型
- 具体子配置定义迁入领域模块，`lib.rs` 用 `pub use` 再导出
- `logging` 配置默认不单独拆文件，除非后续实际扩展到足以形成独立职责；当前优先合并在 `lib.rs` 或相邻主配置模块内，避免人为造碎片
- `Default` 和 `Display` 实现跟随类型所在模块，不再集中在根文件底部
- `validate_runtime()` 可保留总入口，但内部应分发到各子模块校验函数

---

## 2. `agent-kernel/general_turn`

### 现状判断

`general_turn.rs` 同时包含：

- general turn 相关状态结构
- step request 构造
- tool batch 执行
- stream event 发射
- memory summary 逻辑
- 多个测试用 mock provider

这是典型的“流程主干 + 细节执行器 + 测试工具”混放。

基于现有代码位置的快速核对：

- `GeneralTurnStepRequest` / `GeneralTurnToolBatch` 类型定义在文件前部
- `build_general_turn_step_request()` 位于约 `1454`
- `execute_general_turn_step()` 位于约 `1469`
- `execute_general_turn_tool_batch()` 位于约 `1680`
- `finalize_general_turn_artifacts()` / `emit_general_turn_finalization_stream()` / `prepare_general_turn()` 位于后半段

因此方案中的 `step_request / step_execute / tool_batch / finalize / prepare` 边界与当前代码分布基本一致，不是脱离现状的凭空拆分。

### 建议结构

```text
crates/orchestration/agent-kernel/src/general_turn/
├── mod.rs
├── types.rs
├── prepare.rs
├── step_request.rs
├── step_execute.rs
├── tool_batch.rs
├── finalize.rs
├── memory_summary.rs
└── tests.rs
```

### 模块接口

| 模块 | 职责 | 建议暴露接口 |
| --- | --- | --- |
| `mod.rs` | facade | `pub use types::*;` 和顶层 orchestration 函数 |
| `types.rs` | loop state / result 类型 | `GeneralTurnLoopState`、`GeneralTurnStepResult` |
| `prepare.rs` | 首轮准备与上下文初始化 | `prepare_general_turn()` |
| `step_request.rs` | 构建一步 LLM 请求 | `build_general_turn_step_request()` |
| `step_execute.rs` | 执行一步与解析结果 | `execute_general_turn_step()` |
| `tool_batch.rs` | tool call batch 执行与确认 | `execute_general_turn_tool_batch()` |
| `finalize.rs` | artifact 落盘与 stream finalization | `persist_*`、`finalize_*` |
| `memory_summary.rs` | durable memory summary 提取 | `summarize_general_turn_memory()` |

### 关键决策

- `types.rs` 只放 general turn 私有领域状态，不抽成更泛化的公共模块
- `memory_summary.rs` 独立是合理的，因为它依赖 prompt 和 memory，但不应污染主循环
- mock provider 不应继续留在生产文件主体里，统一进入 `tests.rs`
- 该阶段是全仓风险最高的拆分阶段，必须严格遵守“先类型、后实现、再测试”的顺序，尤其注意 `general_turn.rs`、`flow.rs`、`processor.rs` 之间共享类型和 `pub(crate)` 可见性边界

---

## 3. `agent-kernel/flow`

### 现状判断

`flow.rs` 当前至少混合了：

- flow 抽象接口
- `SingleFlow`
- `AutoFlow`
- `FlowFactory`
- callback / test doubles

这类文件适合按 flow kind 和工厂分拆。

### 建议结构

```text
crates/orchestration/agent-kernel/src/flow/
├── mod.rs
├── types.rs
├── single.rs
├── auto.rs
├── factory.rs
├── callbacks.rs
└── tests.rs
```

### 模块接口

| 模块 | 职责 |
| --- | --- |
| `types.rs` | `FlowRunSummary`、`FlowExecution`、trait 定义 |
| `single.rs` | 单一路径 flow |
| `auto.rs` | 自动选择 flow |
| `factory.rs` | `FlowFactory` 和装配逻辑 |
| `callbacks.rs` | `BeforeModelCallback` 等 callback 实现 |

### 关键决策

- `FlowFactory` 与具体 flow 实现分离，否则工厂很快再次膨胀
- callback 机制单独成文件，避免埋在 flow 逻辑中间
- `tests.rs` 内保留 mock provider 和专用测试状态，不进入生产模块

---

## 4. `agent-kernel/processor`

### 现状判断

`processor.rs` 的自然边界非常清晰：它实际上是请求处理器和响应处理器注册表。

### 建议结构

```text
crates/orchestration/agent-kernel/src/processor/
├── mod.rs
├── request.rs
├── response.rs
├── request_kinds.rs
├── response_kinds.rs
├── identity.rs
├── compaction.rs
├── contents.rs
├── planning.rs
└── tests.rs
```

### 模块接口

| 模块 | 职责 |
| --- | --- |
| `request.rs` | `RequestProcessor` trait、output 类型、基础 request pipeline |
| `response.rs` | `ResponseProcessor` trait、output 类型、response pipeline |
| `request_kinds.rs` | `RequestProcessorKind` 及注册顺序 |
| `response_kinds.rs` | `ResponseProcessorKind` 及注册顺序 |
| `identity.rs` | identity / auth / instructions 类 request processor |
| `compaction.rs` | compaction / context cache / interactions |
| `contents.rs` | contents / tool context / output schema |
| `planning.rs` | planning 请求与响应处理器 |

### 关键决策

- 不按“每个 processor 一个文件”拆，这样会产生过多薄文件
- 按处理阶段和语义分组拆，既保留聚合感，又把主文件打散
- request 和 response pipeline 必须分离，这是天然边界
- `planning.rs` 只允许依赖 `processor` 自身的 trait/output 类型、共享 `types`、以及 prompt 相关公共类型；不允许反向依赖 `flow` 或 `general_turn` 的具体实现，避免形成 `processor -> flow/general_turn -> processor` 循环

---

## 5. `orchestration/agent`

### 现状判断

`agent.rs` 里同时有：

- `AgentImpl`
- turn 执行与 active handle 管理
- orchestrator event 转换
- mock agent / mock llm / test runner

它实际上把运行时实现、协议适配和测试全压在一起。

### 建议结构

```text
crates/orchestration/agent/src/
├── lib.rs
├── agent/
│   ├── mod.rs
│   ├── runtime.rs
│   ├── turn.rs
│   ├── event_bridge.rs
│   ├── approval.rs
│   ├── flush.rs
│   └── tests.rs
└── flush_manager.rs
```

### 模块接口

| 模块 | 职责 |
| --- | --- |
| `runtime.rs` | `AgentImpl`、生命周期管理、active turn 跟踪 |
| `turn.rs` | run request 到 kernel 调度 |
| `event_bridge.rs` | kernel event -> orchestrator event 映射 |
| `approval.rs` | confirmation / approval 请求与响应 |
| `flush.rs` | memory flush 相关 orchestration |

### 关键决策

- `flush_manager.rs` 已独立，新的 `flush.rs` 只放 agent 内部 flush 协调逻辑
- `MockAgent` 和 `MockLlmProvider` 必须收敛到 `tests.rs`
- `event_bridge.rs` 单独存在非常重要，因为它属于协议映射层，不应和 turn 执行纠缠

---

## 6. `orchestration/coordinator`

### 现状判断

`coordinator.rs` 至少混合了：

- session title 生成
- session manager
- `CoordinatorImpl`
- `RequestHandler` / `SessionService` 实现
- 多个 mock 类型和测试

这已经不是单一“coordinator”文件，而是多个角色堆叠。

### 建议结构

```text
crates/orchestration/coordinator/src/
├── lib.rs
├── coordinator/
│   ├── mod.rs
│   ├── title.rs
│   ├── session_manager.rs
│   ├── request_handler.rs
│   ├── session_service.rs
│   ├── event_stream.rs
│   └── tests.rs
```

### 模块接口

| 模块 | 职责 |
| --- | --- |
| `title.rs` | `SessionTitleGenerator` |
| `session_manager.rs` | session 选择、创建、缓存、flush 触发 |
| `request_handler.rs` | `RequestHandler for CoordinatorImpl` |
| `session_service.rs` | `SessionService for CoordinatorImpl` |
| `event_stream.rs` | orchestrator event -> gateway event 转发，默认仅作为 `pub(crate)` 内部桥接模块，不对其他 crate 暴露直接调用入口 |

### 关键决策

- `CoordinatorImpl` 可继续作为中心 struct，但具体 trait impl 拆到不同文件
- `SessionManager` 独立比继续做 coordinator 私有内嵌结构更清晰
- request path 和 session service path 是不同协议边界，应显式分开
- `event_stream.rs` 只能作为 coordinator crate 内部的协议桥接层存在；如果它既依赖 orchestrator 类型又面向 gateway 事件输出，必须确保依赖方向仍然是 `coordinator -> agent/gateway types`，不能因为拆文件制造新的跨 crate 循环

---

## 7. `gateway/http/server`

### 现状判断

`server.rs` 混合了：

- HTTP 状态与生命周期
- middleware / auth
- 路由注册
- create session / patch / messages / SSE
- tool approval / dynamic tool result
- mock gateway / tests

HTTP server 文件天然适合按 route domain 切开。

### 建议结构

```text
crates/gateway/http/src/
├── lib.rs
├── server/
│   ├── mod.rs
│   ├── state.rs
│   ├── errors.rs
│   ├── auth.rs
│   ├── router.rs
│   ├── sessions.rs
│   ├── messages.rs
│   ├── approvals.rs
│   ├── lifecycle.rs
│   └── tests.rs
```

### 模块接口

| 模块 | 职责 |
| --- | --- |
| `state.rs` | `HttpApiState`、共享依赖注入 |
| `errors.rs` | `ApiError`、`ApiErrorBody`、响应映射 |
| `auth.rs` | auth middleware、scope 检查 |
| `router.rs` | 路由装配，不承载 handler 细节 |
| `sessions.rs` | create/get/list/patch session handlers |
| `messages.rs` | sync message / SSE message handlers |
| `approvals.rs` | tool approval / dynamic tool result handlers |
| `lifecycle.rs` | `HttpServer`、shutdown、signal handling |

### 关键决策

- 路由注册和 handler 实现必须分离
- `messages.rs` 单独存在，因为 SSE、事件流和普通 session CRUD 的复杂度不同
- middleware / auth 不应和 handler 混放

---

## 8. `common/types`

### 现状判断

`types/src/lib.rs` 的问题不是“逻辑太复杂”，而是“领域类型太多”。这种文件不应保持一把梭，但也不适合过度拆散。

### 建议结构

```text
crates/common/types/src/
├── lib.rs
├── model.rs
├── ids.rs
├── gateway.rs
├── session.rs
├── runtime.rs
└── talos.rs
```

### 模块接口

| 模块 | 职责 |
| --- | --- |
| `model.rs` | `ModelInfo`、model cost / compat 再导出 |
| `ids.rs` | `AgentId`、`SessionId`、`UserId` 等 |
| `gateway.rs` | `GatewayRequest`、`GatewayResponse` |
| `runtime.rs` | `TalosConnectionMode`、environment |
| `session.rs` | `TalosContext`、session 相关输入输出 |
| `talos.rs` | 事件、消息、output item 等主协议类型 |

### 关键决策

- `lib.rs` 保持为对外聚合入口
- 领域类型按协议面拆，不按“全是 struct”拆
- 现有外部调用侧应尽量不感知模块变化，通过 `pub use` 保持稳定

---

## 9. `common/llm` 三个 provider

### 现状判断

`ollama`、`anthropic`、`bailian` 三个 provider 的结构高度相似：

- config
- provider-specific request/response schema
- request mapping
- streaming decode / accumulate
- `LlmProvider` trait impl

这类文件最适合按统一模板拆，三家保持同构。

### 建议统一结构

```text
crates/common/llm/<provider>/src/
├── lib.rs
├── config.rs
├── schema.rs
├── mapping.rs
├── streaming.rs
├── provider.rs
└── tests.rs
```

### 模块接口

| 模块 | 职责 |
| --- | --- |
| `config.rs` | `<Provider>Config` |
| `schema.rs` | provider API request/response DTO |
| `mapping.rs` | `ChatRequest`/`ToolDefinition` 到 provider DTO 的转换 |
| `streaming.rs` | stream chunk 解析、累积器、tool stream 组装 |
| `provider.rs` | `<Provider>Provider` 与 `impl LlmProvider` |

### 关键决策

- 三个 provider 必须统一目录结构和命名，否则后续维护继续分裂
- `schema.rs` 只放 DTO 和 serde 结构，不放业务判断
- `provider.rs` 不应再承载海量私有 DTO
- 三个 provider 必须在同一批次内按同一模板完成拆分，不能拆完一家就暂停，否则中途结构不一致会让 review 和后续迁移基线失真
- provider 拆分必须顺带审视重复逻辑，优先识别可稳定复用的共性，例如 stream accumulator、tool-call chunk 拼装、usage 映射、错误转换；但禁止为了“看起来统一”而强行抽象 provider 特有行为

---

## 10. `common/prompt`

### 现状判断

`prompt/src/lib.rs` 已经有 `assembler`、`budget`、`diagnostics` 等模块，但根文件仍过大，说明 facade 之外的核心类型和 builder 还没有被真正拆出去。

当前实际行数：

- `lib.rs`: `1027`
- `assembler.rs`: `364`
- `types.rs`: `294`
- `budget.rs`: `94`
- `bootstrap.rs`: `67`
- `renderer.rs`: `43`
- `diagnostics.rs`: `36`

这说明当前真正超限的只有 `lib.rs`，现有子模块规模还比较健康。因此 prompt 的目标不是继续把已有子模块打散，而是把 `lib.rs` 中剩余的核心职责明确搬迁出去，同时避免新旧命名重叠。

### 建议结构

```text
crates/common/prompt/src/
├── lib.rs
├── template.rs
├── context.rs
├── builder.rs
├── assembler.rs
├── bootstrap.rs
├── budget.rs
├── diagnostics.rs
├── renderer.rs
└── types.rs
```

### 模块接口

| 模块 | 职责 |
| --- | --- |
| `template.rs` | `PromptTemplate` |
| `context.rs` | `PromptContext` |
| `builder.rs` | `PromptBuilder` |

### 关键决策

- 不引入 `manager.rs`，避免与 `builder.rs`、`assembler.rs` 形成职责重叠；若当前 `PromptManager` 仍然存在，应优先评估其是否可以并入 `builder.rs` 或 `template.rs`
- `PromptBuilder` 应独立，因为它是该 crate 最核心的行为入口
- `types.rs` 只放共享枚举和配置类型
- `lib.rs` 最终只负责统一再导出，不再包含 builder 细节实现
- 执行前必须先以当前行数为依据确认：只有当某个现有子模块继续逼近阈值时，才允许继续二次拆分；不要为了目录对称性去机械增加模块

---

## 11. `cognition/memory/store`

### 现状判断

`store.rs` 同时承载了：

- transcript record 类型
- summary record
- query / search types
- prompt context types
- store 本身实现

这里最大的信号是“类型和存储实现混在一起”。

### 建议结构

```text
crates/cognition/memory/src/
├── lib.rs
├── store.rs
├── transcript.rs
├── summary.rs
├── prompt_context.rs
├── query.rs
└── path_policy.rs
```

### 模块接口

| 模块 | 职责 |
| --- | --- |
| `transcript.rs` | `TalosTurn`、`SessionTranscript*` 全套类型 |
| `summary.rs` | `SummaryRecord`、summary 相关结构 |
| `prompt_context.rs` | `SessionPromptContext`、`SessionPromptContextConfig` |
| `query.rs` | `MemorySearchQuery`、search response |
| `store.rs` | `MemoryStore` 及读写实现 |
| `path_policy.rs` | workspace / session memory 路径约束与落盘路径判定；若后续职责始终只服务于 `store.rs`，可降为 `store.rs` 的 `pub(crate)` 子模块而不对外独立暴露 |

### 关键决策

- `store.rs` 保留为实现入口，不再继续承担所有 record type 定义
- transcript 是 memory 子系统里最稳定也最独立的领域，优先抽离

---

## 命名规范

为避免这轮拆分再次失控，约束如下：

- 使用领域名，不使用 `misc.rs`、`common.rs`、`other.rs`
- 对“协议映射层”统一使用 `mapping.rs`
- 对“provider DTO”统一使用 `schema.rs`
- 对“运行时执行器”统一使用 `runtime.rs` 或 `provider.rs`
- 对“trait facade”统一使用 `mod.rs` / `lib.rs`
- 测试统一使用：
  - `tests/mod.rs`：模块聚合测试入口
  - `tests/<domain>_tests.rs`：单一职责测试组
  - `test_support.rs` 或 `tests/support.rs`：公共 helper，仅在确有跨模块复用时使用

---

## 执行计划

## 阶段 1：`agent-kernel`

目标：

- 先稳定核心执行链路模块边界
- 拆出 `general_turn/`、`flow/`、`processor/`

验收标准：

- 每搬迁一个子模块后立即执行 `cargo test -p talos-agent-kernel`
- 三个文件都降到 `1000` 行以下
- `agent-kernel` 单测通过
- planner / built-in / plan-react 相关集成回归通过

## 阶段 2：`agent` + `coordinator`

目标：

- 把 orchestration 外层胶水层拆清楚
- 把 event bridge 与 session service 显式分离

验收标准：

- 每搬迁一个子模块后立即执行对应 crate 单测
- `agent.rs`、`coordinator.rs` 均降到阈值内
- session/title/approval/flush 回归通过

## 阶段 3：`config` + `gateway/http`

目标：

- 建立基础设施模块边界
- 防止路由、middleware、handler 继续堆叠

验收标准：

- 每搬迁一个子模块后立即执行对应 crate 单测
- `config` 和 `http server` 入口文件成为 facade
- 配置加载、HTTP API 回归通过

## 阶段 4：`prompt` + `memory` + `types` + `providers`

目标：

- 完成通用库的结构统一
- 形成 provider 同构模板

验收标准：

- 每搬迁一个子模块后立即执行对应 crate 单测
- 三个 provider 目录结构一致
- `prompt` / `memory` / `types` 入口文件均降到阈值内

---

## 风险与约束

### 风险 1：为了拆分引入过度抽象

控制方式：

- 只沿既有职责边界拆
- 不新增“搬运参数”的中间 options struct
- 不把显式调用链重写成过度抽象框架

### 风险 2：对外 API 被拆分带崩

控制方式：

- 保留 facade
- 使用 `pub use` 维持已有导出路径
- 优先内部迁移，再评估是否要清理导出面

### 风险 3：测试跟不上拆分

控制方式：

- 每个阶段拆分时同步调整测试归属
- 至少执行相关 crate 单测和 `talos-e2e/run_e2e.sh`

---

## 结论

当前仓库中的大文件问题已经不是零星整理，而是需要按子系统分批治理。合理的拆分方向不是“把所有大文件都打散”，而是：

- 编排链路按执行阶段拆
- 基础设施按协议边界拆
- provider 按统一模板拆
- facade 保持稳定，对外 API 不因内部拆分扩散

按本方案推进后，既能满足单文件行数约束，也能显著改善核心子系统的可读性、可测试性和后续演进空间。
