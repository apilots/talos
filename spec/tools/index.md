# Talos Tools Full Alignment Spec Index

## 1. 目标

本目录用于定义 Talos 工具体系的完整重构方案，目标不是“补几个缺失工具”，而是把 **Talos 的工具平台能力、工具契约、工具执行体验、模型可见工具协议、系统工具实现深度** 全面拉齐到 `claude-code`。

本轮目标明确为：

1. **模型可见工具能力与 Claude Code 1:1 对齐**
2. **Talos 当前 mock/最小实现工具按目标能力直接重写**
3. **允许内部实现名、模块结构、Rust 类型命名与 Claude Code 不同**
4. **外部工具契约、输入输出、行为语义、使用体验与 Claude Code 对齐**

---

## 2. 总体决策

### 2.1 单一公开名原则

Talos 后续采用 **单一公开名 + 内部执行标识 + 展示标签** 的模型，而不是多套对等工具名并存。

| 层 | 含义 | 示例 |
|---|---|---|
| Canonical Public Name | 暴露给模型的唯一工具名，必须与 Claude Code 对齐 | `Read` / `Edit` / `Bash` / `Grep` |
| Internal Executor Id | Talos 内部 registry/executor/rust module 标识，可独立设计 | `read_file` / `grep_files` / `bash_exec` |
| Display Label | UI/TUI 展示标签或摘要，不参与工具协议匹配 | `Grep(pattern=Agent)` |

**强约束**：

- 模型默认只看到一套 canonical public name
- `alias` 仅允许作为迁移期兼容能力，不作为长期公开协议的一部分
- 展示标签不是第二套工具名，不参与模型调用协议
- Talos 当前 snake_case 名称不再直接暴露给模型

### 2.2 Claude Code 的实际做法

Claude Code 不是“完全没有多个名字”，而是：

- 每个工具有一个主 `name`
- 少量工具有 `aliases`
- 查找时通过 `toolMatchesName()` 同时匹配主名与 alias
- UI 通过 `userFacingName()` 与 `getToolUseSummary()` 单独控制展示

也就是说，Claude Code 的真实原则是：

1. **主名唯一**
2. **alias 仅兼容**
3. **展示文本与协议名解耦**

Talos 应对齐这个原则，而不是把 alias 当作常态命名策略。

### 2.3 工具平台先于工具罗列

本次对齐优先级：

1. 工具平台契约
2. 关键系统工具重写
3. 特性工具和 host 绑定工具补齐

原因很简单：如果不先把平台层补齐，后续每个工具都会重复实现输入校验、权限判断、并发判定、结果预算、状态回写、展示摘要，最终不可维护。

### 2.4 不保留生产态 mock

Talos 当前以下工具属于可直接重写对象：

- `web_search`
- `web_fetch`

这些能力在现状中是 mock 或近似 mock，实现重写不会引入严重兼容成本，反而能降低后续系统复杂度。

### 2.5 框架优先，不允许工具层重复实现治理逻辑

本轮对齐的基本方法不是“把 Claude Code 每个工具都抄一遍”，而是：

1. 先补平台层统一抽象
2. 再让具体工具接入这些抽象
3. 新增工具默认复用平台能力，而不是自己再实现一套

这意味着以下能力必须优先沉到框架层：

- input validation
- permission matching
- read-only / destructive / concurrency-safe 判定
- interrupt / cancel 语义
- result budget / persisted output / preview
- read-before-write / stale-write 状态
- transcript / UI / model-facing result mapping

任何后续工具如果仍然在工具实现内部重复发明上述机制，都视为偏离目标架构。

### 2.6 不做长期兼容性方案，只保留必要的迁移顺序

Talos 当前阶段优先最佳方案，不做“为了兼容旧工具形态而长期保留双轨结构”的设计。

这里要区分两件事：

- **允许分阶段 rollout**
  是为了控制实施顺序和风险。
- **不允许长期兼容层**
  是为了避免新平台还没建好，旧语义就被继续固化。

因此本轮原则是：

- 不因为历史工具名或历史参数形态，长期保留第二套协议
- 不因为局部工具还没重写完，就先在工具层复制一套治理逻辑
- 不因为短期切换成本，就放弃平台统一抽象
- 能直接重写的 mock / 最小实现，优先直接替换

### 2.7 方案细化必须先做源码对照探索

后续每次继续完善 `spec/tools` 下任一文档前，都必须先做一轮源码基线探索，至少同时覆盖：

- `claude-code` 中该能力域对应的工具源码
- Talos 当前 `talos-rs/crates/action/tools` 的相关实现

目的不是“读过就算”，而是为了明确三件事：

1. Claude Code 已经把哪些能力沉到工具契约或平台层
2. Talos 当前哪些能力已经存在于骨架中，哪些只是最小实现
3. 本轮文档补充的是平台抽象、工具特例，还是 mock 重写项

统一工作流：

1. 先探索源码现状
2. 提取当前差距与可复用骨架
3. 再补 spec

每一轮开始实现前，也必须重复执行一次同样的流程：

1. 先阅读 `claude-code` 对应能力域的最新源码
2. 再阅读 Talos 当前对应实现
3. 对照差距后再开始编码

不允许出现“spec 编写时看过一次源码，真正实现时直接凭记忆动手”的情况。

禁止的写法：

- 不看当前 Talos 实现，直接写抽象方案
- 不看 Claude Code 对应源码，只按记忆补能力
- 发现 Talos 已有骨架后，仍然在 spec 里写成推翻重来

---

## 3. 当前差距摘要

基于对 `talos-rs/crates/action/tools` 与 `~/data/ailabs/claude-code` 的对比，当前差距主要在八个方面：

1. **工具契约过薄**：Talos 当前 `Tool` trait 只有 `name/description/schema/execute`
2. **执行链路缺少上下文回写**：没有 `read-before-write`、`stale write`、`context mutation`
3. **文件工具能力偏浅**：`grep_files` 只返回文件路径，`edit` 默认全量替换所有匹配
4. **Shell 工具治理不够**：缺少 read-only 判定、命令语义分析、危险命令策略、大输出落盘
5. **网络工具未生产化**：`web_*` 需要从 mock 直接升级为真实实现，`http_request` 则从模型工具集中移除
6. **MCP 平台只完成骨架**：缺少连接生命周期、二进制资源处理、tool discovery
7. **系统级工具缺失**：`Glob`、`LSP`、`ToolSearch`、`NotebookEdit`、`Todo/Plan/AskUserQuestion` 等仍未完整对齐
8. **工具结果治理不足**：目前主要是截断，缺少 persisted output、聚合预算、可恢复结果引用

---

## 4. 当前框架层问题

在“不能破坏当前骨架”的前提下，Talos 现在最需要补的是框架层问题，而不是继续堆工具名。

### 4.1 命名问题

- 当前 Talos 倾向于把 internal executor name 直接暴露给模型
- 如果后续再叠加展示名、兼容名、别名，容易形成多套名字并存
- 多名字并存会直接带来 prompt、测试、tool search、UI 摘要、日志、权限规则的混乱

### 4.2 治理边界过窄

Talos 当前“治理”主要集中在：

- sandbox mode
- approval policy
- output truncate

这还不够。Claude Code 平台层治理已经覆盖：

- `validateInput`
- `checkPermissions`
- `preparePermissionMatcher`
- `isReadOnly`
- `isConcurrencySafe`
- `isDestructive`
- `interruptBehavior`
- `shouldDefer`
- per-tool result policy
- per-message aggregate result budget
- `readFileState` 驱动的写前校验

Talos 如果不把这些治理点补到平台层，后续实现再多工具也会变成局部拼装。

### 4.3 注册层与执行层耦合过深

- 当前 spec/registry 更偏“注册可执行对象”
- 缺少“注册治理语义”的一等抽象
- 导致很多关键能力只能散落在工具实现内部

### 4.4 上下文状态不够

- 没有统一 `read-before-write` 状态
- 没有统一 tool decision / permission state
- 没有统一 result budget state
- 没有统一 interrupt / cancel 语义

### 4.5 结果治理模型过薄

- 截断只能解决“太长”
- 不能解决“多工具同时太长”
- 不能解决“长结果如何引用而不污染上下文”
- 不能解决“UI 与模型看到的内容如何保持一致”

### 4.6 缺少“框架吸收复杂度”的明确约束

Talos 目前容易出现一种风险：

- 平台层抽象不够
- 某个工具需要新能力
- 于是把能力直接塞进这个工具内部
- 后续下一个工具再复制一次

这样做短期快，长期一定失控。

本轮 spec 要明确反过来约束：

- 工具共性优先回收到平台层
- 平台能力一旦形成，后续工具必须复用
- 允许工具特例，但必须建立在统一抽象之上，而不是绕过抽象

---

## 5. 能力分组

本目录按以下主题拆分：

- [01-tool-platform-architecture-alignment.md](/Users/xujiang/data/ailabs/talos/spec/tools/01-tool-platform-architecture-alignment.md:1)
  工具平台层、契约层、执行链路、结果治理、上下文模型、与 kernel/prompt 的集成。

- [02-filesystem-and-editing-tools-alignment.md](/Users/xujiang/data/ailabs/talos/spec/tools/02-filesystem-and-editing-tools-alignment.md:1)
  `Read` / `Edit` / `Write` / `Glob` / `Grep` / `NotebookEdit`。

- [03-shell-and-process-tools-alignment.md](/Users/xujiang/data/ailabs/talos/spec/tools/03-shell-and-process-tools-alignment.md:1)
  `Bash` / `PowerShell` / `TaskOutput` / `TaskStop` 及相关进程控制能力。

- [04-network-and-web-tools-alignment.md](/Users/xujiang/data/ailabs/talos/spec/tools/04-network-and-web-tools-alignment.md:1)
  `WebFetch` / `WebSearch` / 外部 HTTP 能力。

- [05-mcp-and-external-integration-tools-alignment.md](/Users/xujiang/data/ailabs/talos/spec/tools/05-mcp-and-external-integration-tools-alignment.md:1)
  `ListMcpResources` / `ReadMcpResource` / `ToolSearch` / `Skill` / deferred tools。

- [06-agent-planning-and-state-tools-alignment.md](/Users/xujiang/data/ailabs/talos/spec/tools/06-agent-planning-and-state-tools-alignment.md:1)
  `Agent` / `TodoWrite` / `EnterPlanMode` / `ExitPlanMode` / `AskUserQuestion` 等状态型工具。

- [07-feature-gated-and-environment-tools-alignment.md](/Users/xujiang/data/ailabs/talos/spec/tools/07-feature-gated-and-environment-tools-alignment.md:1)
  `LSP`、worktree、browser、cron、monitor、remote trigger、workflow 等特性工具。

- [08-migration-compatibility-and-rollout.md](/Users/xujiang/data/ailabs/talos/spec/tools/08-migration-compatibility-and-rollout.md:1)
  rename/alias、兼容窗口、分阶段迁移、测试矩阵、风险收敛。

- [09-open-decisions.md](/Users/xujiang/data/ailabs/talos/spec/tools/09-open-decisions.md:1)
  聚合当前已经确认的关键决策，作为后续实现边界。

- [10-phase-a-adr.md](/Users/xujiang/data/ailabs/talos/spec/tools/10-phase-a-adr.md:1)
  Phase A 编码前必须确认的平台层 ADR，钉死 trait 拆分、`read_file_state` 作用域和 mutation barrier。

- [11-tool-surface-selection-and-contract-hardening.md](/Users/xujiang/data/ailabs/talos/spec/tools/11-tool-surface-selection-and-contract-hardening.md:1)
  当前公开工具面收口、请求级工具选择、工具契约与结果治理补齐的专项方案。

- [12-tool-contract-status-reconciliation-and-spec-next-steps.md](/Users/xujiang/data/ailabs/talos/spec/tools/12-tool-contract-status-reconciliation-and-spec-next-steps.md:1)
  基于当前源码现状校正“已落地 / 部分落地 / 目标态”的边界，并定义后续 `P0 / P1 / P2` 设计收口路径。

---

## 6. 目标工具清单

### 5.1 核心公开工具

| Claude Code Tool | Talos 对齐目标 | 当前状态 |
|---|---|---|
| `Read` | 完全对齐 | 部分具备，需重写 |
| `Edit` | 完全对齐 | 部分具备，需重写 |
| `Write` | 完全对齐 | 部分具备，需重写 |
| `Glob` | 完全对齐 | 缺失 |
| `Grep` | 完全对齐 | 部分具备，需重写 |
| `Bash` | 完全对齐 | 有 `exec_command` 骨架，需升级 |
| `NotebookEdit` | 完全对齐 | 缺失 |
| `WebFetch` | 完全对齐 | mock，直接重写 |
| `WebSearch` | 完全对齐 | mock，直接重写 |
| `ListMcpResourcesTool` | 完全对齐 | 有骨架，需增强 |
| `ReadMcpResourceTool` | 完全对齐 | 有骨架，需增强 |
| `ToolSearch` | 完全对齐 | 缺失 |
| `Agent` | 完全对齐 | 仅有薄版 collaboration，需要重做 |
| `TodoWrite` | 完全对齐 | 缺失 |
| `EnterPlanMode` / `ExitPlanMode` | 完全对齐 | 缺失 |
| `AskUserQuestion` | 完全对齐 | 缺失 |
| `TaskStop` | 完全对齐 | 缺失 |
| `TaskOutput` | 完全对齐 | 缺失 |
| `Skill` | 完全对齐 | 缺失 |

### 5.2 特性/环境工具

| Claude Code Tool | Talos 对齐目标 | 备注 |
|---|---|---|
| `LSP` | 条件对齐 | 依赖语言服务器 host 能力 |
| `EnterWorktree` / `ExitWorktree` | 条件对齐 | 依赖 git/worktree host |
| `TaskCreate` / `TaskGet` / `TaskUpdate` / `TaskList` | 条件对齐 | 与 Todo/Plan 协同设计 |
| `PowerShell` | 暂不支持 | 当前明确不纳入实现范围 |
| `WebBrowserTool` | 条件对齐 | 与浏览器自动化能力绑定 |
| `ScheduleCron*` | 条件对齐 | 需要触发器与持久化 |
| `RemoteTriggerTool` | 条件对齐 | 需要远端执行环境 |
| `MonitorTool` | 条件对齐 | 需要后台任务/订阅能力 |
| `WorkflowTool` | 条件对齐 | 需要 workflow runtime |
| `SendMessageTool` / `Team*Tool` | 条件对齐 | 取决于多 agent 拓扑设计 |

### 5.3 内部/实验/特定环境工具

这些工具也纳入 inventory。当前已确认：

- 除 `PowerShell` 外，其余 feature-gated tools 纳入完整交付范围
- `PowerShell` 当前不考虑支持
- 未具备 runtime 的工具不得以 mock 顶替

其中以下工具仍属于环境依赖较重的子集：

- `ConfigTool`
- `BriefTool`
- `SnipTool`
- `CtxInspectTool`
- `TerminalCaptureTool`
- `SleepTool`
- `REPLTool`
- `OverflowTestTool`
- 其他 ant-only / feature-gated 工具

对这些工具的原则是：

- 先在 spec 中完成分类与依赖判定
- 再决定是否进入 Talos 默认分发

---

## 7. 总体架构图

```text
                +----------------------+
                |   PromptBuilder      |
                | tool catalog prompt  |
                +----------+-----------+
                           |
                           v
+---------+     +----------------------+     +----------------------+
|  Model  | --> | Tool Registry        | --> | Tool Execution Flow  |
| tool use |     | public name -> spec |     | validate/permission  |
+---------+     | alias -> executor    |     | execute/mutate/store |
                +----------+-----------+     +----------+-----------+
                           |                            |
                           v                            v
                +----------------------+     +----------------------+
                | Tool Context         |     | Result Governance    |
                | read state           |     | truncate/persist     |
                | permission state     |     | summarize/reference  |
                | progress sink        |     | aggregate budget     |
                +----------+-----------+     +----------+-----------+
                           |                            |
                           +------------+---------------+
                                        |
                                        v
                              +------------------+
                              | TUI / Web Render |
                              | display summary  |
                              | progress/result  |
                              +------------------+
```

---

## 8. 阅读顺序

建议按以下顺序阅读：

1. `index.md`
2. `01-tool-platform-architecture-alignment.md`
3. 文件 / Shell / 网络 / MCP / Agent 五个能力域 spec
4. `07-feature-gated-and-environment-tools-alignment.md`
5. `08-migration-compatibility-and-rollout.md`
6. `09-open-decisions.md`
7. `10-phase-a-adr.md`
8. `11-tool-surface-selection-and-contract-hardening.md`
9. `12-tool-contract-status-reconciliation-and-spec-next-steps.md`

---

## 9. 本轮非目标

以下内容不在本轮 spec 里展开实现细节：

- 具体某个第三方 MCP server 的业务逻辑
- 非工具路径的 Planner 算法改写
- 前端 UI 逐像素还原 Claude Code
- 对 Anthropic 内部工具或 ant-only 基础设施的环境复制

本轮只解决一件事：

**把 Talos 的 tools 体系定义成一个可以完整承载 Claude Code 工具能力的系统。**
