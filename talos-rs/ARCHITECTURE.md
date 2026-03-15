# Talos-RS 架构设计文档

**版本**: v2.0  
**最后更新**: 2026-03-14

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
│  (agent/agent-workspace/session/session-store/routing/scheduler/            │
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
│  (tools/tools-policy/tools-sandbox/output/skill/audit)                      │
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
│  (llm/prompt/types)                                                         │
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
| **types/** | 通用类型定义 | `infra/*` |

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
| **tools/** | 工具系统 (read/exec/edit/write) | `infra/*` |
| **tools-policy/** | 工具策略 (allow/deny/elevated) | `tools` |
| **tools-sandbox/** | 沙箱执行 (Docker/本地) | `tools` |
| **output/** | 输出通道 | `infra/*` |
| **skill/** | 技能系统 | `tools` |
| **audit/** | 安全审计 (工具调用审计/速率限制) | `tools`, `infra/logging` |

---

### Orchestration 层（编排）

**定位**：Agent 调度、会话管理、任务规划

| 模块 | 职责 | 依赖 |
|------|------|------|
| **agent/** | Agent 核心 (生命周期) | `common/llm`, `cognition/memory` |
| **agent-workspace/** | 工作空间管理 (AGENTS.md/SOUL.md 注入) | `agent`, `infra/storage` |
| **session/** | 会话管理 (SessionKey/dmScope) | `infra/*` |
| **session-store/** | 会话存储 (sessions.json + JSONL) | `session`, `infra/storage` |
| **routing/** | 路由规则 (Bindings) | `session` |
| **scheduler/** | 任务调度 (cron/heartbeat) | `agent` |
| **planner/** | 任务规划/工作流 | `agent`, `cognition/reasoning` |
| **coordinator/** | 多 Agent 协调 | `agent`, `planner` |
| **queue/** | 消息队列 (steer/followup/collect) | `session` |

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
- 对外仅暴露 Session 视角：`/v1/sessions` + `/v1/sessions/{id}/messages`
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

### 场景 1：用户消息 → Agent 响应

```
用户 (WhatsApp)
    ↓
┌─────────────────┐
│  Gateway (ws/)  │ 接收 WebSocket 消息
└────────┬────────┘
         ↓
┌─────────────────┐
│  Gateway (core) │ 验证/认证
└────────┬────────┘
         ↓
┌─────────────────┐
│  Routing        │ 匹配 Binding → SessionKey
└────────┬────────┘
         ↓
┌─────────────────┐
│  Session-Store  │ 加载会话上下文
└────────┬────────┘
         ↓
┌─────────────────┐
│  Queue          │ steer/followup 模式处理
└────────┬────────┘
         ↓
┌─────────────────┐
│  Agent          │ 执行 Agent 逻辑
│  ├─ Memory      │ 获取短期/长期记忆
│  ├─ Knowledge   │ RAG 检索相关知识
│  └─ Reasoning   │ 任务规划
└────────┬────────┘
         ↓
┌─────────────────┐
│  Tools          │ 调用工具 (read/exec/edit/write)
│  └─ Audit       │ 审计日志 + 速率限制
└────────┬────────┘
         ↓
┌─────────────────┐
│  Output         │ 格式化响应
└────────┬────────┘
         ↓
┌─────────────────┐
│  Gateway (ws/)  │ 发送回复
└────────┬────────┘
         ↓
用户 (WhatsApp)
```

### 场景 2：定时任务（Heartbeat）

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
├── orchestration/session/
├── orchestration/session-store/
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
│   │   ├── llm/
│   │   │   ├── provider/
│   │   │   ├── anthropic/
│   │   │   ├── openai/
│   │   │   ├── ollama/
│   │   │   └── router/
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
│   │   ├── tools/
│   │   ├── tools-policy/
│   │   ├── tools-sandbox/
│   │   ├── output/
│   │   ├── skill/
│   │   └── audit/
│   │
│   ├── orchestration/            # 编排层
│   │   ├── agent/
│   │   ├── agent-workspace/
│   │   ├── session/
│   │   ├── session-store/
│   │   ├── routing/
│   │   ├── scheduler/
│   │   ├── planner/
│   │   ├── coordinator/
│   │   └── queue/
│   │
│   ├── gateway/                  # 网关层
│   │   ├── core/
│   │   ├── ws/
│   │   ├── events/
│   │   └── health/
│   │
│   └── meta/                     # 元认知层（横向切面）
│       ├── monitor/
│       ├── learner/
│       ├── personality/
│       └── maintenance/
│
└── talos/                        # 主 crate（整合所有模块）
    └── Cargo.toml
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
| 6 | 实现 `orchestration/session/` | 🟡 中 | ⏳ 待开始 |
| 7 | 实现 `orchestration/agent/` | 🟡 中 | ⏳ 待开始 |
| 8 | 补充所有 crate 的依赖关系 | 🔴 高 | ⏳ 待开始 |

---

*最后更新：2026-03-14*
