# Talos Tool Surface, Selection, And Contract Hardening

## 1. 目标

本篇用于收敛当前 Talos 工具体系在以下三类问题上的下一步实现方向：

1. **工具公开面与命名统一**
2. **请求级工具选择与 deferred tool 装载**
3. **工具输入输出契约、结果治理与失败反馈补齐**

这不是一篇“新增更多工具”的文档，而是针对当前已经落地的一批工具做 **生产级收口**。

本轮目标：

1. 把当前已经接近可用的工具面收敛成 Claude Code 风格
2. 移除不该继续暴露给模型的工具
3. 统一 memory / file / shell / workflow 工具命名风格
4. 从“默认大部分工具都注册给模型”升级到“全集 + 请求级子集选择”
5. 补齐工具结果协议与失败反馈语义，使模型能够更稳定地重试、总结和收敛

---

## 2. 本轮源码对照结论

### 2.1 Talos 当前现状

从 `talos-rs/crates/action/tools` 当前实现看：

- 新增工具大多已经不是 demo/mock 级别，`Bash/TaskOutput/TaskStop/Read/Grep/Glob/Write/Edit/WebFetch/WebSearch/Agent/TodoWrite/EnterPlanMode/ExitPlanMode/AskUserQuestion/ToolSearch` 已经具备真实执行路径。
- 但工具公开面仍然混有旧风格名字和不该长期暴露的工具：
  - `apply_patch`
  - `list_dir`
  - `memory_search`
  - `memory_get`
- `ToolVisibility::Deferred` 已存在，`ToolSearch` 也已存在，但 **Talos 目前仍然以“静态注册大多数工具 + prompt 约束”作为主模式**，还没有 Claude Code 那种 request-time tool subset selection。
- 工具 trait 仍然偏薄：
  - `name`
  - `description`
  - `parameters_schema`
  - `execute`
- `ToolResultPolicy`、`ToolBehaviorFlags`、`ToolVisibility` 已经落地，但工具结果本体仍以 `ToolOutput { success, output: String, error }` 为主，结构化程度不够。

### 2.2 Claude Code 当前模式

从 `claude-code` 当前源码看：

- `getAllBaseTools()` 定义的是 **当前环境可能存在的完整工具全集**
- 实际给模型的不是全集，而是 request-time 的 `getTools(permissionContext)`
- `Deferred` 不是纯文档概念，而是真正参与请求级工具裁剪
- `ToolSearch` 的职责是帮助模型在 deferred tools 中查找目标工具，再触发后续调用
- 工具契约远比 Talos 厚，至少包括：
  - `inputSchema`
  - `userFacingName`
  - `getToolUseSummary`
  - `isReadOnly`
  - `isConcurrencySafe`
  - `checkPermissions`
  - `renderToolUseMessage`
  - `renderToolUseProgressMessage`
  - `renderToolUseRejectedMessage`
  - `renderToolUseErrorMessage`

因此，Claude Code 的关键差异不是“工具更多”，而是：

1. **公开工具面受 request-time 选择控制**
2. **工具契约支持权限、展示、失败反馈和结果总结**
3. **deferred tool 不是摆设，而是真正参与模型工具暴露策略**

---

## 3. 本轮确认的问题点

### 3.1 `apply_patch` 与 `list_dir` 不应继续作为模型公开工具

这两个工具继续公开会带来两个问题：

- 模型工具面变杂，降低高频工具识别效率
- prompt 侧会持续引导模型使用不再推荐的工作方式

#### 3.1.1 `apply_patch`

Talos 当前 `apply_patch` 更像内部工程能力，而不是模型长期公开工具协议。

后续原则：

- 不再作为模型公开工具
- 不再出现在 prompt tool catalog 中
- 不再出现在默认 allow-list 中
- 可保留为内部 runtime primitive，供 agent/coder runtime 或受控执行链路使用

#### 3.1.2 `list_dir`

Claude Code 当前公开工具面没有长期保留 `list_dir` 这种风格的工具名。

Talos 后续可选路径只有两种：

1. 直接移除目录列举工具，不再给模型专门暴露
2. 如果仍然认为目录列举能力必要，则必须改成与整体命名风格一致的 canonical name，再重新评估是否默认公开

本轮决策先写死为：

- **当前公开工具面直接移除 `list_dir`**
- 若未来确实需要，再以统一命名风格重新引入，不沿用旧名字

### 3.2 `memory_search` / `memory_get` 命名风格不一致

当前 memory 工具仍使用 snake_case 历史命名，和已经公开的新工具面明显不一致。

后续应统一为：

- `MemorySearch`
- `MemoryGet`

同时不只改 public name，还要同步收口：

- Rust 文件名
- module 名
- registry 注册名
- allow-list / deny-list / permission rule
- prompt 目录
- planner / selection hint
- 测试与日志口径

本轮原则：

- 不保留历史 snake_case 作为长期 public name
- alias 仅允许短期迁移
- 最终 canonical public name、Prompt name、UI name 必须一致

### 3.3 当前工具输出契约仍偏弱

Talos 现在虽然已经有部分结构化 runtime result，但平台总出口仍然偏 `String payload`：

- 很多工具把 JSON 再序列化为字符串放到 `output`
- 平台没有统一的 result summary contract
- 平台缺少统一 `progress / rejected / error` 三类结果映射

这直接影响三件事：

1. 模型看到失败时，很难稳定决定是否重试
2. UI/TUI/CLI 很难统一展示
3. 结果治理无法对“模型可见内容”和“操作层完整结果”做明确分层

---

## 4. 目标架构方向

## 4.1 工具公开面改为“全集 + 请求级子集”

Talos 后续需要对齐 Claude Code 的以下模式：

1. registry/catalgo 维护完整工具全集
2. request-time 根据上下文挑选本次给模型的工具子集
3. deferred tools 不在默认 prompt tool catalog 中出现
4. 需要时通过 `ToolSearch` 查找 deferred tools

### 4.1.1 请求级子集选择输入

请求级 tool selection 至少需要参考：

- planner mode
- permission mode
- session connection mode
- feature flags
- tool source
- deferred visibility
- deny/allow rule
- current task shape
- conversation phase

### 4.1.2 第一版最小选择器

第一版不做独立 intent classifier。

先实现一个 deterministic selector，输入包括：

- visible tool catalog
- request context
- planner mode
- permission context
- optional task profile

输出包括：

- `active_tools`
- `deferred_tools`
- `always_loaded_tools`

第一版最小规则：

- 高频主工具默认进入 active set：
  - `Bash`
  - `Read`
  - `Grep`
  - `Glob`
  - `Edit`
  - `Write`
- workflow/state tools 按场景进入 active set：
  - `Agent`
  - `TodoWrite`
  - `EnterPlanMode`
  - `ExitPlanMode`
  - `AskUserQuestion`
- `ToolSearch` 在存在 deferred tools 时进入 active set
- `Skill` 默认 deferred
- MCP / feature-gated / heavy tools 默认 deferred

### 4.1.3 不做“每次都把所有工具塞给模型”

这是本篇最重要的架构约束之一。

Talos 后续不得继续依赖：

- “把多数工具都暴露给模型”
- “再在 prompt 里写一堆优先级说明”

prompt 仍然需要保留工具选择指导，但它应该是 **次级约束**；
真正的主约束应该是 request-time tool subset selection。

## 4.2 工具 canonical name 全面统一

后续命名原则：

- public name 使用 Claude Code 风格 canonical name
- internal name 尽量同步为 canonical name 对应的 Rust 风格标识，而不是历史遗留名
- rs 文件名与模块名优先和 canonical name 对应

推荐方向：

| 当前 | 目标 |
| --- | --- |
| `read_file.rs` / `ReadFileTool` | `read.rs` / `ReadTool` |
| `grep_files.rs` / `GrepFilesTool` | `grep.rs` / `GrepTool` |
| `write.rs` / `WriteTool` | 保持 |
| `edit.rs` / `EditTool` | 保持 |
| `memory_search` | `MemorySearch` |
| `memory_get` | `MemoryGet` |
| `list_dir` | 移除 |
| `apply_patch` | 从模型公开面移除 |

这里的重点不是文件名漂亮，而是：

- 让 public contract、registry name、日志、UI、文档尽量收敛
- 减少 public name / internal name / module name 三套体系长期并行

## 4.3 工具结果协议升级为“三层结果”

Talos 后续工具结果应拆成三层：

1. **raw execution result**
   供 runtime / persistence / replay 使用
2. **model-visible result**
   供模型继续推理、重试、总结使用
3. **renderable result**
   供 CLI/TUI/Web 展示使用

至少补齐以下接口或等价结构：

- `render_progress`
- `render_rejected`
- `render_error`
- `summarize_for_model`
- `summarize_for_transcript`

## 4.4 失败反馈必须可引导模型收敛

Talos 后续不能只把失败当字符串丢回模型。

至少要区分：

- validation failure
- permission rejection
- governance rejection
- runtime execution failure
- stale-write / read-state failure
- deferred tool unavailable / not yet loaded

并提供统一的结构化字段，使模型能判断：

- 是否值得立即重试
- 是否应该换工具
- 是否应该结束并总结

---

## 5. 分项决策

### 5.1 工具面收口

本轮明确决策：

- 公开工具面移除：
  - `apply_patch`
  - `list_dir`
- memory 工具统一改名：
  - `MemorySearch`
  - `MemoryGet`
- `Skill` 继续保留为 deferred 正式工具
- `ToolSearch` 继续保留，并作为 deferred tool 入口

### 5.2 Prompt 与 planner 同步收口

本轮明确决策：

- planner/prompt 中不再引导模型使用 `list_dir`
- planner/prompt 中不再提 `apply_patch`
- 统一改成新的 canonical public names
- prompt 中必须体现“不是所有工具都默认可见”的事实

### 5.3 生产级工具实现的完成定义

后续任何公开工具若要视为生产级，需要满足：

1. 不是 mock/demo
2. 有稳定 schema
3. 有明确 permission/governance 语义
4. 有明确 progress / success / error / rejected 结果语义
5. 有单测
6. 有至少一类真实触发验证证据

---

## 6. 当前对齐状态简表

| 项目 | 当前判断 |
| --- | --- |
| 核心工具真实可用性 | 基本可用，已不是 demo 实现 |
| canonical public names | 未完全收口 |
| 历史旧名清理 | 未完成 |
| request-time tool selection | 未落地 |
| deferred tool 机制 | 基础已具备，但未形成完整模式 |
| result rendering contract | 未完全落地 |
| failure-to-retry guidance | 偏弱 |
| prompt tool exposure strategy | 偏“全量暴露 + 文字引导” |

---

## 7. 实施优先级

下一轮实现优先顺序固定为：

1. **工具面收口与命名统一**
2. **请求级 tool subset selection**
3. **工具结果契约与失败反馈补齐**
4. **再做剩余工具能力边界扩展**

原因：

- 如果不先收口工具面，后续 prompt、UI、policy、测试都会继续分叉
- 如果不先做 request-time selection，deferred tool 只会停留在静态标记
- 如果不补结果契约，模型智能化调用会持续受限

---

## 8. 本轮约束

本轮 spec 落地后，开始实现时遵守以下约束：

- 不跑 e2e 全流程回归
- 不依赖当前耗尽额度的模型验证来推进架构实现
- 先完成静态重构、单测、结构性改造
- 待额度恢复后再跑全流程与真实模型回归
