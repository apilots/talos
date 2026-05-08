# TUI 输入交互、中断语义与 Session Title 设计方案

## 1. 背景

当前 `ui-tui` 存在三类可用性问题：

1. 任务执行中按 `Esc` 无法真正中断当前回合，尤其在 embedded 模式下问题明显。
2. 粘贴长 Markdown / 日志 / 代码时，会被拆成多条输入并提前提交，导致上下文破碎与截断。
3. Session 缺少稳定标题，`/sessions` 只能看到短 session id，切换体验差。

这三个问题虽然表面分散，但都落在同一条“会话交互模型”主线上：

- TUI 应明确区分普通键入、结构化粘贴、执行中断。
- 服务端应明确维护“当前回合状态”和“Session 展示元数据”。
- UI 展示不应依赖无语义 id，而应依赖稳定标题与清晰状态。

本文目标是给出一版可实施、可测试、与当前 Talos 架构一致的初版方案。

---

## 2. 设计目标

### 2.1 目标

1. `Esc` 在聊天主界面执行中时，能够真正取消当前回合，而不是只中断本地 UI task。
2. 长文本粘贴进入 TUI 后应形成 pasted block，而不是拆成多次提交。
3. 用户仍可在 pasted block 之外继续输入补充说明，并直接按 `Enter` 发送整包问题。
4. Session 必须持久化 `title` 字段；创建时先写占位标题 `Untitled Session`，首轮完成后必须覆盖为真实标题。
5. `/sessions` 与历史选择面板必须优先展示 Session title。

### 2.2 非目标

1. 本文不覆盖 Web UI，仅覆盖 TUI 与服务端 Session / turn 协议。
2. 本文不引入通用 metadata json 兜底字段。
3. 本文不在第一阶段支持富附件类型，仅支持 pasted text block。

---

## 3. 现状问题分析

### 3.1 `Esc` 中断现状

TUI 主输入态下，`Esc` 当前只清除聊天选择：

- [talos-rs/apps/ui-tui/src/lib.rs](/Users/xujiang/data/ailabs/talos/talos-rs/apps/ui-tui/src/lib.rs#L812)

本地仅有 `abort_pending_stream_task()`，但它只会终止发起流连接的本地 task：

- [talos-rs/apps/ui-tui/src/lib.rs](/Users/xujiang/data/ailabs/talos/talos-rs/apps/ui-tui/src/lib.rs#L2800)

当前服务端没有 cancel 当前 turn 的公开能力：

- HTTP 路由没有 cancel endpoint：
  - [talos-rs/crates/gateway/http/src/server.rs](/Users/xujiang/data/ailabs/talos/talos-rs/crates/gateway/http/src/server.rs#L160)
- `Gateway` trait 没有 cancel：
  - [talos-rs/crates/gateway/core/src/trait.rs](/Users/xujiang/data/ailabs/talos/talos-rs/crates/gateway/core/src/trait.rs#L7)
- `RequestHandler` trait 没有 cancel：
  - [talos-rs/crates/common/types/src/lib.rs](/Users/xujiang/data/ailabs/talos/talos-rs/crates/common/types/src/lib.rs#L951)

因此，embedded 模式下即使 UI 侧捕获 `Esc`，也无法通知 kernel 停止当前执行。

### 3.2 长文本粘贴现状

事件循环只处理 `Event::Key` 与 `Event::Mouse`，没有启用 bracketed paste，也没有处理 `Event::Paste`：

- [talos-rs/apps/ui-tui/src/run.rs](/Users/xujiang/data/ailabs/talos/talos-rs/apps/ui-tui/src/run.rs#L46)

输入框里任何 `Enter` 都直接触发 `submit_input()`：

- [talos-rs/apps/ui-tui/src/lib.rs](/Users/xujiang/data/ailabs/talos/talos-rs/apps/ui-tui/src/lib.rs#L817)

这会导致长 Markdown 粘贴里的换行被终端解释成多次回车，从而拆成很多条提交。

### 3.3 Session title 现状

当前 Session 模型没有 `title` 字段：

- `SessionRecord` 无 `title`：
  - [talos-rs/crates/common/types/src/lib.rs](/Users/xujiang/data/ailabs/talos/talos-rs/crates/common/types/src/lib.rs#L610)
- `TalosSession` 无 `title`：
  - [talos-rs/crates/common/types/src/lib.rs](/Users/xujiang/data/ailabs/talos/talos-rs/crates/common/types/src/lib.rs#L706)
- SQLite `sessions` 表无 `title` 列：
  - [talos-rs/crates/infra/storage/src/sqlite.rs](/Users/xujiang/data/ailabs/talos/talos-rs/crates/infra/storage/src/sqlite.rs#L257)

因此 `/sessions` 只能展示短 session id：

- [talos-rs/apps/ui-tui/src/lib.rs](/Users/xujiang/data/ailabs/talos/talos-rs/apps/ui-tui/src/lib.rs#L3319)

---

## 4. 目标交互模型

### 4.1 输入模型

TUI 输入区拆成两部分：

1. `draft_text`
   - 用户当前键入的短文本说明
   - 保持单行输入体验
2. `pasted_blocks`
   - 用户粘贴的大块文本
   - 在 UI 中以 block 形式显示

长文本粘贴不再直接写入 `draft_text`，而是生成 block。

### 4.2 发送模型

用户按 `Enter` 时发送整个草稿包：

- `draft_text`
- `pasted_blocks`

因此用户可以：

1. 先粘贴一大段 Markdown
2. 再在输入框补一句说明
3. 直接按 `Enter` 发送

无需改成 `Ctrl+Enter` 发送。

### 4.3 Session title 模型

Session title 采用强语义字段：

```rust
pub struct SessionRecord {
    pub id: String,
    pub title: String,
    // ...
}

pub struct TalosSession {
    pub title: String,
    // ...
}
```

语义约束：

1. title 不能为空字符串
2. title 不是 `Option<String>`
3. 新 Session 创建时先写占位标题 `Untitled Session`
4. 首轮完成后必须覆盖成真实标题

---

## 5. 目标流程图

### 5.1 pasted block 输入流程

```text
terminal paste event
  |
  v
TUI Event::Paste(text)
  |
  +--> short plain text
  |      |
  |      +--> insert into draft_text
  |
  +--> long / multiline text
         |
         +--> create pasted block
         +--> render summary chip
         +--> keep draft_text editable
  |
  v
user presses Enter
  |
  v
compose request from:
  - draft_text
  - pasted_blocks
  |
  v
send one logical user turn
```

### 5.2 `Esc` 中断流程

```text
chat state + in_flight = true
  |
  v
user presses Esc
  |
  v
TUI sends cancel_active_turn(session_id)
  |
  v
HTTP cancel endpoint
  |
  v
Gateway / RequestHandler / Coordinator
  |
  v
Agent runtime lookup active turn
  |
  v
CancellationToken.cancel()
  |
  v
kernel / tool execution stop cooperatively
  |
  v
stream emits cancelled completion
```

### 5.3 Session title 生成流程

```text
create session
  |
  v
persist title = "Untitled Session"
  |
  v
first successful user turn completes
  |
  v
generate semantic title from first user problem
  |
  v
validate title non-empty + bounded length
  |
  v
update session title in storage
  |
  v
/sessions shows title
```

---

## 6. 核心设计

### 6.1 问题 1：`Esc` 中断执行

#### 6.1.1 设计原则

1. 不做 UI 假中断。
2. 必须存在服务端强类型 cancel 能力。
3. embedded 与 remote 共用同一套 turn cancel 协议。

#### 6.1.2 协议补充

新增服务端能力：

```rust
#[async_trait]
pub trait Gateway: Send + Sync {
    async fn handle_request(&self, request: GatewayRequest) -> TalosResult<GatewayResponse>;
    async fn handle_request_stream(
        &self,
        request: GatewayRequest,
    ) -> TalosResult<mpsc::Receiver<TalosEvent>>;
    async fn cancel_active_turn(&self, session_id: &str) -> TalosResult<()>;
}
```

`RequestHandler`、`SessionService`、`AgentOrchestrator` 同步增加 `cancel_active_turn(session_id: &str)`。

#### 6.1.3 运行时模型

在 agent runtime 层维护：

```rust
pub struct ActiveTurnHandle {
    pub session_id: String,
    pub invocation_id: String,
    pub cancel_token: CancellationToken,
}

pub type ActiveTurnRegistry = Arc<DashMap<String, ActiveTurnHandle>>;
```

约束：

1. 每个 session 同时只允许一个 active turn。
2. 新 turn 启动时注册 handle。
3. turn 结束、报错、取消后清理 handle。
4. `active_turns` 必须是并发安全容器；推荐 `Arc<DashMap<String, ActiveTurnHandle>>`，若项目依赖约束不允许，再退回 `Arc<Mutex<HashMap<...>>>`。

#### 6.1.4 TUI 语义

`Esc` 行为按优先级分层：

1. 若存在 approval / picker / history modal，则沿用当前 modal 取消语义。
2. 否则若 `current_state == Chat && in_flight == true`，触发 cancel active turn。
3. 否则执行当前的 selection clear。

#### 6.1.5 stop reason

统一采用强类型方案 A，不再继续使用跨层 `String` 传递 stop reason。

也就是说：

1. kernel 内部使用 `StopReason` 枚举
2. 对外事件与响应中的 `stop_reason` 字段同步改为 `StopReason`
3. 不采用“内部枚举 + 外部字符串 `cancelled`”的折中方案

统一新增：

```rust
pub enum StopReason {
    EndTurn,
    Cancelled,
    // existing...
}
```

涉及范围包括但不限于：

1. `TalosMessageCompletedEvent.stop_reason`
2. `GatewayResponse.stop_reason`
3. `TalosMessageResponse.stop_reason`

TUI 侧收到 `cancelled` 后：

1. 停止 spinner / thinking 状态
2. 记录系统提示
3. 不再继续等待后续 delta

### 6.2 问题 2：长文本粘贴 block 化

#### 6.2.1 设计原则

1. `Enter` 保持“发送”语义，不改成组合键发送。
2. 长文本 paste 走 pasted block，而不是 multiline 编辑器主路径。
3. pasted block 只属于 TUI 输入层语义，不进入服务端协议。
4. 普通短文本粘贴即使带换行，也只进入 `draft_text`，换行作为输入内容保留，不再被当成多次回车提交。

#### 6.2.2 pasted block 判定规则

同时满足以下两个条件才视为 pasted block：

1. 文本包含换行
2. 文本字符数超过阈值，例如 200

否则按普通文本插入 `draft_text`。

等价逻辑：

```rust
fn should_create_pasted_block(text: &str) -> bool {
    text.contains('\n') && text.chars().count() > 200
}
```

设计理由：

1. 真正的大块多行内容更适合 block 化。
2. 普通短文本粘贴不应被过度结构化，否则会破坏快速编辑体验。
3. 短文本粘贴中的换行必须只进入输入框，不应再被解释成多次提交。

#### 6.2.3 TUI 状态结构

建议新增：

```rust
pub struct PastedTextBlock {
    pub id: u64,
    pub text: String,
    pub line_count: usize,
    pub char_count: usize,
    pub preview: String,
}

pub struct InputComposerState {
    pub draft_text: String,
    pub pasted_blocks: Vec<PastedTextBlock>,
}
```

#### 6.2.4 输入发送结构

服务端不感知 pasted block 语义。

也就是说：

1. pasted block 只是 TUI 编辑态表示。
2. TUI 在发送前将 `draft_text + pasted_blocks` 组装成一个完整字符串。
3. 服务端看到的仍然只有一个普通 `TalosInputItem::Text { text }`。

推荐组装格式：

```text
{draft_text}

--- Pasted content #1 ---
{raw pasted text}

--- Pasted content #2 ---
{raw pasted text}
```

约束：

1. pasted block 的边界由客户端拼装文本时显式保留。
2. 服务端不新增协议字段，不改 prompt 组装逻辑，不区分 paste 与键入。
3. pasted block 只解决 TUI 输入体验，不提升为通用 API 语义。

#### 6.2.5 UI 呈现

输入区建议如下：

```text
> explain the root cause and propose a fix
  [Pasted text #1 · 31 lines]
  [Pasted text #2 · 482 chars]
```

交互约束：

1. `Enter` 发送整个 composer
2. 输入框为空时 `Backspace` 删除最后一个 pasted block
3. 支持选中 block 并查看预览 / 删除
4. 发送后 `draft_text` 与 `pasted_blocks` 一并清空

#### 6.2.6 bracketed paste

TUI 启动时启用：

```rust
EnableBracketedPaste
```

退出时关闭：

```rust
DisableBracketedPaste
```

事件循环新增：

```rust
Event::Paste(text) => app.handle_paste(text)
```

### 6.3 问题 3：Session title 强约束

#### 6.3.1 字段定义

Session title 使用强类型、必填字段：

```rust
pub struct SessionRecord {
    pub id: String,
    pub title: String,
    pub context: TalosContext,
    pub model: String,
    // ...
}

pub struct TalosSession {
    pub title: String,
    pub context: SessionContext,
    pub config: TalosConfig,
    pub state: SessionState,
}
```

#### 6.3.2 存储约束

SQLite `sessions` 表新增：

```sql
title TEXT NOT NULL
```

新建 Session 时：

```text
title = "Untitled Session"
```

#### 6.3.3 真实标题生成规则

首轮完成后必须覆盖标题，建议输入来源：

1. 首条有效用户问题为主
2. 必要时辅以首轮 assistant 输出做去歧义

标题生成策略分两级：

1. 优先使用轻量模型生成语义标题，不使用当前会话的高成本主模型。
2. 若轻量模型调用失败、超时或返回非法结果，则回退到 deterministic 规则提炼。

推荐模型策略：

1. 使用低成本、低延迟模型，例如 haiku 级别能力模型。
2. 不绑定 session 当前模型，避免高成本模型为标题生成额外消耗。

输出约束：

1. 单行
2. 无尾部句号
3. 语义简洁
4. 最大长度受限，例如 48 chars
5. 非空

若生成结果非法，则回退到 deterministic 标题生成器，而不是保留 `Untitled Session`。

deterministic fallback 建议：

1. 取首条 user message
2. 去掉首尾空白与冗余空行
3. 截取前 N 个字符或词，例如 48 chars
4. 去掉尾部标点
5. 若仍为空，则使用 `Untitled Session`

#### 6.3.4 为什么不能一直保留占位标题

因为目标语义是：

1. 创建时允许短暂占位
2. 首轮完成后必须得到真实标题
3. 首轮完成后仍保留 `Untitled Session` 视为逻辑错误

#### 6.3.5 更新时机

推荐在 coordinator 记录 turn 完成的相邻路径中执行：

```text
turn completed
  |
  +--> record usage / message_count
  |
  +--> maybe_assign_session_title()
```

这样标题生成与 Session 生命周期一致，不依赖 TUI。

---

## 7. 模块接口清单

| 接口/模块 | 位置 | 变更类型 | 说明 |
|-----------|------|----------|------|
| `run_event_loop` | `apps/ui-tui/src/run.rs` | 修改 | 启用 bracketed paste，处理 `Event::Paste` |
| `TuiApp::handle_input` | `apps/ui-tui/src/lib.rs` | 修改 | `Esc` 在执行中触发 cancel；`Enter` 发送 composer |
| `TuiApp::handle_paste` | `apps/ui-tui/src/lib.rs` | 新增 | 将长文本转成 pasted block |
| `InputComposerState` | `apps/ui-tui/src/lib.rs` | 新增 | 统一管理输入草稿与 pasted block |
| `TalosClient::cancel_active_turn` | `apps/sdk/src/lib.rs` | 新增 | 客户端 cancel 接口；embedded 当前仍通过本地 HTTP gateway 调用 |
| `Gateway::cancel_active_turn` | `crates/gateway/core` | 新增 | turn 取消能力 |
| `RequestHandler::cancel_active_turn` | `crates/common/types` | 新增 | orchestration 取消入口 |
| `SessionService::cancel_active_turn` | `crates/common/types` | 新增 | session 级 turn 取消入口 |
| `AgentOrchestrator::cancel_active_turn` | `crates/orchestration/agent` | 新增 | agent 当前回合取消 |
| `SessionRecord.title` | `crates/common/types` | 新增 | Session 持久化标题 |
| `TalosSession.title` | `crates/common/types` | 新增 | Session 对外展示标题 |
| `sessions.title` | `crates/infra/storage` | 新增 | SQLite schema 字段 |
| `SessionStore::update_session_title` | `crates/orchestration/session-store` | 新增 | 持久化更新 Session title |
| `CoordinatorImpl::maybe_assign_session_title` | `crates/orchestration/coordinator` | 新增 | 首轮完成后生成并更新标题 |
| `/sessions` picker render | `apps/ui-tui/src/lib.rs` | 修改 | 用 title 替代短 session id 作为主展示 |

---

## 8. 核心 API 伪代码

### 8.1 pasted block 处理

```rust
fn handle_paste(&mut self, text: String) {
    if should_create_pasted_block(&text) {
        self.input_composer
            .pasted_blocks
            .push(PastedTextBlock::from_text(text));
    } else {
        self.input_composer.insert_into_draft_text(&text);
    }
}
```

### 8.2 发送 composer

```rust
async fn submit_input(&mut self) -> TalosResult<bool> {
    if !self.input_composer.has_meaningful_content() {
        self.reset_input();
        return Ok(false);
    }

    let text = self.input_composer.compose_user_input_text();
    self.send_composed_message(vec![TalosInputItem::Text { text }]).await
}
```

说明：

1. `compose_user_input_text()` 由客户端负责把 `draft_text` 和 pasted blocks 拼成一段完整文本。
2. 服务端继续只处理一个普通文本 input。
3. 普通短文本粘贴中的换行在 `draft_text` 中原样保留，不再触发隐式提交。

### 8.3 `Esc` 中断

```rust
async fn handle_escape(&mut self) -> TalosResult<()> {
    if self.pending_approval.is_some() {
        self.submit_approval(false).await;
        return Ok(());
    }

    if self.in_flight {
        self.cancel_active_turn().await?;
        return Ok(());
    }

    self.chat_selection = None;
    Ok(())
}
```

### 8.4 Session title 赋值

```rust
async fn maybe_assign_session_title(
    &self,
    session_id: &str,
    first_user_text: &str,
    first_assistant_text: &str,
) -> TalosResult<()> {
    let session = self.sessions.get_session(session_id).await?
        .ok_or_else(|| TalosError::ResourceNotFound(format!("session {}", session_id)))?;

    if session.has_real_title() {
        return Ok(());
    }

    let generated = self
        .title_generator
        .generate(first_user_text, first_assistant_text)
        .await
        .unwrap_or_default();

    let title = validate_or_fallback_title(generated, first_user_text);
    self.sessions.update_session_title(session_id, &title).await
}
```

相关职责建议拆分为三个本地函数：

```rust
fn validate_title(title: &str) -> Option<String>;
fn deterministic_title(first_user_text: &str) -> String;
fn validate_or_fallback_title(generated: String, first_user_text: &str) -> String;
```

约束：

1. `title_generator.generate(...)` 可以失败，因为它依赖模型调用。
2. `validate_or_fallback_title(...)` 不返回 `Result`，因为它的职责就是总能收敛到一个合法标题。
3. 当 `generated` 非法时，回退到 deterministic 标题。
4. 当 `first_user_text` 也无法提炼出有效标题时，最终返回 `Untitled Session`。
5. 因此 LLM 失败不会中断标题赋值流程，只会退化为 deterministic fallback。

### 8.5 active turn 取消

```rust
async fn cancel_active_turn(&self, session_id: &str) -> TalosResult<()> {
    let Some(handle) = self.active_turns.get(session_id) else {
        return Ok(());
    };

    handle.cancel_token.cancel();
    Ok(())
}
```

说明：

1. `no active turn` 视为 no-op，不视为错误。
2. 用户按 `Esc` 的瞬间，turn 可能已经正常完成；此时返回 `Ok(())` 才符合交互语义。

---

## 9. 存储与迁移建议

### 9.1 schema 迁移

SQLite `sessions` 表新增 `title TEXT NOT NULL`。

对旧库建议采用显式迁移逻辑：

1. 检查是否存在 `title` 列
2. 若不存在，则添加列
3. 默认填充 `Untitled Session`
4. 后续通过历史消息回填真实标题

### 9.2 历史数据补标题

建议批量补写规则：

1. 优先读取 session 第一条 user message
2. 用 deterministic title 规则提炼
3. 若仍失败，暂写 `Untitled Session`

注意：

- 对历史库允许迁移后短期残留 `Untitled Session`
- 对新创建 Session，首轮完成后不允许残留 `Untitled Session`
- 历史数据回填不要在启动阶段批量触发 LLM 调用；deterministic 回填即可，避免历史库大时阻塞启动

---

## 10. 测试与验收

### 10.1 单元测试

至少新增：

1. `Event::Paste` 的长文本生成 pasted block
2. 短文本 paste 仍插入 `draft_text`
3. `Enter` 发送 `draft_text + pasted_blocks`
4. 输入框为空时 `Backspace` 删除最后一个 pasted block
5. 执行中 `Esc` 调用 cancel 路径
6. `cancelled` stop_reason 正确透传到 TUI
7. Session 创建时 title 为 `Untitled Session`
8. 首轮完成后 title 被覆盖成真实标题
9. 旧 session migration 后 title 非空
10. `/sessions` 列表优先展示 title

### 10.2 集成测试

至少新增：

1. embedded 模式长任务执行中按 `Esc`，回合被取消
2. remote 模式长任务执行中按 `Esc`，回合被取消
3. 粘贴 30 行 Markdown，仅形成一个逻辑用户请求
4. 首轮完成后查询 session，title 不再是 `Untitled Session`

### 10.3 手工验收

1. 粘贴长 Markdown 后，输入区出现 `[Pasted text #n · x lines]`
2. 用户还能继续写补充说明
3. 直接按 `Enter` 发送整包内容
4. 执行中按 `Esc`，UI 明确提示已取消
5. `/sessions` 中能按语义标题快速选择会话

---

## 11. 分阶段实施建议

### 阶段 1：Session title 强类型落库

目标：

1. `SessionRecord` / `TalosSession` 新增 `title: String`
2. `sessions` 表新增 `title`
3. create/list/get/update 全链路返回 title

成功标准：

1. 新 Session 创建后 title 恒为非空
2. `/sessions` 接口可以拿到 title

### 阶段 2：首轮自动命名

目标：

1. 新 Session 初始 title 为 `Untitled Session`
2. 首轮完成后自动覆盖为真实标题

成功标准：

1. 首轮完成后不再保留占位标题
2. 标题生成失败也会得到 deterministic fallback

### 阶段 3：pasted block 输入模型

目标：

1. 启用 bracketed paste
2. 长文本粘贴生成 pasted block
3. `Enter` 发送整个 composer

成功标准：

1. 长 Markdown 不再拆成多次提交
2. 用户可在 pasted block 之外继续写说明

### 阶段 4：turn cancel 协议

目标：

1. 补齐服务端 cancel 当前 turn 的协议
2. TUI 执行中 `Esc` 触发 cancel

成功标准：

1. embedded / remote 模式都能真实中断当前回合
2. stop reason 统一为 `cancelled`

### 阶段 5：TUI 展示收尾

目标：

1. `/sessions` 列表展示 title
2. pasted block 支持删除、预览、状态提示
3. 中断提示和输入状态提示统一

成功标准：

1. 会话切换与长文本输入体验稳定
2. 手工回归通过

---

## 12. 决策总结

本文的关键决策有三条：

1. `Esc` 中断必须走服务端真实 cancel 协议，不接受 UI 假中断。
2. 长文本粘贴采用 pasted block / 附件化输入模型，而不是把发送键改成组合键。
3. Session title 采用强类型、非空字段；创建时允许短暂占位，但首轮完成后必须覆盖为真实标题。

这三条决策共同收敛到一个方向：

- 输入是结构化的
- 回合是可取消的
- Session 是可识别的

这样后续无论继续扩展 TUI，还是扩展 Web UI / SDK，都会有更稳定的一致语义基础。
