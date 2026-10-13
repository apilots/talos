# 多节点注册与协同验收

运行：`just e2e-group 17_multi_node`。

每个场景启动真实 Cloud Auth、一个 `talos-server`、三个 `talos-worker`，分别使用隔离的
HOME、Workspace、端口和日志。用户 Alice 拥有两个节点，Bob 拥有一个；首个 Cloud
管理员账号只用于完成新实例引导。节点通过公开管理 RPC 创建，不直接写数据库。

| 场景 | 必须观察到的行为 |
| --- | --- |
| `01_registration` | 公开接口生成 UUID/颁发凭证、用户隔离、拒绝非法 token、续租、首个心跳前改名仍能执行、Worker/控制面重启身份稳定、执行中停用后自动清理且不产生延迟文件、轮换/重加入 |
| `02_routing` | 两用户并发执行、文件归属、实时事件序号完整、二轮历史、Worker 崩溃后固定 Session 不漂移、新 Session 选择同用户另一个节点、节点/控制面重启后继续执行 |
| `03_reliability` | Bash 长任务跨租约续期、工具未完成时已收到实时增量、另一用户节点独立运行、完整结算且无遗留活动 Turn |
| `04_collaboration` | 真实模型调用 spawn/wait，子任务文件落节点、父子节点亲和、等待结果正确回传、子任务事件落控制面 |
| `05_workspace` | 控制面存在旧 Workspace 目录时仍写 Worker，自动反思与定时 Dream 使用 Worker 时区，新 Session 读到 Memory；控制面只保留 Session cursor 和模型用量 |
| `06_parity` | 真实附件输入、显式压缩及后续上下文、问答进入等待后重启 Worker 再续接同一 Turn；文件均在 Worker；内嵌执行保持可用 |
| `08_rpc_boundaries` | 已建立连接的 Worker 暂停时 unary/admission 有界 Timeout；控制面暂停时 Worker graceful shutdown 先取消真实 Bash，延迟文件不产生、不伪造完成 |
| `07_lease_expiry` | 暂停控制面进程直至租约过期，Worker 不退出；控制面恢复后重新注册 |

子 Agent 沿父 Session 节点执行；多节点由一个控制面统一管理，并不代表跨用户协作或
子任务自动迁移。PB 描述直接由 `agent-pb/proto` 编译，测试不维护另一套协议。
SQLite 只读检查持久化结果。每个场景失败即失败，后续独立场景仍由 group runner 执行。

`06_parity` 使用支持图片的真实模型，默认 `deepseek-v4-flash-vision-exp`，可通过
`TALOS_E2E_IMAGE_MODEL` 指定已配置且可访问的视觉模型。默认文本模型继续用于其他场景。

前置条件见 [E2E README](../../README.md)。设置 `TALOS_MULTI_NODE_KEEP_TMP=1` 可保留
临时环境用于诊断；其中包含测试凭证。日志、带接收时间的 SSE 和结果保存在
`talos-e2e/reports/17_multi_node/`。

每个场景按实际业务结果验收；未执行的分支和已知失败必须明确记录，不能用测试数量
宣称所有网络故障和 Provider 能力均已覆盖。跨进程未知执行结果的自动恢复不属于本期。
