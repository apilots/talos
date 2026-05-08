# Talos 智能性提升方案：Prompt、Memory 与 Bootstrap 重构

> 目标：参考 `claude-code` 的系统设计，提升 Talos 在真实任务中的意图理解、工具选择、证据收敛和长期协作能力。
> 本文档聚焦服务端 prompt 体系、workspace bootstrap 内容治理、长期记忆筛选策略三部分，暂不涉及 UI 视觉或工具协议改造。

---

## 1. 背景

当前 Talos 已具备：

- 流式工具调用与中途进度反馈
- `project_root` 与 `workspace_dir` 语义拆分
- TUI 中较稳定的 Thinking / Assistant / Tool 交替体验

但真实任务表现仍有明显短板。以用户测试样例为例：

- 用户明确要求“按代码分析，不要看文档直接得出结论”
- 模型先做了一次 `grep_files(pattern="fn main")`
- 随后重复做了相同 `grep_files`
- 已读取 `Cargo.toml` 后再次重复读取
- 过程中又回退去看 `ARCHITECTURE.md`
- 最终形成的是“文档 + Cargo 列表”的浅层总结，而不是“代码证据驱动”的架构分析

这类问题的根因不在 TUI，而在：

1. 服务端 system prompt 对“证据优先级”和“任务协议”约束不够强
2. workspace bootstrap 文件过多，但实际内容大多是占位模板，无法给模型提供有效长期上下文
3. 会话 summary / workspace memory 目前还会混入低价值运行态信息（如 flush 相关记录）

---

## 2. 目标与非目标

### 2.1 目标

1. 提升 Talos 在复杂任务中的“智能管家”感，而不是只做单轮问答 bot
2. 保持纯模型驱动，不引入 intent classifier 或基于前缀的硬编码路由
3. 强化“按代码证据回答、专用工具优先、避免重复探索”的系统行为
4. 精简 `~/.talos/workspace-talos` 下的 bootstrap 文件，只保留少数必要文件
5. 让长期记忆只保留真正有价值的 durable 信息，跳过 flush/noise summary

### 2.2 非目标

1. 本轮不改 TUI transcript 协议
2. 本轮不改工具执行协议
3. 本轮不实现新的 planner 模式
4. 本轮不引入规则驱动工具拦截

---

## 3. 参考 Claude Code 的关键结论

基于对本地 `~/data/ailabs/claude-code` 的对照，Talos 下一阶段最值得借鉴的不是某一句 prompt，而是以下 4 类机制：

### 3.1 专用工具优先是 system prompt 的硬约束

Claude Code 在 system prompt 中明确要求：

- `Read/Edit/Write/Grep/Glob` 优先于 Bash
- 代码调查优先专用工具，而不是 shell 命令
- 工具选择是模型驱动，但“工具哲学”由 system prompt 强约束

### 3.2 记忆系统是正式的 system prompt 组成部分

Claude Code 的 memory prompt 不是占位文件拼接，而是有明确边界：

- 什么内容适合写入 memory
- 什么内容只属于当前任务 progress，不应进入 memory
- memory 如何索引与组织

### 3.3 当前任务进度不等于长期记忆

Claude Code 明确区分：

- `memory`: 对未来会话仍有价值的内容
- `tasks / plan / progress`: 只对当前会话或当前任务有价值的内容

这正是 Talos 现在混入 flush/noise 的主要反例。

### 3.4 输出质量来自“静态系统约束 + 动态上下文”共同作用

Claude Code 并不依赖大量用户填写的 markdown 文件，而是：

- 大部分行为规则放在代码内的 system prompt
- 少量真正有价值的 memory 文件作为长期上下文
- 会话进度通过 summary / progress 机制补足

### 3.5 工具描述本身也是智能度的重要组成部分

Claude Code 不只在 system prompt 里写“优先专用工具”，它还在具体工具描述里继续强化：

- Read 工具强调绝对路径、适合整读还是局部读、何时不要重复读
- Grep 工具强调“搜索任务必须优先用 Grep，不要退回 Bash grep/rg”
- Bash 工具强调“只有 shell 本身是任务时才使用”

Talos 下一阶段也应同步补齐工具 description，而不是只改 system prompt。

---

## 4. 现状问题诊断

### 4.1 Prompt 结构太薄

Talos 当前 prompt 组装主链路在：

- [builder.rs](/Users/xujiang/data/ailabs/talos/talos-rs/crates/common/prompt/src/builder.rs:1)
- [assembler.rs](/Users/xujiang/data/ailabs/talos/talos-rs/crates/common/prompt/src/assembler.rs:1)
- [planner.rs](/Users/xujiang/data/ailabs/talos/talos-rs/crates/orchestration/agent-kernel/src/planner.rs:1)

虽然已有：

- `TOOL_USAGE_GUIDANCE`
- `USER_FACING_PROGRESS_GUIDANCE`
- project context bootstrap 注入

但缺少 3 件关键东西：

1. **证据优先级协议**  
   代码、文件、命令结果是一手证据；文档、README、架构文档是二手证据。当前没有明确把这个层级写死。

2. **任务级协议**  
   “分析项目架构”“调 bug”“做改动方案”“检查环境”这些高频任务，没有系统化工作方法。

3. **长期助手身份**  
   Talos 的“智能管家/长期推进者/主动补上下文”身份还没有变成强 system prompt，而主要停留在用户预期里。

### 4.2 Bootstrap 文件过多，但大多无有效内容

当前 `~/.talos/workspace-talos` 下有：

- `AGENTS.md`
- `BOOTSTRAP.md`
- `HEARTBEAT.md`
- `IDENTITY.md`
- `MEMORY.md`
- `SOUL.md`
- `TOOLS.md`
- `USER.md`

实际情况是：

- `USER.md` 有一定真实价值
- `MEMORY.md` 有价值，但混入了大量低价值 flush 标记
- 其余文件大多还是占位模板

问题不在“文件数量本身”，而在：

- 用户不会维护这么多文件
- 模型读入这些占位内容几乎没有增益
- 反而稀释了真正重要的长期上下文

### 4.3 Session summary 与 durable memory 边界不清

Talos 当前允许 session summary 和 workspace memory 累积，但没有形成足够清晰的模型驱动筛选边界，因此容易把以下内容写进去：

- close flush 标记
- session id
- 临时路径状态
- 无实际增量的执行摘要
- 纯运行态噪音

这些内容对未来会话几乎没有价值，应该跳过。

---

## 5. 设计原则

1. **模型驱动，不做规则路由**  
   不加 intent classifier，不做用户输入前缀硬分流。

2. **大部分行为约束放入 system prompt，不依赖用户手工填大量文件**

3. **少量高价值 bootstrap 文件优于大量占位文件**

4. **长期记忆只保留未来仍有价值的事实、偏好、目标、约束**

5. **任务进度与长期记忆严格分离**

---

## 6. 新的 Prompt 架构

### 6.1 总体结构

```text
Prompt
├── Static Core (code-owned)
│   ├── Identity And Mission
│   ├── Evidence And Tooling Policy
│   ├── Task Protocol Library
│   └── Communication Policy
├── Stable Context (file-owned)
│   ├── USER.md
│   ├── MEMORY.md
│   ├── SOUL.md
│   ├── IDENTITY.md (optional)
│   ├── HEARTBEAT.md (optional)
│   └── project-root AGENTS.md (optional, repo-scoped)
└── Dynamic Context
    ├── Session summary
    ├── Recent messages
    ├── Retrieved memories
    └── Current user input
```

### 6.2 Static Core 设计

#### A. Identity And Mission

由代码内置，不再依赖 `IDENTITY.md` / `SOUL.md`：

- Talos 是长期型智能管家与协作伙伴
- 默认目标是少打扰用户、主动补齐上下文、持续推进任务
- 不是单轮 FAQ bot，也不是只会机械调用工具的 executor

建议核心 prompt 文本：

```text
You are Talos, a long-running AI steward and engineering companion.

Your default posture is proactive, evidence-driven, and completion-oriented:
1. Push the user's work meaningfully forward instead of stopping at shallow analysis.
2. Prefer verified facts over confident speculation.
3. Minimize unnecessary user interruption; ask only when the decision is truly high-risk or blocked.
4. Maintain continuity across turns by using stable user context, durable memory, and project-local rules.
5. Behave like a careful senior collaborator, not a one-shot answer bot.
6. If a task is incomplete, treat the next most useful action as part of the job unless the user explicitly wants only discussion.
7. When context is uncertain, investigate before concluding.
```

#### B. Evidence And Tooling Policy

新增强约束：

- 代码、文件、命令结果是一手证据
- 文档、README、设计稿是二手证据
- 当用户明确要求“按代码分析”时，二手证据只能补充，不能主导结论
- 专用工具优先于 shell
- 等价只读探索不要重复执行

建议核心 prompt 文本：

```text
Evidence and tooling rules:
1. Treat code, file contents, command output, and tool results as primary evidence.
2. Treat README, architecture docs, design notes, and naming conventions as secondary evidence unless the user explicitly asks for a doc summary.
3. If the user asks for code-based analysis, do not let documentation drive the conclusion unless code evidence is missing and you explicitly say so.
4. Prefer dedicated tools over generic shell commands for inspection tasks.
5. Use grep_files to narrow the search space before repeated read_file calls when possible.
6. Use list_dir to map directories, not as a substitute for code reading.
7. Do not repeat an equivalent read-only tool call unless parameters changed materially or you are verifying newly changed state.
8. Separate verified facts from inference in the final answer.
9. If evidence is insufficient, say what is still unverified instead of filling the gap with a confident guess.
```

#### C. Task Protocol Library

不做分类器，但在 system prompt 中内置少量高频协议：

- `Codebase Analysis Protocol`
- `Bug Investigation Protocol`
- `Implementation Planning Protocol`
- `Environment Verification Protocol`

每个协议只用 4-8 条短规则表达，避免冗长。

建议核心 prompt 文本：

```text
Task protocol rules:
1. For codebase analysis, locate structural anchors first, then inspect narrowed files, then summarize from evidence.
2. For bug investigation, reproduce or inspect the failure signal first, then trace the code path, then identify the most likely root cause.
3. For implementation planning, inspect the current code path before proposing changes; do not plan in a vacuum.
4. For environment verification, verify the real path, command, or configuration before making claims about availability or sandbox boundaries.
5. For multi-step work, prefer a narrow investigation loop: search -> inspect -> conclude, instead of broad repetitive exploration.
6. Stop exploring once the evidence is sufficient to answer the user's actual question.
```

#### D. Communication Policy

保留现有“首个工具前短更新”方向，同时强化：

- 进展更新要面向人，而不是日志
- 最终结论要标注已验证事实与推断
- 证据不足时明确说“暂时无法确认”

建议核心 prompt 文本：

```text
Communication rules:
1. Before the first tool call, give one short sentence about what you are about to verify.
2. During work, only give progress updates when you found something load-bearing, changed direction, or enough time passed without visible progress.
3. Keep progress updates concrete and human-readable, not like log lines.
4. In the final answer, lead with the conclusion, then summarize the strongest evidence.
5. Explicitly distinguish verified findings from inference when the task is analytical.
6. If you could not verify something, say so directly and name the missing check.
```

### 6.3 Codebase Analysis Protocol（重点）

针对你这次失败样例，新增如下协议：

```text
When the user asks for project or codebase analysis:
1. Prefer code and repository evidence over architecture documents.
2. Start by locating structural anchors with grep_files.
3. Then inspect narrowed files with read_file.
4. Use list_dir only to map directories, not as a substitute for code reading.
5. Do not repeat an equivalent read-only tool call unless scope changed materially.
6. Treat README, ARCHITECTURE, MODULES, and design docs as secondary evidence.
7. In the final answer, separate "verified from code" from "inferred from docs or naming".
```

这不会替代模型决策，但会明显提升“按代码分析”的稳定性。

### 6.4 Static Core 落地方式

Static Core 不应继续零散地分布在多个弱字符串里，而应在服务端形成 4 个显式 section：

- `IdentityAndMission`
- `EvidenceAndToolingPolicy`
- `TaskProtocolLibrary`
- `CommunicationPolicy`

它们都属于代码内置的 static prompt，不依赖 workspace 文件内容。

补充约束：

- `Codebase Analysis Protocol` 的 7 条规则应并入 `TaskProtocolLibrary` section 内部，不作为第 5 个独立 system section 注入。
- 这样可以避免 `TaskProtocolLibrary` 与 `Codebase Analysis Protocol` 在实现阶段重复装配或顺序冲突。

### 6.5 旧 prompt 常量迁移策略

当前 [planner.rs](/Users/xujiang/data/ailabs/talos/talos-rs/crates/orchestration/agent-kernel/src/planner.rs:1) 中已有：

- `TOOL_USAGE_GUIDANCE`
- `USER_FACING_PROGRESS_GUIDANCE`

它们与新的：

- `Evidence And Tooling Policy`
- `Communication Policy`

存在高度重叠。

本轮必须采用**替换迁移**，而不是并存叠加：

1. 阶段 2 完成后，`TOOL_USAGE_GUIDANCE` 的内容应被新的 `Evidence And Tooling Policy` 吸收并替代
2. 阶段 2 完成后，`USER_FACING_PROGRESS_GUIDANCE` 的内容应被新的 `Communication Policy` 吸收并替代
3. `planner.rs` 不应继续保留两套平行约束文本
4. 若保留常量名用于兼容内部调用，也应改为引用新 Static Core section 的统一内容，而不是继续维护旧文案

目标是：

- 模型最终只收到一套工具哲学约束
- 模型最终只收到一套用户可见沟通约束
- 降低 prompt 冲突与调参复杂度

---

## 7. Workspace Bootstrap 文件治理

### 7.1 核心结论

`~/.talos/workspace-talos` 不应该继续维护 8 个默认且都为占位模板的 bootstrap 文件。  
默认只保留 **3 个核心文件**，其余文件改为“可选存在即加载”，项目级上下文统一使用项目根 `AGENTS.md`。

### 7.2 新的文件模型

| 文件 | 位置 | 是否默认存在 | 维护者 | 作用 |
|------|------|-------------|--------|------|
| `USER.md` | `~/.talos/workspace-talos/USER.md` | 是 | 用户主导，模型可建议补充 | 用户画像、协作偏好、长期沟通偏好 |
| `MEMORY.md` | `~/.talos/workspace-talos/MEMORY.md` | 是 | 模型主导维护 | durable memory 索引与稳定事实 |
| `SOUL.md` | `~/.talos/workspace-talos/SOUL.md` | 是 | 用户或模型共同维护 | Talos 的长期风格、气质、人格补充 |
| `IDENTITY.md` | `~/.talos/workspace-talos/IDENTITY.md` | 否，可选 | 用户或模型按需维护 | Talos 身份、使命、长期产品定位补充 |
| `HEARTBEAT.md` | `~/.talos/workspace-talos/HEARTBEAT.md` | 否，可选 | 用户或模型按需维护 | 长任务汇报、周期性确认、主动跟进策略补充 |
| `AGENTS.md` | `<project_root>/AGENTS.md` | 否，项目自有 | 项目维护者主导 | 当前项目的本地规则、工程约束、协作边界 |

### 7.2.1 `project_root/AGENTS.md` 的加载路径

这是本轮必须明确的实现点。

#### 输入来源

- Embedded / local 模式下，`project_root` 由当前会话上下文中的 `project_root` 字段提供
- 该字段语义上来自 TUI/CLI 当前目录，或用户显式指定的项目路径
- Remote 模式下，`project_root` 来自服务端确认后的项目根目录，而不是客户端本地 cwd

#### 与 `directory` 的关系

当前代码里已经存在两套独立语义：

- `TalosSessionEnvironment.directory`
- `TalosContext.project_root` / session context `project_root`

设计上应明确：

- `directory` 是客户端启动目录 / 会话环境目录，用于展示和环境上下文
- `project_root` 是项目根目录语义，用于项目级规则加载和项目类工具默认根目录

因此本轮不建议把 `directory` 重新解释为 `project_root`，而是直接复用现有强类型 `project_root` 字段。

#### 加载逻辑

1. 如果 `project_root` 存在：
   - 直接尝试加载 `<project_root>/AGENTS.md`
2. 如果文件存在且非空：
   - 作为 stable context 的项目级 section 注入
3. 如果文件不存在：
   - 静默跳过，不回退到全局 `~/.talos/workspace-talos/AGENTS.md`
4. 不再为全局 workspace 自动创建占位 `AGENTS.md`

#### 与全局 bootstrap 的关系

- `USER.md` / `MEMORY.md` / `SOUL.md` 属于全局稳定上下文
- `<project_root>/AGENTS.md` 属于项目级上下文
- 两者同时存在时：
  - 全局文件描述长期身份、用户偏好、长期记忆
  - 项目级 `AGENTS.md` 描述当前仓库特有规则
- 两者不冲突，不合并成单一文件

### 7.3 调整默认 bootstrap 文件

以下文件不再作为“默认自动创建的 bootstrap 模板”：

- `TOOLS.md`
- `BOOTSTRAP.md`

它们的职责迁移如下：

| 旧文件 | 去向 |
|--------|------|
| `AGENTS.md` | 保留，但统一按项目根真实文件语义处理，不再使用全局 workspace 占位模板 |
| `SOUL.md` | 保留为默认核心文件；基础风格并入代码内置 `Identity And Mission` / `Communication Policy`，文件提供长期风格补充 |
| `TOOLS.md` | 并入代码内置 `Evidence And Tooling Policy` |
| `IDENTITY.md` | 保留为可选增强文件；基础身份并入代码内置 `Identity And Mission` |
| `BOOTSTRAP.md` | 并入服务端 session initialization 行为，不再让用户填写 |
| `HEARTBEAT.md` | 保留为可选增强文件；基础 progress policy 并入代码内置 communication / progress policy |

### 7.4 为什么这样更合理

1. 用户真正愿意长期维护的核心文件通常很少，默认强依赖应收敛到：
   - “我是谁、我喜欢怎么协作”
   - “哪些事情值得长期记住”
   - “Talos 应该以什么气质和方式长期陪伴我协作”

2. 工具哲学、基础输出风格、基础 heartbeat 策略，本质上属于产品行为设计，不该转嫁给用户维护 markdown。

3. `SOUL.md` 对“智能管家”产品尤其重要，适合作为默认强依赖文件；`IDENTITY.md` / `HEARTBEAT.md` 仍保留为可选增强文件。

4. 仓库本地规则更适合放在项目自身的 `AGENTS.md` / `README.md` 中，而不是写进全局 Talos workspace。

---

## 8. 基于当前历史信息的内容归档建议

### 8.1 应归入 `USER.md`

从当前对话和已有文件看，以下内容适合长期保留在 `USER.md`：

- 用户是资深研发工程师
- 主要语言偏好是中文
- 能看英文，但不接受被“糊弄”
- 偏好直接、严谨、少废话的协作方式
- 偏好 Vim
- 长期目标是在构建通用人工智能产品 Talos，方向是类似 Jarvis 的智能管家/伙伴

### 8.2 应归入项目根 `AGENTS.md`

对于 Talos 这个长期项目，以下更适合进入项目级上下文，而不是 `USER.md`：

- 项目当前采用纯模型驱动路线
- 不引入 intent classifier
- Prompt 统一由 `PromptBuilder` 构建
- `general_turn` 是统一核心语义
- 配置对象优先透传
- 不写 fallback，异常直接暴露
- 架构与实现以最佳方案优先，不为历史兼容妥协

注意：这些很多已经存在于仓库根 [AGENTS.md](/Users/xujiang/data/ailabs/talos/AGENTS.md:1)，因此项目级上下文统一优先走 repo-local `AGENTS.md`，不再新增 `PROJECT.md` 这个文件类型。

### 8.3 应保留在 `MEMORY.md`

适合 durable memory 的内容包括：

- 用户长期偏好
- 项目长期目标
- 稳定约束
- 已确认的重要决策
- 对未来会话仍然有帮助的协作事实

### 8.4 不应进入 `MEMORY.md`

以下内容应明确排除：

- close flush 标记
- session id
- “本次会话将在关闭时刷新”
- 某次工具运行成功/失败的瞬时状态
- 临时目录、临时 token、临时环境检查结果
- 没有形成可复用结论的中间探索过程

---

## 9. 长期记忆模型优化

### 9.1 记忆分层

Talos 应明确区分：

```text
Persistence
├── Durable Memory
│   ├── USER.md
│   └── MEMORY.md
├── Durable Persona Context
│   └── SOUL.md
├── Optional Identity Context
│   ├── IDENTITY.md
│   └── HEARTBEAT.md
├── Project Context
│   └── project-root AGENTS.md
└── Session / Task Progress
    ├── session summary
    ├── recent messages
    └── tool-use progress
```

### 9.2 Summary 持久化策略

会话级 summary 仍然保留，但改成：

- **完全模型驱动是否保存**
- 没有高价值总结则可以不存
- 输入给 summary 模型的内容必须先去除明显运行噪音

### 9.3 建议的模型决策接口

```rust
struct MemoryPersistenceDecision {
    should_persist: bool,
    summary: Option<String>,
    durability: MemoryDurability,
    reasons: Vec<String>,
}

enum MemoryDurability {
    Skip,
    SessionOnly,
    Durable,
}
```

### 9.4 决策规则

由模型判断：

- 这段信息是否只对当前会话有用
- 是否对未来会话仍有价值
- 是否只是运行噪音
- 是否属于长期偏好、长期目标、稳定事实、稳定协作约束

服务端只做输入净化，不做业务规则硬拦截。

### 9.5 Memory 决策 LLM Prompt

这是本轮必须补齐的核心设计。

#### 模型职责

给定一轮或多轮会话产物的净化摘要，判断：

- 是否值得持久化
- 只应该保存为 session summary，还是 durable memory
- 如果需要持久化，输出高度压缩、去噪后的 summary

#### 建议 prompt 结构

System prompt:

```text
You are deciding whether conversation artifacts are worth persisting.

Your job is to classify the value of the provided artifacts into one of:
- skip: not worth storing
- session_only: useful for the current session, but unlikely to matter in future sessions
- durable: likely useful in future sessions as a stable preference, long-term goal, durable project fact, or recurring collaboration rule

Persistence rules:
1. Skip temporary runtime noise, session bookkeeping, flush markers, transient ids, and one-off execution chatter.
2. Skip summaries that contain no durable fact, no stable preference, no meaningful project decision, and no reusable collaboration rule.
3. Choose session_only for progress that matters within the current session but is unlikely to help in a later session.
4. Choose durable only for information that will likely matter again in future conversations.
5. Durable summaries must be compact, factual, and free of temporary operational detail.
6. If uncertain, prefer session_only over durable, and prefer skip over low-value storage.
```

User prompt:

```text
Artifacts to evaluate:
{{sanitized_artifacts}}

Return JSON:
{
  "should_persist": boolean,
  "durability": "skip" | "session_only" | "durable",
  "summary": string | null,
  "reasons": [string]
}
```

#### Durable 判断样例

应该 `durable`：

- 用户长期偏好
- 用户明确要求系统以后记住的事项
- 项目长期目标或已确认的重要架构决策
- 稳定的协作规则
- 未来会话仍高概率有用的事实

应该 `session_only`：

- 当前调试阶段的中间进展
- 当前任务已确认但只对本轮实现有用的步骤状态
- 需要在同一会话稍后继续使用，但未来会话不一定有用的信息

应该 `skip`：

- close flush 标记
- session id / invocation id
- 临时目录与临时权限状态
- 无新信息的执行摘要
- 工具调用噪音
- 纯运行态描述，没有形成结论

#### 模型选择

建议使用同 provider 下的轻量 judge 模型，而不是主对话模型。

原因：

- 这类分类任务结构化、低创造性
- 成本更低
- 行为更容易稳定

### 9.6 `sanitize_for_memory_decision` 净化规则

在调用 judge 模型前，必须先做结构化净化，避免把明显噪音原样喂给模型。

#### 输入形式

建议输入给 judge 的不是原始 transcript JSON，也不是原始 tool payload，而是**结构化净化后的文本摘要**。

推荐结构：

```text
User intent:
- ...

Verified outcomes:
- ...

Stable facts discovered:
- ...

Candidate preferences or durable rules:
- ...

Ignored runtime noise:
- 省略，不传给 judge
```

这个结构应由**代码侧**从强类型 turn/session/tool 结构中提取并填写，而不是先调用另一个 LLM 做预处理。

原因：

- 净化本身是稳定的结构化裁剪任务
- 若再引入一个预处理 LLM，会增加成本、延迟和行为不稳定性
- 代码提取后再交给 judge，更容易做可测试断言

#### 应删除的内容

进入 judge 前应剔除：

- `session_id`
- `invocation_id`
- `message_id`
- `trace_id`
- `tool_call_id`
- 原始 tool call JSON body
- 原始 tool result 大块 JSON
- flush on close / close-flush 等字面量
- 临时目录、临时 token、临时权限提示
- 纯 lifecycle 事件（queued/executing/completed）

#### 应保留的内容

应保留并提炼为简短文本：

- 用户明确表达的长期偏好
- 用户明确要求未来记住的事项
- 任务中形成的稳定项目事实
- 已确认的重要架构决策
- 稳定协作规则

#### 净化策略

优先采用**结构化字段过滤 + 少量规则清洗**，而不是只靠正则全文硬删。

建议顺序：

1. 先从强类型 turn/session/tool 结构中提取有意义字段
2. 再对提取后的文本做少量规则清洗
3. 最终拼成 judge 可读的短文本块

这样比“把整段 transcript 丢给正则处理”更稳定。

#### 代码侧字段映射建议

建议由 `TurnArtifacts` 映射到以下 judge 输入字段：

- `User intent`
  - 来自当前用户输入或本轮任务目标的归一化文本
- `Verified outcomes`
  - 来自成功工具结果中已被归并为事实的执行结果
- `Stable facts discovered`
  - 来自本轮形成的稳定事实、确认结论、约束变化
- `Candidate preferences or durable rules`
  - 来自用户明确偏好、长期目标、协作规则、要求记住的事项

若某字段无内容，可省略整个段落，不必填空占位。

---

## 10. Prompt 装配与加载路径

```rust
fn assemble_prompt(ctx: TurnContext) -> PromptAssembly {
    let system_sections = vec![
        build_identity_and_mission(),
        build_evidence_and_tooling_policy(),
        build_task_protocol_library(),
        build_communication_policy(),
    ];

    let mut context_sections = vec![
        load_user_md(),
        load_memory_md(),
        load_soul_md(),
        load_optional_identity_md(),
        load_optional_heartbeat_md(),
        load_project_agents_md(ctx.project_root),
        load_session_summary(ctx.session_id),
        load_recent_messages(ctx.session_id),
        load_retrieved_memories(ctx),
    ];

    PromptAssembly {
        system_sections,
        context_sections: context_sections
            .into_iter()
            .flatten()
            .collect(),
        user_input: ctx.user_input,
    }
}
```

说明：

- 伪代码继续对齐现有 `PromptAssembly { system_sections, context_sections, user_input }`
- stable 与 dynamic 在实现层仍然应分别建 section、分别赋予 tier / cache scope / truncation policy
- 这里只是简化展示最终装配结果，不表示两者应在预算语义上混为一层

### 10.1 Summary 持久化伪代码

```rust
async fn maybe_persist_session_summary(turn: TurnArtifacts) {
    let sanitized = sanitize_for_memory_decision(turn);
    let decision = summary_llm.decide(sanitized).await?;

    if !decision.should_persist {
        return;
    }

    match decision.durability {
        MemoryDurability::Skip => {}
        MemoryDurability::SessionOnly => save_session_summary(decision.summary).await?,
        MemoryDurability::Durable => save_durable_memory(decision.summary).await?,
    }
}
```

其中 `sanitize_for_memory_decision(turn)` 应直接复用本节 9.6 的规则，输出面向 judge 的短文本摘要，而不是原始 transcript。

---

## 11. 模块影响面

| 模块 | 影响 |
|------|------|
| `talos-rs/crates/common/prompt` | 重构 system prompt 分层、bootstrap 文件装配逻辑 |
| `talos-rs/crates/orchestration/agent-kernel/src/planner.rs` | 强化工具哲学、任务协议、进展与结论规则，并迁移旧 prompt 常量 |
| `talos-rs/crates/cognition/memory` | 引入 summary 持久化筛选与 durable/session 分层 |
| `talos-rs/crates/action/tools` | 对齐 Claude Code 的工具 description 风格，补强 Read/Grep/exec_command 等关键工具的说明 |
| `~/.talos/workspace-talos` | 精简默认 bootstrap 文件，核心保留 `USER.md` / `MEMORY.md` / `SOUL.md`，其余改为可选增强 |
| `<project_root>/AGENTS.md` | 作为项目级上下文主入口 |

---

## 12. 验收标准

### 12.1 行为目标

以下问题应表现明显改善：

1. “分析当前项目架构，实际用 grep_files 看代码，输出结论，而不是看文档直接得出结论”
2. “先用 grep_files，再按代码证据总结”
3. “不要重复读同一个文件和重复搜同一个 pattern”
4. “如果代码证据不足，就明确说暂时无法确认”

### 12.1.1 可量化回归用例

针对输入：

```text
分析当前项目架构，实际用 grep_files 看代码，输出结论，而不是看文档直接得出结论
```

回归期望：

1. 首批工具调用中至少包含一次 `grep_files`
2. 不允许连续两次完全相同参数的 `grep_files`
3. 不允许连续两次完全相同路径的 `read_file`
4. 若读取了 `ARCHITECTURE.md`，最终回答中必须明确将其表述为二手证据或补充参考，而非主要依据
5. 最终结论中必须出现类似“基于代码可验证”与“推断/补充参考”的区分语义

如果现有 e2e 框架暂不支持机械断言工具序列，则本条至少应作为 `tmux` 手工验证清单执行；若框架支持 transcript/tool event 断言，应优先自动化。

### 12.2 Memory 目标

- 不再把 flush/session id 之类信息写进 durable memory
- 没有价值的 summary 可以跳过
- `USER.md`、`MEMORY.md`、`SOUL.md` 成为真正长期有效的核心上下文

### 12.3 Bootstrap 目标

- 默认自动创建的 bootstrap 文件数量显著减少
- 用户无需维护大量模板 markdown
- 全局 workspace、可选增强文件与项目级 `AGENTS.md` 边界更清晰

---

## 13. 推荐实施顺序

1. 先实现 Static Core 的 4 个 prompt 组件，优先补强 `Evidence And Tooling Policy` 与 `Task Protocol Library`
2. 同步迁移并删除旧 `TOOL_USAGE_GUIDANCE` / `USER_FACING_PROGRESS_GUIDANCE` 文案，确保只保留一套约束
3. 再精简 bootstrap 文件模型与默认模板
4. 再实现 `project_root/AGENTS.md` 的加载路径语义，直接复用现有强类型 `project_root`
5. 再做 memory summary 持久化筛选、`sanitize_for_memory_decision` 与 judge prompt
6. 最后用真实 TUI / e2e 回归验证失败样例

---

## 14. 一句话总结

Talos 下一阶段的核心不是“再多几个工具提示词”，而是把它升级成：

- **代码证据优先**
- **专用工具优先**
- **长期身份明确**
- **bootstrap 极简但高价值**
- **summary 持久化完全模型驱动**

的长期智能管家系统。
