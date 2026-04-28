# HTTP SSE Tool Reliability And Prompt Continuity Design

## 1. 背景

本轮目标不是继续优化 TUI，而是聚焦 Talos 在真实 HTTP SSE 会话里的两个核心可靠性问题：

1. 工具调用失败后，回答是否会直接中断或停止收尾。
2. 多轮对话后，模型是否会出现工具选择退化，表现为：
   - 明确要求使用代码工具时仍优先走弱工具或文档
   - 已有专用工具可用时回退到 `exec_command`
   - 同类任务在同一 session 内逐渐变得更“保守”或更“发散”

同时，本方案需要参考 `claude-code` 当前系统设计，吸收其在 prompt 分层、工具描述、失败补齐、记忆筛选上的成熟做法。

---

## 2. 本轮真实 SSE 验证结论

### 2.1 验证方式

直接通过 `POST /v1/sessions/{id}/messages` + `Accept: text/event-stream` 调用本地 Talos HTTP server。

手工构造并验证了 7 个 case：

1. `architecture_code_only`
2. `tool_failure_salvage`
3. `post_failure_followup`
4. `warmup_1`
5. `warmup_2`
6. `after_smalltalk_tool_round`
7. `specific_grep_then_read`
8. `followup_same_session_tool_strength`

其中 `warmup_1` / `warmup_2` 属于同一 session 的前置轮次，所以独立计数时可视为 2 个 case。

### 2.2 关键观察

#### 观察 A：工具失败后“直接停止回答”本轮没有复现成硬故障

在 `tool_failure_salvage` case 中：

- 模型成功调用 `exec_command(cmd=\"ls /definitely/not/exist\")`
- 工具结果返回失败输出，但 `message.tool_call.completed` 的到达没有导致 SSE 提前中断
- 后续仍继续生成 `message.output.delta`
- 最终存在 `message.completed`

结论：

- 当前 Talos 已具备“单轮内工具失败后继续收尾”的基础能力
- 但需要把这个能力上升为明确的系统契约和自动回归项，不能只依赖当前偶然表现

#### 观察 B：多轮后“完全不会用工具”没有复现

在 `warmup_1 -> warmup_2 -> after_smalltalk_tool_round` 的同一 session 验证中：

- 前两轮闲聊没有工具调用
- 第三轮要求代码分析时，仍然连续调用了 `list_dir` 和 `read_file`
- 最终正常完成

结论：

- 当前问题更像“工具选择质量衰减”而不是“多轮后完全失去工具能力”
- 需要避免把问题误诊为单纯的 prompt 污染或聊天历史污染

#### 观察 C：最稳定复现的问题是工具选择质量和工具优先级漂移

在 `architecture_code_only` case 中：

- 用户明确要求“实际用 `grep_files` 看代码，不要看文档”
- 模型第一步仍先调用 `list_dir`
- 后续大量转向 `read_file(Cargo.toml)` 和目录探索
- `grep_files` 出现得更晚，且不是首轮主路径

在 `specific_grep_then_read` case 中：

- 模型第一步能正确调用 `grep_files(pattern=\"trait LlmProvider\")`
- 第二步能调用 `read_file(api.rs)`
- 但随后又扩张到新的 `grep_files(pattern=\"impl LlmProvider\")`

在 `followup_same_session_tool_strength` case 中：

- 同一 session 继续深入后，仍使用工具
- 但开始出现 `exec_command(grep ...)`，即在已有 `grep_files` 的情况下回退到 shell

结论：

- 核心问题不是“工具链断了”
- 而是“工具优先级和任务协议在多轮与长轮中被稀释”

#### 观察 D：`Tooling` 和 `PlanningInstruction` 在真实请求中反复被裁剪

真实 SSE 流里多次出现：

- `PromptDynamicSectionDropped(... sections: [ExpandedToolCatalog])`
- `PromptDynamicSectionDropped(... sections: [RecentMessages])`

在当前实现中，这些 dynamic section 仍属于高价值上下文来源。一旦被裁剪：

- 用户显式指定工具的优先级会下降
- “专用工具优先、不要回退 shell”的约束会被削弱
- 历史连续性与局部行为约束会同步变弱

结论：

- 当前“多轮后变弱”的最主要嫌疑不是聊天历史本身
- 而是 prompt 预算竞争导致的行为约束衰减

#### 观察 E：项目级 `AGENTS.md` 缺失 warning 会持续进入流

本轮测试里多次出现：

- `PromptBootstrapFileMissing(... AGENTS.md at <project_root>)`

这说明当前项目级 `AGENTS.md` 发现逻辑仍是“只查精确 project_root”，还没有落成“`project_root` 优先、`workspace_dir/AGENTS.md` 回退”的固定优先级。

影响：

- 每轮都额外消耗 warning token
- 对模型来说是持续噪音
- 对用户显式在子目录启动会话的场景不友好

---

## 3. 问题归因

### 3.1 不是单一 prompt 污染，而是“行为约束衰减”

根据真实 case，本轮更合理的归因是：

1. 高价值工具策略 section 被 budget 裁剪。
2. 工具自身 description 还不够强，无法在 section 被裁剪后稳定顶住 shell 回退。
3. session 内没有一个紧凑的“recent tool strategy / recent failures / recent wins”连续性摘要。
4. 项目级 `AGENTS.md` 发现不稳，导致持续 warning 噪音。

### 3.2 当前失败恢复是“存在能力”，但不是“设计契约”

当前工具失败后继续回答是可观察到的，但系统里还缺：

- 明确的 failure recovery policy
- 明确的 forced salvage response 规则
- 明确的 SSE 回归断言

也就是说：

- 现在是“某些 case 能继续”
- 目标应该是“失败后一定给出结构化后续动作或总结，不允许直接停答”

---

## 4. `claude-code` 可借鉴设计

以下结论来自对本地 `~/data/ailabs/claude-code` 源码的对照阅读。

### 4.1 分层 system prompt + 每个工具自己的 prompt

`claude-code` 不只依赖总 system prompt，还给关键工具单独定义本地 prompt：

- `GrepTool`
- `GlobTool`
- `FileReadTool`
- `BashTool`

这意味着：

- 即使全局 prompt 较长，工具边界仍有第二层约束
- Bash 不容易反客为主
- 搜索、读取、文件枚举的职责边界更稳定

### 4.2 失败后先补齐 tool_result，再继续收敛

`claude-code` 在 query 执行层会处理：

- 缺失的 `tool_result`
- discard 旧执行器
- synthetic error tool result
- tool use summary

其要点是：

- 不让 transcript 落在“不完整工具状态”
- 不让旧状态污染新一轮模型输入

### 4.3 记忆不是全量注入，而是“相关 + 新鲜 + 去重”

`claude-code` 的记忆路径会根据：

- 当前任务相关性
- 是否最近已经 surfaced
- `recentTools`

进行筛选。这样能减少：

- 工具说明重复注入
- 历史噪音覆盖当前任务

### 4.4 UI / transcript 层把 tool use 当一等结构

这部分 Talos 最近已经明显接近，但 `claude-code` 的关键仍值得保留：

- tool use / in-progress / completed 是稳定配对结构
- stale / discard 有明确处理
- 最终 transcript 更像“完成过的动作链”而不是“工具日志列表”

---

## 5. 设计目标

下一阶段目标不是“让模型绝不犯错”，而是：

1. 明确要求用代码工具时，优先级稳定。
2. 多轮后工具能力不衰减为 shell-first。
3. 工具失败后一定继续完成回答或给出下一步。
4. prompt 裁剪时，优先保护工具策略，而不是先丢工具策略。
5. `AGENTS.md` / 记忆 / 最近工具历史真正服务连续性，而不是制造噪音。

---

## 6. 系统方案

## 6.1 Prompt Budget Rebalancing

### 问题

当前 `Tooling` 和 `PlanningInstruction` 会在真实请求中被 drop / truncate。

### 方案

将 prompt 中与工具选择强相关的内容重新分成三层：

1. `Static Core`
   - 永不丢弃
   - 包含：
     - `EvidenceToolingPolicy`
     - `ToolChoicePriority`
     - `FailureRecoveryPolicy`
     - `CommunicationPolicy`

2. `Compact Tool Routing Layer`
   - 高优先级、低 token
   - 只描述“优先级和边界”，不展开所有工具细节
   - 目标预算：`80-100 tokens`

3. `Expanded Tool Catalog`
   - 可裁剪
   - 只保留详细 schema / examples /长描述

### 核心原则

- 工具优先级规则必须进入静态不可裁剪层
- 详细工具目录可以裁剪
- `PlanningInstruction` 不再与一般 dynamic section 同优先级竞争
- 将 `max_static_tokens` 从当前 `1200` 提升到约 `2000`
- `Compact Tool Routing Layer` 必须被视为静态核心的一部分，而不是普通 dynamic section

### 直接效果

即使 budget 紧张，也不会先丢：

- “代码调查优先 grep/read/list”
- “不要用 exec_command 代替代码工具”
- “失败后必须继续收尾”

---

## 6.2 Tool Prompt Layering

### 问题

当前 Talos 对工具选择的约束更多来自总 prompt，不够接近 `claude-code` 的“工具本地 prompt”设计。

### 方案

为以下工具增加更强的本地 description / local prompt：

1. `grep_files`
   - 默认用于：
     - symbol lookup
     - entrypoint discovery
     - trait / impl / route / export 搜索
   - 明确：
     - 在代码分析任务中，先于 `exec_command(grep ...)`
     - 在不知道具体文件前，先于 `read_file`

2. `read_file`
   - 默认用于：
     - 已知文件的内容确认
   - 明确：
     - 不用于“盲读未知路径”
     - 需要先通过 `grep_files` / `list_dir` 缩小目标

3. `list_dir`
   - 默认用于：
     - 顶层结构感知
     - 验证路径是目录
   - 明确：
     - 不应替代代码级搜索

4. `exec_command`
   - 默认用于：
     - 真正需要 shell 的任务
   - 明确：
     - 不应用于替代 `grep_files` / `read_file` / `list_dir`
     - 仅在专用工具无法表达时才使用

### 额外优化

工具 description 需要加入 2-3 个“何时不要使用我”的反例，而不只是“我能做什么”。

---

## 6.3 Failure Recovery Contract

### 问题

当前失败后能继续回答，但还不是稳定契约。

### 方案

本轮不引入第二次 salvage LLM call。

本阶段优先对齐 `claude-code` 的恢复思路：

1. 先做 transcript 补齐
2. 先补齐 tool failure 的结构化摘要
3. 先复用现有 finalization / fallback 路径完成收尾
4. 如果上述路径之后仍没有有效 final answer，则允许一次受控的 salvage LLM call

也就是说：

- 默认不额外发起第二次模型请求
- 但如果补齐后仍无有效最终回答，允许一次受控补救 call

在 general turn 收尾语义上增加 Failure Recovery Policy：

1. 如果工具执行失败：
   - 该失败必须进入 transcript 的结构化上下文
   - 模型必须收到一条紧凑 failure summary，而不是只看到原始工具输出

2. 如果失败后模型没有最终回答：
   - 强制进入一次收尾补齐路径
   - 该路径不得再次触发新的普通探索轮
   - 优先使用已有 transcript 和 finalization 逻辑补出非空回答
   - 若仍无有效最终回答，则允许一次受控 salvage LLM call
   - salvage call 只允许：
     - 解释失败
     - 基于现有信息给下一步
     - 基于已有 transcript 做总结

3. 如果连续失败：
   - 不允许直接输出空响应
   - 必须显式说明：
     - 已验证的失败事实
     - 为什么失败
     - 推荐的替代动作

### 约束

若进入 salvage LLM call，必须满足：

- 最多只允许一次
- 必须有显式标记防止循环
- 默认禁止新的普通探索工具调用
- 只允许失败解释、证据总结、下一步建议
- SSE 上允许出现一次短暂停顿，但最终必须恢复并完成 `message.completed`

### SSE 语义要求

失败 case 最终必须满足：

- 出现 `message.tool_call.completed`
- 之后仍有 `message.output.delta`
- 最终有 `message.completed`

---

## 6.4 Session Continuity Layer

### 问题

当前多轮问题更像“行为约束衰减”，需要在连续会话里保留极小但有价值的策略上下文。

### 方案

新增一个紧凑动态 section：

`Recent task continuity`

内容只包含近 1-2 轮的高价值摘要：

- recent_verified_paths
- recent_successful_tools
- recent_failed_tools
- recent_unhelpful_patterns
- recent_answer_mode

示例：

```text
Recent task continuity:
- recent_successful_tools: grep_files, read_file
- recent_failed_tools: exec_command(ls /definitely/not/exist)
- recent_unhelpful_patterns: avoid retrying the same failing path
- current_evidence_mode: code_first
```

### 作用

- 让模型知道“上一轮已经用哪些工具成功”
- 让模型知道“哪些工具调用刚刚失败，不要重复”
- 减少“忘了自己上一轮已经在 code-first 模式里”

### 跨轮数据流

本阶段不引入新的 continuity 子系统，直接复用现有 agent-kernel session state 承载。

现有可复用结构已经存在：

- `InvocationSession`
- `SessionState`
- `StateNamespace::Session`

因此本阶段不是新建状态子系统，而是把 continuity 对象正式接到已有状态槽位上。

1. 在 turn 完成时，从本轮执行结果提取：
   - `recent_successful_tools`
   - `recent_failed_tools`
   - `recent_unhelpful_patterns`
   - `current_evidence_mode`

2. 将其作为一个强类型 continuity 对象，序列化后写入：
   - `InvocationSession.state`
   - `StateNamespace::Session`

3. 下一轮构建 `PromptBuilder` 时：
   - 从 session 的 `StateNamespace::Session` 读取 continuity 对象
   - 通过 `PromptBuilder.session_continuity` 输入槽传入
   - 组装成 `Recent task continuity` section

### PromptBuilder 接入要求

为了让 continuity 真正进入下一轮 prompt，需要显式补齐以下链路：

1. `PromptBuilder` 新增：
   - `session_continuity: Option<SessionContinuity>`

2. `prepare_general_turn` 负责：
   - 从 `InvocationSession.state` 的 `StateNamespace::Session` 读取 continuity
   - 反序列化为强类型 `SessionContinuity`
   - 传给 `PromptBuilder`

3. `assembler.rs` 负责：
   - 将 `SessionContinuity` 渲染为 `Recent task continuity` section
   - 按紧凑 section 的预算与优先级处理

### 类型归属

`SessionContinuity` 定义在：

- `talos-rs/crates/common/prompt/src/types.rs`

原因：

- 该类型本质上是 prompt 组装输入 DTO
- `agent-kernel` 已经依赖 `talos_prompt`
- 放在 `common/prompt` 可避免把 prompt 专属结构塞进更通用的 domain crate
- 通过 `serde` 序列化后写入 `InvocationSession.state`，读取侧再反序列化为强类型即可

### 约束

- continuity 只保留最近 1-2 轮的高价值摘要
- 不直接持久化完整 tool log
- 不把 request 级瞬时噪音写入 continuity

### `recent_unhelpful_patterns` 来源

`recent_unhelpful_patterns` 必须由代码侧机械生成，不依赖模型总结。

生成来源：

1. `tool_error_count`
2. 只读工具参数指纹
3. 明确失败或低增益的工具结果

生成规则示例：

- 同一路径或同一参数指纹的只读调用连续重复，但没有新增证据
- 明确失败的路径或命令，例如：
  - missing path
  - unsupported regex
  - same failing shell command
- 已知低收益的模式，例如：
  - repeated grep with near-identical pattern
  - repeated read of same file window

输出形式应是紧凑、机械的短句，例如：

```text
- avoid retrying the same failing path
- avoid repeating grep on the same pattern without narrowing scope
- avoid re-reading the same file range when no new evidence is needed
```

本阶段禁止：

- 让模型在收尾时自由生成 `recent_unhelpful_patterns`
- 把整段 reasoning 直接塞入 continuity

### Summary / Judge Provider

`Recent task continuity` 提取、session summary 提取、memory durability judge 默认都复用当前 session 的 LLM provider / model family。

默认策略：

- `summary_provider = same_as_session`
- `judge_provider = same_as_session`

后续若需要降成本，再单独设计轻量 judge override；本阶段不引入新的独立 provider 路由。

---

## 6.5 Relevant Memory And Recent Tools Filtering

### 问题

当前 durable memory 和动态上下文虽然已经收敛很多，但还没有把“recent tools / current surfaced context”纳入过滤条件。

### 方案

借鉴 `claude-code` 的 `recentTools` 思路，在相关记忆检索时增加过滤：

1. 如果某条记忆只是重复解释当前已使用工具，不要再注入。
2. 如果某条记忆与当前任务无关，只是历史流程噪音，不要再注入。
3. 如果当前任务已经进入 `code-first` 模式，则优先保留：
   - 项目规则
   - 用户偏好
   - 与代码分析相关的 durable 规则

### 目标

避免“动态 section 预算被重复工具说明和无关记忆吃掉”。

---

## 6.6 `AGENTS.md` Discovery Improvement

### 问题

当前项目级 `AGENTS.md` 只查精确 `project_root/AGENTS.md`。当项目目录下没有该文件时，会持续产生缺失 warning。

### 方案

### 发现顺序

`AGENTS.md` 的发现规则不做向上查找，而是按固定优先级：

1. 优先加载 `session.project_root/AGENTS.md`
2. 若项目目录下不存在，则回退到 `workspace_dir/AGENTS.md`
3. 不再从 `session.project_root` 或 `session.environment.directory` 向上遍历
4. 若两处都不存在，只记录一次 session 级 warning

### 说明

这意味着：

- 项目目录规则只认显式 `project_root`
- 如果项目目录没有 `AGENTS.md`，则使用与 `USER.md` 同目录的 `workspace_dir/AGENTS.md`
- 不通过“向上找最近文件”来推断项目根

### warning 去重

缺失 warning 的去重也复用现有 session state，不新增独立缓存子系统：

1. 定义强类型 `PromptWarningState`
2. 将“已发出过的 bootstrap warning key/path”写入：
   - `InvocationSession.state`
   - `StateNamespace::Session`
3. 后续同一 session 内若再次命中同一 warning key，则不再重复发流

### 类型归属

`PromptWarningState` 定义在：

- `talos-rs/crates/common/prompt/src/types.rs`

原因与 `SessionContinuity` 相同：

- 它是 prompt/bootstrap 组装过程的辅助状态
- 由 `agent-kernel` 写入 session state
- 由 prompt 相关路径读取并参与去重判断

### 直接收益

- 减少无价值 prompt warning
- 避免因为用户从子目录进入而丢失项目规则

---

## 6.7 Near-Duplicate Read-Only Convergence

### 问题

当前仍有轻微近似重复探索：

- pattern 略变的 `grep_files`
- 已足够回答却继续做低增益搜索

### 边界

本阶段只收敛 **单次 turn 内** 的低增益重复，不要求跨轮去重。

- single-turn 内的连续低增益重复：
  - 由 `ToolLoopGuardState`
  - 配合参数归一化指纹与 system nudge 处理
- cross-turn 的重复倾向：
  - 由 `Session Continuity` 中的 `recent_unhelpful_patterns` 承担

也就是说，本阶段不是新做跨轮 read-only guard，而是把现有单轮 guard 补齐并验证到位。

### 方案

增加轻量低增益护栏，但保持 model-driven：

1. 仅对只读工具生效：
   - `grep_files`
   - `read_file`
   - `list_dir`

2. 做参数归一化指纹
3. 识别低增益连续调用
4. 第二次连续低增益时注入 system nudge
5. 第三次连续低增益时优先要求总结或切换路径

### 注意

不做硬规则禁用，不回退到 rule-based router。

---

## 7. 模块影响面

| 模块 | 变更方向 |
| --- | --- |
| `talos-rs/crates/common/prompt/src/assembler.rs` | 重排 static/dynamic section，加入 compact continuity section |
| `talos-rs/crates/common/prompt/src/builder.rs` | 透传 recent continuity state、AGENTS discovery 结果 |
| `talos-rs/crates/common/prompt/src/types.rs` | 增加新的 prompt section kind，以及 `SessionContinuity` / `PromptWarningState` 等共享 DTO |
| `talos-rs/crates/orchestration/agent-kernel/src/general_turn/*` | 失败恢复、salvage pass、低增益护栏 |
| `talos-rs/crates/action/tools/src/tools/*.rs` | 强化 per-tool description |
| `talos-rs/crates/common/prompt` tests | 增加 section 预算和 AGENTS discovery 测试 |
| `talos-rs/tests/integration/scenarios` | 增加真实 HTTP SSE 回归 case |

---

## 8. 验收标准

### 8.1 工具失败恢复

给定：

- `exec_command("ls /definitely/not/exist")`

要求：

- 存在 `message.tool_call.completed`
- 后续仍存在 `message.output.delta`
- 最终存在 `message.completed`
- 最终回答必须包含失败解释和下一步建议

### 8.2 显式工具服从

给定：

- “先用 `grep_files` 搜索 `trait LlmProvider`，再 `read_file` ...”

要求：

- 第一条工具调用必须是 `grep_files`
- 后续可以扩展搜索，但不能先走 `exec_command`

### 8.3 多轮连续性

给定：

- 2 轮闲聊 + 1 轮代码分析

要求：

- 第 3 轮仍能稳定调用代码工具
- 不允许完全退化成无工具空谈

### 8.4 Prompt Budget

要求：

- 在工具型请求中，`EvidenceToolingPolicy` 不能被 drop
- `ToolChoicePriority` 不能被 drop
- `FailureRecoveryPolicy` 不能被 drop

### 8.5 `AGENTS.md` 发现

要求：

- 优先加载 `project_root/AGENTS.md`
- 若项目目录下不存在，则回退到 `workspace_dir/AGENTS.md`
- 不做向上查找
- 同一路径缺失 warning 不应在同一会话里重复刷屏

---

## 9. 实施阶段

## 阶段 1: Prompt Budget Hardening
**目标**: 保护工具策略类 section 不被预算先裁掉。  
**成功标准**:
- 工具优先级规则进入不可裁剪静态层
- `Tooling` 详细目录可裁剪，但 compact routing 不可裁剪
- 相关单测覆盖 drop / truncate 行为
- `Compact Tool Routing Layer` 预算被硬限制在 `80-100 tokens`
- `max_static_tokens` 提升到约 `2000`
- `PromptBudget::default()` 与配置默认值同步提升到新预算
**测试**:
- `cargo test -p talos-prompt --lib`
- `cargo test -p talos-agent-kernel --lib`

## 阶段 2: Tool Prompt Layering
**目标**: 强化 `grep_files` / `read_file` / `list_dir` / `exec_command` 的本地选择边界。  
**成功标准**:
- 四个工具 description 完成重写
- 至少覆盖“何时不该用我”的反例
- 显式 `grep_files` case 首轮服从率提升
**测试**:
- 工具描述相关单测
- 新增 HTTP SSE 回归 case

## 阶段 3: Failure Recovery Contract
**目标**: 将工具失败后继续收尾变成强契约；默认走现有 finalization，必要时允许一次受控 salvage LLM call。  
**成功标准**:
- 工具失败后一定继续输出最终回答
- 空 final response 不允许直接结束
- salvage pass 优先走 transcript 补齐与现有 finalization 收尾
- 若仍无有效最终回答，允许一次受控 salvage LLM call
- 受控 salvage call 默认禁止新的普通探索
**测试**:
- `cargo test -p talos-agent-kernel --lib`
- HTTP SSE failure case

## 阶段 4: Session Continuity And AGENTS Discovery
**目标**: 收敛多轮连续性，并减少 bootstrap 噪音。  
**成功标准**:
- `Recent task continuity` section 生效
- 最近失败工具和成功工具能进入紧凑上下文
- continuity 明确通过 `InvocationSession.state` + `StateNamespace::Session` 跨轮传递
- `PromptBuilder` 新增 `session_continuity` 输入槽
- `prepare_general_turn` 负责从 session state 加载 continuity 并传入
- `AGENTS.md` 按“project_root 优先、workspace_dir/AGENTS.md 回退”生效
- 相同缺失 warning 不再反复进入流
**测试**:
- `cargo test -p talos-prompt --lib`
- 多轮 SSE case

## 阶段 5: Near-Duplicate Read-Only Convergence
**目标**: 进一步压低低增益重复搜索。  
**成功标准**:
- 连续近似重复 `grep_files` 数量下降
- 不破坏正常 narrowing search
- 保持 model-driven
**测试**:
- `cargo test -p talos-agent-kernel --lib`
- 人工 SSE transcript 验证

## 阶段 6: End-To-End SSE Regression
**目标**: 把本轮手工 case 固化成自动回归与人工清单。  
**成功标准**:
- 关键 HTTP SSE case 可自动执行
- 输出报告可判定“工具失败恢复 / 首轮 grep 服从 / 多轮后仍会用工具”
- 保留 1 组人工 tmux/TUI 复核清单
**测试**:
- `./tests/integration/run_e2e.sh`
- 新增 manual SSE regression report

---

## 10. 一句话结论

当前 Talos 的问题不是“多轮后完全不会用工具”，而是：

- 工具策略约束在真实请求里会被 prompt budget 稀释
- 工具本地边界不够强，导致 shell 回退
- 连续会话缺少紧凑策略上下文
- 项目级规则发现不稳，制造额外噪音

因此下一阶段应优先做：

1. prompt budget 硬化  
2. per-tool prompt layering  
3. failure recovery 契约化  
4. session continuity + `AGENTS.md` 固定优先级发现  
5. 低增益重复搜索收敛
