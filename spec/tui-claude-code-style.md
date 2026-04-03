# TUI 消息区域 Claude Code 风格改造设计

**版本**: v1.0  
**状态**: 草案  
**最后更新**: 2026-04-03  
**参考文件**: `talos-rs/apps/ui-tui/src/lib.rs`（当前 ~4800 行）

---

## 1. 背景与目标

### 1.1 问题

当前 TUI 消息区域使用 **Block Card** 风格渲染：每条消息（用户、助手、工具调用）都有独立的 `┌/│/└` 边框包裹，视觉上密度低，边框占用大量垂直空间，工具调用和 AI 回复割裂感强。

```
┌ User ─────────────────────────────────────────────┐
│ 帮我 cargo check 一下                              │
└────────────────────────────────────────────────────┘

┌ ⚙ exec_command ───────────────────────────────────┐
│ ✓ completed  120ms  [Tab to expand]               │
└────────────────────────────────────────────────────┘

┌ Assistant ─────────────────────────────────────────┐
│ 完美！所有检查通过。                                │
└────────────────────────────────────────────────────┘
```

### 1.2 目标

采用 **Claude Code 内联行流（Line-stream）** 风格：无边框，工具调用与 AI 响应内联展示，流式状态用动态 spinner 指示。

```
> 帮我 cargo check 一下

⏺ Bash(cargo check -p talos-agent 2>&1)
  ⎿  Checking talos-logging v0.1.0 ...
     Checking talos-tools-policy v0.1.0 ...
     … +10 lines (ctrl+o to expand)

✢ Thinking… (2s)

⏺ 完美！所有检查通过。
```

### 1.3 范围

仅改造**消息区域渲染逻辑**，不涉及：
- 右侧状态面板
- 输入框
- 布局比例
- 事件处理逻辑

---

## 2. 现状分析（关键路径）

### 2.1 渲染路径

```
chat_blocks (Vec<ChatBlock>)
  └─ rebuild_chat_render_state()           [lib.rs:2981]
       └─ chat_card_lines(block)           [lib.rs:3394]
            ├─ 生成 ┌ header 行
            ├─ block_content_lines(block)  [lib.rs:3184]
            │    ├─ Assistant → render_markdown_lines()
            │    ├─ Thinking  → italic + dim
            │    ├─ Tool      → tool_block_content_lines()
            │    └─ User/Sys  → 纯文本
            └─ 生成 └ footer 行
```

### 2.2 数据结构（不变）

```rust
struct ChatBlock {
    role: ChatRole,          // User/Thinking/Assistant/System/Tool
    content: String,
    tool_name: Option<String>,
    tool_input: Option<serde_json::Value>,
    tool_status: Option<ToolCallStatus>,  // Running / Completed { exit_ok }
    expanded: bool,
    timing_ms: Option<u64>,
}
```

`ChatBlock` 和 `RenderedChatLine` 的数据结构**不需要改变**，改造集中在渲染函数层。

---

## 3. 目标渲染格式

### 3.1 User 消息

```
> 帮我 cargo check 一下
```

- 前缀 `> ` + 青色
- 无边框，无标签

### 3.2 Tool 调用（Running 状态）

```
⏺ Bash(cargo check -p talos-agent 2>&1)
  ⎿  Checking talos-logging v0.1.0 ...
     Checking talos-tools-policy v0.1.0 ...
```

- `⏺`（U+2316）绿色粗体，后接 `ToolName(arg_summary)`
- `⎿`（U+23BF）dim 灰色，首行 output 前缀
- 后续 output 行缩进 5 列（与 `⎿` 内容对齐）

### 3.3 Tool 调用（折叠，Completed）

```
⏺ Bash(cargo check -p talos-agent 2>&1)
  ⎿  … +12 lines (ctrl+o to expand)
```

- 成功：`⏺` 绿色
- 失败：`⏺` 红色 + `✗` 图标

### 3.4 Tool 调用（展开，ctrl+o 触发）

```
⏺ Bash(cargo check -p talos-agent 2>&1)
  ⎿  Checking talos-logging v0.1.0 ...
     Checking talos-tools-policy v0.1.0 ...
     Checking talos-tools-sandbox v0.1.0 ...
     … (全部 12 行，ctrl+o 折叠)
```

### 3.5 Thinking 流式状态

```
✢ Thinking… (3s · ↓ 1.2k tokens)
```

- `✢`（U+2762）+ 帧动画色循环：`Cyan → Blue → Magenta → Cyan`
- 由现有 `spinner_index` 驱动（每 tick 切换颜色）
- 流式结束后此行消失，内容直接追加在 tool 调用之后

### 3.6 Assistant 消息

```
⏺ 完美！所有测试通过。让我输出最终总结：

  ---
  ## 改进完成总结
  ...
```

- `⏺` 白色/浅灰，后接内容首行
- 多行内容缩进 2 列
- Markdown 渲染逻辑不变（复用 `render_markdown_lines()`）

### 3.7 System 消息

```
ℹ Session started · qwen-plus · remote
```

- `ℹ` 前缀 + 灰色 + dim，单行

---

## 4. 实现方案

### 4.1 新增辅助函数

#### `tool_arg_summary(tool_name, input) -> String`

从 `tool_input` 提取人类可读的参数摘要，用于 `⏺ ToolName(summary)` 行：

```rust
fn tool_arg_summary(tool_name: &str, input: &serde_json::Value) -> String {
    match tool_name {
        "exec_command" => input.get("command")
            .and_then(|v| v.as_str())
            .map(|s| truncate(s, 60))
            .unwrap_or_default(),
        "read_file" | "write_file" => input.get("path")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string())
            .unwrap_or_default(),
        _ => {
            // 通用：取第一个字符串字段
            input.as_object()
                .and_then(|m| m.values().find_map(|v| v.as_str()))
                .map(|s| truncate(s, 60))
                .unwrap_or_default()
        }
    }
}
```

#### `spinning_color(spinner_index) -> Color`

```rust
fn spinning_color(spinner_index: usize) -> Color {
    match spinner_index % 4 {
        0 => Color::Cyan,
        1 => Color::Blue,
        2 => Color::Magenta,
        _ => Color::Cyan,
    }
}
```

### 4.2 改造 `chat_card_lines()`

**当前**：输出 `┌ header`、`│ body`、`└ footer` 三段。

**改造后**：按 role 分发，无边框：

```
chat_card_lines(block) ->
  match block.role {
    User      -> inline_user_lines(block)
    Tool      -> inline_tool_lines(block, spinner_index)
    Assistant -> inline_assistant_lines(block)
    Thinking  -> inline_thinking_line(spinner_index)   // 仅在 Running 时追加
    System    -> inline_system_lines(block)
  }
```

> 注：`Thinking` block 在流式完成后不保留，仅作为临时 spinner 行追加在最后。

### 4.3 改造 `tool_block_content_lines()`

折叠时：
```
⎿  … +N lines (ctrl+o to expand)
```

展开时：
```
⎿  line1
   line2
   ...
```

### 4.4 流式 Thinking Spinner 追加

在 `rebuild_chat_render_state()` 末尾，如果当前 agent 处于工作状态（`!active_tool_calls.is_empty()` 或有 Thinking block Running），追加一行动态 spinner：

```rust
if self.is_agent_active() {
    self.rendered_chat_lines.push(thinking_spinner_line(self.spinner_index));
}
```

`is_agent_active()` 判断逻辑：最后一个 block 是 Thinking 且无内容，或有 Running 的 tool call。

---

## 5. 改动范围

| 函数 | 改动类型 | 说明 |
|------|---------|------|
| `chat_card_lines()` [L3394] | 重写 | 去掉边框，按 role 分发 |
| `tool_block_content_lines()` [L3208] | 修改 | 改为 `⎿` 格式 |
| `block_content_lines()` [L3184] | 微调 | Tool/Thinking 路径 |
| `rebuild_chat_render_state()` [L2981] | 微调 | 末尾追加 spinner 行 |
| `chat_role_styles()` [L3470] | 可选删除 | 新格式不再需要统一 role style |
| `decorate_chat_line()` [L3436] | 修改 | 去掉 `│ ` 前缀，改为缩进 |
| 新增 `tool_arg_summary()` | 新增 | 参数摘要提取 |
| 新增 `spinning_color()` | 新增 | spinner 颜色帧 |
| 新增 `thinking_spinner_line()` | 新增 | 流式状态行 |

---

## 6. ASCII 流程图

```
rebuild_chat_render_state()
│
├─ for each ChatBlock:
│    ├─ role == User      → "> {content}" (cyan prefix)
│    ├─ role == Tool
│    │    ├─ "⏺ ToolName(arg_summary)"  (green/red)
│    │    ├─ folded: "  ⎿  … +N lines (ctrl+o to expand)"
│    │    └─ expanded: "  ⎿  line1\n     line2\n..."
│    ├─ role == Assistant → "⏺ {first_line}\n  {rest...}"
│    ├─ role == Thinking  → (skip — handled by spinner)
│    └─ role == System    → "ℹ {content}" (gray)
│
└─ if is_agent_active():
     append "✢ Thinking… ({elapsed}s)" with spinning color
```

---

## 7. 兼容性说明

- `ChatBlock` 数据结构**不变**，存量单测全部保持通过
- `expanded: bool` 字段语义不变，ctrl+o 切换
- `copy_projection` 保持，鼠标复制功能不受影响
- `active_tool_calls` HashMap 保持，只影响 spinner 触发判断
- Markdown 渲染函数 `render_markdown_lines()` 完全复用

---

## 8. 验证方式

1. `cargo build -p talos-ui-tui` 编译无 warning
2. `cargo test -p talos-ui-tui` 全量通过（44 个）
3. 运行 TUI，发一条消息，观察：
   - User 消息显示 `> text`（无边框）
   - 工具执行显示 `⏺ ToolName(args)` + `⎿ output`
   - 流式期间末尾显示 `✢ Thinking…` 动态变色
   - ctrl+o 折叠/展开工具输出

---

*最后更新：2026-04-03*
