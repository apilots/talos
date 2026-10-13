# 阶段 3：场景 E2E 与既有功能回归

历史阶段记录：当前可运行远程场景已收敛到 `17_multi_node`；旧 16 脚手架已移除。
本轮实际验收见[阶段 8](08-execution-boundaries.md)，以下保留当时结果。

状态：**Complete**。多节点运行时缺陷已修复，新增附件、重启续接、压缩、Memory/Dream 和租约过期验收均通过。最终证据见[阶段 6](06-runtime-repairs.md)。
本阶段验证功能，不能以这里的测试替代前两阶段的局部验证或尚未完成的生产接入。

## 3.1 正常用户链路与原功能等价

状态：Complete。依赖：阶段 2，已通过。

沿用 [talos-e2e](../../../talos-e2e/README.md) runner/helpers。独立控制面和独立节点
使用隔离 Talos home、TLS 配置与 Workspace；从原用户 HTTP/SDK 入口发起请求。
embedded 与 remote 共用场景断言，不要求模型生成逐字相同，但业务语义必须一致。
远程主链路由 `16_distributed_agent/remote_kernel_http` 验收；节点管理/重登记继续复用
阶段 1 的跨进程场景，不在本场景重复一遍管理协议。

| 场景 | 实际操作 | 必须观察到的结果 |
| --- | --- | --- |
| 管理到执行 | 用户创建自己的节点或由管理员代建、启动 Runner、注册后用户发起 Turn | 真实 Client 发布后才可分派；任务尚未完成时首批规范事件已抵达协调层/用户，最终由原 Store 保存结果 |
| Workspace 与历史 | 第一 Turn 在节点写带唯一标记的文件，第二 Turn 读取并引用先前对话 | 文件位于正确用户的节点目录；后续仍落同一节点；历史非空且有先前真实结果 |
| 用户交互 | 复用原 Service 的确定性 WaitingForInput/回复/取消回归，并用真实 TLS Harness 验证 remote input | input/continuation 行为与现有 Harness 一致；等待取消不需要活跃 RPC，终态只提交一次 |
| 已有能力组合 | 使用已上传 file_id；对历史触发显式 compact；创建子任务并读取结果 | 文件访问受授权，压缩后仍可继续对话，子任务结果和父子归属正确 |
| Session 生命周期 | 完成后 close/clear，按原定义再次使用；节点受控 shutdown | 与已有 Harness 的状态清理/重建语义相同，无残留可路由的已 shutdown 实例 |

复用现有 `02_session_lifecycle`、`03_messages_http_sse`、`13_interaction/user_action_http`
以及文件/工具相关场景，不再造一套通用 E2E 框架。需要新增节点进程 fixture 时扩展公共 helper。
只为现有场景未覆盖的远程部署参数或验收结果增加断言。

成功标准：用户可见事件、最终结果、控制面持久化和节点副作用彼此一致；原 embedded 流程均通过。
实施时命令：

```sh
just e2e-case 02_session_lifecycle
just e2e-case 03_messages_http_sse
just e2e-case 13_interaction/user_action_http
```

`02_session_lifecycle`、`03_messages_http_sse` 用于 embedded 回归；remote 使用新增的
`16_distributed_agent/remote_kernel_http`。`user_action_http` 依赖真实模型自行选择工具，
只作探索性补充，不替代确定性的 Service 与 TLS Harness 交互场景。

## 3.2 有实际意义的故障与权限场景

状态：Complete。依赖：3.1 的环境/fixture；远程主链路已通过。

| 场景 | 故障/动作 | 必须观察到的结果 |
| --- | --- | --- |
| 凭据与归属 | 用户管理自己的节点，再尝试管理/调用别人的节点；节点使用错误/被撤销 token | 自己节点正常可用，跨用户操作和无效身份被拒绝，无额外节点权限清单，日志/Get/List 不泄露凭据 |
| 生命周期 | 控制面重启后原节点重登记；节点租约到期再重新启动 | node_id/节点行保持；旧 token 失效；到期停止新 admission；重新登记不篡改业务结果 |
| admission 失败 | 在确定性 Service+RPC 集成场景中令实际 admission 事务失败 | 原 permit 未释放，节点没有执行副作用；不靠只检查方法是否调用的 mock |
| 调用中断 | 有唯一文件副作用的长任务执行期间切断 RPC，再刷新 Timeline/请求 interrupt | 不自动重放、不虚报清理成功；已确认事实保留，未知执行仍有原业务占用 |
| 实例更换 | 有未确认结果时重启节点并重登记，再 interrupt/读取 Timeline | 新实例可连接不等于旧执行已结算；按阶段 2.1 已确认的失效契约处理，不重复旧副作用 |
| 用户独占环境 | 两个用户分别使用各自电脑/Pod，交换 node_id 或他人 file_id 发起请求 | 任务只进入所属用户节点；他人控制面数据访问被原 Service 拒绝；个人电脑不被 RPC 新增 Workspace 围栏 |

admission 事务失败适合确定性组件集成，已有阶段 2 证据足够时不重复制造生产 DB 故障。
租约边界以可控时钟作主要覆盖，E2E 只选一次真实到期证明进程动作。中断场景通过受控
本地网络代理/进程控制实现，禁止在生产代码塞测试开关或对任意时间 sleep 后猜测结果。

成功标准：上述故障均由用户可见结果、控制面事实或节点真实副作用证明。网络 unknown
保留不确定性即可，不要求实现未设计的自动恢复。额外故障排列仅在发现新风险时增加。

## 3.3 回归、生成物与验收证据

状态：Complete。依赖：3.1、3.2，均已通过。

工作：

- 复核原 embedded 七方法及关键历史场景；确认没有为 RPC 改写原业务结果或权限语义。
- 仅对改动涉及的包运行 scoped test/check/clippy 和 fmt-check；不以重复全仓测试取代场景证明。
- 检查 PB/配置/公开 API 生成物、锁文件和构建声明；实际变更 App Server 协议时运行
  `just gen-app-protocol`，未修改则不无故生成。
- 记录真实模型/服务凭据、TLS 和运行环境前置条件；没有环境的场景保持未验证，不用模拟结果补通过。
- 更新启动/管理说明与最终 Spec；执行 `pnpm --dir talos-app check:repo`。

证据写入 `talos-e2e/reports/`，临时数据使用既有 `.tmp/`；只引用必要报告与断言，
不保存 token、未脱敏配置或冗长原始业务内容。每条场景需记录版本、部署方式、命令、结果、证据位置。

| 验收组 | embedded | remote | 当前阻塞 |
| --- | --- | --- | --- |
| 正常 Session/Turn、Workspace、历史 | `02_session_lifecycle`、`03_messages_http_sse` 通过 | 双 Turn、节点文件、历史、实时增量与控制面持久化通过 | 无 |
| 交互、压缩、文件与子任务 | 确定性 Service 回归通过 | 真实 TLS input/continuation、Harness 管理方法、附件、subagent 和 ToolImage 通过 | 无；真实模型是否主动选用 AskUserQuestion 不作为协议门禁 |
| 管理/归属、生命周期 | embedded 回归通过 | 重登记/注销、租约归属、节点退出后固定 Session 明确失败通过 | 无 |
| admission 顺序和网络 unknown | Service/Harness 定向回归通过 | 真实 TLS admission/事件、租约撤销中的进行中调用及固定 Session 失效通过 | 无；不重复已有确定性 TLS 场景 |
| 构建、生成物、文档与 scoped checks | 457 个 scoped tests、clippy、repo check 通过 | 新远程 E2E 通过 | 无 |

当前证据（2026-09-21）：

- `just e2e-case 16_distributed_agent/remote_kernel_http` 通过；报告
  `talos-e2e/reports/e2e_report_20260921_121822.md`。场景使用真实 Cloud token、真实模型、
  独立控制面/节点进程和 TLS，证明远程节点优先选择、节点 Workspace 文件副作用、RPC
  实时增量、第二 Turn 历史、控制面 Session/事件持久化和节点退出后的显式失败。
- `just e2e-case 02_session_lifecycle` 与 `just e2e-case 03_messages_http_sse` 通过；报告分别为
  `e2e_report_20260921_121911.md`、`e2e_report_20260921_121928.md`。
- `just e2e-case 13_interaction/user_action_http` 连续两次停在“真实模型未产生
  waitingForInput”；bootstrap 和三种权限模式切换均已通过。该项保持未验证，不把模型
  未遵循提示误报为协议回归，也不作为分布式传输门禁。Waiting/continuation 的业务行为
  由原 Service 回归验证，远程 input 与进行中租约撤销由确定性真实 TLS 场景验证。

阶段 3 完成记录：真实远程双 Turn 主链路、节点 Workspace、历史、实时反馈、控制面持久化、
节点生命周期与固定 Session 失效均有跨进程证据；不适合由真实模型稳定触发的交互和网络
unknown 使用已有确定性 Service/TLS 场景验收。457 个 scoped tests、clippy、repo check、
格式、文档链接和 Shell 检查通过。真实模型 AskUserQuestion 的两次未触发已如实保留，不算通过，
也不表明协议失败。
