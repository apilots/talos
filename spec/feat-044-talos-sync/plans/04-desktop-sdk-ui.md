# 阶段4：Desktop管理runtime与现有产品

Status: Complete（基线实现）；本轮开关/公开SDK接入及一次联合验收以根PLAN为准。

| 子阶段 | 动作 | 成功标准 | 状态 |
| --- | --- | --- | --- |
| 4.1 管理宿主统一 | Desktop与公网Server均保留完整Service/Store/Coordinator和一个Worker；移除云Desktop无DB裁剪/直调；本机/远端经同agent-client RPC SDK | 云登录不更换DB/宿主职责；无G/H可独立本机；Web薄client/纯Worker无管理DB，原stdio接口保留 | Complete（1.2验收，产品SDK/UI留4.3–4.4） |
| 4.2 同步管理与配置 | 部署YAML全服务false优先；原enabled/set_enabled保存当前端账号选择，无binding亦可关闭；可信历史认领、原Token生命周期、DEV/PROD模板/schema | DEV显式false仍免登录；PROD可省略沿登录默认，显式关闭不再自动打开；切账号隔离，DEV不默认连生产 | Complete（2026-10-13补验） |
| 4.3 SDK/API | 原Computer/Project/Session/Timeline/Files方法；sync管理schema由原体系生成 | 无origin routing/context包装/第二history API；空闲Session在当前服务准入 | Complete |
| 4.4 UI | 原Computer组件/本机标识；实时列表/聊天；区分agent远程可达、sync共享状态与本机可用 | H失联不禁用本机执行；G失联仅影响NAT远端；Computer固定，当前协调者透明 | Complete（中性色、独立管理页、Desktop/Web本机身份、真实续跑/关闭后本机执行已验证） |
| 4.6 Computer详情补齐 | Worker采集Linux/macOS硬件，注册上报、原agent_nodes持久化/同步，独立详情组件展示硬件及已有业务metadata | 同一个typed NodeSystemInfo贯穿原RPC/SDK/同步；未知可选字段不伪造，不新增硬件管理器/表；重启/改名保留UUID | Complete（独立lib、原注册/PB/持久化/同步及Linux/macOS真实报告通过） |
| 4.5 自审/截图 | waiting/当前queue、离线/重连、旧附件源、模式/账户切换 | 不假装离线正文或queue可恢复；实际UI截图供Review | Complete（2026-10-13补验） |

管理HTTP与WorkerRPC listener需实际启动；Electron仍使用既有stdio适配同一Application，不增加第二业务宿主。
本机标识与执行路由由当前Host的ComputerAlias和既有Gateway重建，不按同步来源的Embedded mode推断。4.3–4.5须验证同一Computer在自己的Desktop标记本机，在另一个管理宿主上标记远端；Web以当前连接talos-server的localComputerId标记该Server的内嵌Worker为本机；来源environment保真但不能改变绑定Worker的路径解释或调用方式。
更新旧cloud/local宿主分支及09/18断言：管理型Desktop必须有业务DB与loopback Worker listener；个人电脑没有公网入站监听要求。原模式入口不能继续用来销毁本地管理runtime。
验证（实施时）：受影响Web/Desktop/SDK test、typecheck/lint/schema生成、`pnpm --dir talos-app check:repo`；真实页面与服务日志。