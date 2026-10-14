# 阶段 1：统一控制面与节点注册

Status: Complete。依赖：已获用户实施授权（2026-10-11）。
目标：联网宿主使用同一控制面，完成稳定身份、归属、租约和产品管理闭环；尚未安装执行连接的节点不可用。
方案：[design](../design.md)，[contracts](../protocol.md)。

步骤：

1. 在现有 Cloud 配置增加唯一 agent_base_url 默认值 https://agent.talos.love；产品 API 与 Worker 指向同一控制面。
   认证继续使用既有 Cloud Auth 地址。Runner 可显式覆盖私有控制面；outbound 使用系统 TLS 信任或指定 CA。
2. Runner 区分 outbound/direct 配置校验；outbound 不要求监听地址/服务端证书。
   控制面的 trusted_node_ca 仅 direct 拨号需要，outbound-only 不要求它；保持既有 direct 部署功能。
3. 修改现有 Create/Update DTO 的 endpoint presence，新增迁移放宽 agent_nodes CHECK；保护 owner/credential/lease 约束。
   NodeInfo 规范定义唯一；HTTP/stdio/管理 RPC 共用 NodeService 的授权与发布/撤销入口。
4. 完成 Computer list/search/total/read/create/rename/setStatus/rotateCredential，补 Gateway、SDK 和协议生成物。
   个人 owner、本地可信 Actor 按 Spec 授权；共享 embedded 将既有业务访问与管理员实例配置权限分开。
   list/read 保留普通用户有权访问的 embedded，不能把本地权限应用于公网注册。
5. 定义 Desktop cloud/local 连接设置、登录到节点创建的宿主调用、UUID/凭据原子保存与安全 IPC 身份返回。
   保存身份与账号不匹配时明确拒绝；创建结果不确定时通过既有凭据恢复流程处理。
6. 贯通“真实登录 -> 创建 UUID -> 保存 -> 重启读取 -> Register -> Heartbeat/RenewLease”流程。
   出站节点 Register 不再触发 direct endpoint 拨号；直到阶段 2 Connect 发布才 available。

成功标准：同账号 API 列出同一节点；UUID 重启/改名不变；无 endpoint 可合法注册；凭据不进 renderer/日志；
无权节点不可见；未发布连接保持不可执行；共享 embedded 与个人节点权限正确；旧 direct 配置仍可用。

拟执行检查：

- just check -p agent-pb；agent-pb build.rs 生成 protobuf，新增资源列入 build inputs。
- just test -p agent-protocol -p agent-client -p talos-store -p talos-service -p talos-base -p talos-gateway
- just gen-app-protocol；配置 schema 使用现有 owner 的生成命令，同步示例与生成物。
- pnpm --dir talos-app --filter @talos/sdk-core test；SDK browser/app-server scoped test/typecheck。
- 真实认证服务与控制面的公开 API 注册/续租验收；改名、停用和恢复使用真实管理入口，不注入 SQL。

实施切片：

| 切片 | 目标与验收 | 状态 |
| --- | --- | --- |
| 1.1 注册与持久化基础 | optional endpoint 的 domain/PB/SQL 迁移；无 endpoint 注册不可执行；系统 TLS 根/私有 CA；旧 direct 回归 | Complete |
| 1.2 产品管理入口 | 唯一 Node DTO；个人/embedded 可见范围；搜索/分页/total；只改名/状态；六个 HTTP/stdio 方法和 SDK；统一撤销 | Complete |
| 1.3 宿主身份与配置 | 登录后创建/保存 UUID 和凭据、安全身份 IPC、cloud/local 设置、Runner outbound 校验 | Complete |
| 1.4 联网注册验收 | 真认证登录/保存/重启/注册/续租，与阶段 2 Connect 分开验收 | Complete |

实际结果（2026-10-11）：

- 495 个基础 Rust 回归通过，包括真实独立 Worker 子进程 direct 注册/控制面重启/注销，新增迁移与无 endpoint 注册场景。
- 产品入口变更后 348 个 Rust 测试通过，包含实际 HTTP 路由/认证中间件/节点数据库的创建、查询、改名、停用和凭据轮换场景。
- 随后的 Store 查询范围消融改为 typed NodeListScope；12 个相关场景定向复验通过（其余按过滤跳过）。
- 首轮并行回归有一条既有 MCP OAuth 并发授权测试失败（ToolExecutionFailed / Cancelled）；第二轮完整回归及定向复验通过，未改 OAuth 代码。
- SDK core 类型检查与 52 个测试通过；browser/app-server、Desktop 类型检查通过；Desktop IPC 12 个检查与 SDK/desktop lint 通过。
- 配置 101 个测试通过；App protocol 的 JSON schema/TypeScript 由 just gen-app-protocol 生成。
- scoped Clippy、check:repo 与 git diff --check 通过；管理模块拆分后的注册/Computer 定向复验与 Service/Base Clippy 通过。

阶段 1.3 实际结果（2026-10-11）：

以下为043历史切片；云宿主裁剪管理runtime、无本地业务DB及outbound完全无listener的组装已由[044统一Worker RPC阶段](../../feat-044-talos-sync/plans/01-worker-rpc.md)替代。当前Desktop保留完整管理runtime、原业务DB与同一Worker的loopback RPC listener；不将旧切片测试计为新架构验收。

- Desktop main 注入设备名并启动原 app-server；默认 cloud，显式 local 保存后重启同一宿主。没有新增二进制/执行进程。
- cloud 宿主只组装 AccountService、登录缓存和 TalosWorker 注册；不创建本地业务数据库。普通 stdio/Server 与纯本地功能保留。
- 登录与重启恢复后创建/复用 UUID；凭据以 Unix 0600、原子替换和 fsync 写到 conf 下，配置先记录 UUID；pending 标记防止未知结果重复创建。
- 归属、控制面、凭据有效期和 supplied Actor 检查明确失败；默认设备名只用于首次创建，后续用户改名与 UUID 保留。
- 安全身份 IPC 与四个命名宿主方法、SDK/生成物已接线；注册错误通过既有 runtime-state 的 ApiError 反馈，登录结果和有效缓存不被重置。
- Runner 支持 outbound 无监听证书，保留原 direct 配置；共享 Worker 注册组装，退出经 stdio EOF drain/注销，非正常退出明确报错。
- JSONL 入口复验修正 omitted/null 空参数；退出测试发现并修复 Register 提交/取消竞态：已发出请求在原 10 秒超时内等待响应，然后注销，不增加 DB 状态。
- 180 个 Rust 回归通过；最后协议/退出修正后 3 个身份/配置场景及 36 个生命周期/原 direct 子进程回归通过。
- Desktop 34 个、SDK core 52 个、SDK app-server 28 个测试通过；两条原外部 integration 测试仍按环境开关跳过，不计入通过。
- scoped Clippy、类型检查、lint、fmt-check、仓库检查和生成物检查通过；本阶段的最终文档/格式复验见根计划。

Spec 对照：身份与密钥在宿主 conf；Workspace 文件在 Worker；云业务 DB owner 仍是控制面；
无额外 Worker、Registry、表或业务 PB 定义；未把注册成功当作执行可用。没有发现该切片的职责偏离。
阶段 1.3 验证使用受控 TLS NodeRPC、真实 NodeService/Store、真实 Worker 与 JSONL/进程退出；该切片认证为受控 AuthService。
阶段 1.4 的真实认证结果见下方；该切片只验收注册，Connect、环境与云产品 UI 的最终结果见各后续阶段和 [全特性验收](../verification.md)。
实现边界复查：管理方法在 management.rs，Computer 方法在 computers.rs；原 Service 保留租约/认证。
配置 Config 没有独立 schema generator；示例 YAML/配置测试同步；公开 API 使用已有 App protocol generator。

阶段 1.4 实际结果（2026-10-11）：

- 新增 `18_computer_workspace/01_registration`：真实 Cloud Auth、Talos Server、同一 TLS/HTTP2 origin、两个实际 Desktop controller/SDK/app-server 宿主；设备登录走真实 start/approve/poll，不注入缓存 Token，不修改数据库。
- 10 项场景全部通过：登录和 0600 缓存/凭据保存；账号隔离；实际 10 秒心跳/30 秒续租；改名和宿主重启；控制面重启；停用后恢复；同账号两 Computer；跨账号拒绝旧身份；真实 logout 撤销 Token/注册；日志与安全身份无密钥。
- 续租前后租约哈希相同、expiry 延长；控制面重启后哈希变化，证明重新注册。非法 lease 与轮换前 credential 均被拒绝；DB 只读用于断言。
- 真实场景发现并修复：旧 Runner 收到停用后以鉴权错误结束，首次恢复被清理历史错误阻塞。成功执行清理后报告已拒绝/过期注册的原错误，不阻塞恢复/退出；其他注册、join、执行清理错误继续传播，无新抽象或状态。
- 最终完整 E2E 通过（73.22 秒）；旧 `17_multi_node/07_lease_expiry` 通过（138.56 秒）。相关四个 Rust package 的 37 个测试全部通过、0 skipped；scoped Clippy、E2E driver 严格类型检查、Python/shell 语法、fmt-check、check:repo 与 diff-check 通过。
- 证据：[新场景](../../../talos-e2e/scenarios/18_computer_workspace/README.md)，`talos-e2e/reports/18_computer_workspace/01_registration/registration-evidence.jsonl` 与实际进程日志；运行命令 `TALOS_E2E_CADDY_BIN=<caddy> just e2e-group 18_computer_workspace`。
- Spec 对照无职责偏离：宿主保存 UUID/凭据，Worker 负责执行态和本地文件，控制面保存 metadata；outbound 无监听证书，注册仍 available=false。恢复收尾语义已同步 design/protocol。
- 本切片真实 Auth 服务运行于隔离本地实例；认证 approve API 和 Desktop main controller 已实测。Connect、Electron/Web UI 和安装产物分别由阶段 2/4/5 验收，公网部署另列。

阶段 1 Complete。后续 Connect、环境与产品/安装验收已依序完成，见相应阶段及 [验收记录](../verification.md)。


Review 修复：首次 pending 身份由用户显式选择 owned outbound UUID 恢复，后台 Runner 错误沿宿主 JSONL/runtime-state 实时反馈。真实注册定向 3 PASS，身份不变、登录保留、无重复节点。 [实际结果](../verification.md)。
