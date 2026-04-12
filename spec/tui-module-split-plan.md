# TUI 模块拆分方案

## 背景

`apps/ui-tui/src/lib.rs` 当前 7716 行，严重违反单文件 ≤1200 行原则。

文件组成：
- 主代码 ~5050 行（类型定义、trait、TuiApp impl）
- 测试代码 ~2660 行（末尾 `#[cfg(test)] mod tests`）

---

## 现有 lib.rs 结构分析

```
lib.rs
├── [L1-84]     常量 / 辅助函数（format_session_timestamp, session_list_label）
├── [L85-434]   核心类型定义
│   ├── CommandSuggestion
│   ├── AppState / ChatRole / ToolCallStatus
│   ├── ChatBlock / CopyProjection / RenderedChatLine
│   ├── RoundFooterButton / ToolRenderData
│   ├── PastedTextBlock / SelectionPoint / ChatSelection
│   ├── QueuedMessage / PendingApprovalState
│   └── RemoteCommandUpdate
├── [L335-434]  GatewayClient trait + TalosClient impl（~100 行）
├── [L435-512]  TuiApp struct 定义（字段声明）
└── [L513-5050] TuiApp impl（~4540 行）
    ├── 构建/初始化          L523-674
    ├── 状态访问器            L675-712
    ├── 输入处理              L713-971
    ├── 事件轮询/on_tick     L972-1417
    ├── render_ui 入口        L1418-1481
    ├── 消息提交/命令分发     L1482-1641
    ├── 消息发送/流处理       L1642-1977
    ├── 远端模型/会话命令     L1978-2244
    ├── 会话生命周期          L2245-2348
    ├── 输入编辑器方法        L2349-2449
    ├── 滚动                  L2450-2475
    ├── 剪贴板/文本选择       L2475-2755
    ├── 鼠标事件              L2678-2755
    ├── Chat block 管理       L2755-3132
    ├── 审批卡渲染            L3254-3351
    ├── 右侧面板渲染          L3351-3599
    ├── 历史/模型/会话弹窗    L3460-3688
    ├── Chat render state     L3715-3860
    ├── 聊天行渲染            L3856-4343
    ├── Tool 渲染辅助         L4344-4926
    └── Input 行渲染          L3885-5050
└── [L5050-7716] 测试（~2660 行）
```

---

## 拆分目标文件结构

```
apps/ui-tui/src/
├── lib.rs              ← 仅保留 mod 声明和 pub use 再导出，≤50 行
├── types.rs            ← 所有核心数据类型，~350 行
├── client.rs           ← GatewayClient trait + TalosClient impl，~100 行
├── app.rs              ← TuiApp struct 字段定义 + 构建/初始化/状态访问器，~300 行
├── input.rs            ← 输入处理（handle_input/paste/backspace/editor），~500 行
├── stream.rs           ← 事件轮询 + 流处理 + 消息发送，~850 行
├── commands.rs         ← execute_command + 所有 /command 处理器，~650 行
├── chat.rs             ← Chat block 增删改 + assistant/tool 追加方法，~600 行
├── selection.rs        ← 文本选择 + 剪贴板 + 鼠标事件，~500 行
├── render/
│   ├── mod.rs          ← render_ui + render_left_panel + render_approval_card，~400 行
│   ├── helpers.rs      ← decorate_chat_line + 纯渲染工具函数（无 self 字段访问），~100 行
│   ├── panels.rs       ← right_panel / history / model_picker / sessions / autocomplete，~550 行
│   ├── chat_lines.rs   ← rebuild_chat_render_state + 聊天行渲染（user/assistant/thinking/system），~850 行
│   ├── tool_lines.rs   ← tool_block_lines + 各工具专属 build_*_render_data，~700 行
│   └── input_lines.rs  ← input_lines / draft_input_lines / input_status_line，~250 行
├── markdown.rs         （已有，不变）
├── utils.rs            （已有，不变）
└── run.rs              （已有，不变）
```

测试集中辅助函数，按测试关联模块分散：

```
apps/ui-tui/src/
└── test_helpers.rs     ← #[cfg(test)] 公共辅助（create_embedded_app 等，被92处引用）
```

| 测试组 | 放入模块 |
|--------|---------|
| 输入/光标/粘贴 | `input.rs` |
| 流处理/消息顺序/队列 | `stream.rs` |
| 渲染/chat render state/round footer | `render/chat_lines.rs` |
| 选择/复制/鼠标 | `selection.rs` |
| 会话/模型命令 | `commands.rs` |
| 滚动 | `chat.rs` |
| 公共辅助函数 | `test_helpers.rs` |

---

## 关键设计决策

### 1. TuiApp 字段可见性策略

TuiApp 有 50+ 个私有字段。拆分后，各 impl 文件是 `lib.rs` 的子模块（`mod input; mod stream;` 等），而子模块**无法访问父模块中定义的私有字段**。

**决策：TuiApp struct 定义保留在 `lib.rs`，所有字段改为 `pub(crate)`。**

```rust
// lib.rs
pub struct TuiApp {
    pub(crate) current_state: AppState,
    pub(crate) input_buffer: String,
    pub(crate) chat_blocks: Vec<ChatBlock>,
    // ... 所有字段统一 pub(crate)
}
```

理由：
- 所有 impl 文件都在同一 crate 内，`pub(crate)` 权限边界清晰
- 不引入 getter 函数膨胀（50+ 字段全加 getter 代价更大）
- 不用 `pub(super)` 的原因：`render/tool_lines.rs` 是孙子模块，`pub(super)` 只到父级，需要两层

> 注意：`pub(crate)` 不对外暴露（lib.rs 的 pub struct 字段不等于 pub），crate 边界外仍看不到。

### 2. render/tool_lines.rs 的依赖问题

**调查结论**：
- `build_*_render_data(block: &ChatBlock)` 系列：**纯静态方法**，无 `self` 依赖 ✅
- `tool_block_lines(&self, ...)` ：有 `self`，但唯一使用的 self 方法是 `self.decorate_chat_line(...)` 
- `decorate_chat_line(&self, ...)` ：虽然签名含 `&self`，但方法体**完全不访问任何 self 字段**，是一个可以改为关联函数的误标签

**处理方案**：将 `decorate_chat_line` 重构为自由函数，移入 `render/helpers.rs`：

```rust
// render/helpers.rs
pub(crate) fn decorate_chat_line(
    line: Line<'static>,
    body_style: Style,
    indent: usize,
) -> RenderedChatLine { ... }
```

改造后 `tool_lines.rs` 完全无 `self` 字段访问，依赖清晰。

### 3. 测试辅助函数集中管理

测试中 `create_embedded_app`、`create_remote_app_*` 等辅助函数被引用 **92 次**，跨多个测试模块复用。

**决策：集中到 `test_helpers.rs`，通过条件编译暴露给各测试模块：**

```rust
// test_helpers.rs
#[cfg(test)]
pub(crate) fn create_embedded_app() -> TuiApp { ... }

#[cfg(test)]
pub(crate) fn create_remote_app() -> TuiApp { ... }
// ...
```

各测试模块引入：
```rust
#[cfg(test)]
mod tests {
    use crate::test_helpers::*;
    // ...
}
```

---

## 各模块职责与关键 API

### `types.rs`
存放纯数据结构，无业务逻辑：
```rust
pub enum AppState { Chat, History, ModelPicker, SessionsPicker }
pub enum ChatRole { User, Assistant, Tool, System, Thinking }
pub enum ToolCallStatus { Pending, Running, Done, Error }
pub struct ChatBlock { ... }
pub struct RenderedChatLine { ... }
pub struct QueuedMessage { ... }
pub struct PendingApprovalState { ... }
pub enum RemoteCommandUpdate { ... }
// ... 其余 POD 类型
```

### `client.rs`
```rust
#[async_trait]
pub trait GatewayClient: Send + Sync {
    async fn list_models(&self) -> TalosResult<Vec<ModelInfo>>;
    async fn list_sessions(&self) -> TalosResult<Vec<TalosSession>>;
    async fn create_session(&self, params: SessionCreateParams) -> TalosResult<TalosSession>;
    async fn send_message_stream(&self, ...) -> TalosResult<mpsc::Receiver<TalosEvent>>;
    // ...
}
impl GatewayClient for TalosClient { ... }
```

### `app.rs`（impl 块，struct 在 lib.rs）
TuiApp 构建方法 + 公共访问器：
```rust
impl TuiApp {
    pub fn build(...) -> Self
    pub fn new_with_client(...) -> Self
    pub async fn initialize_remote(&mut self)
    pub fn get_current_state(&self) -> &AppState
    fn session_environment(&self) -> TalosSessionEnvironment
    fn build_session_context(&self) -> TalosContext
}
```

### `input.rs`
```rust
impl TuiApp {
    pub fn handle_paste(&mut self, text: String)
    pub async fn handle_input(&mut self, key_event: KeyEvent) -> TalosResult<bool>
    fn insert_char(&mut self, c: char)
    fn handle_backspace(&mut self)
    fn reset_input(&mut self)
    fn update_command_suggestions(&mut self)
    fn compose_user_input_text(&self) -> String
}
```

### `stream.rs`
```rust
impl TuiApp {
    pub fn poll_stream_events(&mut self)
    async fn poll_stream_connect(&mut self)
    fn poll_command_updates(&mut self)
    fn poll_session_connect(&mut self)
    fn poll_close_updates(&mut self)
    fn poll_close_timeout(&mut self)
    pub async fn on_tick(&mut self)
    async fn send_user_message(&mut self, user_message: String)
    async fn start_stream_for_message_internal(&mut self, ...)
    fn handle_stream_event(&mut self, event: TalosEvent)
    fn finish_active_stream(&mut self, reason: &str)
    fn abort_pending_stream_task(&mut self)
}
```

### `commands.rs`
```rust
impl TuiApp {
    async fn execute_command(&mut self, command_line: &str) -> TalosResult<bool>
    fn refresh_models_command(&mut self) -> TalosResult<()>
    fn status_command(&mut self) -> TalosResult<()>
    fn switch_remote_model_command(&mut self, requested: &str) -> TalosResult<()>
    fn open_history(&mut self)
    fn toggle_model_picker(&mut self)
    fn close_remote_session_command(&mut self) -> TalosResult<()>
    async fn ensure_remote_session(&mut self) -> TalosResult<String>
    fn start_remote_session_connect(&mut self)
    fn spawn_remote_command_task<F>(&mut self, ...)
    fn load_session_and_turns(&mut self, session: TalosSession)
    fn delete_history_session(&mut self)
}
```

### `chat.rs`
```rust
impl TuiApp {
    fn push_block(&mut self, role: ChatRole, ...)
    fn remove_chat_block(&mut self, idx: usize)
    fn rebuild_chat_history(&mut self)
    fn append_assistant(&mut self, text: &str)
    fn append_thinking(&mut self, text: &str)
    fn push_tool_call_block(&mut self, ...)
    fn complete_tool_call_block(&mut self, ...)
    fn flush_pending_assistant_output(&mut self)
    fn discard_active_pending_blocks(&mut self)
    fn sync_history_entry(&mut self, idx: usize)
}
```

### `selection.rs`
```rust
impl TuiApp {
    fn copy_chat_to_clipboard(&mut self)
    fn copy_round_for_start_block(&mut self, start_block_index: usize)
    fn copy_selection_to_clipboard(&mut self)
    fn selected_chat_text(&self) -> Option<String>
    fn normalized_selection_points(&self) -> Option<(SelectionPoint, SelectionPoint)>
    fn selection_display_range_for_line(&self, ...) -> Option<(usize, usize)>
    fn handle_mouse(&mut self, mouse_event: MouseEvent)
    fn selection_point_from_mouse(&self, mouse_event: MouseEvent) -> Option<SelectionPoint>
}
```

### `render/helpers.rs`
将 `decorate_chat_line` 从 `&self` 方法重构为自由函数（方法体不访问任何 self 字段）：
```rust
pub(crate) fn decorate_chat_line(
    line: Line<'static>,
    body_style: Style,
    indent: usize,
) -> RenderedChatLine { ... }

pub(crate) fn line_plain_text(line: &Line<'_>) -> String { ... }
pub(crate) fn line_display_width(line: &Line<'_>) -> usize { ... }
```

### `render/mod.rs`
```rust
impl TuiApp {
    pub fn render_ui(&mut self, f: &mut Frame)
    fn render_left_panel(&mut self, f: &mut Frame, rect: Rect)
    fn render_approval_card(&self, f: &mut Frame, rect: Rect)
    async fn submit_approval(&mut self, approved: bool)
}
```

### `render/chat_lines.rs`
```rust
impl TuiApp {
    fn rebuild_chat_render_state(&mut self, chat_content_rect: Rect)
    fn visible_chat_lines(&self) -> Vec<Line<'static>>
    fn chat_card_lines(&self, block: &ChatBlock, ...) -> Vec<RenderedChatLine>
    fn user_block_lines(&self, block: &ChatBlock, ...) -> Vec<RenderedChatLine>
    fn assistant_block_lines(&self, block: &ChatBlock, ...) -> Vec<RenderedChatLine>
    fn thinking_block_lines(&self, block: &ChatBlock, ...) -> Vec<RenderedChatLine>
    fn system_block_lines(&self, block: &ChatBlock, ...) -> Vec<RenderedChatLine>
    fn round_footer_line(&self, ...) -> RenderedChatLine
    fn decorate_selection(&self, line_index: usize, line: &RenderedChatLine) -> Line<'static>
}
```

### `render/tool_lines.rs`
（改造后：`tool_block_lines` 调用 `render::helpers::decorate_chat_line`，不再依赖 `&self`）
```rust
impl TuiApp {
    fn tool_block_lines(&self, block: &ChatBlock, content_width: usize) -> Vec<RenderedChatLine>
}

// 纯静态关联函数（无 self）
impl TuiApp {
    fn build_tool_render_data(block: &ChatBlock, tool_name: &str) -> ToolRenderData
    fn build_memory_search_render_data(block: &ChatBlock) -> ToolRenderData
    fn build_exec_command_render_data(block: &ChatBlock) -> ToolRenderData
    fn build_read_file_render_data(block: &ChatBlock) -> ToolRenderData
    fn build_fallback_tool_render_data(block: &ChatBlock, tool_name: &str) -> ToolRenderData
    fn tool_arg_summary(tool_name: &str, input: &serde_json::Value) -> String
}
```

### `render/input_lines.rs`
```rust
impl TuiApp {
    fn input_lines(&self, width: u16) -> Vec<Line<'static>>
    fn draft_input_lines(&self, width: u16) -> Vec<Line<'static>>
    fn input_status_line(&self, width: u16) -> Line<'static>
    fn composer_line_count(&self, width: u16) -> u16
    fn input_line_plain(&self) -> String
}
```

### `render/panels.rs`
```rust
impl TuiApp {
    fn render_right_panel(&self, f: &mut Frame, rect: Rect)
    fn render_history_panel(&self, f: &mut Frame, rect: Rect)
    fn render_model_picker(&self, f: &mut Frame, rect: Rect)
    fn render_sessions_picker(&self, f: &mut Frame, rect: Rect)
    fn render_command_autocomplete(&self, f: &mut Frame, rect: Rect)
    fn command_autocomplete_panel_height(&self) -> u16
}
```

---

## 预期各文件行数

| 文件 | 预计行数 |
|------|---------|
| `lib.rs` | ~120（struct定义50字段 + mod声明） |
| `types.rs` | ~350 |
| `client.rs` | ~110 |
| `app.rs` | ~200 |
| `input.rs` | ~500（含测试） |
| `stream.rs` | ~850（含测试） |
| `commands.rs` | ~650（含测试） |
| `chat.rs` | ~600（含测试） |
| `selection.rs` | ~500（含测试） |
| `test_helpers.rs` | ~150 |
| `render/helpers.rs` | ~100 |
| `render/mod.rs` | ~400 |
| `render/chat_lines.rs` | ~900（含测试） |
| `render/tool_lines.rs` | ~700 |
| `render/input_lines.rs` | ~350（含测试） |
| `render/panels.rs` | ~550 |

所有文件均 ≤ 1000 行，符合 ≤1200 行原则。

---

## 实施顺序（调整后）

依赖关系图：

```
types.rs
    └── client.rs
    └── app.rs (struct in lib.rs)
            ├── render/helpers.rs（纯函数，无依赖）
            ├── render/tool_lines.rs（依赖 helpers.rs）
            ├── render/input_lines.rs（仅依赖 TuiApp 字段）
            ├── render/panels.rs
            ├── render/chat_lines.rs（依赖 helpers.rs）
            ├── render/mod.rs
            ├── selection.rs
            ├── chat.rs
            ├── stream.rs
            ├── commands.rs
            └── input.rs
```

| 阶段 | 文件 | 说明 |
|------|------|------|
| P0 | `types.rs` | 所有模块依赖，最先拆，解锁后续 |
| P1（可并行） | `client.rs` | 独立 trait，无副作用 |
| P1（可并行） | `render/helpers.rs` | 纯函数，无 struct 依赖 |
| P1（可并行） | `test_helpers.rs` | 测试辅助，不影响主代码 |
| P2 | `render/tool_lines.rs` | 依赖 helpers.rs 完成后 |
| P2 | `render/input_lines.rs` | 仅依赖 TuiApp 字段，可并行 |
| P3 | `selection.rs` | 与主流程解耦度高 |
| P3 | `render/panels.rs` | 独立 render 方法组 |
| P4 | `chat.rs` / `commands.rs` | 核心流程，需仔细迁移测试 |
| P5 | `input.rs` / `stream.rs` | 核心流程，测试最多 |
| P6 | `render/chat_lines.rs` | 行数最多，最后处理 |
| P6 | `render/mod.rs` / `app.rs` | 收尾整理 |

**每步执行后必须**：`cargo build -p talos-ui-tui && cargo test -p talos-ui-tui`

---

## Rust impl 拆分机制说明

Rust 允许同一 struct 的 impl 分散在多个文件，机制如下：

```rust
// lib.rs
pub struct TuiApp { pub(crate) field: T, ... }
mod input;
mod stream;
mod render;

// input.rs — 同 crate 内子模块，可访问 pub(crate) 字段
impl TuiApp {
    pub fn handle_input(&mut self, ...) { self.input_buffer... }
}

// render/mod.rs
impl TuiApp {
    pub fn render_ui(&mut self, f: &mut Frame) { ... }
}
```

注意：
- `mod input` 声明在 `lib.rs` 中，`input.rs` 自动成为子模块
- `render/mod.rs` 对应 `mod render` 声明
- 所有子模块共享 `pub(crate)` 可见性范围
- 方法仍为私有（`fn`），不对 crate 外暴露
