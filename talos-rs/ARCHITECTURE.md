# Talos-RS 架构设计文档

**版本**: v2.1
**最后更新**: 2026-07-29

> 说明：本文档描述 Talos 的目标架构与设计意图，不作为实时 crate 清单。
> 当前实际挂载模块以 `Cargo.toml` workspace members 与 `MODULES.md` 为准。

---

## 项目背景

**目标**：用 Rust 编写一个 AI 智能管家底层框架，类似贾维斯（Jarvis）的软件版大脑。

**参考**：OpenClaw 的设计（Gateway/Session/Agent/Tools/Channels 等概念）

**命名来源**：塔罗斯（Talos）- 希腊神话中的青铜自动巨人，守护克里特岛

---

## 整体架构

```
┌─────────────────────────────────────────────────────────────────────────────┐
│  用户层 (User Surfaces) - WhatsApp/Telegram/Discord/CLI                      │
└─────────────────────────────────────────────────────────────────────────────┘
                                    ⬇
┌─────────────────────────────────────────────────────────────────────────────┐
│  Gateway 层 - 统一入口/出口                                                   │
│  (core/ws/events/health)                                                    │
└─────────────────────────────────────────────────────────────────────────────┘
                                    ⬇
┌─────────────────────────────────────────────────────────────────────────────┐
│  Orchestration 层 - 编排                                                      │
│  (agent/agent-workspace/session/store/routing/scheduler/                    │
│   planner/coordinator/queue)                                                │
└─────────────────────────────────────────────────────────────────────────────┘
                                    ⬇
┌─────────────────────────────────────────────────────────────────────────────┐
│  Cognition 层 - 认知                                                          │
│  (memory/memory-store/knowledge/reasoning/decision/prompt)                  │
└─────────────────────────────────────────────────────────────────────────────┘
                                    ⬇
┌─────────────────────────────────────────────────────────────────────────────┐
│  Action 层 - 执行                                                             │
│  (tools/tool-search/mcp/plugin/skills/tools-policy/tools-sandbox/audit)     │
└─────────────────────────────────────────────────────────────────────────────┘
                                    ⬇
┌─────────────────────────────────────────────────────────────────────────────┐
│  Perception 层 - 感知                                                         │
│  (input/channel/listener/transport)                                         │
└─────────────────────────────────────────────────────────────────────────────┘
                                    ⬇
┌─────────────────────────────────────────────────────────────────────────────┐
│  Infra 层 - 基础设施（纯技术）                                                 │
│  (runtime/config/error/logging/storage/auth)                                │
└─────────────────────────────────────────────────────────────────────────────┘

┌─────────────────────────────────────────────────────────────────────────────┐
│  Common 层 - 通用库（跨层使用，有业务语义）                                      │
│  (llm/prompt/types/fs)                                                      │
└─────────────────────────────────────────────────────────────────────────────┘
    ⬆️ ⬆️ ⬆️ ⬆️ ⬆️ ⬆️ ⬆️ ⬆️ ⬆️ ⬆️ ⬆️ ⬆️ ⬆️ ⬆️ ⬆️ ⬆️ ⬆️ ⬆️ ⬆️ ⬆️ ⬆️ ⬆️ ⬆️ ⬆️ ⬆️ ⬆️

┌─────────────────────────────────────────────────────────────────────────────┐
│  Meta 切面 - 横向贯穿所有层                                                    │
│  (monitor/learner/personality/maintenance)                                  │
└─────────────────────────────────────────────────────────────────────────────┘
```

---

## 分层职责

### Common 层（新增）

**定位**：跨层通用的、有业务语义的模块

| 模块 | 职责 | 依赖 |
|------|------|------|
| **llm/** | LLM 抽象层 | `infra/*` |
| └─ `provider/` | `LlmProvider` trait + 消息类型 | - |
| └─ `anthropic/` | Anthropic API 实现 | `provider` |
| └─ `openai/` | OpenAI API 实现 | `provider` |
| └─ `ollama/` | Ollama API 实现 | `provider` |
| └─ `router/` | 多提供者路由/故障转移 | `provider` |
| **prompt/** | Prompt 模板引擎 | `infra/*` |
| └─ `template/` | 模板渲染 | - |
| └─ `context/` | 上下文组装 | - |
| └─ `presets/` | 预设模板 | - |
| **types/** | 通用强类型对象，包含 session、tool、plugin、MCP 等跨端共享类型 | `infra/*` |
| **fs/** | 通用文件系统封装，提供目录操作、原子写入、文件锁等安全文件操作 | `infra/error` |

**为什么独立**：
- LLM 有业务语义（ChatMessage/Role/Prompt），不是纯技术设施
- 多层需要调用 LLM（Cognition/Action/Gateway）
- 避免污染 `Infra` 层的纯粹性

---

### Infra 层（基础设施）

**定位**：纯技术基础设施，无业务语义

| 模块 | 职责 | 依赖 |
|------|------|------|
| **runtime/** | 异步运行时封装 (tokio) | - |
| **config/** | 配置管理 (openclaw.json 风格) | `error`, `logging` |
| **error/** | 统一错误处理 | - |
| **logging/** | tracing 集成 | `error` |
| **storage/** | 存储抽象 (KV/SQL/Vector) | `error`, `config` |
| └─ `kv/` | KV 存储 trait | - |
| └─ `sql/` | SQL 存储 trait | - |
| └─ `vector/` | 向量存储 trait | - |
| └─ `backup/` | 备份/恢复 | `storage` |
| **auth/** | 认证/配对 (device pairing) | `error`, `storage` |

---

### Perception 层（感知）

**定位**：外部输入处理

| 模块 | 职责 | 依赖 |
|------|------|------|
| **input/** | 多模态输入处理 | `infra/*`, `common/types` |
| **channel/** | Channel trait (WhatsApp/Telegram 等) | `infra/*`, `common/types` |
| **listener/** | 事件监听 (日历/邮件/webhook) | `infra/*` |
| **transport/** | 传输适配 (HTTP/WS/CLI) | `infra/*` |

---

### Cognition 层（认知）

**定位**：记忆、知识、推理、决策

| 模块 | 职责 | 依赖 |
|------|------|------|
| **memory/** | 记忆系统 (短期/长期/程序性) | `infra/*`, `common/llm` |
| **memory-store/** | 记忆持久化 | `memory`, `infra/storage` |
| **knowledge/** | 知识图谱/RAG | `memory`, `common/llm` |
| **reasoning/** | 推理引擎 (任务规划) | `memory`, `common/llm` |
| **decision/** | 决策系统 (优先级/风险评估) | `reasoning` |
| **prompt/** | Prompt 管理（归入 common） | - |

---

### Action 层（执行）

**定位**：工具调用、输出

| 模块 | 职责 | 依赖 |
|------|------|------|
| **tools/** | 工具系统、内置 tools、tool router、`tool_search` 按需发现 | `infra/*`, `common/types` |
| **mcp/** | MCP client runtime，基于 `rmcp` 发现并执行外部 MCP tools | `common/types` |
| **plugin/** | Plugin lifecycle，统一管理 Skills 与 MCP server plugins 的 source、scan、install、enable、disable、update、uninstall | `common/types`, `common/fs` |
| **skills/** | Skill manifest/document 解析和 prompt 注入领域模型 | `common/types` |
| **tools-policy/** | 工具策略 (allow/deny/elevated) | `tools` |
| **tools-sandbox/** | 沙箱执行 (Docker/本地) | `tools` |
| **output/** | 输出通道 | `infra/*` |
| **audit/** | 安全审计 (工具调用审计/速率限制) | `tools`, `infra/logging` |

**工具可见性原则**：
- 系统内置工具默认进入可见上下文。
- Plugin 提供的外部能力不默认污染上下文，必须通过 `tool_search` 按需发现和激活。
- MCP server 本身由 Plugin 管理生命周期，MCP server 暴露的具体 tools 由 `talos-mcp` 投影到 `talos-tools` 后再被模型调用。
- Skills 不注册为 executable tools；Skills 通过 `PromptBuilder` 注入任务指导。

---

### Orchestration 层（编排）

**定位**：Agent Runtime 注册与分发、Session/Turn 管理、Subagent 协作和任务规划

| 模块 | 职责 | 依赖 |
|------|------|------|
| **agent/** | `AgentDefinition`、`AgentHarness`、Runtime SPI 和公共生命周期 | `common/llm`, `cognition/memory` |
| **agent-workspace/** | 工作空间管理 (AGENTS.md/SOUL.md 注入) | `agent`, `infra/storage` |
| **session/** | Session/Turn 生命周期和 Subagent 父子执行拓扑 | `infra/*` |
| **store/** | `talos-store`，project/session DB 持久化 | `infra/storage` |
| **routing/** | 路由规则 (Bindings) | `session` |
| **scheduler/** | 任务调度 (cron/heartbeat) | `agent` |
| **planner/** | 任务规划/工作流 | `agent`, `cognition/reasoning` |
| **coordinator/** | Agent Registry、Agent 选择、binding 解析和 Harness 分发 | `agent`, `routing` |
| **queue/** | 消息队列 (steer/followup/collect) | `session` |

#### Agent Runtime 与 Subagent 协作边界

Talos 将“可注册的多 Agent Runtime”和“父子 Session/Turn 协作”建模为两套正交机制。前者决定由哪个 Agent 实现执行 Turn，后者描述同一 Agent binding 下的递归协作拓扑。

**Agent 启动与注册**：

```text
AgentDefinition
    -> AgentHarnessFactory
    -> AgentHarness(AgentRuntime)
    -> Coordinator.register_agent()
    -> AgentRegistry
```

`AgentDefinition` 是从产品配置解析出的构造领域对象。Factory 根据 definition 创建包含具体 `AgentRuntime` 的 `AgentHarness`，再由 Coordinator 注册到 `AgentRegistry`。Registry 只保存可路由的 descriptor 和 Harness，不保存 definition，也不负责实例化 Runtime。

**Turn 执行分发**：

```text
Turn dispatch
    -> Coordinator
    -> AgentRegistry
    -> AgentHarness
    -> AgentRuntime
```

Coordinator 根据 `DispatchBinding` 选择已注册 Agent。`Routed` binding 经过路由策略选择 Agent；`Exact` binding 直接解析指定 `agent_id`。Coordinator 只调用稳定的 Harness API，不依赖或向下转型具体 Runtime。

**Subagent 协作**：

```text
Parent Turn
    -> collaboration tool
    -> SubagentService
    -> Child Session/Turn
    -> inherited DispatchBinding::Exact { agent_id }
    -> Coordinator
    -> AgentRegistry
    -> same registered AgentHarness instance
    -> its AgentRuntime
```

`spawn_agent` 等 collaboration tools 是模型可调用的协作控制入口。属于 `talos-service` 的 `SubagentService` 负责父子关系、生命周期控制和结果投影；Child Session/Turn 继承调用者已经确定的 `agent_id`，并通过标准 Turn dispatch 重新解析到同一个已注册 `AgentHarness` 实例及其 `AgentRuntime`。

这两套机制必须保持以下约束：

1. `AgentDefinition` 描述可注册的 Agent Runtime，不表示 Subagent 节点。
2. Subagent 是 Session/Turn 执行拓扑，不创建新的 Agent Runtime 抽象。
3. `SubagentService` 不拥有 Agent Registry，也不直接调用具体 Runtime。
4. Coordinator 不管理 Subagent 树，只负责 Agent 选择、binding 解析和执行分发。
5. Child Turn 必须继承父级的确定 Agent binding，并复用完整核心执行链。
6. 跨 `AgentDefinition` 委派是独立的未来能力，不属于当前 `spawn_agent` 的继承语义。
7. Gateway 继续保持 Session-first；内部 Agent Runtime 和 Subagent 树不改变对外 Session/Turn 契约。

---

### Gateway 层（网关）

**定位**：统一入口/出口

| 模块 | 职责 | 依赖 |
|------|------|------|
| **core/** | Gateway 核心 trait | `infra/*`, `orchestration/routing` |
| **ws/** | WebSocket 服务器 | `core`, `infra/*` |
| **events/** | 事件系统 (agent/chat/presence) | `core` |
| **health/** | 健康检查 | `core`, `meta/monitor` |

**v1 对外 API 契约（Session-first）**：
- 对外仅暴露 Session/Turn 视角：`/v1/sessions` + `/v1/turns/start`
- 不暴露 `agent` / `run` 概念
- 不提供 `/v1/messages*` 与 `/v1/responses*` 双轨入口
- 请求/响应采用统一 `input[]/output[]` item 结构，支持 SSE 事件流

---

### Meta 切面（横向）

**定位**：贯穿所有层的横向关注点

| 模块 | 职责 | 实现方式 |
|------|------|----------|
| **monitor/** | 系统监控 (性能/健康) | `Monitorable` trait + 指标收集 |
| **learner/** | 持续学习 (反馈/优化) | 事件订阅 + 策略优化 |
| **personality/** | 人格管理 (IDENTITY.md/SOUL.md) | 中间件模式 + 响应调整 |
| **maintenance/** | 维护任务 (session prune/rotate) | 定时任务 + 存储维护 |

**横向切面实现**：
```rust
// 1. Trait 实现
pub trait Monitorable {
    fn metrics(&self) -> Metrics;
    fn health(&self) -> HealthStatus;
}

// 2. 中间件模式
pub struct PersonalityMiddleware<S> {
    inner: S,
    personality: Personality,
}

// 3. 事件订阅
pub struct Learner {
    events: EventStream,
}
```

---

## 核心数据流

### 场景 1：用户输入 → Agent 响应

```
用户输入
    ↓
┌─────────────────┐
│ Gateway / App   │ 接收 HTTP、JSON-RPC 或 Channel 输入
└────────┬────────┘
         ↓
┌─────────────────┐
│  TalosService   │ 解析 Session/Turn 请求
└────────┬────────┘
         ↓
┌─────────────────┐
│ Session / Turn  │ 持久化输入并确定 DispatchBinding
└────────┬────────┘
         ↓
┌─────────────────┐
│  Coordinator    │ 解析 Routed 或 Exact binding
└────────┬────────┘
         ↓
┌─────────────────┐
│ AgentRegistry   │ 返回已注册的 AgentHarness
└────────┬────────┘
         ↓
┌─────────────────┐
│ AgentHarness    │ 执行公共生命周期和事件归一化
└────────┬────────┘
         ↓
┌─────────────────┐
│ AgentRuntime    │ 执行 Agent 逻辑
│  ├─ Memory      │ 获取短期/长期记忆
│  ├─ Knowledge   │ RAG 检索相关知识
│  ├─ Prompt      │ 注入 AGENTS、Skills 和动态上下文
│  └─ Reasoning   │ 任务规划
└────────┬────────┘
         ↓
┌─────────────────┐
│  Tools          │ 调用内置工具或经 tool_search 激活的外部工具
│  ├─ MCP         │ 执行 MCP server 暴露的 tool
│  └─ Audit       │ 审计日志 + 速率限制
└────────┬────────┘
         ↓
┌─────────────────┐
│ Session/Timeline│ 持久化输出和执行事件
└────────┬────────┘
         ↓
┌─────────────────┐
│ Gateway / App   │ 投影并发送响应
└────────┬────────┘
         ↓
用户
```

### 场景 2：父 Turn 创建 Subagent

```text
Parent AgentRuntime
    ↓ calls collaboration tool
SubagentService
    ↓ validates caller and inherited agent_id
Child Session
    ↓ creates Child Turn with Exact binding
Coordinator
    ↓ resolves the same registered Agent
AgentRegistry
    ↓ returns the registered Harness
AgentHarness
    ↓
AgentRuntime
    ↓
Child Timeline / result projection
```

Subagent 控制操作可以启动、恢复、中断或关闭 Child Turn，但不能绕过 Session/Turn 生命周期，也不能把 Child Turn 静默切换到其他 `AgentDefinition`。

### 场景 3：定时任务（Heartbeat）

```
┌─────────────────┐
│  Scheduler      │ Cron 触发
└────────┬────────┘
         ↓
┌─────────────────┐
│  Agent          │ 执行任务
│  └─ LLM         │ 调用模型 (common/llm)
└────────┬────────┘
         ↓
┌─────────────────┐
│  Output         │ 推送结果
└────────┬────────┘
         ↓
┌─────────────────┐
│  Monitor        │ 记录执行指标 (meta/monitor)
└─────────────────┘
```

---

## 模块依赖规则

```
┌───────────────────────────────────────────────────────────────────────────┐
│  依赖方向（从上到下，允许跨层依赖下层）                                      │
├───────────────────────────────────────────────────────────────────────────┤
│                                                                           │
│  Gateway → Orchestration → Cognition → Action → Perception → Infra       │
│     ↘         ↘            ↘          ↘          ↘           ↘          │
│      └─────────┴────────────┴──────────┴──────────┴───────────┴── → Common │
│                                                                           │
│  Meta (横向切面，可访问所有层)                                              │
│                                                                           │
└───────────────────────────────────────────────────────────────────────────┘
```

**规则**：
1. **上层可依赖下层** - Gateway 可依赖所有下层
2. **同层模块不交叉依赖** - 通过上层协调
3. **所有层可依赖 Common** - LLM/Prompt 是通用能力
4. **所有层可依赖 Infra** - 基础设施是基础
5. **Meta 可访问所有层** - 通过 trait/事件/中间件
6. **重要：所有层和模块** - 严格遵守高内聚低耦合设计原则，每个层级或模块对外封装标准化能力 API，调用方不感知模块实现内部细节

---

## 关键设计原则

| 原则 | 说明 | 示例 |
|------|------|------|
| **Crate 独立** | 每个子 crate 可独立编译、测试、发布 | `talos-llm-provider` 可单独发布 |
| **依赖单向** | 下层不依赖上层，同层不交叉依赖 | `Infra` 不知道 `Agent` 存在 |
| **Trait 驱动** | 关键抽象用 Trait 定义 | `Channel`, `Agent`, `Tool`, `LlmProvider` |
| **可插拔** | 渠道/工具/技能可动态注册 | 新增 WhatsApp 渠道只需实现 `Channel` trait |
| **配置分离** | 配置与代码分离，支持热更新 | `openclaw.json` 独立于代码 |
| **可观测性** | 内置 tracing 指标收集 | 所有模块自动输出结构化日志 |
| **横向切面** | Meta 层贯穿所有层 | `Monitor` 统一收集指标 |

---

## 开发顺序（Phased）

### Phase 1 - 基础设施 + Common（Week 1-3）

```
优先级 1：基础中的基础
├── infra/error/       ✅ 已完成
├── infra/logging/     ⏳ 待实现
├── infra/config/      ⏳ 待实现
└── common/types/      ⏳ 待实现

优先级 2：LLM 抽象
├── common/llm/provider/   ⏳ 待实现
├── common/llm/anthropic/  ⏳ 待实现
└── common/prompt/         ⏳ 待实现

优先级 3：运行时 + 存储
├── infra/runtime/     ⏳ 待实现
├── infra/storage/     ⏳ 待实现
└── infra/auth/        ⏳ 待实现
```

### Phase 2 - 认知核心（Week 4-6）

```
├── cognition/memory/
├── cognition/memory-store/
├── cognition/knowledge/
└── cognition/reasoning/
```

### Phase 3 - Agent + 会话（Week 7-9）

```
├── orchestration/store/
├── orchestration/service/
├── orchestration/agent/
└── orchestration/agent-workspace/
```

### Phase 4 - 网关 + 渠道（Week 10-12）

```
├── gateway/core/
├── gateway/ws/
├── perception/channel/
└── perception/transport/
```

### Phase 5 - 执行 + 编排（Week 13-15）

```
├── action/tools/
├── action/tools-policy/
├── orchestration/routing/
└── orchestration/scheduler/
```

### Phase 6 - Meta 切面 + 整合（Week 16-18）

```
├── meta/monitor/
├── meta/learner/
├── meta/personality/
├── meta/maintenance/
└── talos/ (主 crate 整合)
```

---

## 目录结构

```
talos-rs/
├── Cargo.toml                    # Workspace 根配置
├── ARCHITECTURE.md               # 本文档
├── README.md                     # 项目说明
│
├── crates/
│   ├── common/                   # 通用库（跨层使用）
│   │   ├── fs/
│   │   ├── llm/
│   │   │   ├── provider/
│   │   │   ├── anthropic/
│   │   │   ├── bailian/
│   │   │   └── openai/
│   │   ├── prompt/
│   │   └── types/
│   │
│   ├── infra/                    # 基础设施层
│   │   ├── runtime/
│   │   ├── config/
│   │   ├── error/
│   │   ├── logging/
│   │   ├── storage/
│   │   └── auth/
│   │
│   ├── perception/               # 感知层
│   │   ├── input/
│   │   ├── channel/
│   │   ├── listener/
│   │   └── transport/
│   │
│   ├── cognition/                # 认知层
│   │   ├── memory/
│   │   ├── memory-store/
│   │   ├── knowledge/
│   │   ├── reasoning/
│   │   └── decision/
│   │
│   ├── action/                   # 执行层
│   │   ├── mcp/
│   │   ├── plugin/
│   │   ├── skills/
│   │   ├── tools/
│   │   ├── tools-policy/
│   │   └── tools-sandbox/
│   │
│   ├── orchestration/            # 编排层
│   │   ├── agent/
│   │   ├── agent-kernel/
│   │   ├── store/
│   │   ├── service/
│   │   ├── router/
│   │   └── coordinator/
│   │
│   ├── gateway/                  # 网关层
│   │   ├── app-protocol/
│   │   ├── app-server/
│   │   ├── core/
│   │   └── http/
│
├── apps/
│   ├── app-server-test-client/
│   ├── cli/
│   ├── sdk/
│   └── ui-tui/
│
└── bin/
    ├── app-server/
    └── talos-server/
```

---

## Native Tool Calling（2026-03 现代化）

### 设计目标

- 统一 `FunctionCall` / `FunctionResponse` / `ToolDefinition`，避免 Agent 侧手写 JSON 协议解析。
- `LlmProvider` 直接支持 `chat_with_tools` 与 `stream_with_tools`，三家 Provider（OpenAI / Anthropic / Ollama）都走原生工具调用。
- `ToolRegistry` 可导出工具 schema，并通过 `ToolContext` 向工具执行传递会话上下文与状态。

### 统一抽象

- `ChatRequest` 扩展字段：`tools`、`tool_choice`、`parallel_tool_calls`。工具循环上限由 agent-kernel 控制。
- `ChatResponse` 扩展字段：`tool_calls`，通过 `has_tool_calls()` / `get_tool_calls()` 读取。
- `ToolStreamEvent` 统一流式事件：`TextDelta`、`ToolCallStart`、`ToolCallDelta`、`ToolCallEnd`、`Done`。

### Provider 行为

- **OpenAI**：原生 `tools/tool_calls`，`arguments` 字符串与 `serde_json::Value` 双向转换。
- **Anthropic**：`tool_use` / `tool_result` block 映射到统一类型；流式解析 `content_block_*` 事件。
- **Ollama**：普通聊天走 `/api/chat`；当 request 包含 tools 时走 OpenAI 兼容接口 `/v1/chat/completions`。

### Agent 执行循环

1. 发送 `chat_with_tools(request)`。  
2. 若返回 `tool_calls`，逐个执行工具并生成 `Role::Tool` 消息。  
3. 将工具结果回注到 messages，继续调用模型直到无 `tool_calls` 或达到轮数上限。  
4. 流式场景通过 `stream_with_tools` 透传工具调用生命周期事件。

---

## 下一步行动

| # | 行动项 | 优先级 | 状态 |
|---|--------|--------|------|
| 1 | 实现 `infra/logging/` | 🔴 高 | ⏳ 待开始 |
| 2 | 实现 `infra/config/` | 🔴 高 | ⏳ 待开始 |
| 3 | 创建 `common/llm/provider/` | 🔴 高 | ⏳ 待开始 |
| 4 | 创建 `common/types/` | 🟡 中 | ⏳ 待开始 |
| 5 | 实现 `cognition/memory/` | 🟡 中 | ⏳ 待开始 |
| 6 | 实现 `orchestration/store/` | 🟡 中 | ⏳ 待开始 |
| 7 | 实现 `orchestration/agent/` | 🟡 中 | ⏳ 待开始 |
| 8 | 补充所有 crate 的依赖关系 | 🔴 高 | ⏳ 待开始 |

---

*最后更新：2026-03-14*
