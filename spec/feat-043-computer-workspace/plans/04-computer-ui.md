# 阶段 4：Desktop 云接入与 Computer UI

Status: Complete。依赖：阶段 1–3。
目标：共享云目录和历史，交付确认的独立 Computer 栏；本机执行与远端切换行为一致。
方案：[design](../design.md)。

步骤：

1. 客户端 SDK composition 显式选择 cloud/local：云模式使用云产品 API，纯本地复用 stdio。
   复用已有登录/账户机制；Cloud Auth 与控制面 Actor 对齐，Desktop main 保存节点身份/凭据，renderer 不持有 NodeAuth。
2. 使用阶段 1.3 已实现的 desktop/workerIdentity、connection 与 workerEnroll 宿主接口，完成界面接线；复用既有 binary resolver、打包和进程管理。
   在 app-server 内区分云 Worker 宿主组装和纯本地业务组装，同一 home 只运行一个 Worker，不增加 Desktop worker 二进制。
   消费已有 runtime-state 的结构化 ApiError；登录初始化、断网和 Worker 接入失败各有明确状态，注册失败不重置登录，不用本地 DB 冒充云历史。
3. features/computers 复用 app store；独立 ComputerRail、ComputerOverflowMenu、ComputerItem 与 ComputersPage。
   三列布局保留既有 Sidebar、用户按钮/菜单；快捷三项、完整浮层服务端搜索/分页、管理入口按确认稿。
4. 本机根据控制面 + UUID 匹配，设备名称旁轻量标签，窄栏 tooltip；不同 Desktop 标识不同，Web 不猜本机。
   首次优先本机，接入中显示进度；最近选择/Session 链接优先；失败/离线不自动改选其他 Computer。
5. 节点详情与个人改名、启停/凭据恢复走同一 Service；启停指注册可用性，不杀 Desktop 进程。
6. 项目/会话/环境页和新任务同步携带目标；运行任务不迁移、不因切换中断；草稿与云历史区分客户端/服务端 owner。
   快速切换、账户退出、控制面变更按作用域失效缓存；旧响应不能覆盖新页面。
   普通用户的 embedded 历史使用相同选择器和深链接恢复；按既有执行权限操作，只隐藏无权管理的实例设置。
   远端 OAuth 打开控制面 relay 授权 URL，通过原 ReadMcpAuthentication 展示结果；不向客户端复制 Worker token。
7. 键盘焦点、浅色/深色、i18n、长列表、离线历史和明确错误完成实际 Web/Electron 验收。

成功标准：本地 Desktop 默认本机并产生本机效果；切远端后产生远端效果；第二个 Desktop/Web 看见并执行第一台节点；
本机标签不跟随选中/在线变化；云模式业务不进本地 DB；纯本地功能保持；用户菜单与固定会话归属无回归。

拟执行检查：

- pnpm --dir talos-app --filter talos-web test；lint；typecheck；build（分别执行）。
- pnpm --dir talos-app --filter talos-desktop check
- SDK browser/app-server scoped parity checks；新增 IPC 更新 allowlist 和生成物。
- 实际 Electron + Web 操作截图，覆盖登录注册、本机/远端切换、同账号跨客户端历史及纯本地模式。

实际结果：宿主身份/模式/注册复用阶段 1.3 接口；云产品 SDK composition 和独立 Computer 组件已实现。384 项 Web、94 项 TS SDK、34 项 Desktop 回归通过，工作区 lint/typecheck 通过。真实 Electron/Web 的 15 项产品验收全部通过，包括从 DMG 安装的 Desktop；加强完成结果/页面助手回复断言后再次通过，报告 `talos-e2e/reports/e2e_report_20261011_170445.md`。
边界核查：UI 不直连 Harness、不持节点密钥、不维护租约；Desktop 不开第二个云业务控制面。
交付与消融的最终结果见阶段 5 和 [验收记录](../verification.md)。

## 实施切片

| 切片 | 目标、依赖与验收 | 状态 |
| --- | --- | --- |
| 4.1 客户端接入 | 复用 host connection/identity；cloud 使用产品 HTTP SDK，local 使用 stdio。身份/控制面作用域、目标参数对等，真实 bootstrap 不回退。验证 SDK/Web typecheck、实际身份与 RPC 目标场景 | Complete |
| 4.2 Computer 组件 | 独立 Rail/Item/Overflow/Page；默认本机、轻量标签、完整搜索/分页和管理，保留现有账号菜单。依赖 4.1；验证键盘/主题/i18n 与实际截图 | Complete |
| 4.3 产品接线 | Project/Session/env/new Turn 目标绑定、草稿/缓存隔离、固定会话恢复和离线状态。依赖 4.1–2；真实 Electron/Web 操作 + 前端 checks，再对照 Spec | Complete |

当前：4.1–3 已完成。云模式直接访问同一个产品控制面，本地模式复用原 stdio API。

4.1 实际结果：37 项客户端测试、全工作区 typecheck、Web lint 通过；新增 stdio 目标保留场景后，Web 全量 384 项通过。云 product API 使用 host endpoint 与实际 Actor，stdio 目标参数保持；凭据只由宿主持有。

4.2–3 实际结果：真实本机/远端文件效果和完成结果、跨 Desktop/Web 历史、运行中切换、改名/启停/凭据恢复、旧内嵌会话权限、个人模型配置刷新、作用域草稿、22 节点分页、离线历史、键盘/主题/中英标签、退出换账号隔离全部通过。采用实际 Electron/Chrome 截图，纯本地 DEV smoke 也通过。具体边界与消融见 [验收记录](../verification.md)。


Review 修复：同一 Sidebar 的窄屏 dialog、Project 单一 query owner、未用 MemorySlice 消融、完整 talos query 前缀；首次注册结果丢失有显式 UUID 恢复入口。当前源码真实 UI 20 场景通过，结果见验收记录。 [实际结果](../verification.md)。
