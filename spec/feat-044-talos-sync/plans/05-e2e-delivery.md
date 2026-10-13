# 阶段5：真实多管理节点/Worker E2E与回归

Status: Complete。当前验收结果见[验收结论](../verification.md)，不提交测试过程产物。

新增独立 `19_computer_sync` group，复用已有isolated home/端口/Auth/Provider/helpers，每个场景证明具体业务边界。

| 子阶段 | 动作 | 成功标准 | 状态 |
| --- | --- | --- | --- |
| 5.1 服务与配置 | 默认DEV SQLite启动G/A/B/Worker/H/Auth/TLS；PG发布前切片只切H连接隔离PG，不使用生产库 | 默认用例不用PG即可运行；PG迁移/typed存储/核心同步及恢复另验收；Talos A/B原库不变 | Complete（最新原生/Docker × SQLite/PG持久切片及PG真实SQL失败回归通过） |
| 5.2 核心业务 | 按下表执行注册/多端目录与完整历史/实时进展/跨控制面续跑/结果与资源回调/真实工具效果，实际Electron/Chrome截图 | 原Computer不迁移；只有当前协调者执行落库，其他端通过sync看见；截图与事实对应 | Complete（2026-10-13补验） |
| 5.3 故障与回归 | 真实容器停止/重启/网络隔离、ACK/SQL故障、账号隔离、sync关闭与无云冷启动，跑受影响旧case | 所有核心case通过；未运行明确报告，不以healthy/mock代替 | Complete（2026-10-13补验） |
| 5.4 工程与稳定性 | 分别测量两个工程冷/重复/dev/test构建、target/bin/镜像；固定SDK/服务候选版重复group/持久重启，验证PROD域名/可选sync部署 | 工程内无重复target或runtime构建产物；版本/次数/时长可追溯；稳定通过才推进生产发布 | Complete（最终候选一次8/8，868.81s；registry接入369/369、独立Sync20/20及PG持久场景2/2通过） |

拓扑/镜像/配置/编译约束见[工程方案](https://github.com/apilots/talos-sync/blob/7230f9fdf294e7ae218bdb6d3cf1c4ef60203380/spec/feat-044-talos-sync/deployment-and-build.md)。纯Worker新增build/worker/Dockerfile独立镜像（无Server/WebUI），复用既有worker binary，Sync独立镜像；只起一个管理DB不满足多端验收。

| 场景 | 必须证明 |
| --- | --- |
| 独立单机、不依赖Talos云 | 从未登录且不启动G/H，用DEV YAML冷启动Desktop/server；经loopback RPC创建/连续Turn/input/cancel/文件读写，重启仍有原历史。不是仅从已建立云租约后切断网络。 |
| 两Desktop管理runtime+各自embedded Worker RPC+独立hub+云Server/Web | 同账号Computer/Project/Session一致；B自己的Coordinator续跑A的idle Session，真实DeepSeek/工具效果只在A Computer；结果先回B提交，再同步各端。 |
| 双NAT完整RPC/回调 | 隔离A/B入站访问，只放行主动连G；原Node Connect+调用者连接完成admission/commit、progress/settlement、history/附件/memory/子Agent回调；B有真实落库，A/G无代执行事实。 |
| 内嵌与独立Worker | 本机loopback、远端直连/主动Connect均走同PB/adapter，只有一个Worker实例；真实Docker/Pod部署单独记录，不用命名为pod的本地进程冒充。 |
| 创建控制面不参与新Turn | 阻断A的Session管理API，保持Worker RPC/Connect可达；B仍准入落库，证明不回创建Service。若同进程Worker一并停机则应不可达。 |
| 实时/回调/子Agent | 逐batch进度、waiting回应、子树、工具图/附件、memory水位/notice/失败；不同caller并发回调不串端，当前queue保留原内存限制。 |
| 并发与结果丢失 | 本机/远端同时Start仅一个模型执行；跨端同clientRequestId返回原完成事实，不产生第二输入；不能自动重放无幂等调用。 |
| 历史游标与故障恢复 | 终态尚未shared、sync/CP/Worker断连/重启、SQL失败/ACK丢失；未确认清理不被当idle，恢复不漏不重，不接管运行Turn。 |
| 云故障彼此独立 | 分别停止H、G、两者；H停止时当前协调者继续执行/commit且outbox可恢复；G停止/云lease失效仅关闭远程入口，本机新Turn/任务不受影响，H不代传命令。 |
| 离线与补齐 | 本机离线连续Turn可用；重新同步完整后另一CP续跑，同Computer不迁移，纯Worker不运行sync或Session DB。 |
| 数据生命周期 | 空Project/计数/设置patch、clear分页reset、delete parent tree、旧事件不复活；完整原事件对象一致。 |
| 八项最终Review回归 | 同Worker非Project自定义目录及权限继承/显式覆盖；取消/回退实时展示；幂等完整原requestId响应；B最后Local未Shared后Worker重启拒绝A旧历史游标；双NAT在线源FileService读/改/删、离线显式失败；登录默认与显式禁用；talos-cloud-sdk真实introspect/撤销及账号隔离。 |
| 单WSS协议 | SDK单socket处理typed Text/原Loro、Bearer/有界队列；并发batch typed错误关联room/batchId且一次报告；无REST/SSE、下载无成功Ack；重连/分片/背压可恢复。 |
| 新Sync双后端 | 默认SQLite完整group；本地PG只验H的迁移/原子commit/持久Ack/账号并发顺序/GC/DB故障/重启及两端核心同步。不重复UI/无关模型case，不迁移Talos原数据库。 |
| 账号/资源权限 | 正常真实Auth登录、重启凭据、不同账号隔离；外置附件源离线明确不可读，无token/hash/配置secret同步。 |
| 旧产品回归 | 更新09/17/18旧无DB/无listener断言：管理型Desktop/Server都有DB和Worker listener，本机经SDK loopback；纯Worker/薄client无管理DB。个人PC无公网入站，direct/outbound/stdio原业务回归。 |

验证（实施时）：`just e2e-group 19_computer_sync`、相关`just e2e-case`、scoped Rust/TS检查、原生成物/repo/diff。
DEV YAML用于纯本地回归；同步认证仍走真实Auth流程，不通过关鉴权或SQL注入来证明登录/隔离成功。
实施时修订旧18 README/helpers及cloud bootstrap断言，不能为跑旧断言保留过时无DB/直调分支；历史PASS保留原日期/源码，不改写为本新架构验收。
G=agent.talos.love对应实例，H=独立sync实例。NAT用真实网络隔离/只允许出站连接证明，不能以同Mac可访问对方loopback端口冒充；记录实际网络与进程拓扑。
DeepSeek已恢复资金，实施阶段执行真实模型验收。
真实Desktop/Web截图和日志保存在忽略的output/validation目录，供本地Review；Spec只维护验收结论和运行范围。
如实区分同Mac隔离进程、跨物理机、真实Docker/Pod、公网agent/sync部署及打包签名；未运行项不记PASS。

成功标准：新group与受影响旧场景通过，证据覆盖全部边界；自审Spec偏离和消融后按用户授权交付/提交。
同步服务Docker容器重建保留hub ID/checkpoint/feed；管理节点保留各自DB，Worker UUID/Workspace持久且不互相共享。
生产模板配置agent.talos.love/sync.talos.love，sync可禁用且容器可不启动。测试稳定前不发布；本地E2E不宣称公网生产已验证。
方案：[inventory](../persistence-inventory.md)、[Worker](../worker-coordination.md)、[验收](../verification.md)。
完整传输：[RPC/NAT](../rpc-routing.md)。
