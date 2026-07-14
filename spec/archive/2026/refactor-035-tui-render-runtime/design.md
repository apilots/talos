# Talos TUI Render Runtime Refactor

Status: archived
Archived: 2026-07-14
Owner: agent
Created: 2026-07-07
Updated: 2026-07-09

## 背景与问题

Talos TUI 当前存在两个用户可见问题：

- 长对话中滚动不够丝滑，滑动时有明显卡顿。
- 退出 TUI 后，alternate screen 被恢复，用户在终端 scrollback 中看不到刚才的对话记录。

这两个问题表面上都是展示问题，本质上是 TUI 渲染架构边界不清：

- Ratatui viewport 同时承担交互 UI、历史 transcript、live stream 展示。
- 主循环固定 50ms tick 并每轮 draw。
- `render_ui` 每帧重建聊天 rendered lines。
- 历史 replay 和 live delta 最终进入同一批 `chat_blocks` / `rendered_chat_lines`，缺少 finalized transcript 边界。
- 已完成内容没有同步写入 terminal scrollback，退出后只能依赖 alternate screen 当前画面。

本方案定义一个小型 Talos TUI render runtime，不引入大型 TUI 框架，不迁移到 Claude Code 的 Ink/React 栈。

## 目标

- 将 TUI 从固定 tick 全量刷新改为事件驱动刷新。
- 将历史 replay 渲染和 live stream 渲染拆成两条路径。
- 将 finalized transcript 作为终端历史的事实源。
- 将 finalized transcript 增量同步到 terminal scrollback/stdout，使退出后历史保留。
- 固定唯一 terminal mode：主对话 transcript 属于 primary terminal scrollback，不提供 alternate-screen 主历史模式。
- 不再维护 TUI 自己的聊天历史滚动框；已完成历史交给 terminal primary scrollback 原生滚动。
- 保持 Ratatui 作为当前交互层 renderer，不重写整套 UI。
- 保持迁移前 TUI 用户可见渲染行为：执行中 live 文本、tool call、tool progress、metrics、status line、input composer、overlay/picker 的样式和反馈节奏不得因架构迁移发生产品语义变化。
- 形成清晰的小框架边界，后续 TUI 功能继续落在该边界内。
- 以新的小框架作为主线重建 TUI 展示架构，最终完全替换当前主渲染链路。
- 保持 CLI mode 行为不被 TUI render runtime 迁移破坏；CLI 与 TUI 共享业务能力，但 terminal rendering path 必须解耦。

## 非目标

- 不引入 Claude Code 的 Ink/React-like renderer。
- 不引入通用 TUI framework 替代当前 Ratatui 架构。
- 不在本轮改变后端 stream event 协议。
- 不改变 Web UI、SDK、app-server API。
- 不重构 CLI mode，不改变 CLI 参数、exec 行为、event rendering 语义或集成测试入口。
- 不把完整历史仅在退出时一次性补打印到 stdout。
- 不通过简单升级 Ratatui 版本解决架构问题。
- 不借 terminal scrollback 同步改写 live UI 样式、thinking/tool 展示语义、metrics/input 区域或快捷交互效果。
- 不保留新旧两套长期并行的 transcript 渲染路径。
- 不保留 TUI app-owned chat selection、Copy Round 或内部历史 scroll viewport。
- 不提供多 terminal mode 配置；本方案只支持 primary scrollback 保留主 transcript 这一种用户模式。
- 不在旧 `run loop -> chat_blocks -> rebuild_chat_render_state -> rendered_chat_lines -> ratatui draw` 链路上继续局部修补。
- 不保留临时 `v2` 目录、旧兼容分支、fallback 或旧路径测试。

## 基线实现事实

方案制定时的 TUI 相关事实：

- `talos-rs/apps/tui/src/run.rs` 使用 `EnterAlternateScreen` / `LeaveAlternateScreen`。
- `run_event_loop` 每轮执行 `app.on_tick().await` 和 `terminal.draw(...)`。
- 事件读取使用 `event::poll(Duration::from_millis(50))`。
- `talos-rs/apps/tui/src/render/ui.rs` 在 render path 中调用 `rebuild_chat_render_state(chat_content_rect)`。
- `rebuild_chat_render_state` 会清空并重建 `rendered_chat_lines`。
- `rebuild_chat_render_state_if_rect` 已存在，但当前 render path 仍强制重建。
- `get_session_history` 和 `replay_history_snapshot` 已提供历史加载入口。
- live stream 事件已经有独立处理链路，最终更新 `chat_blocks` 与 transcript 相关状态。
- `talos-rs/apps/tui/Cargo.toml` 当前使用 `ratatui = "0.28"`。

## 当前落地进展

- 临时骨架已提升为正式 `talos-rs/apps/tui/src/render_runtime/` 模块，按 `runtime`、`transcript`、`render`、`common` 拆分职责。
- 已移除 TUI 默认入口的 `EnterAlternateScreen` / `LeaveAlternateScreen`，运行入口改为 primary screen interaction mode。
- 已实现 renderer-neutral transcript block / line / style、ANSI projection、terminal transcript writer、flush ordering 和 primary terminal contract test。
- 已实现 render runtime controller、frame scheduler、dirty/cycle contract、history renderer、live reducer、timeline adapter 和 engine。
- `replay_history_snapshot` 和 live `AgentEventPayload` 已喂给 render runtime engine，旧 UI path 不再作为 transcript 事实源。
- finalized transcript 已改为通过 Talos `PrimaryTerminal` adapter 同步到 primary scrollback；生产路径不再通过 ANSI writer 直接写当前 live frame，也不再依赖普通 Ratatui `insert_before` 推动历史。
- frame fingerprint draw gate 已并入 render runtime controller，相同 UI fingerprint 的 idle cycle 不再固定 draw；crossterm `poll/read` 已收进 `CrosstermRuntimeEventSource`，`run_event_loop` 只消费 `TerminalRuntimeEvent`。
- visible transcript 已由 transcript projection 生成；`/status`、`/help`、session compact/clear、model/session/stream 错误等本地命令输出已进入 finalized transcript；viewport 不再依赖旧 `chat_blocks` 覆盖度判断；旧 `chat_blocks -> rendered_chat_lines` 主覆盖路径已删除。
- viewport 已覆盖空态、width-aware wrapping、finalized transcript、in-flight live tail、active tool snapshot 和空等待 spinner。
- 已删除 TUI app-owned chat selection/copy projection、Copy Round、footer button hit testing 和 mouse capture；终端历史查看、拖拽选择和复制统一交给 terminal emulator 原生能力。
- finalized transcript lines 已携带 stable `TranscriptLineId` 到 visible projection，后续 viewport 裁剪与 live rendering 可以继续基于稳定 line identity 演进。
- live、history 和 terminal projection 已恢复按 `TranscriptBlockKind` 渲染的边界：Thinking、Commentary、FinalAnswer、Tool、SystemNotice 不允许在最终投影中退化为同一种纯文本路径。
- finalized round 已按 block 组装渲染：User/Command 作为 round 起点，Thinking 稳定排在 FinalAnswer 前，FinalAnswer 统一使用 message bullet，RoundFooter 与下一轮 user/command block 之间保留稳定空行。
- TUI viewport projection 和 terminal flush projection 使用同一组 block renderer，并保持 timeline/store 顺序；renderer 不允许按 kind 把 Thinking、Tool、FinalAnswer 分组重排，避免执行中和完成后顺序漂移。
- terminal flush 分批写入时，synced history 与下一批 pending round 之间保留组件级空行，避免 session header、footer 和下一轮消息贴在一起。
- 已同步到 terminal scrollback 的 finalized block 不再进入 Ratatui inline viewport，避免 session header / welcome card / completed history 在 primary scrollback 和 inline viewport 中双重显示。
- 聊天区右侧 Ratatui scrollbar 和保留列已删除，历史查看和定位继续收敛到 terminal scrollback 原生能力。
- 没有 live tail 或 agent activity 时，Ratatui inline chat panel 收缩为 0 高度；执行中 live round 若接在已同步历史之后，会先渲染一个组件级空行，再渲染 live block。
- Ratatui inline viewport 高度由 mutable live projection、input/status/metrics/overlay 内容计算，不再把 pending finalized transcript 当成 live 区域高度来源；`PrimaryTerminal` fixed viewport 在短历史时贴着已写入 transcript 后方，历史超过可视区域时才夹到终端底部。
- live chat panel 不再绘制额外背景或边框；执行中的 mutable tail、metrics、input/status 与 finalized transcript 共用 0 列起始边界，metrics/status 不绘制整行背景，避免 live tail 看起来像独立黑色 block。
- final answer projection 必须保留模型输出的语义换行；代码块、列表和段落在 TUI viewport 与 terminal scrollback 中使用同一 block renderer，不允许在 terminal projection 中被合并成单行。
- `PrimaryTerminal` 持有 finalized transcript 的 `history_bottom` 和 fixed viewport 坐标。flush 时先清理 live viewport，再从 `history_bottom` 顺序追加预 wrap 后的 finalized rows；当 transcript 超出可视历史区域时，必须使用正常 primary screen scroll 进入 terminal emulator scrollback，不再使用受限 scroll region，避免长 transcript 被可视历史区吃掉。
- `PrimaryTerminal` fixed viewport 必须支持运行期 resize。event loop 每轮在 transcript flush / Ratatui draw 前按当前 app state 同步 viewport 高度，使执行中的 mutable live tail、metrics、input/status 位于同一个 current turn tail 区域，而不是承载整轮 pending finalized transcript。
- Tool result projection 必须基于结构化 `TalosContentItem` 类型生成摘要和明细；Bash、FileRead、SearchResults、Diff、Reference 等结果不得只通过 `display_text()` 泛化输出。
- Tool renderer 已恢复迁移前的分类优先级和可见投影语义：Diff/Edit/apply_patch 优先完整展开；Bash、FileRead、SearchResults、Reference、Generic 默认只写入折叠摘要，完整 tool input/output 进入 copy markdown，避免隐藏详情污染 terminal scrollback。
- tool started、tool args delta 和 tool progress 只更新 live projection，不进入 finalized transcript；tool completed 才生成稳定 transcript block，避免空 `Read/Bash`、`execution started` 等中间生命周期内容残留到退出后的终端历史。
- Terminal scrollback flush 在 turn in-flight 期间必须允许稳定 finalized blocks 增量进入 primary scrollback；Ratatui viewport 只保留仍会变化的 mutable tail。Round footer 使用独立同步 watermark，确保正文提前 flush 后 `✻ Brewed for ...` 仍只输出一次。
- tool completed projection 已包含 ToolResult markdown 数据，避免删除旧 tool block 后丢失工具输入/输出结构化投影能力；可见 transcript 只保存 TUI 当前可见内容。
- `round_copy_text`、`format_block_for_round_copy`、Copy Round button、chat selection/copy projection 和相关旧 test helper 已删除；后续只保留目标态正向测试。
- `/history` TUI command 已删除；session 列表和切换只保留 `/sessions` 入口，避免两个命令表达同一个 `list_sessions` + `get_session_history` 后端业务链路。
- active tool snapshot 已进入 interactive projection；in-flight 且只有 tool 活动时，visible viewport 继续走 transcript projection，而不是退回旧 spinner。
- live stream text 主路径已停止写入旧 text blocks；stream 相关测试改为验证 interactive projection；history replay 已停止回写旧 `chat_blocks` 和旧 live text helper，历史内容只进入 transcript。
- 旧 chat role、旧 chat block renderer、旧 tool renderer、旧 status styled renderer、相关旧测试和旧 wrapping helper 已删除；复制成功/失败这类临时 UI 提示已改为 status hint，不再写入 transcript。
- 旧 `TranscriptState`、`stream_text` live buffer、tool block 状态机、final-answer 缓存语义、active user block 索引和相关旧测试已删除；stream 事件只投递到 render runtime 与必要运行态，不再维护第二套 transcript。
- 旧 `chat_blocks/chat_history` 辅助状态、chat role、chat block 类型和旧 mirror getter 已删除；生产路径不再维护第二套聊天历史，测试只覆盖目标态行为。
- primary mode enter -> transcript flush -> leave contract 已覆盖 finalized transcript 保留、draft/status 临时 UI 不进入 scrollback、无 alternate-screen 序列。
- 旧 `chat_scroll/chat_manual_scroll`、PgUp/PgDn 聊天滚动入口和内部长期历史滚动测试已删除；live viewport 永远跟随最新 live projection，长期历史查看交给 terminal scrollback。
- Ratatui draw path 已改为只接收 `viewport + overscan` 可见窗口，不再把完整长历史交给 `Paragraph` 再滚动；长 transcript 测试覆盖窗口行数上限和 TestBackend 可见输出。
- 切换 session / 清空聊天时会 reset transcript store，避免跨场景 projection 污染。
- CLI mode 的 parse、exec 和 event lifecycle 测试保持通过；TUI render runtime 迁移不得改变 CLI 参数和 exec 渲染语义。
- 当前 gate 已通过：`just fmt`、`just fix -p talos-tui`、`cargo check -p talos-tui`、`git diff --check`、`just test -p talos-tui`。`talos-tui` 当前 187 个测试通过；tmux 手工验证覆盖启动、tool round live 状态、完成态和 `/quit`。

旧状态结构已不再允许作为 TUI transcript 事实源。临时 `v2` 目录名已删除，frame fingerprint gate 已并入 render runtime controller，terminal event source 已进入 render runtime；Copy Round、app-owned chat selection/copy、mouse capture 和内部长期历史滚动状态已删除，不再保留旧 poll/draw 主路径。

复用判断：

- 当前主渲染链路不可作为新架构地基：`run loop -> chat_blocks -> rebuild_chat_render_state -> rendered_chat_lines -> ratatui draw`。
- 这条链路把 source state、render cache、viewport、terminal mode 混在一起，和目标 runtime / transcript store / projection / terminal writer 边界冲突。
- 旧实现只能作为已经完成的迁移来源，不能作为新框架的核心依赖，也不能以兼容分支或旧路径测试形式保留。
- 可复用的是输入数据、局部渲染能力、样式 helper、wrapping utility、tool rendering 规则、history/live event 语义和测试 fixture。

参考实现判断：

- Codex Rust TUI 的方向适合借鉴：事件驱动 frame request、stream commit tick、finalized history cell 写入终端 scrollback、resize 时从 transcript source 重新回放。
- Claude Code 的原则适合借鉴：虚拟化、screen diff、live 增量提交。但其 Ink/React-like renderer 复杂度不适合 Talos 直接迁移。
- 本仓库当前使用 `ratatui = "0.28"`。Ratatui 提供 immediate mode buffer rendering，不提供应用级事件增量框架。

## 设计总览

目标架构：

```text
                 App / Stream Events
                        |
          +-------------+-------------+
          |                           |
   History Replay                 Live Stream
 get_session_history()          delta events
          |                           |
          v                           v
  History Renderer            Live Renderer
  batch render/cache          commit tick / small batches
          |                           |
          +-------------+-------------+
                        |
              Transcript Store
          finalized styled lines/cells
                        |
        +---------------+---------------+
        |                               |
 Terminal Transcript Writer       TUI Viewport
 stdout / scrollback sync         live tail / input / overlay
        |                               |
  exit 后仍可见                 event-driven redraw
```

核心思想：

- 历史内容一旦 finalized，就从 live mutable block 中退役。
- terminal primary scrollback 是已完成对话记录的唯一用户可见历史。
- `Terminal Transcript Writer` 负责把 finalized transcript projection 同步到 primary terminal scrollback。
- Ratatui inline viewport 只负责当前交互层：输入框、状态栏、metrics、live tail、overlay、picker/panel。
- terminal sync status 只表示对应 finalized block 是否已写入 primary scrollback，不能作为 TUI viewport 可见性条件。
- 滚动历史时读取 transcript cache 的可见窗口，而不是每帧从 `chat_blocks` 全量重排。

## 设计决策

### 1. 以 render_runtime 承载主渲染框架

TUI crate 内的正式主渲染框架位于 `render_runtime`。该模块不是兼容层，也不是临时并行路径；它是 transcript projection、terminal scrollback sync、live rendering 和 frame scheduling 的 owner。

当前模块形态：

```text
talos-rs/apps/tui/src/render_runtime/
  runtime/
    controller.rs
    event_loop.rs
    frame_scheduler.rs
    dirty.rs
  transcript/
    store.rs
    block.rs
    line.rs
    projection.rs
    writer.rs
  render/
    history.rs
    live.rs
    ansi.rs
    ratatui.rs
  common/
    style.rs
```

最终约束：

- 新功能和架构性改动只进入 `render_runtime` 或它明确 owner 的调用边界。
- 旧 `render_ui` 全量重建主链路不得重新成为 transcript 事实源。
- 不再维护旧 chat render state、旧 ChatBlock renderer、旧 tool renderer 或旧 transcript mirror。
- 不允许新增从 `render_runtime` 反向依赖旧 render 主链路的核心逻辑。
- 不允许保留兼容逻辑、fallback 或旧路径测试。

### 2. 新增 TUI runtime 小框架

新增轻量 runtime 边界，负责事件调度、frame 合并、dirty 标记和终端 transcript flush。

建议模块边界：

```text
talos-rs/apps/tui/src/runtime/
  event_loop.rs
  frame_scheduler.rs
  dirty.rs
  terminal_transcript.rs
  transcript_store.rs
```

该 runtime 不持有业务语义，不解析 tool call，不知道 agent 协议细节。业务事件仍由现有 TUI app reducer 消费。

最终模块必须保留这些职责边界，不能把 runtime、transcript store、terminal writer 和业务 reducer 再揉回单个 render loop。

### 3. 主循环改为事件驱动

主循环接收以下事件：

- keyboard / mouse / paste
- resize
- stream event
- history loaded
- live commit tick
- frame requested

状态变化后设置 dirty flag 并 request frame。多个 request 合并。idle 时不 draw。

所有 terminal writes 必须在同一个 TUI event loop 中串行执行。`TerminalTranscriptWriter` 不允许绕过 frame scheduler 在后台线程直接写 stdout，避免与 Ratatui draw 交错。

### 4. 固定 Primary Scrollback Terminal Mode

Talos TUI 只支持一种用户可见 terminal mode：

```text
Primary terminal scrollback = finalized transcript projection
Ratatui viewport             = interaction layer + live tail + cached transcript viewport
Alternate screen             = not used for main conversation history
```

主对话 transcript 不进入 alternate screen。运行过程中 finalized 内容必须增量写入 primary scrollback，退出时不清屏、不重新补打一份完整历史、不丢失已完成内容。

允许使用 Ratatui 绘制临时交互 UI：

- input area
- live mutable tail
- status/runtime/context line
- autocomplete
- overlay / picker / panel

这些临时 UI 不进入 terminal scrollback，除非它们已经被归档为 transcript block。

实现约束：

- 真实 TUI terminal 必须通过 Talos `PrimaryTerminal` adapter 管理 primary scrollback 写入、finalized transcript bottom 和 Ratatui fixed viewport。
- 每次 finalized transcript flush 后必须重建 fixed viewport terminal 并立即重画 Ratatui viewport，恢复迁移前 live UI 的当前状态。
- 禁止在 production path 中用 ANSI writer 直接向当前 screen 写 finalized transcript；该方式无法保护 live frame，会造成 transcript 与 input/status/metrics 重叠。
- `EnterAlternateScreen` / `LeaveAlternateScreen` 不得回到主对话历史路径。
- frame fingerprint 必须覆盖 live projection、metrics、input/status 和 overlay 相关状态，避免 stream/tool delta 因 idle draw gate 被跳过。

该决策必须在实现早期通过伪终端 contract test 证明：

- finalized line 写入后存在于 primary scrollback。
- 退出 TUI 后 primary scrollback 中仍能看到 transcript。
- raw mode、cursor、mouse capture 和 bracketed paste 能正确恢复。

### 5. 拆分 History Renderer 与 Live Renderer

历史 replay：

- 输入为 `SessionHistorySnapshot` / timeline items。
- 输出 finalized transcript blocks / lines。
- 可批量 render 并写入 `Transcript Store`。
- 首次进入 TUI 或切换 session 时使用。

live stream：

- 输入为 stream delta 和 tool lifecycle event。
- 输出 live block 更新。
- 通过短周期 commit tick 更新 live tail；只有达到明确 flush point 的内容才能提交为 finalized transcript。
- commit 后写入 `Transcript Store` 并进入 terminal writer pending queue。

### 6. Terminal Scrollback 成为已完成历史事实源

`Transcript Store` 负责保存尚未写入 terminal scrollback 的 finalized 内容和必要的 live/runtime source：

- block id / round id
- renderer-neutral styled segments
- rendered line cache
- copy projection
- source metadata
- width-aware cache key
- terminal sync status

它不保存无限内容。需要定义内存上限、行数上限和必要裁剪策略。

`Transcript Store` 只服务一个 canonical visible transcript projection：

- terminal projection：写入 primary scrollback，退出后用户在终端中看到。
- optional test/plain projection：用于 contract test 或 debug，必须从 terminal projection 派生。

不允许分别维护 TUI-visible、terminal-visible、copy-visible 三套事实源。terminal projection 一旦写入 primary scrollback，默认按写入时宽度冻结。

Ratatui live projection 和 terminal projection 必须从同一组 renderer-neutral styled segments / canonical cell renderer 派生，避免执行中 TUI 样式和完成后 scrollback 样式由两套逻辑分别维护。

TUI 不再拥有聊天区 selection/copy。拖拽选择和复制由 terminal emulator 对 primary scrollback 原生处理。Copy Round、RoundFooterButton、chat selection anchor 和 copy projection 已从主路径删除，后续不能重引入。

### 7. Terminal Transcript Writer 负责 scrollback 同步

writer 接收 finalized transcript lines，并在合适时机 flush 到真实 stdout/terminal scrollback。

约束：

- 不从 live viewport 全量复制。
- 不在退出时全量补打一遍作为主路径。
- flush 只面向 primary terminal scrollback，不支持 alternate-screen 主 transcript 写入。
- flush 前后不能破坏当前输入区、光标和 raw mode 状态。
- flush 单位默认是 finalized transcript block；只有明确 append-only 的 streaming text 可以按 line flush。
- 已写入 terminal scrollback 的内容不能再被原地修改，后续修正必须作为新的 transcript block 追加。

目标效果是：用户运行过程中已经逐步形成终端 scrollback，退出后自然保留历史。

writer flush ordering 是固定 contract：

```text
1. receive finalized transcript projection
2. suspend or clear the current interaction layer
3. flush pending finalized transcript to primary stdout/scrollback
4. repaint Ratatui interaction layer from current app state
5. restore cursor position, raw mode related terminal state, mouse capture, and bracketed paste
```

因此 writer integration 必须和 frame scheduler 串行。一次 event loop turn 可以先处理 pending transcript flush，再 request/repaint interaction frame；不能让 transcript flush 与 `terminal.draw` 并发写同一个 stdout。
flush 前用于清理 current interaction layer 的空白重绘只能是临时 projection，不能改变 `Transcript Store`，也不能让已 flush 的 block 从后续 TUI viewport 中消失。

建议 flush point：

| 内容类型 | Flush point |
| --- | --- |
| User input | 用户提交后立即 finalized |
| Commentary | 对应 message/item completed 或确认不再修改时 |
| Thinking | thinking block completed；折叠状态不进入 terminal projection |
| Final answer | assistant text committed 或 turn completed |
| Tool call header | 执行中只在 live viewport 展示；不单独写 finalized transcript |
| Tool progress | 执行中只在 live viewport 展示稳定摘要，不写入可被覆盖的 spinner/status |
| Tool result | tool call completed 后按结构化 content item 写入最终摘要和可见结果 |
| System notice | notice completed 后写入 |
| Status/runtime line | 不进入 terminal projection |
| Overlay/picker/panel | 不进入 terminal projection |

现有 stream mutation 到 finalized transition 的目标映射：

| 现有事件/handler 类型 | Live mutable 行为 | Finalized transition |
| --- | --- | --- |
| user input submission | 立即追加 user block | 提交后立即写入 transcript store；terminal flush 等 turn 稳定 |
| history replay snapshot | 批量构建历史 block | 作为 already-finalized history 写入 transcript store；是否回放到 terminal 由 session load policy 决定 |
| assistant/commentary text delta | 更新当前 live text block | message/item completed 或 `AssistantTextCommitted` 后 finalized |
| final answer delta | 更新当前 live final-answer block | `AssistantTextCommitted` 或 `TurnCompleted` 后 finalized |
| reasoning/thinking delta | 更新当前 live thinking block | thinking item completed 后 finalized；折叠状态不写入 terminal projection |
| tool call started / args delta | 更新 live tool header 和参数摘要 | 不生成 finalized block；等待 tool completed |
| tool progress delta | 更新 live tool progress/status | 不生成 finalized block；spinner、running status、可覆盖 progress 不写入 terminal |
| tool completed | 按 tool content type 写入最终 result / evidence | completed 后 finalized；terminal flush 等 turn 稳定 |
| system notice / error | 更新 notice block | notice completed 后 finalized |
| status/runtime/right panel state | 更新临时 UI | 默认不进入 terminal projection；若需要归档，必须先转换为 explicit transcript block |

### 8. Ratatui Inline Viewport 只画当前交互层

Ratatui 继续用于：

- input area
- status/runtime/context line
- live tail
- overlay/picker/panel

Ratatui inline viewport 只负责 input、status、live tail、overlay、picker 和 panel。已完成历史进入 terminal primary scrollback。

执行中的 active/live cell 可以在 inline viewport 中实时变化；turn 完成后必须冻结为 finalized transcript，写入 terminal scrollback，并从 live viewport 中移除。

### 9. Resize 走 transcript source 重新排版

宽度变化时，不能依赖旧 rendered lines。应从 transcript source 按新宽度重建对应 cache。

resize 需要处理：

- terminal scrollback reflow 策略
- visible viewport cache invalidation
- selection/copy projection 更新
- live block 宽度重排

目标决策：

- interactive projection 按当前 viewport width 重新排版。
- terminal projection 已写入 primary scrollback 后按写入时宽度冻结。
- 不在 resize 时重写真实 terminal scrollback。
- resize 后新写入的 finalized transcript 使用新的 terminal width。

### 10. Ratatui 版本不是主风险

可以单独评估从 `ratatui = "0.28"` 升级到当前稳定版本，但本方案不依赖升级。

性能根因在 Talos 当前渲染策略：

- 固定 tick
- render path 全量重建
- 历史/live 未分层
- terminal scrollback 未同步

升级 Ratatui 不能替代 runtime 重构。

## 影响范围

主要影响：

- `talos-rs/apps/tui/src/run.rs`
- `talos-rs/apps/tui/src/lib.rs`
- `talos-rs/apps/tui/src/render/ui.rs`
- `talos-rs/apps/tui/src/render/chat_lines.rs`
- `talos-rs/apps/tui/src/stream_events.rs`
- `talos-rs/apps/tui/src/stream_updates.rs`
- `talos-rs/apps/tui/src/input.rs`
- `talos-rs/apps/tui/src/selection.rs`
- `talos-rs/apps/tui/src/render_runtime/`
- TUI snapshot / scroll render tests

不应影响：

- app-server API
- SDK API
- provider runtime
- tool execution semantics
- Web UI
- CLI mode business behavior and command/event semantics

## 验证策略

必须覆盖：

- idle 状态不持续 redraw。
- stream delta 触发有限 redraw，并在 idle 后停止 commit tick。
- 滚动只改变 viewport offset，不重建完整 transcript。
- 滚动事件不得触发全量 `rebuild_chat_render_state`。
- 可见区渲染行数受 viewport height + overscan 上限约束。
- idle 状态不得调用 `terminal.draw`。
- history replay 使用 history renderer，不走 live delta 路径。
- live finalized 内容进入 transcript store。
- terminal transcript writer 按增量 flush finalized lines。
- 退出 TUI 后，已完成 transcript 留在终端 scrollback。
- resize 后 interactive projection 按新宽度正确 reflow，terminal projection 对已写入内容保持写入时宽度。
- terminal scrollback 原生复制得到的 plain text 布局和 finalized transcript 一致。
- render runtime 主路径覆盖旧 TUI 功能后，旧全量重建链路被删除。
- 最终代码不保留 `v2` 目录名、legacy adapter、fallback、兼容分支、双主路径或旧路径测试。
- 长 transcript 渲染只向 Ratatui 传入 `viewport + overscan` 窗口，并有 TestBackend 可见输出回归。
- CLI mode parsing、exec lifecycle 和 event rendering 回归测试继续通过。

测试形式：

- 单元测试覆盖 frame scheduler、dirty flags、transcript store。
- TUI render snapshot 覆盖历史 replay、live tail、滚动、resize。
- 端到端或伪终端测试覆盖退出后 scrollback 保留。
- CLI tests 覆盖 `talos exec`、默认 TUI parse、turn completed / final answer event lifecycle。
- 性能回归测试至少记录长 transcript 滚动时 render/rebuild 次数。
- 迁移完成时必须有 dead code 检查，确认旧 render path、临时 adapter 和 fallback 已清理。
- 最终 gate 必须包含 `just fmt`、`just fix -p talos-tui`、`git diff --check`、`just test -p talos-tui`。

## 开放风险

- 旧 `chat_blocks` 已删除；后续不得重新引入第二套 transcript mirror、内部历史滚动或兼容路径。
- live commit tick 过快会带来刷新压力，过慢会让流式输出不跟手；当前实现通过 frame request 合并和 frame fingerprint gate 限制 idle redraw。
- TUI app-owned copy/selection 已删除；后续改动不得继续围绕它扩展。
- Terminal writer 需要非常小心地处理当前交互区擦除、重绘和光标恢复，否则会造成 scrollback 与 live viewport 互相污染。
- render runtime 已是主路径；后续清理不得重新引入临时 `v2` 目录或双路径。
