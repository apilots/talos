# Talos Plan-ReAct 本地项目架构分析优化方案

**状态**: 草案  
**最后更新**: 2026-04-04  
**范围**: 仅整理方案，不修改代码实现

---

## 1. 背景

在 Talos 当前的 `PlanReAct` 模式下，针对“分析本地绝对路径项目架构”类请求，已经出现了两类典型问题：

1. 模型在已具备本地文件工具能力的情况下，仍会走浅层轨迹：
   - `list_dir`
   - 多次 `read_file`
   - 直接输出架构总结

2. 模型输出了 `/*ACTION*/`，但后续没有有效动作内容，或者没有补足预期的结构化探索步骤。

这说明当前问题已经不再是“完全不会调用工具”，而是：

- 工具能力表达不一致
- prompt 对任务轨迹约束过软
- flow 对异常轨迹缺少明确检查
- TUI 对协议异常的可观测性不够

本方案文档仅用于收敛问题、提出优化方向，供 review 使用。

---

## 2. 目标与非目标

### 2.1 目标

1. 提升本地项目架构分析任务的轨迹稳定性
2. 明确 `list_dir`、`grep_files`、`read_file` 在架构分析中的职责边界
3. 识别并规范 `/*ACTION*/` 空动作异常
4. 收敛 prompt、工具描述、flow、TUI 可观测性的改进方向
5. 输出可分阶段落地的优化清单

### 2.2 非目标

1. 不引入独立 intent classifier
2. 不新增基于输入前缀的规则路由
3. 不引入第二套 planner 或第二套工具执行引擎
4. 不在本次文档阶段直接修改 Rust 实现

---

## 3. 现象复盘

### 3.1 典型轨迹

用户请求：

```text
分析 /Users/xujiang/data/ailabs/PageIndex 项目架构，使用 plan react，始终使用中文
```

实际轨迹：

```text
list_dir(project_root)
read_file(README.md)
read_file(requirements.txt)
read_file(run_pageindex.py)
read_file(pageindex/page_index.py)
final_answer
```

模型文本轨迹中同时出现：

- `/*PLANNING*/`
- `/*ACTION*/`
- `/*REASONING*/`
- `/*FINAL_ANSWER*/`

但缺失：

- `grep_files`
- 模块关系搜索
- 对 `README.md truncated=true` 的显式处理
- 对已声明“后续读取项”的兑现检查

### 3.2 轨迹问题总结

| 问题 | 表现 | 风险 |
|------|------|------|
| 结构搜索缺失 | 未调用 `grep_files` | 架构分析停留在“读几个文件”的浅层浏览 |
| 过早收尾 | 读少量关键文件后直接 final | 报告缺少模块边界、调用链、导出关系 |
| 推理冗长 | 每步重复“已读取/下一步应” | token 浪费，TUI 噪声高 |
| 证据分层不清 | 将 README 文案、代码事实、推断混合 | 最终结论可信度下降 |
| 协议异常不可见 | `/*ACTION*/` 之后缺少有效动作反馈 | 用户难以判断是模型停住还是执行链异常 |

---

## 4. 根因分析

### 4.1 Prompt 对“架构分析”的轨迹约束过软

当前 prompt 会建议：

- 先 `list_dir`
- 再搜索
- 再读文件

但该约束更接近“推荐路径”，不是“最低合格轨迹”。  
模型很容易走以下最省事路径：

```text
list_dir -> read_file -> read_file -> read_file -> final
```

在模型看来，这已经足以生成一份“看起来合理”的总结。

### 4.2 工具描述没有建立清晰的职责边界

对模型而言，当前三个工具很容易被混淆：

- `list_dir`: 目录探测
- `read_file`: 内容读取
- `grep_files`: 普通搜索工具

问题在于 `grep_files` 没有被塑造成“架构发现阶段的核心工具”，而只是一个可选检索工具。因此模型倾向于直接跳过它。

### 4.3 Flow 对轨迹质量没有做硬检查

当前 flow 更关注：

- 是否有 tool calls
- 是否有 final answer

但没有充分检查：

- 目录级架构分析是否做过结构搜索
- 是否存在“声明下一步但未完成”的情况
- 是否存在 `/*ACTION*/` 但没有有效 act 结果的情况

因此模型只要产生了一个“形式合法”的 final answer，就可能结束整轮。

### 4.4 TUI 无法清晰暴露协议层异常

当模型输出：

```text
/*ACTION*/
```

而没有进一步可见的工具阶段信息时，用户很难判断：

- 是模型没有发起工具调用
- 还是工具已调用但渲染链没有给出有效反馈
- 还是当前轮次被提前终止

这会显著增加调试成本。

---

## 5. 特殊异常：`/*ACTION*/` 空动作问题

### 5.1 问题定义

本文将以下现象定义为 **空动作异常（Empty Action Step）**：

```text
模型输出了 `/*ACTION*/`
但该步骤之后：
1. 没有实际 tool call
或
2. 有工具调用，但没有与其对应的可解释动作内容
或
3. 声明了下一步动作，但未兑现就直接结束
```

### 5.2 为什么这很奇怪

`PlanReAct` 中的 `ACTION` 不应只是一个格式标签，而应代表“准备执行或正在执行具体动作”的协议状态。

因此：

- 仅输出 `/*ACTION*/` 文本，本身不能算有效动作
- 仅说“现在读取某文件”，但未形成与之对应的执行链，也不能算有效动作

如果允许空动作通过，会导致：

1. 用户误以为系统卡住
2. flow 难以判断是正常结束还是无进展
3. TUI 看起来像“执行到一半没东西了”

### 5.3 优化原则

对于 `/*ACTION*/`，建议后续实现满足以下原则：

1. `ACTION` 必须可被解释为一个有效 act step
2. 若无工具调用，不能把 `ACTION` 视为完成态
3. 若 `ACTION` 后未产生有效推进，应进入 replan 或 no-progress 分支
4. TUI 应能明确标识“空动作异常”，而不是沉默结束

---

## 6. 设计目标

### 6.1 任务轨迹目标

针对“本地目录 + 项目架构分析”请求，目标轨迹应为：

```text
1. list_dir(项目根目录)
2. grep_files(入口 / 核心符号 / 配置 / 导出关系)
3. read_file(README / manifest / 入口 / 核心模块)
4. 输出按证据分层的架构总结
```

### 6.2 最低合格标准

对于目录级架构分析，至少需要满足：

1. 已完成目录探测
2. 已完成至少一次结构搜索
3. 已读取至少一个入口文件或对外 API 文件
4. 已读取至少一个核心模块文件
5. 最终回答区分“代码事实 / 文档声明 / 推断”

---

## 7. 任务识别策略

### 7.0 Session 环境信息

建议不要为 `directory` 访问语义再引入额外散字段，而是在 `Session` 上统一挂一个最小环境信息结构：

```rust
pub struct SessionEnvironment {
    pub directory: PathBuf,
    pub connection_mode: ConnectionMode, // Embedded | Remote
}
```

实例化责任建议如下：

- 主要由 TUI 在创建或恢复 session 时填充
- `directory` 取自 TUI 启动时的当前本地目录
- `connection_mode` 取自 TUI 当前连接模式（`embedded` / `remote`）
- 后续由 session metadata 透传给 planner / flow / prompt 组装逻辑消费

其语义约定如下：

- `directory`
  - 表示 TUI 启动时的本地当前目录
  - UI 展示时可渲染为 `~/data/ailabs/talos` 这类短路径
- `connection_mode`
  - 表示当前 TUI 的连接模式
  - `Embedded` 表示执行端与 TUI 共享本地执行环境
  - `Remote` 表示执行端不应默认拥有对 TUI 本地目录的访问能力

后续本地目录分析相关的 prompt 和 flow 逻辑，均应消费该结构，而不是继续依赖模糊的 `workspace` 概念。

### 7.1 识别原则

“目录架构分析任务”不应依赖模型自报，也不应依赖自由语义推断。  
后续若需要在 prompt 或 flow 中启用专门约束，建议采用：

1. **输入启发式初判**
2. **Session 环境门控**
3. **工具观测二次确认**

只有三层都成立时，才进入“目录级架构分析约束”。

### 7.2 输入启发式初判

建议使用轻量、可解释的启发式组合，而不是独立 intent classifier：

1. 用户输入中出现本地绝对路径
2. 用户输入中出现目录级分析意图词，例如：
   - `分析`
   - `架构`
   - `结构`
   - `模块`
   - `梳理`
   - `项目`
3. 路径形态更像目录而不是单文件，例如：
   - 路径末尾没有常见文件扩展名
   - 或后续首个文件工具使用了 `list_dir`

仅满足其中单一条件时，不应直接进入场景守卫。

### 7.3 工具观测二次确认

在执行过程中再做一次确认：

1. 首个本地文件工具为 `list_dir`
2. 或目标路径经工具验证为目录

这一步的意义是避免仅靠用户输入文字猜测任务类型。  
时序上应明确区分：

- request 阶段只做“可能是目录架构分析”的弱识别
- execution 阶段先读取 `SessionEnvironment`
- 仅当 `connection_mode == Embedded` 时，才允许升级到本地目录分析路径
- 在收到首批工具观测后，才升级为“已确认的目录架构分析任务”

### 7.4 Session 环境门控

建议将 `SessionEnvironment` 作为目录分析能力的前置门控：

- 若 `connection_mode == Embedded`
  - `directory` 可被视为本地可验证上下文
  - 用户给出的本地绝对路径允许直接进入本地文件工具探测
- 若 `connection_mode == Remote`
  - `directory` 仅表示用户当前本地上下文
  - 不应默认视为服务端或远端执行端可访问目录
  - 不应直接启用“本地目录架构分析专用守卫”

这条规则的目标是避免出现：

- TUI 显示了本地目录
- 模型就误以为远端执行端一定能访问该目录

### 7.5 不采用的识别方式

本方案不建议将以下方式作为主判定依据：

1. 仅靠模型在 `REASONING` 中自称“这是架构分析”
2. 仅靠 `trace.is_directory_architecture_task()` 这类未定义语义函数
3. 引入新的独立意图分类器

---

## 8. 优化方案

### 8.1 Prompt 层优化

#### 8.1.0 Prompt 与 Flow 的边界原则

后续实现建议遵循以下分层原则：

- `Prompt` 层负责提升模型走正确轨迹的概率，修正默认偏好
- `Flow` 层只拦截高价值、低歧义、可观测的违规行为

换句话说：

- 能靠 prompt 约束解决的问题，优先不下沉到 flow
- 只有当某类偏差具备明确事件信号，且会显著破坏任务质量时，才进入 flow 守卫

例如：

- “目录架构分析应先 `grep_files` 再深读”适合 prompt 优先、flow 兜底
- “`/*ACTION*/` 后无有效推进”适合直接作为 flow 级异常
- “声明下一步承诺兑现”适合先做 prompt 约束，flow 只保留轻量启发式版本

对于时序要求更严格的场景，建议按两段处理：

1. request 阶段先基于输入启发式决定是否注入目录架构分析提示
2. execution 阶段先读取 `SessionEnvironment.connection_mode`
3. 仅在 `Embedded` 下，再基于工具观测决定是否启用场景专用 flow 守卫

#### 8.1.1 强化目录架构分析的最低轨迹

建议在 `PlanReAct` 指令中将下述内容从“建议”升级为“最低要求”：

- 当用户给出本地绝对路径且目标是“项目架构分析”，并且 `connection_mode == Embedded` 时：
  - 先使用 `list_dir`
  - 再使用 `grep_files`
  - 再读取文件
- 不要用连续多次 `read_file` 替代结构搜索

若 `connection_mode == Remote`，则 prompt 应明确：

- `directory` 只是用户当前本地工作上下文
- 不应默认将其解释为远端可访问目录
- 不应在未验证可达性的前提下直接承诺本地目录探测

#### 8.1.2 压缩中间推理格式

建议将中间推理收敛为固定三段：

```text
发现:
缺口:
下一步:
```

避免每一步重复输出：

- “已成功读取……”
- “由于 xxx 优先级最高，因此……”
- “这证实了……”

#### 8.1.3 补充 `truncated` 处理规则

当 `read_file` 返回 `truncated=true` 时，应显式要求模型：

1. 标记该文件只读到部分内容
2. 不把它当作完整事实来源
3. 继续读取其他关键文件进行交叉验证

#### 8.1.4 降级“承诺后续动作兑现”为轻量启发式

原方案中的：

- `has_unfulfilled_declared_next_steps`

实现难度偏高，因为它要求对自然语言推理中的“下一步承诺”做稳定解析。

建议调整为更轻量的目标：

仅在模型 `REASONING` 中出现**显式文件名承诺**时，才做启发式检查，例如：

- `` `pageindex/retrieve.py` ``
- `` `pageindex/config.yaml` ``
- `` `pageindex/utils.py` ``

若这些文件名在后续没有作为工具参数出现，再将其标记为：

- `explicit_file_followup_missing`

为降低误报，建议匹配时做路径归一化，至少支持：

- basename 级匹配，例如 `retrieve.py`
- 相对路径匹配，例如 `pageindex/retrieve.py`
- 绝对路径匹配，例如 `/Users/.../pageindex/retrieve.py`

不建议做全路径字符串精确匹配。

该能力建议：

1. 阶段 1 只放在 prompt 中弱约束
2. 阶段 2 若实现 flow 版本，也仅检查显式文件名，不做一般 NLP 承诺解析

---

### 8.2 工具描述层优化

#### 8.2.1 重写工具角色描述

建议把工具职责表述为：

| 工具 | 推荐语义 |
|------|----------|
| `list_dir` | 本地目录轮廓探测 |
| `grep_files` | 架构发现、入口定位、符号关系搜索 |
| `read_file` | 基于搜索结果的定点深读 |

关键是让模型理解：

- `grep_files` 不是“可选搜索”
- 而是目录架构分析中的**结构发现阶段**

#### 8.2.2 区分单文件解释与目录架构分析

建议在工具说明或 planner 指令中显式区分：

- 单文件解释：`read_file` 可直接开始
- 目录架构分析：必须包含 `list_dir + grep_files`

---

### 8.3 Flow 层优化

本节建议拆分为：

1. **通用 PlanReAct 守卫**
2. **本地目录架构分析专用守卫**

避免将场景特化逻辑误写成通用规则。

#### 8.3.1 通用 PlanReAct 守卫：空动作异常

建议在 flow 中单独识别：

```text
ACTION declared
but no effective action progress
```

触发后续策略：

1. 标记为 `empty_action_step`
2. 触发一次 replan
3. 注入更明确的纠正提示

建议纠正语义：

```text
你上一步声明了 ACTION，但没有形成有效动作推进。
请直接发起实际的下一步工具动作，或明确结束原因。
```

这类守卫属于通用 PlanReAct 能力，不仅适用于本地目录架构分析。

#### 8.3.2 本地目录架构分析专用守卫：引入结构探测完成态

建议将“做过文件探测”与“做过结构探测”分开：

```text
has_file_probe:
  list_dir / read_file / grep_files 任一成立

has_structure_probe:
  至少一次 grep_files
```

目录架构分析场景下，应要求二者同时满足。  
但该守卫仅在 `SessionEnvironment.connection_mode == Embedded` 时启用。

#### 8.3.3 本地目录架构分析专用守卫：定义浅层分析轨迹

建议将以下轨迹定义为 **浅层分析（Shallow Investigation）**：

```text
list_dir
read_file
read_file
read_file
final_answer
```

若目标是目录架构分析，且在 final 前没有结构搜索，则不应视为合格完成。

#### 8.3.4 本地目录架构分析专用守卫：收紧 final answer 的结束条件

目录架构分析中，若同时满足以下任一情况，则不应直接 final：

1. 没有 `grep_files`
2. 存在 `explicit_file_followup_missing`
3. 关键文件读取被截断但没有补证据

同样，该门禁仅适用于 `Embedded` 模式下已确认的本地目录架构分析任务。

---

### 8.4 TUI 可观测性优化

#### 8.4.1 明确显示异常类型

建议在执行 summary 或调试区域增加更具体的异常归类：

- `final_without_structure_probe`
- `empty_action_step`
- `explicit_file_followup_missing`

#### 8.4.2 保留整轮复制能力

当前每轮复制能力已经很适合调试，建议后续保留，并确保复制内容中包含：

- 原始 `ACTION/REASONING/FINAL_ANSWER` 标签
- 工具调用输入输出
- execution summary

便于直接复现问题。

#### 8.4.3 针对 `ACTION` 空步给出轻量反馈

当检测到：

```text
/*ACTION*/
```

但后续没有有效推进时，建议 TUI 给出一条轻量提示，例如：

```text
Action declared without effective progress
```

避免用户误判为界面卡死。

---

## 9. 目标流程

### 9.1 目录级架构分析理想流程

```text
User Request
   │
   ▼
Request Phase:
  Input heuristic identifies possible directory architecture task
   │
   ▼
Execution Phase:
  Read SessionEnvironment
   │
   ├── connection_mode == Remote
   │      └── treat directory as local user context only
   │
   └── connection_mode == Embedded
   │
   ▼
  call list_dir(project_root)
   │
   ▼
  tool observation confirms directory target
   │
   ▼
grep_files(path=project_root, query=entrypoints/config/core symbols)
   │
   ▼
read_file(README / manifest / entrypoints / core modules)
   │
   ▼
Check:
 - has_structure_probe?
 - truncated evidence handled?
 - explicit file follow-up handled?
   │
   ├── no  -> replan / continue
   └── yes -> final answer
```

### 9.2 `ACTION` 异常处理流程

```text
Model Response
   │
   ▼
contains ACTION tag?
   │
   ├── no  -> normal flow
   └── yes
        │
        ▼
effective action progress exists?
        │
        ├── yes -> continue
        └── no
             │
             ▼
      classify as empty_action_step
             │
             ▼
      replan with corrective instruction
             │
             ▼
      if repeated -> abort with explicit reason
```

---

## 10. 模块接口建议

本节仅描述建议的职责边界，不代表本次需要新增全部接口。

| 模块 | 建议职责 | 可能需要的能力 |
|------|----------|----------------|
| `session` / session metadata | 暴露环境上下文 | `SessionEnvironment { directory, connection_mode }` |
| `planner.rs` | 注入目录架构分析任务约束 | 基于 `SessionEnvironment` 的本地路径提示、`ACTION` 行为约束 |
| `flow.rs` | 检查轨迹质量与异常分类 | 输入启发式 + session 环境门控 + 工具观测确认、`has_structure_probe`、`empty_action_step`、`explicit_file_followup_missing` |
| `tools/*` 描述 | 统一工具心智模型 | 强化 `grep_files` 的结构发现语义 |
| `ui-tui/src/lib.rs` | 呈现环境与异常可观测性 | `directory` 展示、`connection: embedded/remote`、更清晰的执行 summary / 复制调试内容 |

### 10.1 核心伪代码

```rust
fn is_possible_directory_architecture_task(input: &str) -> bool {
    contains_absolute_path(input) && contains_architecture_intent_terms(input)
}

fn can_use_local_directory_probe(env: &SessionEnvironment) -> bool {
    matches!(env.connection_mode, ConnectionMode::Embedded)
}

fn confirm_directory_architecture_task(trace: &TurnTrace) -> bool {
    trace.first_local_file_tool_is_list_dir() || trace.target_path_confirmed_as_directory()
}

fn can_finish_architecture_analysis(trace: &TurnTrace) -> bool {
    if !trace.has_file_probe() {
        return false;
    }
    if can_use_local_directory_probe(trace.session_environment())
        && confirm_directory_architecture_task(trace)
        && !trace.has_structure_probe()
    {
        return false;
    }
    if trace.has_explicit_file_followup_missing() {
        return false;
    }
    if trace.has_truncated_key_evidence_without_followup() {
        return false;
    }
    true
}

// "effective action progress" is intentionally event-based rather than semantic:
// - new tool calls emitted
// - new tool observations appended
// - a qualified final answer produced
// - or a replan step explicitly triggered from the previous failure
fn classify_plan_react_step(response: &ModelResponse, trace: &TurnTrace) -> StepDecision {
    if response.declares_action() && !trace.has_effective_action_progress() {
        return StepDecision::EmptyActionStep;
    }
    // Request phase only gives a weak hint. Scene-specific final gating
    // should activate after execution has confirmed the directory target.
    if is_possible_directory_architecture_task(trace.user_input())
        && can_use_local_directory_probe(trace.session_environment())
        && confirm_directory_architecture_task(trace)
        && response.is_final_answer()
        && !can_finish_architecture_analysis(trace)
    {
        return StepDecision::ReplanRequired;
    }
    StepDecision::Normal
}
```

---

### 10.2 辅助定义说明

为避免伪代码中出现未定义语义，建议将以下检查统一定义为事件轨迹函数：

- `has_file_probe`
  - 至少出现一次 `list_dir` / `read_file` / `grep_files`
- `has_structure_probe`
  - 至少出现一次 `grep_files`
- `can_use_local_directory_probe`
  - `SessionEnvironment.connection_mode == Embedded`
- `has_effective_action_progress`
  - 至少出现一次新的 tool call
  - 或至少追加一次新的 tool observation
  - 或本轮给出合格 final answer
  - 或本轮进入带原因的显式 replan
- `has_explicit_file_followup_missing`
  - `REASONING` 中出现显式文件名
  - 且后续轨迹中没有对应工具参数
  - 匹配时应做路径归一化，至少支持 basename / 相对路径 / 绝对路径三类匹配

---

## 11. 分阶段落地建议

### 阶段 1：Prompt 与工具语义

目标：

- 强化目录架构分析的最低轨迹
- 强化 `grep_files` 角色
- 规范 `truncated` 与显式文件 follow-up 的处理

收益：

- 实现成本低
- 可快速提升成功率

局限：

- 仍不能保证 100% 稳定

### 阶段 2：Flow 轨迹检查

目标：

- 加入 `has_structure_probe`
- 加入 `empty_action_step`
- 保留轻量的 `explicit_file_followup_missing`
- 收紧 final answer 条件

收益：

- 将“建议”升级为可执行约束
- 解决“模型会用工具但轨迹不达标”的问题

### 阶段 3：TUI 可观测性增强

目标：

- 让空动作、过早收尾、缺结构搜索等问题可见
- 提升问题复现与 review 效率

收益：

- 降低调试成本
- 提升对 planner 行为的可解释性

---

## 12. Review 关注点

建议 review 时重点确认以下问题：

1. 目录架构分析是否应该强制至少一次 `grep_files`
2. `/*ACTION*/` 空动作是否应作为独立协议异常处理
3. “显式文件 follow-up 未执行”是否需要进入 flow 级轻量检查
4. `README truncated=true` 是否必须触发补充证据规则
5. 最终回答是否需要显式区分：
   - 代码已验证
   - 文档已声明
   - 基于证据的推断

---

## 13. 结论

本问题的本质，不再是“模型是否会调用工具”，而是：

**模型已经会调用工具，但当前 Plan-ReAct 仍缺少对本地项目架构分析任务的轨迹约束、空动作异常识别和结构化证据门禁。**

因此后续优化应遵循以下顺序：

1. 先修 prompt 和工具语义
2. 再补 flow 轨迹检查
3. 最后补 TUI 可观测性

其中，`/*ACTION*/` 后没有有效内容，应该被视为明确的协议异常信号，而不是普通文本现象。
