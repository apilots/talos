# 阶段 5：真实多 Computer E2E 与交付

Status: Complete。依赖：阶段 1–4。
目标：真实产品路径验证整个特性，回归旧功能，删除重复实现和无意义测试。
方案：[design](../design.md)，[现有 E2E 约定](../../../talos-e2e/README.md)。

## 验证拓扑

| 场景 | 真实宿主与 API 路径 |
| --- | --- |
| 联网多客户端 | 一个隔离 Cloud Auth + Talos Server；Desktop A 自带 Worker、独立 Worker B 同用户，Worker C 另一用户；Web 与 Desktop D 同账号连接同 Server |
| 纯本地回归 | 独立隔离 home；Desktop -> 本地 app-server -> 内嵌 Worker；不连接云，不读取联网模式业务记录 |
| 现有 direct 回归 | 既有 17_multi_node 拓扑与资源回调；不得以新传输替换测试从而遗漏原功能 |

复用既有 fixture、认证与 home/端口分配；Desktop A/D 真实 Electron 启动，A 的 app-server 内嵌 Worker 和产品 SDK 接同一 Server。
联网场景使用既有真实设备登录流程，实际 Electron/Chrome 完成认证和身份恢复；
纯本地 smoke 使用隔离 DEV YAML，无需登录。不关闭鉴权或注入 SQL 替代联网验收。
公网 agent.talos.love 部署另做同一入口的 TLS/HTTP/2/长连接验证。

步骤：

1. 新增 talos-e2e/scenarios/18_computer_workspace，注册 runner 所需真实 binaries，记录统一 preflight 和入口命令。
   先增加注册场景，再逐阶段加入执行/环境/UI 场景；本阶段做全 group 回归，不把功能验证全部拖到最后。
2. 经公开 API 创建足够节点覆盖快捷项/浮层；查询设置 limit=2 证明多页搜索/total，不能用八台节点假装覆盖默认50条分页。
3. 验证登录创建/保存 UUID、改名重启、续租、CP 重启、旧连接替换、停用/轮换/账号隔离；Worker 不开放执行监听端口。
4. Desktop A 默认本机并产生真实文件，切 B 后远端文件改变；D/Web 选择 A，同一云 Session/Timeline 可见。
   输入/审批/compact/子任务/附件/图片/Dream 回调与实时事件用真实 Provider 验证，不用 fake Provider 冒充。
5. 目录/项目同路径隔离，Workspace/Memory/插件/MCP/模型配置修改检查实际 Worker 文件与运行时；配置准入竞态按节点验证。
   设置不同默认 workspaceId，并在 Worker 更改默认后继续旧 Session，验证原绑定不变。
   用不同宿主上的浏览器完成目标 Worker OAuth，检查错误 state/issuer 不消耗有效 attempt、重复/超时/断连处理和凭据位置。
6. 界面覆盖菜单完整列表、轻量本机标签、深浅色、快速切换、草稿/运行任务、直达旧会话、离线与最近选择恢复。
   注册个人 Worker 前，普通用户经原 API 创建 embedded Project/Session；之后在新 UI 找到并继续原会话，仍无共享配置管理权。
7. 断连中已提交工具操作不得自动重放；错误与未确认结果按既有契约显示。纯本地和共享Server权限单独回归。
   沿用安装产物验收，证明打包 app-server 内的 Worker 可注册/执行，不依赖开发二进制覆盖；无需新增 Worker 打包项。
   慢消费者只使对应调用过载失败，另一调用 Interrupt/commit/回调仍可完成；同调用事件与结算顺序正确。
8. Review diff，对照 Spec 删除重复 DTO、RPC状态、注册/环境管理器和废弃控制面I/O；保留实际用户/故障场景测试。
   更新功能文档/生成物/部署要求，提交与 PR 等待届时授权，使用英文 Conventional Commits 和 PR。

成功标准：18 group 实际全通过、既有关键 group 通过、Electron/Web 路径有截图/日志和节点文件证据；
隔离 fixtures 清理正确；未验证公网部署明确列出，不能由本地 loopback 结果声称公网已上线。

拟执行检查：

- just e2e-group 18_computer_workspace（新增后）；just e2e-group 17_multi_node；just e2e-group 09_app_server
- pnpm --dir talos-app check:repo；just fmt-check；git diff --check；实际变更范围 quality gates。
- 公网测试仅在已有部署和测试授权范围内；未部署时记录 Not Verified，不用修改真实用户数据来验收。

前提：相邻 talos-cloud 或 TALOS_CLOUD_REPO、protoc、Python grpcio/protobuf/PyYAML、真实模型凭据、Electron/UI 环境。
实际结果（2026-10-11）：

- 18 的 11 个场景均实际通过：01–10 完整 group 回归通过，新增 11 单独完成产品/安装验收。最后加强断言的安装版 11 共 15 项 PASS，93.88 秒；报告 `talos-e2e/reports/e2e_report_20261011_170445.md`。不把早于新增 11 的 group 命令声称为一次运行了 11 场景。
- `just e2e-group 17_multi_node` 的 8 个旧 direct 场景和 `09_app_server` 的 4 个场景通过；真实 DeepSeek/MCP/Memory/Skills 效果保留。
- DMG 打包、挂载复制和 Finder 布局通过；实际安装 Talos.app 的两个 bundled app-server 进程路径被断言并记录，没有开发二进制/资源覆盖。云注册、本机/远端执行、实时最终回复与跨客户端历史通过；DEV 无登录本地 smoke 也通过。
- Rust scoped nextest 227 + SDK/embedded 14 项通过，Web 384、TS SDK 94、Desktop 34 项通过；全 Rust workspace 编译、scoped Clippy、工作区 lint/typecheck、生成物、仓库/错误契约、fmt 与 diff 检查通过。既有环境门控 integration/sandbox 测试未由单元命令执行，具体范围见阶段记录，不计为已通过。
- 消融移除未使用 ConfigReloadPlan 配置副本、重复宿主订阅与前端服务端记录持久化。修复旧 MCP fixture 的重复配置/Ready/终态时序、Memory 任意事件上限；UI 断言实际完成结果和助手最终回复，避免提示词标记冒充反馈。
- fixtures 正常停止并删除；只清理本任务遗留 home/凭据，不改用户配置。保存四份既有 conf 修改，不提交、不部署。

Spec 对照与完整命令范围见 [验收记录](../verification.md)。公网 agent.talos.love、跨物理机器网络和发布签名仍为 Not Verified；这些部署验证不由本地通过结论替代。
下一步：本期本地实施与验收完成；生产部署和 Git 交付按后续授权执行。


Review 扩展：真实按钮发起登录及 UI 轮询；pending UUID 的管理页恢复；第二 Desktop/Web 向首个 Desktop Worker 实际派活；旧 embedded Project/Session 继续执行；窄屏、标题刷新、暂停 Worker deadline 和 reload 后 Memory/compact。历史安装证据与当前源码验收分开记录。 [实际结果](../verification.md)。
