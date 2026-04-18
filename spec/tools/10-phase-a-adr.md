# Phase A Platform ADR

## 1. 目的

本篇是进入 Phase A 编码前的前置 ADR，用来钉死平台层最容易返工的三个设计点：

1. `ToolExecutor` / `ToolDescriptor` 的拆分方式
2. `ReadFileStateStore` 的作用域与版本语义
3. `context_mutation` 的提交时机与 barrier 机制

---

## 2. 决策一：不使用超大 `ToolBehavior` trait

### 2.1 问题

如果把 schema、权限、调度、展示、结果治理、执行全部压进一个 Rust trait：

- 会产生大量默认方法样板
- `dyn Trait` 过厚
- 静态信息错误地进入动态分发层

### 2.2 决策

采用三层拆分：

- `ToolExecutor`
  只承载动态执行和少量运行时判定
- `ToolDescriptor`
  承载静态 schema / governance / presentation / result policy
- `ToolSpecBuilder`
  负责把 descriptor + defaults 归一化成注册时的 `RegisteredToolSpec`

### 2.3 结果

- 工具实现只需要关心执行逻辑和少量输入相关动态行为
- 平台默认值集中在 builder/descriptor 层
- 后续新增治理点优先扩展 descriptor/policy structs，而不是继续给 trait 加方法

---

## 3. 决策二：`ReadFileStateStore` 为 session-scoped

### 3.1 问题

`Read -> Edit/Write` 的安全约束需要跨多轮持续存在。

如果做成 per-turn：

- 多轮对话编辑同一文件会丢失状态
- 用户会在下一轮无故被要求重新读取所有文件

如果做成 session-scoped 但没有版本语义：

- 又会把旧读取错误地当成永久有效

### 3.2 决策

`ReadFileStateStore` 定义为：

- session-scoped
- 每个文件记录版本化读取状态
- 每轮可以派生 turn-local read set，但不作为主状态源

记录至少包含：

- 文件路径
- session 内读取序号
- turn 序号
- content hash
- mtime
- size

其中：

- `content_hash` 默认使用 `SHA-256`
- 对大文件不要求全文哈希；可使用前 `N KB` 预览窗口的 `SHA-256` 作为性能受控的版本指纹
- 具体 `N` 由平台配置控制，但不得退化成不带哈希的纯 `mtime/size` 模式

### 3.3 结果

- 支持多轮持续编辑
- 能识别会话外修改造成的 stale-write
- 成功写入后可以立即刷新状态，而不是强制重新读

---

## 4. 决策三：`context_mutation` 只能在 batch barrier 提交

### 4.1 问题

并发执行时，如果工具拿到共享可写状态直接更新：

- mutation 顺序不可控
- 很难回滚
- 平台状态会被工具实现隐式篡改

### 4.2 决策

工具执行阶段只返回：

- `payload`
- `persisted_result`

不直接写共享状态。

由 `ToolScheduler` 在 batch barrier 后：

1. 调用 `build_context_mutation()` 收集本批所有 `pending_mutations`
2. 串行提交到状态存储
3. 若批次失败，可丢弃未提交 mutation

### 4.3 结果

- 并发批次内无序写问题被平台层消解
- `ReadFileStateStore` / `ToolDecisionState` / `ToolResultBudgetState` 都有统一提交点
- 工具实现与共享状态解耦

---

## 5. Phase A 编码约束

Phase A 编码必须满足：

- 不再新增超大动态 trait
- 不允许工具直接持有共享可写状态句柄并修改
- 不允许把 `read_file_state` 做成 per-turn-only 临时缓存

如果实现阶段发现需要偏离本 ADR，必须先回写本篇再动代码。
