# Agent Provider 架构

[English](RUNTIME_ARCHITECTURE.md)

> Historical translation. The [English document](RUNTIME_ARCHITECTURE.md) owns the current
> 2026-09-07 contracts, diagrams, deployment rules and verification guidance.

## 职责链

```text
TalosService
    |
    +-- owns product lifecycle and durable state transitions
    |
    v
Coordinator
    |
    +-- resolves Routed or Exact Agent bindings
    |
    v
AgentRegistry
    |
    +-- returns a registered AgentHarness
    |
    v
AgentHarness
    |
    +-- owns common live lifecycle, admission, cancellation, and event consistency
    |
    v
AgentProvider SPI
    |
    +-- creates one in-memory AgentSession per Talos Session
    |
    +-- KernelAgentProvider [current] -> KernelAgentSession -> Agent Kernel
    +-- CodexAgentProvider [planned]
    +-- ClaudeCodeAgentProvider [planned]
    `-- PiAgentProvider [planned]
    |
    v
Standard AgentEvent / AgentRunResult -> Timeline
```

`TalosService` 是面向产品的 facade。协议适配器调用它，而不是直接组装 Store、Provider
或 Registry。Agent Kernel 子系统（`talos-agent-kernel`）是 `KernelAgentProvider` 使用的
私有执行 engine，不是未来所有 Agent Provider 背后的通用实现。

## 多 Agent 与 Agent Provider SPI

多 Agent 注册与 Provider 实现是两个独立维度：

```text
Agent configuration
    |
    +-- AgentDefinition: default  -> kind=kernel -> KernelAgentProvider
    +-- AgentDefinition: reviewer -> kind=kernel -> KernelAgentProvider
    +-- AgentDefinition: coder    -> kind=codex  -> CodexAgentProvider [planned]
    +-- AgentDefinition: analyst  -> kind=claude -> ClaudeCodeAgentProvider [planned]
    `-- AgentDefinition: pi       -> kind=pi     -> PiAgentProvider [planned]
    |
    v
AgentDefinitionLoader
    |
    v
Composition Root selects a ready AgentProvider from typed config
    |
    v
AgentRegistry stores multiple AgentHarness registrations by Agent id
    |
    v
Router / Coordinator selects one registration for a Session or Turn
```

多个 Agent Definition 可以使用同一个 Provider 实现，同时具有不同 identity、Model、
Workspace 或 typed Provider config。新增实现面向 `AgentProvider + AgentSession` SPI，
并封装在公共 Harness 生命周期之后；Coordinator 不能向下转换或按 Provider kind 分支。

这套扩展边界已经实现并且与具体 Provider 无关。Harness 只保存 SPI；Kernel 专用的
request、event、continuation 和 transcript 均留在 `KernelAgentProvider` 之后。当前
composition 只注册 typed Kernel Provider。Codex、Claude Code 和 Pi 必须等对应 Provider
实现、typed config、capability 和 composition-root 分支完整后才可选择；不存在到
Kernel 的 fallback。

## Kernel-backed Turn 执行

以下是内置 `KernelAgentProvider` 链路。其他 Provider 可以使用不同私有 backend，但必须
返回相同的标准 Agent event、result、capability、cancellation 和 continuation 行为。

```text
Turn request
     |
     v
Validate request and Session state
     |
     v
Persist user input and create the Turn
     |
     v
Resolve Agent binding through Coordinator
     |
     v
Build bounded context from history, memory, Skills, and project inputs
     |
     v
Call the selected model provider
     |
     +-- text/reasoning delta ----------> Timeline
     |
     +-- tool request -> policy -> approval -> sandbox/tool execution
     |                                      |
     |                                      v
     |                              tool result -> Timeline
     |                                      |
     +--------------------------------------+
     |
     v
Persist final response and terminal Turn state
```

Timeline 保持模型输出、工具活动、用户交互、错误和最终响应的顺序。客户端渲染或
投影这一有序模型，而不是从 transport 细节重新推断执行状态。

## Subagent 执行

Subagent 协作与 Agent Provider 注册是两套正交机制。

```text
Parent Session / Turn / Agent binding
                 |
                 v
Collaboration tool validates the parent context
                 |
                 v
Create Child Session and Child Turn
                 |
                 v
Inherit the parent's exact Agent binding
                 |
                 v
Re-enter Coordinator -> same AgentHarness -> same AgentProvider implementation
                 |
                 v
Child Timeline and result projection
```

Subagent 不能静默选择其他 Agent definition，也不能绕过正常 Session/Turn 生命周期。
如果未来引入跨 Agent delegation，必须作为独立且显式的产品能力设计。

## 工具能力链路

```text
Model tool call
      |
      v
Tool registry and deferred discovery
      |
      v
Permission policy and optional user interaction
      |
      +-- denied -------------------------> explicit error event
      |
      v
Execution plan and sandbox selection
      |
      +-- built-in tool
      +-- Skill-backed instruction flow
      `-- MCP server tool
      |
      v
Typed result -> bounded Timeline event -> next model input
```

Policy、Sandbox、MCP 初始化和 schema 错误必须保持可见。没有显式策略决策时，
Provider 不得通过权限更弱的执行路径重试。

## Context 规则

- Context 增量构建，不重写历史 item。
- 每个注入 item 都有来源、边界、裁剪规则和 token 预算。
- 单个 item 不得超过配置的 hard cap。
- Tool result 和文件内容在注入模型前必须有界。
- Compaction 生成显式、持久化的历史，而不是客户端私有摘要。

## 状态规则

- Store Record 保留在 Store 边界内部。
- Service API 暴露领域 Request/Response，不暴露数据库 Record。
- 文件状态按需要使用锁、临时写入和原子替换。
- 多资源更新必须定义顺序、回滚和恢复行为。
- 只有确定执行结果后才能写入 Turn 的终态。
