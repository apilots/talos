# Memory 与 Compaction 剩余收敛计划

关联 Spec：`spec/refactor-039-memory-storage-retrieval/design.md`

## 已完成边界

- Daily Markdown 已作为长期记忆事实源，SQLite FTS5 仅作为可重建索引。
- Memory search/get 已使用稳定 ID、用户 scope、importance 权重和确定性排序。
- PageIndex、旧 Memory 大接口、session close flush 和 Memory 内混入的 Session 持久化能力已删除。
- Compaction 复用现有 timeline delta 和 Session 级 reservation，没有新增协议或全局锁。

以上内容不再重复设计或重构。本计划只记录仍阻塞真实用户路径的问题。

## 阶段 5：最后一轮正确性收敛

**目标**：修复最后一次只读 Review 确认的真实断点，不增加新概念、框架或兼容层。

**成功标准**：

- `MEMORY.md` 在 8,000 chars 内完整加载；写入或加载超限时明确失败，不静默裁剪。
- `memory_get` 复用 `MemoryInfo`：完整 tool result 参与下一轮，context rebuild 使用既有 `RetrievedMemories` section，不维护第二套 retrieved-memory 对象。
- Session 的 `latest_turn_seq` 和 `memory_processed_until_seq` 不会被旧记录写回退；daily maintenance 以 artifact 为事实读取 Session，不因派生 progress 写失败永久漏处理，并能刷新已有 High memory 的用户。
- Web 在新 Session、idle 断线和 `ActiveTurnRunning` 下不会悬挂或丢草稿；公开 SDK 保持原有 idle-close 默认。
- TUI 真实 `/compact` 复用现有 Session timeline delta，replay 后仍能恢复 compaction busy 和输入 guard。
- 删除本次新增但未被真实主链使用的重复客户端 buffering 实现。

**测试**：只运行上述行为对应的 Rust/前端 scoped 测试、typecheck 和 lint；每项测试验证真实行为，不新增反向兼容测试。

**状态**：已完成

## 阶段 6：完整 E2E 与真实前后端手工验证

**目标**：在阶段 5 通过后验证本次 Memory、Session timeline 和 Compaction 的完整用户路径。

**成功标准**：

- 完整 E2E 首次运行通过；若失败，只针对失败 case 修复并复跑该 case，不重复执行完整 E2E。
- 使用 `conf/talos-dev.yaml` 手动启动 Talos Server 和 Web。
- 手工验证 Memory search/get、首条消息、`/compact` 状态、同 Session 输入阻塞与草稿保留。
- 保存能够证明关键行为的截图和服务日志结论。

**测试**：项目完整 E2E；真实 Server + Web 用户路径。

**状态**：已完成

## 阶段 7：最终只读审查与提交

**目标**：确认实现已经收敛，再按功能与模块整理本地提交。

**成功标准**：

- 多个 subagent 并行只读审查 Memory、Session/Compaction、Web/Desktop/TUI，不修改代码。
- 审查确认没有新增协议、manager、状态机、重复 DTO、重复状态源、死代码或未使用测试；仅保留已确认必要的 deterministic compaction trim recovery。
- Spec 与代码边界一致；不再保留过度规定的恢复流程或 cursor owner 矛盾。
- formatter、受影响 crate lint/test、前端 typecheck/lint 均通过。
- 按功能、模块和场景拆分提交，不做纯机械拆分，不跳过 hook。

**测试**：最终只读审查、格式化、lint/typecheck 和提交 hook。

**状态**：已完成

## 明确不做

- 不新增 compaction 协议、SessionBusy 协议、event manager、retry manager 或第二套状态源。
- 不新增临时数据库恢复 manager、rebuild 状态机、向量库、每用户数据库或 Memory 专用 agent loop。
- 不增加 wall-clock 性能断言，不做无关 UI 组件抽象或顺手重构。
- 不为已删除实现保留 fallback、兼容入口或反向测试。
