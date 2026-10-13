# 阶段 5：多节点真实验收

状态：Complete。独立审查与真实多节点 group 已完成；下文首轮失败是历史证据，生产修复和最终 7/7 验收见[阶段 6](06-runtime-repairs.md)。

## 5.1 独立全局审查

状态：Complete。

- 新独立 subagent 已审查所有阶段的生产接线、Spec 和测试。
- P1：正常续租调用 install_registration 撤销旧租约，在途 Turn 被取消。真实 Server/Worker + sleep 40 已复现；控制面报告 ServiceUnavailable/RunCleanupUnconfirmed，仍留活动 Turn，SSE 等待终态超时。
- P1：控制面正常退出对远程 Harness 执行 shutdown；Worker 保持运行并重新登记，但未重建被关闭的 Harness。多节点 E2E 已复现 available=true 而原 Session 执行返回 resourceNotFound。
- P1：RPC 同时 select progress 与 settlement，未 drain 完事件即可返回结算。
- P1：Dream Memory 与自动 Memory reflection 仍使用控制面本地路径，远程节点读不到其产物。
- P2：远程子 Agent wait 被共享的 10 秒传输 timeout 截断，原业务允许 60 秒。新 group 已记录真实 wait_agent 错误结果及 Worker 的 TimeoutExpired 日志。
- 消融候选：current_registration() 仅被测试调用，可去掉该接口及多余 token 明文副本；注册 capabilities 无实际匹配消费者，需要确认后精简。保留 admission、节点亲和及必要的流清理状态。
- 原单节点 E2E 本次 27.54 秒通过，但直接 INSERT 节点表，不能证明用户创建节点的链路，也未覆盖首次续租。

## 5.2 多节点场景实现

状态：Complete。依赖：5.1。

新建 `17_multi_node` group。真实 Cloud Auth、Server、三个独立 Worker（同用户两个节点，另一用户一个节点）；分别使用独立 HOME、Workspace、监听端口和日志。PB 描述来自现有 agent-pb，不复制协议定义。

入口与覆盖矩阵：[17_multi_node/README.md](../../../talos-e2e/scenarios/17_multi_node/README.md)。共享 fixture 仅负责真实进程、TLS、公开 API 和只读观测；不新增产品接口或假 Provider。

- 注册生命周期：通过 CreateNode 管理 RPC 创建并获取凭证；验证隔离、错误凭证、并存、续租、Worker 重启、控制面重启、禁用及凭证轮换。
- 节点路由：两个用户并发真实模型 Turn、节点文件与控制面历史、同用户节点选择、固定 Session 不漂移、节点停止后新 Session 路由到另一个可用节点、重启后恢复访问本地文件。
- 执行可靠性：真实 Bash 长任务跨越续租，实时记录 SSE，并验证完整事件序列和终态；其他用户节点仍可执行。
- 子 Agent 协同：真实 spawn/wait、子任务文件副作用、父子节点绑定、等待结果回传和控制面持久化。按当前设计父子保持同一节点，不发明跨节点子 Agent 迁移。
- Workspace 归属：远程 Session 的 transcript、summary、continuation、MEMORY.md 和 Dream Memory 必须只落在 Worker；控制面仅保存 metadata 和 Session/Turn 数据。自动 Memory reflection 也遵守此边界。

成功标准：所有场景均从公开 API 进入；不写数据库制造注册或执行状态，不使用假 Provider；每个场景输出 PASS/FAIL，失败保留可定位日志。

## 5.3 执行和验收边界

状态：Complete。依赖：5.2。首轮揭示真实缺陷，阶段 6 完成生产修复、必要消融和七场景复验。

命令：`just e2e-group 17_multi_node`；Python 语法、ShellCheck、`git diff --check`、仓库检查。

真实运行后逐项记录结果。已知生产 bug 不能通过改测试阈值、缩短任务或跳过断言规避。场景代码完成与产品验收通过分别记录；附件、等待用户输入续接、压缩和 Memory 位置的完整等价性仍须各自验收，不能用本 group 的局部通过代替。

### 首轮结果（2026-09-21）

`just e2e-group 17_multi_node`：1 PASS、4 FAIL；总报告位于
`talos-e2e/reports/e2e_report_20260921_164318.md`，每个场景保留 Server/Worker 日志和带接收时间的 SSE JSONL。

| 场景 | 实测结果 |
| --- | --- |
| 注册生命周期 | PASS，82.54 秒；公开管理接口、两个普通用户隔离、非法 token、续租、Worker 重启、Server 重登记、停用/轮换/重新接入均通过 |
| 路由/隔离 | FAIL，84.53 秒；并发文件隔离、历史、崩溃后固定 Session 不迁移、新 Session 选择另一节点、Worker 重启读取原文件均通过；Server 重启后 available=true，但执行返回 resourceNotFound |
| 续租长任务 | FAIL，95.03 秒；确认工具副作用出现前已收到实时增量，另一用户节点可独立完成；长任务在续租后被取消，SSE 无终态 |
| 子 Agent 协同 | FAIL，110.63 秒；真实 spawn/wait 已执行，首个 wait_agent 返回错误，Worker 日志记录约 10 秒 TimeoutExpired；随后又受到续租取消影响，父 Turn 无终态 |
| Workspace 归属 | FAIL，26.16 秒；Worker transcript 通过；控制面出现 MEMORY.md 和 MEMORY.md.lock，违反远程 Workspace 全归 Worker 的原则；新 Session 读取 Memory 的后续断言尚未执行 |

静态验证：Python 语法、ShellCheck、`git diff --check`、仓库/error-contract 检查、文档链接检查通过。
独立 subagent 复核过新 fixture，已纠正 transcript 路径和 waitingForInput 事件名，并加强实际 pending wait 与实时反馈断言。

### 审查时确定的修复顺序（已在阶段 6 落实）

1. 修复续租不撤销在途执行、Server 重启后的 Worker Harness 生命周期，先恢复可靠执行。
2. 保证 progress 全部转发后再结算，并按业务等待时间设置远程 wait 超时。
3. 将自动 Memory reflection、Dream Memory 维护的文件读写接到目标 Worker；控制面保留调度策略和业务记录。
4. 消融仅测试消费的公开接口/token 副本及确认无消费者的注册能力字段；不删除有实际故障观测能力的测试。
5. 复跑本 group，并补附件、用户交互续接、压缩和 Dream Memory 的专项验收后，再评估完整功能交付。

以上为首次“先梳理改进列表”的范围和历史发现；用户随后授权实施，结果以[阶段 6](06-runtime-repairs.md)为准。
