# Talos 系统架构

[English](ARCHITECTURE.md)

> Historical translation. The [English document](ARCHITECTURE.md) owns the current
> 2026-09-07 contracts, diagrams, deployment rules and verification guidance.

## 文档目标

本文定义 Talos 的稳定系统边界，描述当前架构以及未来工作应使用的扩展点。
本文不是 crate 清单，也不是实现路线图。

## 整体架构

这张 ownership map 展开根 README 中同一套长期分层，不表示执行时序。`[current]`
表示已有实现边界，`[planned]` 表示明确的演进目标。

```text
+--------------------------------------------------------------------------------+
| Products and integrations [current]                                            |
| Web | Desktop | TUI | Command mode | External applications | Channels          |
+-----------------------------------------+--------------------------------------+
                                          |
                                          v
+--------------------------------------------------------------------------------+
| Client and embedded SDK capability surface                                     |
| TypeScript: Web -> Browser TalosClient | Desktop -> AppServerClient [current]  |
| Rust: TUI/command -> EmbeddedServerClient | AppServerClient [current]          |
| Language-neutral domain contract [current] | Cross-SDK parity [planned]        |
+-----------------------------------------+--------------------------------------+
                                          |
                                          v
+--------------------------------------------------------------------------------+
| Gateway and application lifecycle [current]                                    |
| Transport: Browser HTTP/SSE | Desktop JSON-RPC stdio | TUI Embedded Gateway    |
| Channels -> adapters | auth | validation | protocol and event projection       |
| TalosService: Session | Turn | Project | Timeline | Plugin | user interaction  |
+-----------------------------------------+--------------------------------------+
                                          |
                                          v
+--------------------------------------------------------------------------------+
| Agent orchestration and provider integration                                   |
| Router / Coordinator / AgentRegistry / multiple Harnesses [current]            |
| Selection -> AgentHarness -> AgentProvider -> AgentSession [current]           |
|   +-- KernelAgentProvider [current] -> Agent Kernel reasoning/tool loop        |
|   +-- CodexAgentProvider [planned]                                             |
|   +-- ClaudeCodeAgentProvider [planned]                                        |
|   `-- PiAgentProvider [planned]                                                |
| AgentEvent / AgentRunResult / capabilities / lifecycle control [current]       |
+--------------------------+--------------------------+--------------------------+
| Semantic capability domains; these columns are not an execution sequence       |
+--------------------------+--------------------------+--------------------------+
| Perception               | Cognition                | Action                   |
| Channel I/O [current]    | Memory/Prompt [current]  | Tools/Skills [current]   |
| Media [current]          | Context/LLM [current]    | MCP/Policy [current]     |
| Environment [planned]    | Strategy [planned]       | Approval [current]       |
|                          |                          | Sandbox [current]        |
+--------------------------+--------------------------+--------------------------+
| Infrastructure [current]                                                       |
| Store | Storage | Filesystem | Config | Auth | Automation | Logging            |
+--------------------------------------------------------------------------------+

+--------------------------------------------------------------------------------+
| Metacognition [planned cross-cutting evolution]                                |
| Evaluation | Self-monitoring | Learning | Identity and personality             |
| Observes defined layers and changes state only through their typed contracts.  |
+--------------------------------------------------------------------------------+
```

图中的技术概念使用英文，以保证代码、协议和中英文文档之间可以直接对应。
能力域各行描述的是语义 owner，不是第二条请求时序，也不与 crate 一一对应。Channel
integration 通过对应 Transport 和 Gateway adapter 进入系统；Perception 负责事件/媒体
标准化和输出适配。选中的 Agent Provider 使用 Cognition 和 Action 能力，并负责自身的
reasoning/tool loop。

## 边界职责

| 边界 | 负责 | 禁止承载 |
| --- | --- | --- |
| Products and integrations | 第一方交互以及上层应用组装 | Agent 执行业务规则 |
| Client SDK | 语言无关产品语义、完整能力 API、共享领域对象和 transport 选择 | 服务端 Record 或 UI 状态 |
| Gateway | Transport、认证、校验和协议投影 | Agent 执行策略 |
| Application Service | 产品生命周期和领域编排 | 协议 envelope 或渲染逻辑 |
| Coordinator 与 Harness | Agent binding、分发和执行态生命周期 | 客户端特有行为 |
| Agent Definition | 稳定 Agent identity、Provider kind、Model 和 typed Provider config | Provider 私有 handle |
| Agent Provider SPI | Provider Session factory/lifecycle，以及强类型 Session Turn/管理契约 | Routing 或持久化产品生命周期 |
| Agent Kernel | 内置 Kernel Agent 的 reasoning/tool loop 子系统 | 其他 Provider 实现或持久化产品生命周期 |
| Perception | 外部事件和媒体标准化，以及 Channel 输入/输出适配 | Session 生命周期或 Agent 决策 |
| Cognition | Memory、有界 Context、Model 和 Prompt 能力 | 产品生命周期或副作用策略 |
| Action | Tool、Skill、MCP、权限、审批和 Sandbox 能力 | Agent routing 或协议投影 |
| Infrastructure | 技术持久化、配置和可观测性 | 产品策略 |
| Metacognition（规划） | 通过显式契约实现 Evaluation、自我监控、学习、Identity 和 Personality | 隐式修改状态，或绕过生命周期和策略 |

## 稳定领域链路

所有客户端进入同一套产品生命周期：

```text
Client request
     |
     v
Gateway validation and projection
     |
     v
Application service creates or resumes Session and Turn
     |
     v
Coordinator resolves the Session's Agent binding
     |
     v
Agent harness obtains an AgentSession from the selected AgentProvider
     |
     v
AgentSession executes Turns and emits standardized events and results
     |
     v
SDK projects the same domain result through its selected transport
```

HTTP、JSON-RPC 和 embedded client 可以有不同 envelope，但不能分别定义不同的
Session、Turn、Timeline、Tool 或 Agent 语义。

## 核心领域术语

| 术语 | 含义 |
| --- | --- |
| `Session` | 包含多个 Turn 的持久化会话与 Agent binding 边界 |
| `Turn` | Session 内一次持久化执行 |
| `Timeline` | 以 Session 寻址的有序事件流，事件标识其所属 Turn |
| `AgentDefinition` | 选择一个 Provider kind 及其 typed config 的已配置 Agent identity |
| `AgentHarness` | 以一个 Agent id 注册的公共生命周期 facade |
| `AgentProvider` | 创建并管理 Provider-owned 内存态 AgentSession 的进程级 SPI |
| `AgentSession` | 执行 Turn 和 Session 管理的 Provider-owned 内存态 handle，不是持久化产品状态 |
| Agent Kernel | 内置 Kernel Agent 的单 Turn 有界 reasoning/tool loop 子系统（`talos-agent-kernel`） |

## 依赖方向

```text
Product binaries and user surfaces
                 |
                 v
SDK product API
                 |
                 v
Transport and Gateway adapters
                 |
                 v
Application service and orchestration
                 |
                 v
Agent Provider and focused capabilities
                 |
                 v
Infrastructure and shared types
```

依赖应指向稳定抽象。下层不依赖上层产品组装；数据库 Record 留在 Store 边界；
协议 envelope 留在 Gateway 边界；渲染状态留在客户端。

Rust 是当前权威服务端实现。语言无关的产品契约与生成/共享领域类型用于支持能力
对齐的多语言 SDK，而不要求为每种语言复制一套服务端编排和领域模型。

## 跨层不变量

- `Session` 和 `Turn` 是 Agent 工作的持久化生命周期边界。
- `Timeline` 是用户可见运行进度的有序事实来源。
- Agent binding 由 Coordinator 解析，不能由工具或客户端内部选择。
- 可以注册多个 Agent Definition；每个已启用且受支持的 Definition 解析为一个 Harness 和已配置 AgentProvider。
- AgentProvider 实现通过 AgentSession 契约输出相同的 Agent event 和 result。
- 当前 composition 注册 KernelAgentProvider；新增 Provider 必须提供 typed config、capability test 和显式 composition-root 分支。
- Subagent 创建 Child Session/Turn，并重新进入正常分发链。
- Perception、Cognition 和 Action 保持为 owner 明确、职责聚焦的能力域。
- Metacognition 可以观察和评估系统，但不能绕过 Service、Agent Provider、策略或状态边界。
- 核心领域对象保持强类型，并在协议投影之间共享。
- Context、tool result、文件读取和 list API 都必须有明确大小上限。
- 状态写入必须考虑并发、部分失败和崩溃恢复。
- 工具权限与沙箱决策必须显式失败，禁止用 fallback 弱化策略。

## 演进方式

未来能力应扩展现有边界：

| 变化 | 扩展边界 |
| --- | --- |
| 新模型 Provider | LLM provider implementation，以及配置、组装和 Provider 测试 |
| 新 Agent 实现 | 增加 typed config、AgentProvider/AgentSession 实现、composition-root 分支和 capability test |
| 新工具或外部服务 | Tool registry、Skill 或 MCP capability |
| 新执行隔离方式 | Sandbox SPI 和平台实现 |
| 新客户端或协议 | 先扩展 SDK；协议差异放在 SDK transport 或 Gateway adapter 中 |
| 新语言 SDK | 复用语言无关的产品契约，并证明能力对齐 |
| 新持久化后端 | Store 或 Storage 接口后的实现 |
| 新 Channel | 进入同一 Application Service 的 Perception/channel adapter |
| 新 Metacognition 能力 | 增加显式 observer/evaluator 边界；状态变更重新进入现有强类型 Service API |

只有现有 owner 无法保持内聚时才新增 crate。不要为假设性需求增加平行 manager、
registry、event bus 或协议特有领域模型。

## 详细文档

- [Agent Provider 架构](RUNTIME_ARCHITECTURE_zh.md)
- [客户端架构](CLIENT_ARCHITECTURE_zh.md)
- [错误模型](ERROR_MODEL_zh.md)
- [Rust workspace 模块](../reference/RUST_MODULES_zh.md)
