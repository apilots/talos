# MCP 与图片链路提交验证记录

日期：2026-09-11。范围为本次整合提交中的 MCP、文件与图片、工作区接口及错误提示改动；Composer 交互优化保留此前独立提交。

## 交付范围

- 设置页面添加 stdio / Streamable HTTP MCP，导入单服务 JSON 或 `mcpServers`，通过 Plugin manifest 管理安装、启停、替换和卸载。
- rmcp 升至 3.2.0，接入新发现/订阅并保留 SDK 旧服务协商；后台连接、进程退出观察、工具目录更新、取消与收尾由现有连接层负责。
- 支持 Bearer、API Key、本地 OAuth，凭据私有存储并绑定服务；刷新和存储故障保留原始错误来源，公共响应脱敏。
- 工具图片经文件存储进入模型上下文；HTTP、App Server、Embedded SDK 提供授权文件内容读取。消息区域展示简洁缩略图，点击展开，支持关闭与 Esc。
- 四份 YAML 加入 DeepSeek Vision 模型，仅该新增模型声明图片输入；沿用 Chat Completions。
- 包含另一 worktree 合入的工作区信息接口、设置页展示及系统通知/模型警告的强类型错误投影。

## 全局审查与消融

三位 Subagent 分别审查后端 MCP/Plugin/凭据、前端/Desktop/TS SDK、其余 Rust 模型/文件/网关链路，发现并修复三项 P2：

| 场景 | 最小修复与验证 |
| --- | --- |
| Embedded SDK 读图片恒返回接口未实现 | 补齐现有接口，直接使用同一 Actor 调用 FileService 返回 bytes。真实隔离 DEV Embedded 进程验证上传、内容/元数据一致、删除后拒绝读取及正常关闭；修复前同一探针失败。 |
| HTTP/STDIO 切换后隐藏输入阻断保存或误带凭据 | 仅校验和提交当前传输字段。参数化用户场景三项在修复前失败，修复后通过；浏览器双向切换保存后分别进入 Ready。 |
| 市场 manifest 大小写重复请求头被静默覆盖 | 最终 HTTP 头构造显式拒绝重复。复用既有真实 HTTP/Plugin 场景验证重复头失败、正常 API Key/Bearer 服务可调用；修复前同一场景失败。 |

扩大测试发现现有默认模型清单断言遗漏新 Vision 模型，补齐预期，未放宽断言。

此前消融已删除重复启动期限、Runtime 纯转发、手写资源 JSON、无生产者的 Unknown 变体、重复协议元信息及弱环境测试。移除旧握手回退和版本检查会分别导致旧服务接入失败、未知版本被误接受，因此保留。取消、错误、凭据失效和内容边界的负向测试有当前生产场景，未批量删除。

提交前按文件职责整理 MCP 翻译、配置读写、工具目录及附件转换，复用现有 owner；SDK 测试合并重复初始化并保留行为断言。没有新增 manager、协议框架或通用抽象，未调整仓库规模限制。所有新增文件及修改通过 changed-file policy。

## 最终验证

| 验证 | 实际结果 |
| --- | --- |
| 19 个受影响 Rust 包，`just test` / nextest，并行度 2 | 853 项通过，10 项按既有配置跳过 |
| 全前端 `pnpm --dir talos-app ci:frontend-app` | Web 382、Core SDK 52、App Server SDK 26、Browser SDK 10、Desktop 30 项通过；2 项原有跳过；lint、类型、构建和仓库检查通过 |
| Rust 质量与产物 | 19 包 `just fix`、格式、协议生成通过；保留既有 ts-rs 对 `deny_unknown_fields` 的提示 |
| 真实 MCP E2E | 现有 `09_app_server/mcp_e2e` 使用隔离 DEV 派生配置、真实 DeepSeek Vision 和官方 Playwright，安装/启停/发现/导航/截图/卸载及 App Server 退出通过，最终轮 19.77 秒 |
| 重建并重启 HTTP 后端 | `just build -p talos-server` 成功；旧 PID 15749 正常退出，新 PID 21551 使用仓库 `conf/talos-dev.yaml`，健康检查 200 |
| 新 HTTP 进程上的故障验证 | 市场重复头返回 `invalidRequest`，健康服务 Ready，后端持续健康；日志有服务名、错误和捕获位置 |
| 新 HTTP 进程上的图片验证 | 历史双图缩略图解码成功（1280×900、1254×1254），分别展开，按钮/Esc 关闭通过 |

本地完整日志、探针源代码、重启记录和截图位于 `output/mcp-global-mr-2026-09-11/`，不纳入源码提交。最终 E2E 报告文件名为 `e2e_report_20260911_085658.md`。临时测试插件、插件源、HTTP fixture 和 Embedded 探针 example 均已清理。

## 验证边界

Context7、GitHub PAT/OAuth、Notion 真实账号及 Windows 进程清理仍待外部条件；本期不实现 Cloud 多用户。此前 Desktop 逐场景 DEV 验证已有记录，本轮没有重新验收独立 Desktop 安装包。HTTP 重启已实际成功；不据此宣称穷尽或修复了历史所有关闭挂起场景。
