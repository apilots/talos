# 阶段 2：主动连接与标准 Harness 执行

Status: Complete（代码、scoped 回归与恢复后的四个真实执行 E2E 切片均通过）。依赖：阶段 1。
目标：不开放 Worker 入站端口，完整执行已有 Harness API，实时反馈到唯一控制面。
方案：[protocol](../protocol.md)。

步骤：

1. agent-pb 增加 Connect 与 typed transport envelopes；保留既有 Harness Request/Response、auth 和结果对象。
   核查 admission/commit、流 half-close、Dream callback、多路并发、取消与 deadline，不加入业务状态或 JSON 通用命令。
2. 扩展现有 NodeRpcServer/Publisher，验证租约后安装 RemoteAgentClient；沿用 Registry 和发布锁。
   当前连接/租约匹配后才发布，替换旧连接时关闭旧句柄；旧连接回调不能撤销新连接。
3. agent-client 内聚 direct/outbound 传输；AgentNodeRunner 协调注册、连接、心跳/续租和撤销。
   有界通道不阻塞 reader；失联停止准入，释放旧执行，错误按既有处理 owner 回传，不重发已提交任务。
   reader 非阻塞分派；队列满只让对应调用以 ServiceUnavailable 失败，走已有取消/未确认结果语义。
   同 writer 有界控制帧容量与跨调用公平调度，不越过同调用顺序；控制帧本身不可投递时明确断连清理。
4. TalosWorker lib 使用云 SessionData/SubagentControl/ToolImage 资源依赖；独立和 Desktop 宿主复用同一 runner/adapter。
   Desktop 继续使用已打包的 app-server 内嵌 Worker，在云模式安装 Runner/adapter 与云资源依赖；
   不额外启动 talos-worker 进程。纯本地保留 app-server 本地业务组装，独立 binary 复用同一 lib。
5. 云端 Service 固定目标节点与 Session，贯通 Start/SubmitInput/Interrupt/Compact/Clear/Close/Memory/Dream。
   Preserve Shutdown contract，但 Computer 停用不是退出 Desktop/关闭电脑；宿主释放仍由实际退出流程管理。
6. 验证 Worker -> 云资源 RPC -> 控制面持久化 -> Web/SDK 实时订阅；桌面不充当业务中继。

成功标准：Worker 无 Harness 监听端口仍能执行；标准输入/审批/compact/子任务/附件/图片/Memory 能力不退化；
progress 和 settlement 实时进入控制面；断线/停用/租约过期无静默回退、假成功或自动重放；重连同 UUID 可接新任务。

拟执行检查：

- just test -p agent-protocol -p agent-client -p talos-agent -p talos-worker -p talos-base -p talos-coordinator -p talos-service
- just check -p agent-pb；just fmt；对应包 scoped clippy。
- 真实 Server + outbound Worker 验证一条 Turn 和资源回调链；检查 Worker 不监听执行端口且 Server 不拨入 Worker。
- 有意义的断连、重启、旧连接替换、commit 前取消/commit 后失联和慢消费者场景；不测试静态字段存在。
- 一个调用消费者停止读取、另一个调用完成 Interrupt/commit/Dream reply；过载明确失败，不丢事件后报告成功。

实施切片与验收状态：

| 切片 | 结果与范围 | 状态 |
| --- | --- | --- |
| 2.1 | Connect PB 信封、全部 10 个既有 Harness 方法共用 RPC adapter；业务接口/结果/PB 不变 | Complete |
| 2.2 | agent-client direct/outbound 封装、有界多路队列、公平 writer、取消/half-close/deadline、单调用过载失败 | Complete |
| 2.3 | 当前租约/连接发布与撤销、旧连接替换保护、Runner 清理后重注册；Desktop 和 standalone 共用 Worker/资源客户端 | Complete |
| 2.4 | 相关回归与入口检查通过；routing/reliability/collaboration/parity 真实 E2E 已验收 | Complete |

实际结果（2026-10-11）：

- `just test -p agent-protocol -p agent-client -p talos-agent -p talos-worker -p talos-base -p talos-coordinator -p talos-service`：最终 310 个测试全部通过，0 skipped。
- 新增六个真实交互回归（controlled Provider / 本地 TLS gRPC，无外部模型）：admission/commit/input/compact/管理方法；commit 前取消与 commit 后断连；慢消费者与并行 commit/Interrupt/Dream；未确认清理阻止重注册；旧 Connect 替换与鉴权/停用；Server 长连接优雅退出。
- 保留 direct 与 embedded 既有 Harness 回归，以及实际历史/附件/Memory cursor/子任务/图片资源 gRPC 回归；没有复制 Provider 业务或添加测试专用生产分支。
- `just check -p agent-pb`、相关包 scoped Clippy `--all-targets`、fmt-check、check:repo、Python/shell 语法和 diff-check 通过。11 份文档、28 个本地引用及 Spec/PB 定义一致性检查通过。PB 生成物保持生成器管理；新增传输信封不含实现。生成器对新 carrier oneof 使用 Box，去除新增 large-enum 告警，不修改既有业务 PB；仅剩两条既有生成物告警，无 lint 抑制。
- 三个入口 `talos-worker-bin`、`talos-server`、`app-server` 的 `just check ... --all-targets` 通过，包括测试目标编译；未执行外部 E2E。
- 实际 E2E（用户暂停之前）：注册场景 10 项再次全部通过（Connect 开启），报告 `talos-e2e/reports/e2e_report_20261011_103240.md`；三个 outbound Worker 无 TCP listener 检查通过。真实任务请求已到 Worker，Provider 额度/凭据拒绝通过现有错误/Timeline 回传；这不是成功执行验收。
- 新组 18 的 routing/reliability/collaboration/parity 脚本复用已有真实场景，仅切 outbound 配置；用户明确暂停后没有再执行 E2E。DeepSeek HTTP 402；Bailian HTTP 400（Arrearage）；其他已配置外部 Provider 认证拒绝。无假模型成功替代。
- 发现并修复：网络 buffered 帧突发误判过载；先发 unary 后创建冗余 request queue；旧连接回收与新连接发布；Server shutdown 长连接 drain；permit consume 期间失联；未确认清理后错误重注册；代理 502 被错误视作终态 InternalError。网络错误转换归入 Client 单一 helper。

对照 Spec：

- 上层 Coordinator/Service 继续只调用 AgentHarness，transport choice/注册/重连在 Clients 与 adapter；业务请求/结果不改，没有 RPC 状态表、第二份 Registry、Provider 定义或自动重放。
- Worker 复用同一 TalosWorker lib、已有文件/Memory/Provider owner，云资源客户端直达控制面；Desktop 没有新增执行进程或本地产品 DB。
- 注册本身不发布可执行能力；Ready 在句柄安装后，available 仍要求健康心跳。当前 revocation hook 匹配才能撤销，避免旧流清理新句柄。
- 容量/控制优先级/同调用顺序、原 admission commit、认证/取消/Dream callback 均有回归证据；未确认执行不伪装成功，未确认清理阻止自动重连。
- 未发现代码职责偏离。该切片未提前引入环境/UI 职责；阶段 3、4 的最终实现和验收见对应计划。
- 上述 310 个回归与暂停记录为恢复 E2E 前的历史证据；恢复后的实际成功验收见下文。阶段 3/4/5 后续功能不据此计完成。

最终状态：阶段 2 Complete；后续环境与 UI/交付验收见阶段 3–5。本期不提交或部署。

- 2026-10-11 E2E 已恢复：18/02_routing、18/03_reliability 均 PASS，证据分别在同组对应目录的 scenario.log；并发运行的总报告时间戳相同，以场景目录为准。覆盖 owner/节点隔离、历史与文件、失联不回退、Worker/Server 重启、实时进度先于完成、并行其他节点、实际续租和最终无残留 active Turn。04/05 继续验收。

- 恢复后的最终实际结果：18/02_routing、03_reliability、04_collaboration、05_parity 全部 PASS；17 项真实场景断言通过，分别在对应目录 scenario.log。
- 真实 DeepSeek 验证：两个 owner/多个节点、完整历史与文件、失联不回退与 Worker/Server 重启、长工具跨续租/实时流和另一节点并行、子 Agent 的超过 10 秒 wait callback 与固定节点、鉴权附件/视觉、compact 的 Worker summary 与保留上下文、用户问题等待后 Worker 重启恢复、共享 TalosWorker 内嵌执行。
- 新增阶段 3.1 未破坏这些已定义执行能力；控制面仍只持业务/Timeline/资源记录，工具与 Workspace 文件在执行 Worker，没有资源中继或新的执行状态表。


Review 修复：控制侧 Client 独立执行有限 RPC deadline，并复用原调用取消通道；真实停止响应的 Worker 场景及长任务/续租/并发回归通过。 [实际结果](../verification.md)。
