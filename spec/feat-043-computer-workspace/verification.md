# feat-043 验收与边界对照

Status: Complete（2026-10-11 Review 修复与当前源码验收）。原阶段通过证据保留在下表，本轮结果单列。
方案：[design](design.md)，阶段：[产品 UI](plans/04-computer-ui.md)、[E2E 与交付](plans/05-e2e-delivery.md)。

## 原阶段实际结果

| 验证范围 | 命令与观测结果 |
| --- | --- |
| 注册、执行、环境与 OAuth | `just e2e-group 18_computer_workspace`：当时已存在的 01–10 场景全部通过，真实 DeepSeek；包括原十分钟 OAuth 过期/断连与实际工具效果 |
| 旧 direct | `just e2e-group 17_multi_node`：8 个场景全部通过 |
| 旧 app-server | `TALOS_TEST_PORT=18091 TALOS_APP_SERVER_TEST_PROVIDER=deepseek TALOS_APP_SERVER_TEST_MODEL=deepseek-v4-flash just e2e-group 09_app_server`：4 个场景全部通过 |
| Rust | Service/Worker/MCP/Protocol 的 scoped nextest 227 项通过；Rust SDK/embedded SDK 14 项通过；`just check --workspace`、scoped Clippy、fmt 通过 |
| TypeScript | Web 384、SDK core/browser/app-server 94、Desktop 34 项通过；工作区 lint/typecheck、生成物、仓库与错误契约检查通过。两项既有宿主集成测试由单元命令跳过，真实宿主由 E2E 单独覆盖 |
| 真实产品 UI | 11_product_ui 的真实 Electron A/D + Chrome Web 15 项全部通过；加强完成结果和助手回复断言后再次通过，93.88 秒。报告 [e2e_report_20261011_170445](../../talos-e2e/reports/e2e_report_20261011_170445.md) |
| 本地 Desktop | DEV YAML、无需登录；开发版和从 DMG 安装的应用 smoke 都通过；安装版使用 bundled app-server |
| DMG | arm64 SNAPSHOT 打包、挂载复制、Finder 布局检查通过；安装版云注册/产品路径全部通过；断言两个实际 bundled app-server 进程路径 |

拓扑是同一 Mac 上隔离的真实 Auth/Server、三个独立 Worker、两个真实 Electron Desktop 内嵌 Worker和 Chrome；
所有变更经产品 API，使用独立 home、账号、端口、TLS 测试 CA 与真实 Provider，没有 SQL 注入登录/节点/业务状态。
DEV YAML 用于本地无鉴权回归；联网场景保留真实登录，避免把无鉴权结果当作账号隔离证据。

18 全部 11 场景均有实际通过结果：01–10 在完整 group 命令验收，后来新增的 11 单独验收。
安装版命令：`TALOS_E2E_DESKTOP_APP=/tmp/talos-computer-delivery-installed/Talos.app/Contents/MacOS/Talos TALOS_E2E_CADDY_BIN=/tmp/talos-e2e-tools/caddy just e2e-case 18_computer_workspace/11_product_ui`。
安装版本地命令：`TALOS_DESKTOP_SMOKE_APP=/tmp/talos-computer-delivery-installed/Talos.app/Contents/MacOS/Talos node talos-app/desktop/scripts/desktop-smoke.mjs`。
实际截图在 [场景目录](../../talos-e2e/reports/18_computer_workspace/11_product_ui)，本地 smoke 记录在 `.tmp/desktop-smoke/20261011_085624`。

## 原阶段实现与 Spec 对照

本节描述2026-10-11的043验收基线，不代表当前架构。044已替换“云Desktop无本地业务DB”的组装：Desktop app-server与talos-server均保留完整管理runtime、自己的业务DB和单个内嵌Worker，本机与远端使用同一RPC SDK；注册与命令中转由G负责，各管理端通过H同步账号数据。本机标识来自当前管理宿主的Worker身份。当前契约与验收见[044设计](../feat-044-talos-sync/design.md)和[044验收](../feat-044-talos-sync/verification.md)。

- Desktop 云模式只有一个已打包 app-server 内的 Worker；业务 HTTP/SSE 接唯一控制面，没有第二份本地业务 DB。
- 注册身份仍为控制面签发的持久 UUID；改名、重启、启停与显式凭据恢复不改变节点身份。
- Computer 组件独立，保留原账户按钮；本机由控制面地址 + UUID 匹配，Web 多节点要求显式选择。
- Session/Project 归属及 Timeline 在控制面；配置、MCP 密钥、Workspace 文件和默认目录解析在目标 Worker。
- 原 AgentHarness/WorkerEnvironment、业务 DTO、Client 与错误契约保持统一；没有新增授权表、RPC 状态表、执行账本或配置镜像。
- Config/插件提交复用按节点准入和 Worker 执行互斥；OAuth 回到原 Worker attempt，控制面只有有界短期路由。

## 修复与消融

- 修复 Desktop 初始化先读宿主来源、stdio 目标参数丢失、默认草稿消费顺序、旧 Session 恢复覆盖新选择、个人配置权限加载中误跳转、原 UUID 凭据恢复接线。
- 缓存/草稿按账号、控制面和资源隔离；清除旧作用域查询与迟到响应写入，Computer 切换保留原执行。
- 补齐 Rust GatewayClient 的既有 Computer 六个方法，复用规范 DTO；没有新增一套业务类型。
- 删除 ConfigReloadPlan 未使用配置副本、重复 runtime-state 订阅和前端对服务端项目/Memory 记录的持久化；保留客户端拥有的草稿/选择/别名。
- 旧 MCP 验收等待真实工具目录 Ready 和 Turn 终态后卸载；Memory 终态采集以时间界限代替任意 1024 事件上限。隔离 fixture 移除重复插件配置，目录断言使用 Worker 返回的规范路径。
- UI 自动化等待按钮可操作、页面与输入框就绪；保留真实交互，不加生产代码的测试分支。临时模型凭据不留在 smoke 截图目录。

## 验证范围之外

`agent.talos.love` 公网部署、真实跨物理机器/移动网络、发布签名/notarization 未验证。
本次安装产物为本地 ad-hoc 签名 SNAPSHOT；没有提交、发布或修改生产服务。
本地 TLS/HTTP2 主动连接证明协议链路，不代表公网部署已上线。

## 独立 Review 修复验收（2026-10-11）

- 注册恢复：管理页向既有 desktop/workerEnroll 传显式 owned/outbound UUID；保留 pending 屏障和 saved UUID 约束。Runner 异步错误由宿主处理一次，原 ApiError 经 JSONL/SDK/runtime-state 反馈，不清登录、不新增状态表。
- Worker 环境：MemoryReader/Dream 共享 Worker 实时配置和同一 MemoryStore；配置发布前校验，操作内单快照。Project 状态 RPC timeout 保留 metadata；权限错误按原契约传播。删除四个无调用 Worker getter。
- 传输修复：真实 SIGSTOP 验收发现 outbound 期限只在 Worker 执行，控制侧等待无界；Client 增加同一调用期限及原 cancel 通道，零期限执行流保持原行为。真实 18/06 完整 PASS（报告 [184531](../../talos-e2e/reports/e2e_report_20261011_184531.md)）；18/03 长任务/续租/其他节点并发/清理完整 PASS（报告 [184601](../../talos-e2e/reports/e2e_report_20261011_184601.md)）。
- Memory reload：18/08 完整 PASS，包括实际成功 MemorySearch/MemoryGet 的 Timeline、compact 后续聊、Worker 文件与同 UUID 重启（报告 [185308](../../talos-e2e/reports/e2e_report_20261011_185308.md)）。一次 Provider summary 含尾随文本，被既有 JSON 校验明确拒绝（[184833](../../talos-e2e/reports/e2e_report_20261011_184833.md)）；未放宽输出校验，完整复验通过。
- UI 消融：删除 Store 项目记录/merge/replace/add 路径、未消费 MemorySlice/假数据及退役的项目状态测试；标题与侧栏共用按节点 Project query。窄屏复用同一个 Sidebar 的 native dialog，关联 rail 控件并恢复焦点。
- 缓存：LLM/WebSearch/Profile 纳入现有 talos 清理前缀。账号切换在 AppProviders 的 Store 变更边界清理缓存，随后挂载新账号 observer；真实 Web reload 验证标题/节点恢复。在途请求回归验证旧缓存清除和新缓存保留；恢复旧前缀后回归失败。
- Rust：Worker/Service 208 PASS；Base/AppServer/AgentClient 64 PASS；AppProtocol 15 PASS；全 workspace check、scoped Clippy `-D warnings`、生成物通过。
- Desktop：35 PASS、既有 integration 门控 1 skipped；SDK app-server 28 PASS、既有 integration 门控 1 skipped；typecheck/lint 通过。真实 UI 最终 20 CASE PASS，见下一项。

本轮使用当前开发 Electron 与 Rust 二进制；修复前 DMG 的安装验收不冒充修复后安装产物证据。公网部署、物理跨机、正式签名/notarization 仍未验证。

- 当前真实产品 UI：18/11 完整 20 CASE PASS，151.20 秒，报告 [191759](../../talos-e2e/reports/e2e_report_20261011_191759.md)。实际登录按钮/轮询、pending UUID 的明确恢复、第二 Desktop/Web 向首个 Desktop Worker 派活、旧 embedded Project/Session 续跑、重启折叠标题、窄屏 Escape/焦点、搜索分页/中文本机标签和第二账号全部验收。语言断言先用实际 server search 定位本机，不假设 UUID 排序的第一页一定包含本机；第二次登录只审批新打开的设备授权页面。
- 最终 Web 全量 384 PASS；在途缓存/新缓存保留的定向断言 PASS。新增 JSONL 原错误送达断言的 Desktop 注册 3 项 PASS；scoped Clippy、Web/SDK/Desktop lint/typecheck、fmt-check、生成物、check:repo、diff-check PASS。
- 本地 DEV 无登录：`pnpm --dir talos-app/desktop exec node scripts/desktop-smoke.mjs` PASS；证据 `.tmp/desktop-smoke/20261011_111003`。首次直接 node 启动缺少 electron PATH 的任务临时 Home 已清理，成功 smoke 保留截图而不保留模型凭据。

全部修复均经过实现、相关检查、真实场景和 Spec 自审。没有新增表/执行账本/Registry/配置镜像；工作区边界、现有 Harness 业务接口与统一 ApiError 保持不变。

## 本地独立提交前 Review（2026-10-11）

- 对照注册/Connect、Desktop 两种组装、Worker 环境 owner、节点亲和、鉴权及缓存边界，未发现新增阻断行为；四份 conf 用户改动与 feat-044 草案排除出此特性提交。
- staged policy 发现七处文件行数超限；将已有 ClientMethod、GatewayClient 实现、资源矩阵场景、输入历史投影和 SDK routing 测试按职责移到独立文件。业务方法、协议对象和测试断言保持原样，没有新增状态层或测试用例。
- 修复拆分时的 import/EOF 问题后，实际检查：`just fix` scoped packages PASS；Rust SDK/embedded/AppProtocol/test-client nextest 30 PASS；SDK core 56 PASS，Web 384 PASS；两者 typecheck/lint PASS；协议重新生成、仓库/error contract、staged file policy、diff-check PASS。
- 先前真实多节点 E2E 证据继续有效，见上方记录；本轮不将 unit/scoped 检查当作公网/跨物理机验收，也不声称重新执行了完整模型 E2E 或重新打包 DMG。
