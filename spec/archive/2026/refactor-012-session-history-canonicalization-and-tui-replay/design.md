# Session 历史规范化与 TUI 回放改造方案

## 1. 背景

当前 Talos 的 Session 历史存在两个直接问题：

1. 历史回放顺序失真
2. TUI 历史工具块渲染退化为 `unknown`

根因不是单点 UI bug，而是“历史持久化模型”和“实时事件渲染模型”不一致：

- 持久化层当前将一个 turn 拆成 `messages / reasoning / tool_calls / tool_results`
- 历史接口再按固定类别顺序重组
- TUI 历史回放将工具项当普通文本块插入
- TUI 实时渲染则依赖结构化工具块元数据

因此，只要出现工具调用、thinking、最终回答交织的回合，历史回放就会天然失真。

本文目标是给出一版统一方案，让：

- Session 历史只存在一种 canonical 表达
- 服务端历史读取返回顺序稳定、结构完整的 turn
- TUI 的实时渲染与历史回放尽量复用同一套渲染逻辑

本文是设计方案，不包含实现。

---

## 2. 设计目标

### 2.1 目标

1. 一个 turn 内的历史项必须按真实出现顺序持久化。
2. 历史读取不得依赖“按类别拼装”恢复顺序。
3. TUI 历史回放与实时流式渲染应共享同一套 message/tool block 语义。
4. 工具调用历史回放时必须保留 `tool_name / call_id / arguments / result / status`。
5. 不再允许“DB 里看起来有历史、但回放层无内容”的多真源状态。

### 2.2 非目标

1. 本文不覆盖 Web UI。
2. 本文不讨论 workspace memory 的 markdown 格式。
3. 本文不做历史兼容读取链路设计，项目当前阶段优先收敛而非兼容。

---

## 3. 现状问题分析

### 3.1 当前 turn 持久化模型有序信息丢失

当前 `persist_general_turn_artifacts` 落盘的 `TalosTurn` 结构为：

- `messages`
- `reasoning`
- `tool_calls`
- `tool_results`

这意味着一个原始执行序列：

```text
UserInput
Reasoning
ToolCall
ToolResult
OutputText
```

被拆成了 4 组并行数组，而不是一个有序事件流。

因此原始顺序在持久化瞬间已经丢失。

### 3.2 当前历史 API 通过类别顺序重组，天然失真

`get_session_turns` 当前按以下顺序组装 `TalosOutputItem`：

1. `messages`
2. `reasoning`
3. `tool_calls`
4. `tool_results` 当前甚至未完整回放

结果是历史回放顺序不等于真实执行顺序。

### 3.3 TUI 实时与历史渲染走两条不同路径

当前 TUI：

- 实时工具事件使用结构化工具块
- 历史工具项回放时只插入普通 `ChatRole::Tool` 文本块

这会导致：

1. 工具名称丢失，渲染回退为 `unknown`
2. 工具状态丢失，无法正确展示 running/done/error
3. 工具参数摘要与结果摘要退化

### 3.4 当前存在多真源问题

当前系统至少存在两份“历史相关真源”：

- DB `sessions` 元信息
- Session 目录下的持久化 turn/transcript

如果二者不同步，就会出现：

- 列表能看到 session
- 历史区却加载不到内容

这类状态必须被设计层禁止。

---

## 4. 设计原则

### 4.1 单一 canonical turn

一个 turn 只能有一种 canonical 表达，不允许：

- 一份按类别拆分
- 一份按顺序重建
- UI 再自己猜顺序

### 4.2 顺序优先于分类

历史回放最重要的是“顺序”，不是“分类整洁”。

分类可以作为渲染辅助信息存在，但不能代替顺序。

### 4.3 数组顺序就是主排序真源

历史恢复顺序应直接使用 `items` 数组顺序，不依赖按时间戳二次排序。

原因：

- 执行层已经维护了有序 `output_items`
- 数组顺序天然稳定
- 无需引入额外 `item_index`
- 避免同毫秒事件的排序歧义

### 4.4 服务端与 TUI 共享同一种 item 语义

实时流式和历史回放最终都应落到同一组 item 类型：

- user_input
- reasoning
- tool_call
- tool_result
- output_text
- execution_summary

---

## 5. 核心方案

### 5.1 新的 canonical turn 结构

建议将 `TalosTurn` 直接收敛为：

```rust
pub struct TalosTurn {
    pub turn_id: String,
    pub session_id: String,
    pub seq: u64,
    pub timestamp: DateTime<Utc>,
    pub items: Vec<TalosOutputItem>,
}
```

关键点：

1. 不引入新的 item payload 类型
2. 直接复用现有 `TalosOutputItem`
3. `items` 顺序就是历史回放顺序
4. 一个 turn 只保留一种 canonical 表达

### 5.2 关键约束

1. `items` 必须保持原始 append 顺序。
2. `ToolResult.call_id` 必须对应本 turn 里已出现的 `ToolCall.call_id`。
3. 一个 turn 内可有多个 `OutputText` / `Reasoning` item，不做强行合并。
4. `ExecutionSummary` 是否保留，由产品层决定，但若保留也必须进入 `items`。

### 5.3 为什么不继续拆数组

继续拆数组会带来三个问题：

1. 顺序天然丢失
2. tool call 与 tool result 关联只能靠二次推断
3. UI 历史回放无法复用实时渲染模型

因此必须放弃“按类别持久化”的设计。

### 5.3 复用执行层已有的 ordered items

执行层当前已经存在按顺序收集的 `executed.output_items: Vec<TalosOutputItem>`。

因此最小改动路径不是再造一套 canonical item 类型，而是：

1. `TalosTurn.items` 直接使用 `Vec<TalosOutputItem>`
2. `persist_general_turn_artifacts` 直接复用 `executed.output_items`
3. 仅在持久化时将 `UserInput` prepend 到最前面

这样可以避免：

- 二次建模
- 再次重建顺序
- 执行层和持久化层语义漂移

---

## 6. 持久化设计

### 6.1 文件格式

Session 历史文件采用单一 JSONL 格式：

```text
sessions/<session_id>/<YYYY-MM-DD>.jsonl
```

每一行是一条强类型 session artifact record。

当前已经存在统一 artifact envelope，实施时直接复用：

```rust
pub enum SessionArtifactRecord {
    Turn { record: TalosTurn },
    Transcript { record: SessionTranscriptRecord },
    Summary { record: SummaryRecord },
}
```

说明：

- 文件格式只有一种：JSONL
- artifact 类型可以多种，但都走同一个 envelope
- Session 历史回放只消费 `Turn`
- `Transcript` 仅供内部执行态恢复
- `Summary` 仅供 memory/prompt 侧使用

### 6.2 目录职责

| 路径 | 作用 | 是否参与 TUI 历史回放 |
|------|------|----------------------|
| `sessions/<id>/<day>.jsonl` | Session canonical artifacts | 是，仅 `Turn` |
| `memory/<day>.md` | Workspace memory | 否 |
| DB `sessions` 表 | Session 索引元信息 | 否，除列表信息 |

### 6.3 DB 约束

DB `sessions` 表只保留索引型字段：

- `session_id`
- `title`
- `created_at`
- `last_activity`
- `message_count`
- `usage`
- `model`

DB 不保存历史正文。

---

## 7. 服务端改造方案

### 7.1 写入链路改造

当前 `persist_general_turn_artifacts` 应从“拆分数组”改为“直接持久化 ordered items”。

目标流程：

```text
general_turn 执行完成
  |
  v
读取 executed.output_items
  |
  v
在最前面插入 UserInput
  |
  v
append SessionArtifactRecord::Turn
```

伪代码：

```rust
let mut items = vec![TalosOutputItem::UserInput { text: input.to_string() }];
items.extend(executed.output_items.clone());

let turn = TalosTurn {
    turn_id,
    session_id,
    seq,
    timestamp: now,
    items,
};
```

### 7.2 历史读取接口改造

`get_session_turns(session_id)` 不再做类别重组，而是：

1. 从 session artifacts 中读取 `Turn`
2. 直接返回 `turn.items`
3. 不再做类别重组

伪代码：

```rust
async fn get_session_turns(session_id: &str) -> TalosResult<Vec<TalosMessageResponse>> {
    let turns = memory_store.load_session_turns(session_id).await?;

    turns.into_iter().map(|turn| {
        TalosMessageResponse {
            request_id: turn.turn_id.clone(),
            message_id: turn.turn_id.clone(),
            session_id: turn.session_id.clone(),
            model: String::new(),
            output: turn.items,
            reasoning: Vec::new(),
            stop_reason: StopReason::EndTurn,
            usage: TalosUsage::default(),
        }
    }).collect()
}
```

说明：

- `output` 是唯一历史回放真源
- `reasoning` 字段不再作为历史恢复来源

### 7.3 `TalosMessageResponse` 语义收敛

建议明确：

- `output` 是回放真源
- `reasoning` 不再作为历史语义的一部分

本次实施可直接停止在历史链路中使用 `TalosMessageResponse.reasoning`。

### 7.4 服务端模块接口清单

| 模块 | 接口/结构 | 改造点 |
|------|-----------|--------|
| `talos_memory::store` | `TalosTurn` | 收敛为 `items: Vec<TalosOutputItem>` |
| `talos_memory::session_store` | `append_turn/load_turns` | 只读写 canonical turn |
| `agent-kernel/general_turn` | `persist_general_turn_artifacts` | 直接复用 `executed.output_items` |
| `coordinator` | `get_session_turns` | 直接返回 `turn.items` |
| `common/types` | `TalosOutputItem` | 继续作为对外 item 语义 |

### 7.5 Prompt context / hot cache 迁移

这是实施时必须显式处理的一项改造，不能遗漏。

当前：

- `remember_hot_messages` 依赖 `turn.messages`
- `build_session_prompt_context` 也依赖 `turn.messages`

当 `TalosTurn` 收敛为 `items` 后，若不改这两处，会出现：

- hot cache 为空
- prompt context 取不到最近消息
- LLM 上下文静默断裂

因此需要同步改为：

1. 从 `turn.items` 里提取可进入 prompt context 的消息项
2. 至少提取：
   - `UserInput`
   - `OutputText`
3. `Reasoning / ToolCall / ToolResult / ExecutionSummary` 默认不进入 recent_messages

目标流程：

```text
append turn
  |
  v
remember_hot_messages(turn.items -> recent chat messages)
  |
  v
build_session_prompt_context(turn.items -> recent_messages)
```

---

## 8. TUI 改造方案

### 8.1 核心原则

TUI 不应区分“历史工具块”和“实时工具块”两套渲染语义。

目标是统一成一条路径：

```text
实时 SSE 事件
  -> 结构化 block 更新

历史 turn items
  -> 同一套结构化 block 更新
```

### 8.2 当前问题

当前历史回放：

- `ToolCall` -> `push_block(ChatRole::Tool, "...")`
- `ToolResult` -> `push_block(ChatRole::Tool, "...")`

这会丢失：

- `tool_name`
- `tool_input`
- `tool_call_id`
- `tool_status`

而实时工具事件使用的是结构化 block：

- `push_tool_call_block`
- `complete_tool_call_block`

### 8.3 目标渲染模型

建议引入统一回放入口：

```rust
fn replay_history_item(&mut self, item: &TalosOutputItem) {
    match item {
        TalosOutputItem::UserInput { text } => { ... }
        TalosOutputItem::Reasoning { text } => { ... }
        TalosOutputItem::ToolCall { call_id, name, arguments } => {
            self.push_tool_call_block(...)
        }
        TalosOutputItem::ToolResult { call_id, tool_name, content, is_error } => {
            self.complete_tool_call_block(...)
        }
        TalosOutputItem::OutputText { text } => { ... }
        TalosOutputItem::ExecutionSummary { text } => { ... }
    }
}
```

这样：

- 历史工具块与实时工具块使用同一套 block 数据结构
- `unknown` 回退路径只会在真正数据缺失时出现

### 8.4 TUI 历史加载流程

```text
用户切换 session
  |
  v
清空当前 chat blocks
  |
  v
GET /sessions/{id}/turns
  |
  v
for turn in turns:
    for item in turn.output:
        replay_history_item(item)
```

### 8.5 TUI 模块接口建议

| 模块 | 接口 | 改造点 |
|------|------|--------|
| `ui-tui/src/lib.rs` | `SessionTurnsLoaded` 分支 | 改为调用统一 replay 入口 |
| `ui-tui/src/lib.rs` | `push_tool_call_block` | 保留 |
| `ui-tui/src/lib.rs` | `complete_tool_call_block` | 保留 |
| `ui-tui/src/lib.rs` | 新增 `replay_history_item` | 历史与实时桥接层 |

---

## 9. 目标流程图

### 9.1 历史写入

```text
User turn starts
  |
  v
executed.output_items already ordered
  |
  v
prepend user_input
  |
  v
append SessionArtifactRecord::Turn to JSONL
```

### 9.2 历史回放

```text
TUI switch session
  |
  v
GET /sessions/{id}/turns
  |
  v
Coordinator loads TalosTurn[]
  |
  v
use turn.items as-is
  |
  v
TUI replay_history_item(item)
  |
  v
same block model as streaming path
```

---

## 10. API 与伪代码

### 10.1 Core API 伪代码

```rust
pub struct TalosTurn {
    pub turn_id: String,
    pub session_id: String,
    pub seq: u64,
    pub timestamp: DateTime<Utc>,
    pub items: Vec<TalosOutputItem>,
}

async fn append_turn(record: &TalosTurn) -> TalosResult<()>;
async fn load_session_turns(session_id: &str) -> TalosResult<Vec<TalosTurn>>;
async fn get_session_turns(session_id: &str) -> TalosResult<Vec<TalosMessageResponse>>;
fn replay_history_item(app: &mut TuiApp, item: &TalosOutputItem);
fn remember_hot_messages_from_items(turn: &TalosTurn) -> Vec<TalosMessage>;
fn build_session_prompt_context_from_items(session_id: &str) -> SessionPromptContext;
```

### 10.2 模块接口清单表

| 模块 | Trait/Struct/方法签名 | 说明 |
|------|-----------------------|------|
| `talos_memory::store` | `TalosTurn` | Session 历史 canonical turn |
| `talos_memory::session_store` | `append_turn(record: &TalosTurn)` | 持久化 canonical turn |
| `talos_memory::session_store` | `load_session_turns(session_id: &str)` | 读取 canonical turn |
| `talos_coordinator` | `get_session_turns(session_id: &str)` | 历史接口输出 |
| `talos_ui_tui` | `replay_history_item(item: &TalosOutputItem)` | 历史 UI 回放入口 |
| `talos_memory::session_store` | `remember_hot_messages(...)` | 从 `turn.items` 提取 recent chat messages |
| `talos_memory::session_store` | `build_session_prompt_context(...)` | 从 `turn.items` 提取 recent_messages |

---

## 11. 迁移与清理策略

### 11.1 当前阶段策略

项目尚未上线，优先最佳方案，不保留复杂兼容链路。

因此建议：

1. 直接切换到新的 canonical turn schema
2. 清空本地旧 `sessions` 历史目录
3. 清空本地 `sessions` 表运行态数据
4. 不保留 `turn_v2` 与旧分类式 turn 的读取兼容

### 11.2 清理原则

1. 读取链路中不得长期保留多格式 `if/else`
2. 不允许历史 API 猜测旧格式
3. 不允许 TUI 为旧格式做特殊渲染分支

---

## 12. 测试方案

### 12.1 服务端单测

1. 单个 turn 内 `items` 顺序 roundtrip 保持不变
2. 包含 `ToolCall + ToolResult + OutputText` 的 turn 回放顺序正确
3. `get_session_turns` 返回结果包含完整 `ToolResult`
4. `remember_hot_messages` 从 `turn.items` 中正确提取 user/assistant recent messages
5. `build_session_prompt_context` 不因 schema 迁移而静默丢历史消息

### 12.2 TUI 单测

1. 历史回放 `ToolCall` 后 block 具备正确 `tool_name`
2. 历史回放 `ToolResult` 后 block 状态为 completed/error
3. 历史回放与实时流式在相同输入下得到一致 block 结构
4. 历史回放不再出现 `unknown` 工具名回退

### 12.3 集成测试

1. 创建带工具调用的 session
2. 关闭后重新打开 session
3. 校验 UI 历史显示顺序：
   - user
   - reasoning
   - tool call
   - tool result
   - output text

---

## 13. 风险与开放问题

### 13.1 风险

1. 历史 API 与流式事件的 item 语义若未完全对齐，TUI 仍会残留双通路逻辑
2. hot cache / prompt context 若迁移不完整，会造成 LLM 上下文静默断裂
3. 老测试大量依赖旧结构时，改造面会超出预期

### 13.2 开放问题

1. `ExecutionSummary` 是否应继续作为独立 item 暴露给 TUI？
2. `Transcript` 是否仍需持久化到 session artifacts，还是迁移到独立 debug 目录？
3. `TalosMessageResponse.reasoning` 是否直接从历史路径中移除，而不仅是停止使用？

---

## 14. 推荐决策

建议按以下顺序推进：

1. 先改 `TalosTurn` 为 `items: Vec<TalosOutputItem>`，并直接复用 `executed.output_items`
2. 同步迁移 `remember_hot_messages / build_session_prompt_context`
3. 再改 `get_session_turns`，直接返回 `turn.items`
4. 最后改 TUI，统一历史与实时渲染入口

这是最短闭环路径，也最符合当前 Talos “不做兼容包袱设计”的项目约束。
