# 阶段1：统一Worker RPC与调用者边界

Status: Complete（基线实现）；本轮开关/公开SDK接入及一次联合验收以根PLAN为准。

| 子阶段 | 动作 | 成功标准 | 状态 |
| --- | --- | --- | --- |
| 1.1 RPC host | TalosWorker封装listener/adapter/shutdown；配置解耦runner transport；管理与Worker endpoint分离；复用同Worker实例 | 已有完整runtime经loopback client；独立direct/outbound均bind同host，失败不回退；Desktop完整管理runtime在1.2提前接入，产品同步/配置留4.2–4.4 | Complete |
| 1.2 注册与鉴权 | 原CreateNode UUID/凭据/租约、embedded别名；本机NodeAuth与云remote lease生命周期隔离；注册runner用同host | 从未云注册也可本机工作；云租约过期不撤销本机入口/任务；无凭据同步/免鉴权例外 | Complete |
| 1.3 跨控制面/NAT | 复用Node Connect；增加已认证Coordinator入口，原Worker call/result按当前连接路由，取消/ACK/流控复用 | A/B无公网监听仍可调用；结果只回B落库；不经sync、不重放RPC、不用G的Coordinator代执行 | Complete（2026-10-13补验） |
| 1.4 资源回调 | 原资源client附invocation反向到B；Dream原双向流；补源peer FileService产品读/改/删载体（不要求active Turn） | history/File/memory/subagent/tool image到B；在线NAT源FileService真实可达、无磁盘直访/H正文通道 | Complete |
| 1.5 自审/验证 | 独立单机、真实direct/隔离NAT、agent断连/lease撤销；原取消/清理与stream；移除被替换direct组装 | 不启动sync即可完成阶段1；本机和云故障隔离，实际RPC业务行为不变 | Complete（2026-10-13补验） |

listener先ready再注册Harness/开放管理API；管理HTTP可承载注册/Connect，Desktop stdio保留同一Application。
上层AgentHarness/RunParams/Result不变；只扩必要认证/调用context。Session 历史游标跨控制面准入在阶段3接入。
本阶段不依赖Loro/同步库/服务；使用当前控制面已有事实完成原RPC调用，自测不得把G/H可用作为本机启动条件。

验证（实施时）：`just test -p talos-worker -p agent-client -p talos-agent -p talos-base`，相关配置/协议生成与scoped check/Clippy。
实际场景启动server/app-server+Worker listener，原Start/Input/Interrupt/工具文件回传经过真实RPC；不只mock直调成功。
当前实际结果（2026-10-12）：1.1–1.5完成。原1.1的194项检查和真实E2E属于历史切片；下列各阶段记录本阶段新增验收，不累计冒充全特性通过。
对照Spec：同Worker/adapter、业务PB/SDK及资源owner复用；只新增鉴权/调用上下文、闭合原资源/文件RPC分支与内存活跃连接，不增加执行DB或sync命令通道。Session完整事实、Worker 历史游标和多端产品续跑在阶段3，未据载体通过宣称完成。

方案：[Worker](../worker-coordination.md)、[design](https://github.com/apilots/talos-sync/blob/7230f9fdf294e7ae218bdb6d3cf1c4ef60203380/spec/feat-044-talos-sync/design.md)、[验收](../verification.md)。
链路与验收：[RPC/NAT](../rpc-routing.md)。

1.2接线决策（已完成，2026-10-12）：云Desktop必须复用完整管理runtime，因而将4.1中删除“无管理DB云宿主”的组装调整提前到本阶段；4.1的同步/产品UI仍未开始。RegisteredWorker给已有host附加注册时只拥有云注册生命周期；账号退出/凭据恢复不得停止本机Worker/listener。本机embedded别名与注册UUID分别绑定认证对象；不改变既有Session绑定。验证覆盖同一Worker本机/云SDK访问、改名与恢复UUID、撤销云注册后本机继续工作。

1.2验收（2026-10-12）：Complete。管理runtime保留单Worker、单RPC host；RegisteredWorker::attach只附加注册，原UUID与host embedded别名分别绑定租约/本机凭据；Desktop保存的登录先验证再附加，Server显式runner复用同runtime；云注册失败可报告且不阻断已组成的本机入口。移除51行旧无管理runtime云stdio与重复认证分派，未新增业务DB/请求账本。真实TLS/loopback测试验证云RPC创建目录、本机读取一致、注销后本机继续读取；原执行RPC测试验证云UUID/token不可冒用host身份、本机运行Turn不受云撤销影响。Desktop的3项注册场景修复后全部通过；scoped Clippy-tests、repo/error-contract/fmt/diff通过。真实18/01注册E2E全部10个场景通过（69.98s），包括原10s心跳/30s续约、Desktop/G重启、凭据恢复、多Computer/账号/注销及日志无凭据泄漏。
测试过程：新管理runtime fixture最初未apply_derived_paths，误沿用默认业务DB路径；现已显式应用临时Home派生路径，并用HOME内受控临时目录满足原picker边界，不修改生产picker规则；未对现有用户DB做恢复/删除。阶段2动态写入workspace/migration的中间状态曾令构建/重启失败，重新隔离稳定状态后真实重启通过，没有修改已发布SQL或绕过checksum。
下一步：1.3认证调用者Connect及原调用结果中转；1.4资源RPC按当前invocation返回调用者。同步/全量Session恢复仍不属于本阶段，不提前宣称跨控制面续跑验收。

1.3验收（2026-10-12）：Complete。新增账号认证的Coordinator Connect，一个HTTP/2 channel可复用多个单Computer流；Gateway核对真实登录/Computer owner，替换为目标Worker lease，复用既有callId/sequence/取消/ACK/窗口，不执行业务准入或写Turn事实。SDK隐藏中转。真实TLS双跳回归验证Start/commit/Input/压缩/管理、Dream模型双向流及128帧进度/背压；原direct/outbound/云撤销/断连回归保留，88项nextest通过（0 skipped）。实际Gateway测试验证跨账号拒绝及B经G访问A目录。1.4资源回调尚未实现；不把该协议切片计作共享历史续跑或完整双NAT模型任务验收。
下一步：1.4按当前invocation绑定原资源端口，通过认证Coordinator连接反向到调用者；本机资源也使用受保护loopback RPC。

1.4验收（2026-10-12）：Complete。资源client按invocation冻结原auth/caller，Kernel/工具端口沿现有接口选择本机受保护loopback或G→当前B的反向载体；不更改业务Harness/RunParams/Result，不修改共享client endpoint。B SDK核对活跃callId及Session/Turn/project scope，B原handler再核对Session持久绑定，不能从B数据库校验外部Worker租约。附件/子agent强化原scope，本机subagent携带上下文缺项已修。源FileService读/改/删独立于active Turn，原FileInfo包括scope，tags/metadata的None与空值保真，正文只由源owner访问；选择同peer最新在线连接，断开无fallback。223项受影响nextest通过（0 skipped），新本机/当前owner资源端口回归1项通过（39 filtered），严格Clippy/repo/error-contract通过。实际02 Session E2E、18/02 routing和18/04 collaboration通过；双跳callback与源FileService真实受控TLS测试通过。阶段3仍需接sourcePeer物化/业务选择及跨控制面Session事实，阶段5仍需联合Docker产品验收。
下一步：阶段1.5原direct/取消/租约/断连/清理回归、文档接口对照及最终检查，阶段2独立agent继续并行。

1.5验收（2026-10-12）：Complete。223项包级测试、新增资源owner回归、严格Clippy、fmt/repo/error-contract/diff均通过。真实17/08 direct边界E2E通过（58.61s），验证受控实际Server/Auth/Worker的背压上限、长工具取消和关停清理；18/03 outbound可靠性E2E通过（66.59s），验证原续租期间长任务、终态前进度和隔离清理。结合02真实本机执行、18/01真实注册/身份恢复、18/02 routing和18/04协作，不启动sync即可验证阶段1边界。协议自审核对caller为PB对象、G生成连接UUID、同peer多Computer流、原资源/源FileService授权及stream载体，无执行账本、任意callback URL或不同数据owner兜底。
实施分层：旧大资源handler按鉴权/业务图像/协作/活跃路由/源FileService载体拆为职责内聚模块；新增生产模块均小于500行。现有agent-client resources保留原资源SPI的实现owner，增加认证scope但未新造同名业务模型；Kernel仅接原资源端口resolver，不依赖上层Service。原未发布云Worker-only Desktop组装、管理内嵌直调及全局运行时agent_control注入已移除。
剩余依赖：阶段3接完整事实、公共Session/历史游标准入、storagePeer产品选择和运行中产品交互；阶段4接多端产品配置/UI；阶段5完成独立19 Docker联合验收。本阶段没有UI新增，不用静态截图代替RPC行为证据。下一步按依赖进入阶段3，阶段2由独立agent完成最终验收。

2026-10-13：对应六项遗漏已沿现有边界修正；423/423作用域测试、独立Sync20/20与最终完整8/8验收通过。原生amd64及发布输入不列待办，暂不发布生产。详见[六项修复验证](../verification.md)。
