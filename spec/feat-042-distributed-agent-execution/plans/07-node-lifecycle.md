# 阶段 7：稳定身份与租约生命周期修复

状态：Complete。第二轮独立审查的五项缺陷已修复，新增场景及真实多节点 group 全部通过。

## 7.1 稳定身份和注册发布

状态：Complete。

- 用户确认沿用创建流程：CreateNode 自动生成 UUID v4，返回在 NodeInfo/NodeCredential；Worker 保存到原 runner.node_id 配置，重启和注册一直携带。
- 名称与 endpoint 不是身份。改名保留租约/Client；改地址撤销旧租约，重新注册同一节点，不增加节点行。
- Register/Unregister/Update/Rotate 在现有 publisher 互斥内完成目录变更与 Registry 更新，防止旧注销删除新 Client。用户认证、执行、心跳、续租均不占用该互斥。
- 拒绝空部署，避免零个 Harness 的节点被报告为可执行。

成功标准与结果：首次心跳前改名后可以执行；旧注销与新注册重叠不会丢失新句柄；改地址后使用原 UUID/凭据重登记；Worker/Server 重启不新增节点行。组件回归与真实注册/路由场景通过。

## 7.2 执行清理

状态：Complete。依赖：7.1。

- Runner 所有正常/错误退出均通知 Host 清除租约，明确鉴权拒绝不再等待旧租约自然过期。
- 进度发送即使遭遇背压也同时响应租约撤销/到期；同 token 续租仍只更新截止时间，保持事件顺序。
- 等待 admission commit 期间也响应租约撤销；不新增执行状态或协议对象。

成功标准与结果：填满实际 adapter 队列并停止读取后，撤销/到期仍调用原 Harness interrupt，且不伪造成功结算。真实 Kernel Bash 已启动时停用节点，Worker 自行结束，延迟文件没有写出；测试没有主动杀 Worker 代替清理。长任务跨续租和实时流仍通过。

## 7.3 场景验收和文档

状态：Complete。依赖：7.1、7.2。

- 扩展原多节点注册场景，使用公开 CreateNode 返回的 UUID；所有路由/文件断言使用真实节点身份。
- 原有协同、Memory/Dream、附件、压缩、等待续接、内嵌执行及租约到期场景保持并通过。
- 同步节点 API、Store、SQL 说明及 shutdown 契约：Server 只关闭自己拥有的 embedded Harness。

## 实际验证（2026-09-21）

| 检查 | 结果 |
| --- | --- |
| `just test -p talos-service -p talos-base -p talos-agent -p agent-client -p talos-worker-bin` | 284 passed，0 skipped |
| `just test -p talos-base agent_node_rpc::tests::lifecycle` | 最终 endpoint/锁边界调整后 2 passed；其他 29 项按筛选未运行，与上一行重复计数 |
| `just clippy -p talos-service -p talos-base -p talos-agent -p agent-client -p talos-worker-bin -p agent-protocol --all-targets` | 通过；最终锁范围调整后再检查 talos-base 全 targets 通过 |
| `just fmt-check`、`pnpm --dir talos-app check:repo` | 通过 |
| Python 编译、ShellCheck、`git diff --check`、Spec 本地链接 | 通过，63 个链接有效 |
| `just e2e-group 17_multi_node` | 7 PASS / 0 FAIL |

真实场景使用一个 Server、三个 Worker 和 Cloud 认证，调用真实模型。
`talos-e2e/reports/e2e_report_20260921_221243.md`（本地生成报告，不入库）。
已有 ts-rs serde 属性解析警告仍存在，没有禁用测试、lint 或 hooks。

设计核对：未新增状态表、远程执行账本、业务 DTO 或管理器；UUID 沿用现有 node_id 字段；
上层仍使用同一 AgentHarness，Workspace 文件仍由绑定 Worker 持有。
不承诺未测试的全部网络故障组合；未知结果的跨进程自动恢复仍不在本期范围。
本轮未执行 Git 提交。
