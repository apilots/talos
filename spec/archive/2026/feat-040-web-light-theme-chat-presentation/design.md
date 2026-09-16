# Web 白色主题与共享对话详情呈现设计

Status: archived
Owner: agent
Created: 2026-08-20
Updated: 2026-08-20
Related: `archive/2026/feat-017-web-ui-design`, `archive/2026/refactor-037-web-desktop-timeline-render-runtime`, `feat-031-profile-usage-statistics`

## 1. 结论

本设计将 Talos Web 白色主题统一为克制、轻量的工作台视觉，并精简对话历史详情页的信息层级。回答正文是每个 Turn 的第一视觉主体；执行过程继续完整可追溯，但在 Turn 完成后自动折叠。

核心决定如下：

1. 白色主题使用浅灰 Sidebar 与白色 Main 的轻微色差建立左右分区，删除页面网格、装饰光晕和重阴影。
2. 对话详情移除用户头像、Assistant 头像和 `Final answer` 装饰标题；用户消息保留轻量浅灰气泡，Assistant 回答直接渲染正文。
3. 现有 Timeline 是独立组件，其内部结构、图标、连接线、Tool Call、间距、展开内容和既有视觉形态不做任何修改。
4. Timeline 外层的 Process disclosure 负责 `Working/Worked`、耗时和展开状态。执行中默认展开，完成后自动折叠，用户仍可重新展开现有 Timeline。
5. Turn Metrics 不放入 Timeline，也不放入 Process disclosure；它们在最终回答下方以轻量行内 Footer 展示，并与复制、反馈等回答操作形成统一 Footer 区域。
6. Turn Metrics 展示 `tokens/s`、`tokens`、`rounds`、`tools` 和 `errors`；不展示 `tool_successes` 或 `Completed`。
7. `tokens/s` 使用现有单次 Turn Metrics 派生：`total_tokens / (duration_ms / 1000)`。它表示整个 Turn 的平均 Token 速率，不宣称为 Provider 模型解码速度。
8. 普通弹窗和用户交互面板删除装饰性标题图标与按钮图标，统一为标题、说明、内容和底部操作的简单布局。
9. 功能性图标继续保留；新增 Metrics 图标只使用小尺寸、低对比度的统一线性图标，不使用胶囊、独立卡片或图标底座。
10. 色彩体系只调整白色主题；对话结构、Process disclosure、Turn Metrics Footer、Composer、Modal 和图标层级调整由浅色与深色主题共享。深色主题保留现有色调，不单独重新配色。

## 2. 背景与问题

当前 Web 白色主题已经具备完整的 Sidebar、对话流、Process disclosure、Timeline、最终回答、Composer 和弹窗能力，但视觉层级仍有以下问题：

- Sidebar 与 Main 主要依赖边框区分，背景明度差不足。
- 米白背景、网格、光晕、面板阴影和大量圆角共同出现，使白色主题偏暖且卡片感过重。
- 用户头像、Process 图标、Assistant 图标、`Final answer` 标题、状态图标和操作图标在同一 Turn 内重复表达角色或状态。
- Process 完成态同时展示事件数量、状态、Rounds、Tools、Success、Errors、Tokens 和耗时，视觉权重接近最终回答。
- Turn Metrics 当前归属于 Process 区域，Timeline 折叠后难以同时满足“回答简洁”和“Metrics 可见”。
- 弹窗与交互面板存在装饰图标、较强遮罩模糊、重阴影和多层容器。
- Composer 同时强调多个配置控件，输入内容和主操作不够突出。

本设计参考现代白色生产力工具的低对比度层级和行内 Metrics 表达，但不复制外部产品结构，也不放弃 Talos 已有 Timeline 视觉与能力。

## 3. 目标

1. 建立统一、稳定、低噪声的 Web 白色主题视觉层级。
2. 让 Sidebar、Main、Composer、Modal 和消息区域通过颜色、留白与细边框自然分层。
3. 让最终回答成为 Turn 完成后的主要内容，同时保留可重新展开的完整执行过程。
4. 在回答下方持续展示本轮 Metrics，不要求用户重新展开 Process。
5. 保持现有 Timeline 组件的结构、能力和视觉形态不变。
6. 统一装饰图标与功能图标的边界，减少不必要的头像、图标底座和状态重复。
7. 保证浅色与深色主题、Desktop Web、窄屏 Web、历史恢复和流式执行使用相同的展示语义。
8. 通过组件测试、Snapshot 和真实页面视觉验证覆盖用户可见变化。

## 4. 非目标

1. 不重新设计或重写 Timeline 组件。
2. 不改变 Timeline item、Tool Call、Reasoning、Commentary 或 UserAction 的协议结构。
3. 不新增 Turn Metrics API、后端字段、Store 或统计服务。
4. 不把 `tokens/s` 定义为模型纯生成吞吐量、首 Token 延迟或 Provider 性能基准。
5. 不重新设计深色主题色板、品牌色或既有明暗关系；共享组件的结构、间距、图标与交互调整仍必须在深色主题生效。
6. 不重新设计 Settings、Files、Plugins、Memory 等业务页面的信息架构；它们只消费统一后的白色主题基础 Token。
7. 不引入新的图标库、动画库、Design System 框架或通用 Layout 框架。
8. 不重写消息排序、Timeline 恢复、Streaming、Retry 或 Resume；仅修复取消 Turn 的服务端状态落库、刷新恢复和队列暂停语义。

## 5. 当前实现事实

### 5.1 主题与布局

- 白色主题由 `talos-app/web/src/app/styles/index.css` 中的 CSS Variables 提供。
- `AppLayout` 使用固定 Sidebar 与 Main 布局；Sidebar 和 Main 当前都大量消费 `card` Surface。
- 页面背景包含 Grid、Brand glow 和 Info glow。
- `Panel` 为多个页面、Composer 和弹窗提供统一圆角、边框、半透明背景与阴影。

### 5.2 对话与执行过程

- `MessageList` 将 Timeline items 分为 Process items 和 Final Answer items。
- Process 在执行中自动展开，在完成后自动折叠，并支持用户点击重新展开。
- `TurnMetricsBlock` 当前位于 Process summary，并展示状态、Rounds、Tools、Success、Errors 和 Tokens。
- 最终回答当前包含 Assistant 图标和 `Final answer` 标题。
- Timeline 内部使用现有 Timeline rail、item icons、ToolCallBlock 和 UserAction renderer。

### 5.3 Metrics 契约

现有 `TurnExecutionMetrics` 已经提供：

```text
model_rounds
tool_calls
tool_successes
tool_errors
stop_reason
usage.input_tokens
usage.output_tokens
usage.total_tokens
usage.cache_creation_input_tokens
usage.cache_read_input_tokens
duration_ms
```

`duration_ms` 是整个 Turn 的执行时间，包含模型调用、工具执行和其他 Turn 内等待。客户端可以基于现有字段派生 Turn 平均 Token 速率，不需要新增协议字段。

## 6. 白色主题设计

### 6.1 基础颜色

以下目标颜色只适用于白色主题。最终实现允许在视觉验证中做小幅度调整，但必须保持语义关系：

| Token | Target | Usage |
| --- | --- | --- |
| Main background | `#FFFFFF` | 对话与页面主内容 |
| Sidebar background | `#FAFAF9` | 左侧导航与历史列表 |
| Secondary surface | `#F8F8F6` | 输入、Hover、次级区域 |
| Selected surface | `#ECECEA` | 选中导航与会话 |
| Border | `#E6E6E2` | 分割线和必要边界 |
| Primary ink | `#20211F` | 标题与正文 |
| Secondary ink | `#777A75` | 描述、Metadata、Metrics |
| Tertiary ink | `#969994` | Placeholder 和弱操作 |
| Brand | Existing Talos amber | 执行中、Focus、少量品牌状态 |
| Danger | Low-saturation dark red | 错误和危险操作 |

### 6.2 Surface 规则

- Sidebar 与 Main 通过背景色差分区，边框只作辅助。
- 移除 Body Grid、Brand glow 和 Info glow。
- 普通内容不使用悬浮卡片阴影。
- Composer 和 Modal 可以使用单层轻阴影，但不能同时叠加 Glow、Blur 和多层 Outline。
- 普通选中态只使用浅灰 Surface，不使用 Brand outline。
- 降低全局圆角密度；圆角只服务于可点击控件、输入容器和 Modal。

### 6.3 Timeline 兼容边界

Timeline subtree 不参与本轮组件视觉重构：

- 不修改 Timeline-owned class、DOM hierarchy、icons、rail、spacing、ToolCall rendering 或 item surfaces。
- 全局 Token 调整不能意外改变 Timeline 已有计算样式；必要时通过兼容变量保持其当前视觉值，而不是修改 Timeline 组件。
- Timeline Snapshot 必须证明内部视觉结构没有随外层设计发生非预期变化。

## 7. 页面布局

### 7.1 Sidebar

- 固定使用浅灰背景，与白色 Main 保持稳定明度差。
- 一级导航保留现有功能性图标，图标颜色统一为中性灰。
- Active item 使用浅灰背景，不使用 Brand 边框或阴影。
- Talos、New chat、一级导航、Project 与 Session 行共享相同的圆角和交互 Surface；Hover 直接使用 Selected surface，不降低透明度。
- 根级 Session 行的 Active/Hover 背景铺满侧栏滚动内容区，不为滚动条或操作图标预留额外的背景缩进。
- Project 下的 Session 只缩进文字内容，Active/Hover 背景仍铺满侧栏滚动内容区；选中子 Session 不联动选中父 Project 行。
- Project 和 Chat section header 降低字号与对比度。
- Session delete action 只在 Hover、Focus 或 Active 时显示。
- Account 区使用细分割线与普通文本，不增加独立浮层感。

### 7.2 Main 与会话 Header

- Main 使用白色背景。
- 会话正文宽度目标为 `760-820px`；Composer 可以略宽，但必须与正文中轴对齐。
- 会话 Header 只保留标题和必要操作，不重复展示模型、连接或 Turn 状态。
- Header 使用细底部分割线，不使用独立 Card。

## 8. 对话详情呈现

### 8.1 用户消息

- 右对齐浅灰气泡，最大宽度为正文区域的约 `65%-75%`。
- 移除用户 Avatar。
- 使用轻量边界或纯 Surface，不使用重阴影和 Brand glow。
- Timestamp、Copy 等辅助操作位于气泡下方或在 Hover/Focus 时出现。
- Attachment 保留必要的文件识别，不增加装饰性图标容器。

### 8.2 Assistant 回答

- 移除 Assistant Avatar。
- 移除 `Final answer` 标题和对应图标。
- Markdown 正文直接开始，并保持现有 Code、Table、List 和 Link 能力。
- Turn Metrics Footer 位于回答正文之后。
- Copy、Helpful 和 Not helpful 位于 Metrics Footer 之后，并保持更低视觉权重。

### 8.3 Turn 间距

- 同一 Turn 内的 User message、Process、Answer 和 Footer 通过一致的纵向节奏关联。
- 不使用外层 Turn Card 包裹整个对话。
- 相邻 Turn 使用留白分隔，不额外增加粗分割线。

## 9. Process Disclosure 与 Timeline

### 9.1 组件边界

```text
TurnPresentation
├── UserMessage
├── ProcessDisclosure
│   ├── ProcessSummary
│   └── ExistingTimeline
└── FinalAnswer
    ├── AnswerBody
    ├── TurnMetricsFooter
    └── ResponseActions
```

`ProcessDisclosure` 只拥有 Summary、展开状态和 Timeline slot。`ExistingTimeline` 保持现有实现与视觉。

### 9.2 执行中展开

```text
Working for 1m 22s                                      ˅
──────────────────────────────────────────────────────────

[Existing Timeline]
```

- 新 Turn 有 Process items 后默认展开。
- 耗时按秒更新。
- Summary 不展示 Activity icon、事件数量、状态 Badge 或 Metrics。
- 用户手动折叠后，普通 Timeline delta 不重新强制展开。
- WaitingUserAction 等必须处理的状态可以重新展开。

### 9.3 执行中折叠

```text
Working for 1m 22s                                      ›
──────────────────────────────────────────────────────────
```

- Timeline 继续更新，但不展示内部内容。
- Summary 必须继续更新时间，并通过 `aria-expanded` 暴露状态。

### 9.4 完成后折叠

```text
Worked for 1m 46s                                       ›
──────────────────────────────────────────────────────────

Answer body...
```

- Turn 完成后自动折叠。
- 用户可以重新展开完整 Timeline。
- 完成语义由 `Worked` 和折叠状态表达，不额外展示 `Completed`。

### 9.5 失败与取消

失败和取消使用明确文本状态：

```text
Failed after 38s                                        ›
Stopped after 24s                                       ›
```

- 异常状态可以使用克制的 Danger 色。
- 不增加大型 Alert icon、Background 或 Badge。
- 服务端接受中止请求后，必须先将对应 Active Turn 持久化为 `cancelling`，再调用运行时取消。
- `cancelling` 期间到达的普通 Timeline 事件不能把 Active Turn 回退为 `running`；只有终态事件可以结束该状态。
- `StopReason::Cancelled` 必须阻止队列自动派发，队列中的后续消息只能由用户显式启动。
- 页面刷新只恢复服务端 Active Turn 和 Timeline，不调用队列 `start`，也不把 `cancelling` 当作已完成。
- Process Summary 优先使用 Turn Metrics 判断终态；Metrics 缺失时回退到 Timeline item 的 `failed` / `cancelled` 状态。
- 展开的 Tool Call 同步显示 `Failed` 或 `Cancelled`，不能保留 `Running`。

## 10. Turn Metrics Footer

### 10.1 位置与内容

Metrics 只在 Final Answer 下方展示一次：

```text
Answer body...

32.67 tokens/s   978.2k tokens   11 rounds   16 tools   0 errors

Copy   Helpful   Not helpful
```

展示顺序固定为：

1. `tokens/s`
2. `tokens`
3. `rounds`
4. `tools`
5. `errors`

不展示：

- `tool_successes`
- `Completed`
- Input/Output/Cache token breakdown
- Stop reason 文本，除非 Turn 以异常状态结束且需要用户理解

### 10.2 Tokens/s 语义

客户端基于同一个 `TurnExecutionMetrics` 计算：

```text
tokens_per_second = usage.total_tokens / (duration_ms / 1000)
```

约束：

- `duration_ms` 为 `null`、`0` 或不可用时，不展示 `tokens/s`。
- `TurnExecutionMetrics` 整体不可用时，不展示 Footer，也不从 Session 或 Profile 聚合值回填。
- Metrics 存在时按协议原值展示 `total_tokens`，包括真实的零值；不在客户端猜测 Provider usage availability。
- 该值描述整个 Turn 的平均 Token 速率，包含工具执行和其他 Turn 内耗时。
- UI Label 保持简洁为 `tokens/s`；Accessible description 或 Tooltip 必须说明它是 Turn average，不是 Provider decode throughput。
- 计算只存在一个共享 Formatter，不允许组件各自实现不同精度和 Compact 规则。

### 10.3 数字格式

- Tokens 使用现有 Compact number 语义，例如 `978.2k`、`1.2m`。
- Tokens/s 小于 `100` 时最多保留两位小数；大于等于 `100` 时使用一致的 Compact formatter，避免超长数字。
- Rounds、Tools 和 Errors 使用整数。
- 非有限值不渲染。
- `errors = 0` 使用普通 Muted 色；`errors > 0` 时仅 Error item 使用 Danger 色。

### 10.4 图标与样式

- 每个 Metric 使用语义明确的现有 Lucide 线性图标。
- 图标目标尺寸 `12px`，Stroke width 约 `1.5`。
- 图标与文字使用相同 Muted 色，不设置圆形底座、胶囊、边框或背景。
- Footer 字号目标为 `11-12px`，使用 Tabular numbers。
- Items 使用 Flex wrap；窄屏换行，不横向滚动。
- Metrics 与回答正文间距目标为 `12-16px`，与 Response Actions 保持明确但紧凑的间距。
- 深色主题复用完全相同的 Footer DOM、顺序、字号、图标和响应式规则，但继续使用现有深色主题语义颜色。

## 11. Composer

- Composer 使用白色 Surface、细边框和单层轻阴影。
- 输入正文保持第一视觉层；Attachment、Permission、Provider、Model、Reasoning 等控件统一降低对比度。
- Project 与 Runtime location 作为低权重状态行，不与输入内容争夺注意力。
- Send/Stop 保持唯一明确的主操作。
- 不删除现有功能，不改变 Model、Permission、Attachment 或 Queue 行为。

## 12. Modal 与用户交互面板

- 普通 Modal 使用标题、说明、内容、Footer actions 的统一结构。
- 移除标题前的 Folder、Shield、Message 等装饰性图标。
- Close、Expand 等功能性图标可以保留，但不使用独立图标底座。
- 普通 Modal 目标宽度为 `420-460px`；目录浏览等复杂 Modal 可以保留更大宽度。
- 降低 Backdrop blur 和 Shadow，不使用多层浮光。
- 带说明的选项优先使用单列 Row，避免两个窄 Card 并排。
- Danger action 使用暗红文字或轻边框，不使用大面积高饱和背景。
- UserAction 必须继续保持可达、可恢复和明确 Focus，不因视觉精简隐藏审批或问题。

## 13. 动效、响应式与可访问性

### 13.1 动效

- Process 展开/折叠目标时长为 `160-200ms`。
- 不增加装饰性入场动画。
- 支持 `prefers-reduced-motion`，关闭非必要 Transition。

### 13.2 响应式

- Desktop 保持 Sidebar 与 Main 双栏。
- 窄屏沿用现有 Sidebar collapse 行为。
- User message、Metrics Footer、Composer controls 和 Modal actions 必须自然 Wrap。
- 不允许 Metrics 或 Composer control 造成页面横向滚动。

### 13.3 可访问性

- Process Summary 使用原生 `details/summary` disclosure，支持 Keyboard、Focus visible 和展开状态语义。
- 仅 Hover 显示的操作必须在 Keyboard focus 下同样可见。
- Muted text 和 icons 必须满足目标背景上的可读对比度。
- Icon-only control 必须保留 Accessible name。
- Metrics icon 不能成为唯一语义来源；每项必须同时有文字 Label。

## 14. 影响范围

主要影响：

- Web theme tokens 与基础 Surface。
- App layout、Sidebar selection 和 Chat detail layout。
- User/Assistant message presentation。
- Process disclosure outer presentation。
- Final Answer footer 与 Metrics formatter。
- Composer surface 与 control hierarchy。
- Modal 与 UserAction presentation。
- Talos Service Active Turn 的 `cancelling` 状态落库、迟到事件保护和刷新恢复语义。
- 对应的 Component tests、Snapshots 和 E2E visual verification。

明确不影响：

- Rust Metrics production、Tool execution 和 Agent runtime 协议。
- App Protocol、HTTP API、Rust SDK 与 TypeScript SDK shape。
- Timeline component internals。
- Dark theme color palette；共享呈现结构仍同步调整。
- Session persistence、Streaming、Retry、Resume 和 UserAction lifecycle。

## 15. 兼容性

- 本设计不改变 Wire object、RPC method、HTTP route、SDK API 或持久化 schema；只完善既有 Active Turn 状态的转换时机。
- 历史 Session 已持久化的 `TurnExecutionMetrics` 可以直接渲染新的 Footer。
- 历史数据缺少 `duration_ms` 时只省略 `tokens/s`，其他可用 Metrics 继续展示。
- 不使用 Session usage、Profile usage 或客户端累计值回填缺失的 Turn Metrics。
- Dark theme 不消费新的白色主题目标色值，但必须消费相同的消息、Process、Metrics Footer、Composer 和 Modal 结构调整。

## 16. 验证策略

### 16.1 Component 与行为测试

- Process 执行中默认展开。
- Process 完成后自动折叠。
- 用户手动折叠后普通 Timeline delta 不强制重新展开。
- WaitingUserAction 保持操作可见。
- Final Answer 下方只出现一处 Turn Metrics Footer。
- Metrics 不再出现在 Process summary。
- Timeline component props、DOM shape 和既有 Snapshot 保持不变。
- `tokens/s` 对合法 Metrics 使用确定公式计算。
- `duration_ms` 为 `null` 或 `0` 时省略 `tokens/s`。
- `total_tokens = 0` 且耗时有效时按公式展示零速率，不用 Session 累计值替换。
- Metrics 缺失时不显示伪造零值或 Session fallback。
- `errors > 0` 时仅 Error item 使用 Danger presentation。

### 16.2 视觉验证

- 新会话空状态。
- 长短用户消息、Attachment 和 Markdown Answer。
- Working expanded、Working collapsed、Worked collapsed、Worked reopened。
- WaitingUserAction、Failed 和 Stopped。
- 无 Metrics、部分 Metrics、完整 Metrics、大数字与错误状态。
- Sidebar Active/Hover、Composer、Delete Modal、Directory Modal 和 Question/Approval panel。
- 浅色主题与深色主题下的 Desktop、窄屏和 Reduced motion。
- Timeline before/after Snapshot 对比，确认其内部视觉形态未被修改。

### 16.3 质量门

- Web scoped tests、Snapshot、Lint、Typecheck 和 Build 全部通过。
- 用户可见文本和布局变化经过实际浏览器截图检查。
- 不新增重复 Metrics DTO、Formatter、Timeline renderer 或 Modal framework。
- 不引入新依赖。
- Breaking change 审查确认协议、SDK、配置和持久化均无变化。

## 17. 风险与后续

1. `tokens/s` 受工具执行和等待时间影响，不能与 Provider decode throughput 横向比较。Tooltip 和文档必须保持该边界。
2. 全局 Theme token 调整可能意外影响 Timeline。实现必须用 Snapshot 和 Computed style 对比保护其当前视觉。
3. 移除 Avatar 和标题后，连续 Turn 的分组可能变弱。需要通过 Turn spacing 和 User bubble alignment 保持归属感。
4. Muted Metrics 在浅色或深色主题的部分显示器上可能对比度不足，两套主题的视觉验证都必须兼顾轻量感和可读性。
5. 如果未来后端提供 Model generation duration，可在新 Spec 中重新定义 Provider decode tokens/s；不得静默改变本设计中 Turn average 的既有语义。
