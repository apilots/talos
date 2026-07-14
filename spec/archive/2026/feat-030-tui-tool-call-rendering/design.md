# Talos TUI 工具调用实时渲染方案

Status: archived
Archived: 2026-07-14
Owner: agent
Created: 2026-07-06
Updated: 2026-07-06

## 背景与问题

当前 TUI 已经能展示工具调用，但在连续执行、长输出和编辑类工具场景下存在几个明显问题：

- 工具执行过程的实时反馈不够像连续 transcript，用户需要在多个块之间推断当前状态。
- `Working...` 和扫描条样式视觉噪声较高，也不符合参考风格。
- 工具调用前缺少简短行动说明，用户不容易理解下一批工具为什么被调用。
- `Edit` / `ApplyPatch` 如果只显示文件列表，会丢失用户最关心的实际修改内容。
- 不同工具类型当前容易走统一文本摘要，无法充分利用 TUI 已经接收到的结构化输出。
- 最终总结如果被 Markdown renderer 重新排版，会破坏终端 transcript 中用户可复制、可审查的原始 message text，包括 Markdown 字符、ASCII 图、表格和缩进结构。

本方案定义 Talos TUI 中工具调用执行过程的目标展示方式。核心目标是让执行过程像终端 transcript 一样连续、简洁、可追踪。

当前设计稿位置：

- `design/talos-tui-tool-call-display.html`

## 目标

- 每批工具调用前先输出一句简短说明，解释为什么要调用这些工具。
- 工具调用本身保持单行头部 + 少量证据行，不做卡片化展示。
- 执行中状态实时反馈，但不刷屏。
- `Edit` / `ApplyPatch` 必须完整展示每个文件的修改内容。
- 按 TUI 当前实际收到的 `TalosContentItem` 类型渲染，不照搬参考图里 Talos 不支持的工具。
- 最终总结直接按模型输出的原始 message text 展示，不做 Markdown 结构化渲染。
- 不改变现有输入框、输入框上下紧挨的布局和底部 model/context 行。

## 非目标

- 不重做输入框。
- 不改变输入框上方 metrics/runtime 行、输入框、底部 context 行的基础结构。
- 不引入新的 TUI 渲染依赖。
- 不改后端代码。
- 不改变后端工具调用、权限、动态工具请求、stream event 协议或 API。
- 不改变 `TalosContentItem` wire shape。
- 不把 `Edit` / `ApplyPatch` 简化成只有文件列表的摘要。
- 不改 Web UI。

## 当前实现事实

实现只落在 TUI 展示层和 TUI 现有 stream event 消费链路中，不进入后端工具执行、协议、SDK 或 Web UI。

主要相关文件：

- `talos-rs/apps/tui/src/render/ui.rs`
- `talos-rs/apps/tui/src/render/input_lines.rs`
- `talos-rs/apps/tui/src/render/tool_lines.rs`
- `talos-rs/apps/tui/src/stream_events.rs`
- `talos-rs/apps/tui/src/chat.rs`
- `talos-rs/apps/tui/src/types.rs`

TUI 已经能接收结构化工具内容，渲染应优先基于这些类型，而不是解析拼接后的文本：

- `Text`
- `Json`
- `FileRead`
- `Diff`
- `SearchResults`
- `BashOutput`
- `WebReference`
- `ArtifactRef`
- `McpResourceRef`
- `Progress`

## 后端边界评估

本轮不改后端代码。

判断依据：

- 目标主要是 TUI 的展示策略变化：工具头部、证据行、折叠/展开、执行中提示、copy projection。
- TUI 已经能收到 Bash、Diff、FileRead、SearchResults、JSON、引用和进度等内容。
- `Edit` / `ApplyPatch` 的完整 diff 应从 TUI 已收到的 `Diff` 或文本输出中渲染，不要求后端重新生成终端展示文本。
- 工具调用前的简短说明来自 `AssistantText/Commentary`，不要求后端工具运行时增加新语义。

如果现有内容字段不足，TUI 只做降级展示：

- 拿不到 exit code 时，显示可用 stdout/stderr 证据，不新增后端字段。
- 拿不到精确新增/删除统计时，从已有 diff 文本或内容行在 TUI 侧计算。
- 拿不到结构化匹配数时，显示原始搜索输出的关键行和展开入口。
- 拿不到动态输入的细分状态时，沿用现有工具状态，不新增协议。

## 总体展示模型

一轮对话按连续 transcript 展示：

1. 用户输入灰色横条。
2. 折叠的 thinking 行。
3. 工具调用前的 commentary 简短说明。
4. 一个或多个工具调用。
5. 执行中的推理状态行。
6. 完成态和最终回复。

最终回复展示时保留原始 message text，不把标题、列表、表格、代码块、ASCII 图或 box drawing 重新转换成 TUI Markdown 组件。

目标形态：

```text
› 重新设计 TUI 里工具调用执行过程的实时展示

∴ Thinking (ctrl+o to expand)

● 我先确认 Talos 当前 TUI 的渲染入口和后端真实工具输出模型，避免只照搬外观。

● Read(talos-rs/apps/tui/src/render/ui.rs)
  ⎿ Read 260 lines (ctrl+o to expand)

● Grep(TalosContentItem|ToolCallStatus)
  ⎿ Found 41 matches across 9 files (ctrl+o to expand)

✻ Brewing...
```

## 工具调用前说明

每批工具调用前显示一句简短说明，表达当前行动意图。

规则：

- 一句话为主，最多两行终端换行。
- 作为 `Commentary` transcript 文本展示，使用低调圆点。
- 不标记为工具，不做卡片。
- 不重复完整计划，只说明下一批工具调用的目的。

示例：

```text
● 我先确认 Talos 当前 TUI 的渲染入口和后端真实工具输出模型，避免只照搬外观。
● 接下来我对照 Claude Code 的终端实现，重点看它如何把说明、工具批次、执行中状态、完成摘要拆开。
● 现在我把这个模式套回 Talos：Read/Grep/Glob 保持逐个工具渲染，Bash/TaskOutput 保留尾部输出，Edit 展示完整 diff。
```

## 工具头部展示

工具调用头部使用单行：

```text
● Bash(pnpm --dir talos-app/web test -- Sidebar SettingsProfileSection)
● Read(talos-rs/apps/tui/src/render/tool_lines.rs)
● Grep(TalosContentItem|ToolCallStatus|input_status_line)
● Edited 6 files (+6 -19)
```

规则：

- 左侧固定一个状态点。
- 工具名和紧凑输入放在同一行。
- 超长输入按可视宽度截断。
- 不再使用“工具名一行 + summary 一行 + details”的重型结构。

状态点映射：

- 排队：灰色或蓝色弱化圆点。
- 运行中：红色或强调色闪烁圆点。
- 等待动态输入：黄色圆点。
- 成功完成：绿色圆点。
- 失败、取消、超时：红色圆点。

## 工具内容展示

工具内容使用 `⎿` 风格证据行。

默认折叠形态：

```text
  ⎿ Read 260 lines (ctrl+o to expand)
```

多行证据：

```text
  ⎿ PASS src/features/settings/components/SettingsProfileSection.test.tsx
    PASS src/components/Sidebar.test.tsx
    +23 lines (ctrl+o to expand)
```

规则：

- 非编辑类工具默认只显示一到两行核心证据。
- 隐藏内容统一通过 `ctrl+o to expand` 展开。
- 短输出可以直接显示，长输出不直接铺满 scrollback。
- 优先使用结构化字段，不从展示文本反向解析。

## 长任务流式交替渲染

长任务执行过程中会持续交替输出 commentary message、tool call 和 tool result。TUI 必须按真实事件顺序连续渲染，不把多个 Read/Grep/Glob 或多批工具调用合并成一个大块。

目标形态：

```text
● 我先看 TUI 渲染入口和工具内容模型。

● Read(talos-rs/apps/tui/src/render/tool_lines.rs)
  ⎿ Read 360 lines (ctrl+o to expand)

● 这里已经有 typed content 分发，我继续确认 stream event 如何进入 chat block。

● Grep(TalosContentItem|ToolCallStatus)
  ⎿ Found 41 matches across 9 files (ctrl+o to expand)

● 现在对照输入框和 runtime 行，避免改动用户输入区域。

● Read(talos-rs/apps/tui/src/render/input_lines.rs)
  ⎿ Read 214 lines (ctrl+o to expand)
```

规则：

- 每个工具调用都有独立头部和独立证据行。
- commentary 简短说明按模型输出位置插入 transcript，不后移、不合并。
- thinking/activity 行可以显示历史累计摘要，但不能替代主 transcript 中的单个工具结果。
- 只有同一个工具自身的增量输出可以更新同一个工具块，例如 Bash stdout、TaskOutput 轮询、Progress 更新。
- 不把连续 Read/Grep/Glob 合并成一个“探索块”，否则长任务过程会失去实际执行顺序。

## Thinking 展示

折叠 thinking：

```text
∴ Thinking (ctrl+o to expand)
```

聚合 activity：

```text
Thought for 12s, read 5 files, searched for 4 patterns (ctrl+o to expand)
```

规则：

- 默认折叠。
- 能聚合时显示耗时、读取文件数、搜索次数、目录列表次数等摘要。
- 完整 thinking 只在 transcript、verbose 或展开模式显示。

## 执行态展示

不使用 `Working...`。

目标形态：

```text
✻ Brewing...


live · 4 rounds · 3 tools · 3 ok · 0 err · 34.3k tokens · 7.5s
```

规则：

- transcript 尾部保留旋转雪花和彩色光影，表达当前 turn 仍在执行。
- runtime metrics 行和 `✻ Brewing...` 之间保留两个空行，不粘在一起。
- 底部 context 行不重复显示 `✻ Brewing`。
- 具体工具运行状态由工具块自身表达，例如 `✻ Bash(...)` + `⎿ Running...`。

## 最终总结展示

最终总结直接按模型输出的原始 message text 展示，不做 Markdown 结构化渲染。这里的“原始 message text”包括 Markdown 字符、普通纯文本标题、`---` 分隔线、ASCII 图、box drawing、表格、缩进代码和链接文本。

目标形态：

```text
深度探索完成。以下是完整的分析报告。

---

Prompt Build 深度分析

一、整体管线：从用户输入到 API 调用

用户在 agent-kernel 中触发一个 turn
        │
        ▼
┌───────────────────────────────────────────────────────┐
│  general_turn/prepare.rs (38k)                        │
│  ┌─────────────────────────────────────────────────┐  │
│  │ 1. 加载 session_continuity（上轮残留状态）       │  │
│  │ 2. build_prompt_builder()                       │  │
│  │ 3. rebuild_prompt_assembly()                    │  │
│  └─────────────────────────────────────────────────┘  │
└───────────────────────────────────────────────────────┘

二、Builder 设计解析

PromptBuilder 是典型的 Fluent Builder，但有几个值得关注的设计决策：

1. 引用生命周期而非所有权
   pub(crate) tool_registry: Option<&'a dyn ToolDefinitionProvider>,

2. workspace/project 两级 bootstrap 文件分层
   - Workspace 层：SOUL, IDENTITY, USER, HEARTBEAT, MEMORY
   - Project 层：仅 AGENTS.md

┌──────────────────────────┬──────┬────────────────────────────────────┐
│         测试模块         │ 行数 │              覆盖重点              │
├──────────────────────────┼──────┼────────────────────────────────────┤
│ builder_tests.rs         │ 387  │ Builder fluent API、工具集成       │
│ session_history_tests.rs │ 509  │ History 压缩、compact_cursor 过滤  │
└──────────────────────────┴──────┴────────────────────────────────────┘

✻ Cooked for 51s
```

规则：

- `AssistantText/FinalAnswer` 的 delta 如果提前到达，TUI 先按 item 缓存，不立刻落屏。
- 收到明确的 `AssistantTextCommitted(FinalAnswer)` 或 item completed final content 后，再打开 `FinalAnswer` block 并输出缓存内容。
- final block 打开后，后续 final delta 继续增量追加。
- completed final content 作为 canonical 文本；如果缓存 delta 少于 completed 内容，TUI 补齐缺失尾部。
- 保留原始 `#`、`##`、`-`、`1.`、`---`、代码围栏、表格、ASCII 图、box drawing、缩进和链接文本。
- 如果模型输出 `## 总结`，TUI 显示 `## 总结`；如果模型输出 `总结`，TUI 不补 `##`。
- 不把 Markdown 标题改成 TUI 标题样式。
- 不把列表重新缩进成自定义 bullet 组件。
- 不把 `---` 替换成终端横线组件。
- 不对 ASCII 图、box drawing 或缩进代码做结构重排。
- 不在总结里做语法高亮或富文本排版。
- 允许基本 wrapping，但 wrapping 不能改变原始字符；对 ASCII 图和表格应尽量保持原行，避免破坏布局。
- final answer 不加圆点、不加角色标题，copy projection 必须复制同一份原始 message text。

## Bash / TaskOutput 展示

`BashOutput` 使用 shell progress 风格。

运行中无输出：

```text
✻ Bash(just test -p talos-auth)
  ⎿ Running... 7.0s
```

运行中有输出：

```text
✻ Bash(pnpm --dir talos-app/web test -- Sidebar SettingsProfileSection)
  ⎿ Running... stdout: PASS src/features/settings/components/SettingsProfileSection.test.tsx · 7.5s
```

失败：

```text
● Bash(pnpm --dir talos-app check:repo)
  ⎿ exit 1 File exceeds legacy line baseline: src/shared/i18n/resources.ts (1116)
    scoped tests passed; repo baseline issue remains unrelated
```

规则：

- 默认展示最后几行关键输出，而不是完整 stdout。
- 失败时必须显示 exit code 和关键 stderr。
- 有耗时时显示耗时。
- 有截断时显示截断提示。
- 完整 stdout/stderr 保留在展开视图中。

`TaskOutput` 复用 Bash 输出风格，但要明确轮询续接：

```text
● TaskOutput(task_id=42)
  ⎿ streaming stdout: compiling talos-app-protocol v0.1.3
    next poll continues from offset 8192
```

## Read / Grep / Glob 展示

Read：

```text
● Read(talos-rs/apps/tui/src/render/tool_lines.rs)
  ⎿ Read 360 lines, found typed content dispatch
```

Grep：

```text
● Grep(TalosContentItem|ToolCallStatus|input_status_line)
  ⎿ Found 41 matches across 9 files (ctrl+o to expand)
```

Glob：

```text
● Glob(**/*tool*.rs)
  ⎿ Found 37 paths (ctrl+o to expand)
```

规则：

- 使用结构化输出中的路径、行范围、命中数、文件数、more/partial 标记。
- 连续 Read/Grep/Glob 必须分开渲染为独立工具行，保持真实执行顺序。
- thinking/activity 行可以补充累计摘要，例如 `read 11 files, searched for 4 patterns`，但不能替代单个工具结果。
- 每个工具行都保留自己的证据行，方便用户审查具体输入和结果。

## Edit / ApplyPatch 展示

编辑类工具必须在 TUI 中完整显示每个文件的修改内容。

目标形态：

```text
● Edited 6 files (+6 -19)
  ⎿ talos-app/web/src/features/connection/account-type.ts (+1 -1)
    1 -export { formatAccountType, type AccountType } from "@/shared/types/account";
    1 +export type { AccountType } from "@/shared/types/account";

  ⎿ talos-app/web/src/shared/types/account.ts (+1 -12)
    1 -export type AccountType = "free" | "plus" | "pro" | "max";
    1 +export type AccountType = string;
```

规则：

- 展示每个被修改文件。
- 展示每个文件中所有实际新增/删除行。
- 未变化上下文可以折叠。
- 删除行使用红色背景，新增行使用绿色背景。
- copy projection 和展开模式都必须保留完整 diff。
- 不能只显示文件列表摘要。

## Reference / JSON 展示

Web 引用：

```text
● WebSearch(Talos API schema)
  ⎿ docs.rs/talos-types API reference snippet
```

MCP 资源：

```text
● ReadMcpResource(server:uri)
  ⎿ application/schema resource
```

JSON：

```text
● ToolSearch(MCP resource tools)
  ⎿ activated_tools: list_mcp_resources, read_mcp_resource
    unavailable_tools: 0
```

规则：

- 优先显示人能读懂的 title、URL、resource URI、label 或关键 JSON 字段。
- 完整 JSON 只在展开模式显示。
- 默认不直接铺大型 JSON。

## AskUserQuestion 展示

动态输入请求必须显示为等待状态：

```text
● AskUserQuestion
  ⎿ awaiting_user_response: accept the enum wire breaking change?
```

规则：

- 使用等待/动态输入状态颜色。
- 必须显示实际问题。
- 动态请求解决前，turn 不应显示为完成。

## Runtime / Footer 行

输入框上方保留 runtime/metrics 行。

执行中：

```text
✻ Brewing...


live · 2 rounds · 10 tools · 8 ok · 1 err · 21.4k tokens · 28.0s
```

完成：

```text
✻ Brewed for 1s
```

规则：

- 不使用 `Working...`。
- 该行和输入框保持分离。
- 不改变输入框行为。
- 底部 model/context 行保持现有位置。

## Copy Projection

复制 transcript 时应保留有效信息：

- 保留工具名、紧凑输入、证据行、diff 行。
- 不复制动画状态。
- 不复制纯视觉装饰。
- 编辑类工具复制时必须保留完整 diff。

## 受影响区域

- TUI transcript 渲染。
- TUI 输入框上方 runtime/metrics 行。
- TUI copy projection。
- TUI snapshot 或等价视觉测试。

## 验证策略

影响该方案的实现必须补 TUI snapshot 或等价视觉测试。

最低覆盖：

- Bash 运行中无输出。
- Bash 运行中有尾部输出和折叠行数。
- Bash 失败，包含 exit code 和 stderr。
- TaskOutput 轮询输出。
- FileRead 折叠和展开。
- SearchResults 折叠和展开。
- ApplyPatch 多文件完整 diff。
- Json 输出折叠和展开。
- AskUserQuestion 等待输入状态。
- runtime 行执行中和完成态。
- 最终总结按原始 message text 输出，不被 Markdown renderer 改写。

## 开放风险与后续

- 超大生成文件 diff 是否需要每文件最大可见高度。
- 某些工具只有纯文本输出时，TUI 侧摘要提取可能不如结构化内容精确；本轮仍限制在 TUI 降级展示内解决。
