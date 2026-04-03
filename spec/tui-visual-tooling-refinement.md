# Talos TUI 视觉与工具详情优化方案

**版本**: v1.0  
**状态**: Draft  
**最后更新**: 2026-04-03  
**关联文档**:
- `spec/tui-claude-code-style.md`
- `spec/tui-improvement.md`
**目标实现文件**: `talos-rs/apps/ui-tui/src/lib.rs`

---

## 1. 背景

当前 TUI 已完成一轮 Claude Code 风格迁移，但实际使用中暴露出以下问题：

1. 工具块完成后视觉状态不稳定，用户感知仍像 `Running`
2. 工具块折叠态信息密度过低，无法判断“执行了什么”“命中了什么”“结果是否合理”
3. 展开态 details 缺乏按工具类型定制的结构化信息
4. 消息区内容整体过于贴近左边界，阅读压迫感明显
5. 右侧面板默认占据主布局，干扰主阅读路径
6. Assistant 回答被拆散渲染，Markdown 只在局部生效，破坏整体结构

本方案目标不是继续“更像 Claude Code”，而是恢复 Talos TUI 的核心可读性，同时保留已验证有效的流式工具反馈风格。

---

## 2. 用户问题归纳

### 2.1 工具信息表达不足

当前折叠态通常只显示：

```text
● memory_search(talos 项目结构)
└ Running…
```

即使执行完成，也可能仍只给出抽象摘要，缺少：

- 查询条件
- 命中数量
- 命中项预览
- 执行命令
- 工作目录
- exit code / stderr / stdout 归类

这导致用户必须“猜测工具做了什么”，无法快速确认工具行为是否符合预期。

### 2.2 视觉密度分布失衡

当前问题：

- 正文文本贴近左边缘
- block 内部首行与正文起点不统一
- 灰色提示语与正文竞争注意力
- 右侧面板默认存在，主聊天区横向空间被压缩

### 2.3 Markdown 渲染退化

期望行为应为：

- Assistant 整个消息 body 统一按 Markdown 解析
- 再将解析后的 `Line/Span` 结果映射到终端显示

当前退化现象：

- 有序列表局部被渲染
- 标题、段落、引用、代码块之间的整体结构变弱
- 回答更像“按行样式处理”，而不是“完整 Markdown 文档”

---

## 3. 目标与非目标

### 3.1 目标

1. 工具块折叠态即可回答“做了什么、结果怎样”
2. 工具块展开态提供结构化 details，而非无差别输出堆叠
3. 默认主视图专注聊天区，右侧面板可按需打开
4. 统一 block 左侧留白，显著改善阅读舒适度
5. Assistant 恢复整块 Markdown 渲染
6. 工具状态在视觉和数据上保持一致，不出现“完成了但仍像 running”的错觉

### 3.2 非目标

1. 本轮不拆分 `ui-tui/src/lib.rs`
2. 本轮不引入新依赖
3. 本轮不重做输入框和底部状态栏
4. 本轮不改变网关事件协议

---

## 4. 设计原则

### 4.1 信息优先于装饰

工具块首先要回答“发生了什么”，而不是先强调图标或风格。

### 4.2 折叠态也必须可用

折叠态不是“只剩标题”，而是默认视图，必须有足够判断信息。

### 4.3 Assistant 以文档为单位渲染

Markdown 渲染必须以整个回答为输入，而不是按局部行做样式拼接。

### 4.4 主聊天区优先

右侧面板属于诊断辅助区，不能默认压缩主内容阅读宽度。

---

## 5. 目标交互

### 5.1 默认主布局

```text
┌──────────────────────────────────────────────────────────────┐
│                                                              │
│   [chat content only by default]                             │
│                                                              │
│   ● memory_search(talos 项目结构)                            │
│   └ Done (3 hits · 0.8s)                                     │
│     query: talos 项目结构                                     │
│     hit: spec/tui-improvement.md                              │
│     hit: spec/tui-claude-code-style.md                        │
│     (ctrl+o to expand)                                        │
│                                                              │
│   ## TUI 改造总结                                             │
│                                                              │
│   这轮主要解决了以下问题：                                    │
│   1. ...                                                     │
│                                                              │
└──────────────────────────────────────────────────────────────┘
```

特点：

- 聊天正文距离左边界保留统一内边距
- 默认不显示右侧边框和右侧面板
- tool block 折叠态有可判断的内容

### 5.2 右侧面板开启后

```text
┌──────────────────────────────────────────┬───────────────────┐
│                                          │                   │
│   [chat content]                         │   Context         │
│                                          │   Tools           │
│   ● exec_command(cargo test -p ui-tui)   │   Todos           │
│   └ Done (exit:0 · 8.5s)                 │                   │
│     cmd: cargo test -p ui-tui            │                   │
│     dir: ~/data/ailabs/talos/talos-rs    │                   │
│     (ctrl+o to expand)                   │                   │
│                                          │                   │
└──────────────────────────────────────────┴───────────────────┘
```

要求：

- 右侧面板仅在显式切换后显示
- 打开时重新分配宽度
- 关闭后聊天区立即回收宽度

### 5.3 Tool Block 折叠态

#### 搜索类工具

```text
● memory_search(talos 项目结构)
└ Done (3 hits · 0.8s)
  query: talos 项目结构
  hit: spec/tui-improvement.md
  hit: spec/tui-claude-code-style.md
  (ctrl+o to expand)
```

#### 命令类工具

```text
● exec_command(cargo test -p talos-ui-tui)
└ Done (exit:0 · 8.5s)
  cmd: cargo test -p talos-ui-tui
  dir: /Users/xujiang/data/ailabs/talos/talos-rs
  stdout: 48 tests passed
  (ctrl+o to expand)
```

#### 文件读取类工具

```text
● read_file(spec/tui-improvement.md)
└ Done (412 lines · 0.1s)
  path: spec/tui-improvement.md
  preview: # Talos TUI 优化设计文档
  (ctrl+o to expand)
```

### 5.4 Tool Block 展开态

```text
● exec_command(cargo test -p talos-ui-tui)
└ Done (exit:0 · 8.5s)
  cmd: cargo test -p talos-ui-tui
  dir: /Users/xujiang/data/ailabs/talos/talos-rs
  status: success
  ───────────────────────────────
  running 48 tests
  test tests::test_tool_block ... ok
  ...
  (ctrl+o to collapse)
```

展开态要求：

1. 先展示结构化参数区
2. 再展示摘要/元信息
3. 最后展示完整输出
4. 长输出保持缩进，不顶格贴边

---

## 6. 工具块信息模型

### 6.1 新的显示层次

每个 Tool Block 分为三层：

```text
Header
  -> 工具名 + 参数摘要

Status Summary
  -> Running / Done / Error + 关键结果

Detail Preview or Expanded Detail
  -> 折叠态展示关键字段和少量预览
  -> 展开态展示完整结构化信息和完整输出
```

### 6.2 工具类型到详情字段映射

| Tool Type | 折叠态默认字段 | 展开态补充字段 |
|-----------|----------------|----------------|
| `memory_search` | `query`、命中数、前 2 个 hit | 全部 hits、每条摘要、原始输出 |
| `exec_command` | `cmd`、`dir`、`exit`、stdout 首行摘要 | `cmd`、`dir`、`exit`、timing、stdout/stderr 全量 |
| `read_file` | `path`、行数/预览首行 | `path`、完整内容 |
| `write_file` | `path`、写入大小/首行摘要 | `path`、写入内容摘要 |
| `grep/search` | `pattern`、命中数、首批位置 | 全部命中位置、上下文 |
| `http_request` | `method url`、status code | headers/body 摘要与原始内容 |
| fallback | 参数首字段、输出前 2 行 | 原始参数与原始输出 |

### 6.3 Tool Summary 生成规则

#### Running

```text
└ Running...
```

#### Success

按工具类型生成更具体摘要：

- `memory_search`: `└ Done (3 hits · 0.8s)`
- `exec_command`: `└ Done (exit:0 · 8.5s)`
- `read_file`: `└ Done (412 lines · 0.1s)`
- fallback: `└ Done (0.8s)`

#### Error

- `exec_command`: `└ Error (exit:1 · 0.7s)`
- fallback: `└ Error (0.7s)`

### 6.4 结果预览提取策略

折叠态不直接展示完整 `content`，而是做工具感知型提取：

1. 如果可以从结构化 JSON 推导结果摘要，则优先展示摘要
2. 如果没有结构化信息，显示前 1 到 3 行 output preview
3. 如果 output 为空，但有参数，至少显示参数区
4. 如果工具结果是“0 命中/空列表”，要显式写出 `0 hits`

---

## 7. Assistant Markdown 恢复方案

### 7.1 当前问题

当前 Assistant 渲染更接近：

```text
按行装饰文本 -> 局部 span 着色 -> 局部列表勉强可见
```

而非：

```text
完整 Markdown 文本 -> 统一解析 -> 生成结构化 lines -> 做终端换行
```

### 7.2 目标流程

```text
assistant raw content
  -> render_markdown_lines(full_message)
  -> Vec<Line>
  -> wrap_line_visual()
  -> apply default white fg when missing
  -> render
```

### 7.3 关键要求

1. 整个 Assistant 消息只调用一次 Markdown 解析
2. 标题、列表、引用、代码块、分割线保持原有层次
3. 换行发生在 Markdown 解析之后，而不是之前
4. 不允许“只对 1/2/3 列表项做局部 markdown”

### 7.4 期望视觉效果

```text
## 改进完成总结

这轮主要优化了以下内容：

1. 工具块折叠态支持默认展示关键详情
2. Assistant 回答恢复整块 Markdown 渲染
3. 默认主视图隐藏右侧辅助面板

```rust
fn main() {
    println!("hello");
}
```
```

---

## 8. 布局与留白优化

### 8.1 聊天区左侧内边距

新增统一聊天内容左边距 `CHAT_CONTENT_PADDING_LEFT`：

- 推荐值：`2`
- block 首行与后续正文共享同一内容起点系统

### 8.2 Tool / Assistant / System 对齐策略

```text
left border
  + outer padding
    + block content start
      + header
      + body
```

要求：

1. 所有 block 至少离左边界 2 列
2. tool 的详情行相对于 header 再缩进 2 列
3. 提示语如 `(ctrl+o to expand)` 再弱化一级，不抢正文注意力

### 8.3 右侧面板默认隐藏

默认布局：

- 不渲染右侧 panel border
- 不占用右侧宽度
- 聊天区全宽使用

快捷键开启后：

- 渲染右侧 panel
- 主区收缩
- 状态栏显示当前 panel 状态

---

## 9. 快捷键设计

| 快捷键 | 行为 |
|--------|------|
| `Ctrl+O` | 切换最后一个 tool block 的展开/折叠 |
| `Ctrl+B` | 切换右侧面板显示/隐藏 |
| `Tab` | 保留现有工具块焦点切换语义，若已有 |

说明：

1. `Ctrl+B` 新增用于右侧面板 toggle
2. 若当前没有 tool block，`Ctrl+O` 无操作
3. 右侧面板隐藏是默认状态，不依赖配置文件也可工作

---

## 10. 目标渲染流程图

```text
incoming chat block
  |
  v
role dispatch
  |
  +--> User ------> plain text renderer
  |
  +--> System ----> system multiline renderer
  |
  +--> Tool ------> tool detail extractor
  |                  |
  |                  +--> summary builder
  |                  |
  |                  +--> folded preview builder
  |                  |
  |                  +--> expanded output builder
  |
  +--> Assistant -> full markdown renderer
                        |
                        +--> wrapped visual lines
                        +--> color patching
  |
  v
apply chat left padding
  |
  v
compose final rendered lines
```

---

## 11. 核心 API 伪代码

### 11.1 右侧面板开关

```rust
struct TuiApp {
    right_panel_visible: bool,
}

fn toggle_right_panel(&mut self) {
    self.right_panel_visible = !self.right_panel_visible;
}
```

### 11.2 Tool Block 渲染

```rust
fn tool_block_lines(&self, block: &ChatBlock, width: usize) -> Vec<RenderedChatLine> {
    let detail = build_tool_display_detail(block);

    vec![
        render_tool_header(&detail),
        render_tool_summary(&detail),
        render_tool_body(&detail, block.expanded, width),
    ]
}
```

### 11.3 Tool Detail 提取

```rust
fn build_tool_display_detail(block: &ChatBlock) -> ToolDisplayDetail {
    match block.tool_name.as_deref() {
        Some("memory_search") => build_memory_search_detail(block),
        Some("exec_command") => build_exec_command_detail(block),
        Some("read_file") => build_read_file_detail(block),
        _ => build_fallback_tool_detail(block),
    }
}
```

### 11.4 Assistant 整块 Markdown

```rust
fn assistant_block_lines(&self, block: &ChatBlock, width: usize) -> Vec<RenderedChatLine> {
    let markdown_lines = render_markdown_lines(&block.content);
    wrap_and_render_markdown_lines(markdown_lines, width, CHAT_CONTENT_PADDING_LEFT)
}
```

---

## 12. 模块接口清单

| 接口/函数 | 位置 | 变更类型 | 说明 |
|-----------|------|----------|------|
| `tool_block_lines` | `ui-tui/src/lib.rs` | 修改 | 重做工具块折叠/展开呈现 |
| `system_block_lines` | `ui-tui/src/lib.rs` | 修改 | 调整 system 多行渲染与留白 |
| `assistant_block_lines` | `ui-tui/src/lib.rs` | 修改 | 恢复整块 Markdown 渲染路径 |
| `toggle_right_panel` | `ui-tui/src/lib.rs` | 新增 | 切换右侧面板显隐 |
| `build_tool_display_detail` | `ui-tui/src/lib.rs` | 新增 | 抽象工具详情提取 |
| `render_tool_summary` | `ui-tui/src/lib.rs` | 新增 | 统一生成状态摘要文案 |
| `render_tool_folded_preview` | `ui-tui/src/lib.rs` | 新增 | 生成折叠态预览 |
| `render_tool_expanded_body` | `ui-tui/src/lib.rs` | 新增 | 生成展开态详情与输出 |

---

## 13. 分阶段实施建议

### 阶段 1: 恢复主阅读体验

目标：

1. Assistant 恢复整块 Markdown
2. 聊天区统一左侧留白
3. 右侧面板默认隐藏

成功标准：

1. 标题、列表、代码块整体结构恢复
2. 主聊天区宽度明显提升
3. 正文不再贴左边缘

### 阶段 2: Tool Block 信息重构

目标：

1. 重做 tool summary
2. 折叠态补参数与预览
3. 展开态补结构化 details

成功标准：

1. 不展开也能判断工具行为
2. 展开后可见命令/命中/路径等关键信息
3. 完成态和运行态视觉语义明显不同

### 阶段 3: 交互打磨

目标：

1. `Ctrl+B` 切换右侧面板
2. 优化提示语样式和缩进
3. 用真实会话截图回归

成功标准：

1. 键盘切换流畅
2. 视觉节奏统一
3. 手工回归通过

---

## 14. 测试与验收

### 14.1 单元测试

至少新增：

1. tool 折叠态显示参数和预览
2. tool 展开态显示结构化 details
3. memory search 折叠态显示 hits 摘要
4. exec_command 折叠态显示 `cmd/dir/exit`
5. assistant 整块 markdown 保留标题/列表/代码块结构
6. `Ctrl+B` 切换右侧面板状态

### 14.2 手工验证

执行：

```bash
cargo run -p talos-ui-tui
```

验证清单：

1. 默认启动时右侧面板隐藏
2. `Ctrl+B` 后右侧面板显示，再次按下隐藏
3. 提问触发 `memory_search` 时，折叠态能看到 query 和 hit 摘要
4. 提问触发 `exec_command` 时，折叠态能看到 `cmd/dir/exit`
5. `Ctrl+O` 展开后可以看到更完整 details
6. Assistant 回答中的标题、列表、代码块均整体按 Markdown 渲染

---

## 15. 风险与注意事项

### 15.1 风险

1. 如果继续把工具输出只当纯文本处理，details 抽取会长期不稳定
2. 如果右侧面板显隐只改渲染，不改布局计算，可能出现宽度残留问题
3. 如果 Markdown 解析和换行顺序错误，会再次退化成“局部 markdown”

### 15.2 约束

1. 不引入 fallback 文案掩盖真实无结果情况，应明确显示 `0 hits`
2. 不使用任意 metadata JSON 直接透传到 UI 文案层，应优先提取强语义字段
3. 展示信息必须以用户可判断为目标，而不是日志堆砌

---

## 16. 待确认问题

1. 右侧面板 toggle 快捷键是否接受 `Ctrl+B`，还是希望使用 `Ctrl+]` / `F2`
2. tool block 折叠态默认展示 2 行还是 3 行预览更合适
3. `memory_search` 的 hit 文案是否优先展示 path，还是 path + score
4. command 输出是否需要在展开态区分 stdout/stderr 两个分区

