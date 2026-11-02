# Talos 多端同步接入接口

Status: Implemented；本轮账号选择专项已自测，统一8/8验收通过，见[本轮证据](verification.md)。共享wire结构/SDK API由[独立Sync契约](https://github.com/apilots/talos-sync/blob/7230f9fdf294e7ae218bdb6d3cf1c4ef60203380/spec/feat-044-talos-sync/contracts.md)维护，Talos只定义原业务事务接入和产品接口。

## 3. 库 API 与宿主端口

`TalosSync` 是Talos内管理节点集成lib的facade；`SyncHub` 属于同级独立talos-sync工程的服务包。内部store/records/runner默认私有。
管理端lib依赖原domain/storage/error，原Service/Store注入数据adapter；独立server不依赖这些业务层或Talos源码路径。
管理端通过talos-sync-sdk接入；SDK/server使用公开types与SDK的单份codec/文档能力。原DTO生成任务仅在协议更新时运行，生成的独立wire定义随types发布，正常服务构建不读取Talos源码。共享types/SDK不含SQL/业务事务，客户端和hub事务归各自宿主。

| 参数/对象 | 关键字段 |
| --- | --- |
| SyncOptions / SyncServerConfig | 宿主hubEndpoint / 服务listener、storage、auth、limits；endpoint由配置所有，不散落业务。 |
| SyncBindParams | 已验证scope、sourceUserId、ComputerAlias[]；peer从持久identity读取。 |
| SyncStartParams | 一个已安装binding、现有账户凭据获取callback、TLS信任和重连间隔；库不另存Token。 |
| SyncScanParams / SyncScanPage | binding、闭合resource kind、cursor?、limit / resource keys、nextCursor。 |
| SyncExportParams | binding、resource；export在传入的原StorageTransaction固定读cut中提供已有typed facts。大历史按同一读cut分批capture，不拼接reset前后。 |
| SyncResourceSummary / SyncPage | resource key、computerId?、version、deleted / data、nextCursor、feedCursor。 |
| VerifiedCredential | SDK宿主callback提供已验证accountId/accessToken；服务再经cloud SDK校验Token并取得账号，无独立Computer授权集合。 |
| SyncStatus | connection、pendingChanges、lastAcknowledgedAt?、lastError?: ApiError。 |

| API | Contract |
| --- | --- |
| `TalosSync::open(storage, SyncOptions)` | Open persistent identity/documents; no network or business execution. Options contains hub endpoint. |
| `bind_account(actor, SyncBindParams)` | Return SyncBinding after verified account/source-user/Computer association; no implicit anonymous claim. |
| `enabled(account_id)` / `set_enabled(account_id, bool)` | Read/persist the verified account choice in this host DB, even before discovery; no network, YAML write or other-host preference propagation. |
| `capture(tx, SyncCaptureParams)` | Append typed changes in the existing business transaction. No network await; rollback removes capture. |
| `start(adapter, SyncStartParams)` | Return SyncHandle; authenticate, discover, export/import and reconnect. Credentials come from existing account lifecycle. |
| `SyncHandle::catch_up(resource, minimum_version?)` | Recover and materialize a complete resource cut on the handle's existing authenticated connection. None requests the current hub cut; an unpublished minimum fails explicitly. |
| `SyncHandle::flush(resource)` | Fix the current transaction's committed outbox sequence and VV, then await a durable hub cut including that VV without waiting for later commits. A remote terminal Delete superseding this cut returns Conflict rather than a false ACK. Used before opening that 历史游标 to a different coordinator. |
| `SyncHandle::subscribe_changes()` | Bounded resource/reset/delete summaries after the original import commits; a lagged reader reloads original APIs. Presence remains transient in the SDK. |
| `SyncHandle::status()/subscribe()` / `shutdown()` | Existing safe ApiError + bounded status watch; bounded stop/join. No global handle registry. |


`TalosSync::start`返回绑定一个账号的`SyncHandle`；catch_up/flush使用有界命令与oneshot复用该runner连接，业务不直接使用socket。SDK只持有连接状态/有界网络缓冲，不再建第二份outbox/业务存储；已发送而未确认的增量从集成层持久状态恢复。关闭当前端账号时runner退出，capture仍保留本机提交；重新启用需显式start。
服务端ACK仍由`SyncHub`持久事务决定，SDK不能提前替hub确认；认证使用现有账号Token，不增加SDK专属Token租约。
`SyncVersion.version_vector`始终保存/传输官方不透明编码；相等、包含与并集使用SDK的`version_equal`/`version_includes`/`version_union`解码后调用官方VersionVector语义，不能以字节相等、字节哈希或自造排序判定共享历史游标。

纯本地未绑定时沿用原Store/API，不调用catch_up/flush，也不为请求建立账号绑定。已绑定但H不可达时capture持久outbox，不能等待网络才提交本机业务。
当前控制面已持有所需历史游标的完整事实时不必每次Start访问H；flush仅为把本端新事实开放给另一协调者，不是同一协调者继续执行的前置条件。

```rust
pub struct SyncCaptureParams {
    pub binding: SyncBinding,
    pub resource: SyncResourceKey,
    pub changes: Vec<SyncChange>,
    pub created_at: i64,
}
pub struct SyncImportBatch {
    pub scope: SyncScope,
    pub resource: SyncResourceKey,
    pub version: SyncVersion,
    pub changes: Vec<SyncChange>,
}
pub trait SyncDataAdapter: Send + Sync {
    // Scan existing authorized Stores for bootstrap, including empty Projects.
    fn scan(&self, params: SyncScanParams)
        -> impl Future<Output = TalosResult<SyncScanPage>> + Send;
    // Export canonical facts from the same original transaction read cut.
    fn export(&self, tx: &mut dyn StorageTransaction, params: SyncExportParams)
        -> impl Future<Output = TalosResult<Vec<SyncChange>>> + Send;
    // Materialize facts only; no recovery tasks, locks, accounting or Agent calls.
    fn apply(&self, tx: &mut dyn StorageTransaction, batch: &SyncImportBatch)
        -> impl Future<Output = TalosResult<()>> + Send;
}
```

scan 按 sourceUser/既有 keyset 返回资源键和 nextCursor；export固定原事务的generation/high-water cut；分页不拼接reset前后。已有资源初次绑定须在启用增量业务hook前完成baseline capture，不能用缺少原字段的diff重建空文档。
apply 在文档/cursor 同一事务中调用原 Store 的导入路径；导入标记禁止再次 capture，不能触发业务工作流。
完成 Session 所需 Project/Computer/tree/File metadata 一并补齐；确有未到的数据明确不可执行，不用空对象兜底。
active 展示从同步文档读取，原 agent_active_turns 只保留本端实际协调任务；导入 running 不安装假执行任务。
本地 accounting/反思等副作用只由实际执行该 Turn 的原 owner 运行，导入采用事实替换/去重。
## 5. 产品 API、SDK 与实时反馈

现有 Computer/Project/Session/Timeline/Files API 继续唯一；新增本端 `sync/enable`、`sync/disable`、`sync/status` 及对应 HTTP 管理入口。
enable 完成真实登录/可信认领，disable 停网络但保留 outbox/binding；status 返回 Disabled/Connecting/Connected/Disconnected、未确认变更和安全错误。
SDK 隐藏 peer/同步版本；不引入 origin routing header、syncContext 列表包装或 sessionSync/list/history 平行 API。

选中 Computer 后，当前服务从原 Store 读取已物化 Session，由自己的 Coordinator 开始空闲 Turn。
执行中的 Input/Interrupt 与内存 queue 由 agent-client 的当前 Turn 通道交给实际协调者处理，不按创建节点路由。
Workspace/模型/配置/Skills/MCP 都用现有 WorkerEnvironment；OAuth start/completion 保留同一个当前 attempt，sync不复制 secret 或 OAuth 状态。
附件上传归当前FileService；旧附件只通过正文源peer的FileService读取，不直接访问远端磁盘/OpenDAL、不经H传正文。当前FileService依据sync File的storagePeerId选择本端读取或经G管理连接调用源FileService；远端记录使用原FileStore内闭合位置分支，不伪造本端storageId/objectKey。读取、tags/metadata更新及delete在源FileService完成，再正常capture metadata/tombstone；import/tombstone不对接收端磁盘执行删除。具体载体见[routing](rpc-routing.md)。

实时链：Worker -> 当前协调者原事务 -> capture -> hub durable update -> 其他端原事实/Timeline通知。
执行结果先通过loopback/direct/agent Connect到当前协调者，commit后立即通知本端UI；后半段sync失败只影响其他端同步进度。
资源端口保持原PB，Gateway按当前invocation把请求/结果双向送到当前协调者；sync不传资源RPC或作为其鉴权凭据来源。
每个 committed batch 唤醒同步，不等 Turn 结束；原唯一 History reducer 处理已物化 Timeline，不加 Replica result 模型。
Timeline 页游标属于本端 DB/account/Session/generation；导入稳定键映射后使用本地 event ID/seq，reset 使旧游标失效。
SDK/UI 按 endpoint/account/Computer/Session 隔离缓存，断连展示最后状态；旧回调不穿透账号切换。
公共错误保留 TalosError/ApiError code 与 localization；不公开诊断 Display，不重复报告执行错误。

## 已实现的宿主接入（阶段3）

- `TalosApplicationRuntime::start_account_sync(&RequestIdentity, SyncBindParams, CredentialCallback) -> Arc<SyncHandle>`：原AuthService复核账号及本机Computer alias；安装原四个Store的账号capture binding，启动SDK runner，将导入事实通知到原Timeline。自动登录和持久enable/disable由阶段4产品流程负责。
- `TalosApplicationRuntime::connect_computer(&RequestIdentity, computer_id)`：G认证/部署发现、Coordinator.Connect、Harness/Environment客户端与连接回收内聚在宿主。自身Computer保留loopback Harness，但仍建立到G的管理回传连接。同步导入目录不颁发本地凭据/租约，不重复注册。
- `shutdown_sync()`：关闭已安装管理连接/runner；未确认历史游标仍为Local，不伪造Shared确认。网络恢复沿已有status/flush确认历史游标。
- 原Service使用`ActiveSessionCommands`闭合原Input/Interrupt/六项MessageQueue DTO；原FileService使用`FilePeerAccess`读取源正文和源侧改删。两者由宿主的同一个G SDK/账号credential callback组装，不在Sync库中处理RPC或业务授权。
- 原通知增加`session/activeTurnChanged`，只承载Session ID及原SessionActiveTurnResponse。Timeline缺口仍从本端原Store replay；管理端导入通知积压后按账号分页刷新原投影，不创建fake claim。
- 原业务DB新增`sessions.history_generation`；clear与原事实在同一事务递增。FileStore持久位置为Local/Peer二选一；源侧存储键不暴露给Peer调用者。只有G保留注册credentials/lease。

## 阶段4产品控制接口（已接线）

原`TalosService`统一提供`sync_status(actor)`、`enable_sync(actor, SyncEnableRequest)`、`disable_sync(actor)`；HTTP为`GET /v1/sync/status`、`POST /v1/sync/enable`、`POST /v1/sync/disable`，原stdio JSON-RPC为`sync/status`、`sync/enable`、`sync/disable`。Web与Desktop的原SDK均暴露`client.sync.status/enable/disable`；schema/TS由原app-protocol生成器维护，不另建历史或管理协议。

`SyncEnableRequest { claimLocalHistory: bool }`默认false，只有可信本机管理宿主允许显式认领匿名历史。原SessionStore按事务更新Session/Project/File的业务owner，保留Worker、Workspace、旧filesystem user namespace和稳定事件ID；现有Worker排他检查阻止执行中的认领。

`SyncStatusResponse { configuredEnabled: bool | null, connection: disabled | connecting | connected | disconnected, pendingChanges: number, lastAcknowledgedAt: number | null, lastError: ApiError | null, localComputerId: string | null, canClaimLocalHistory: bool }`。本机UUID只来自当前已验证账号拥有的注册身份；未注册的可信本机沿用原内部embedded别名；当前账号不拥有共享服务宿主的private Worker时返回null，不把未授权节点注入Computer列表。技术配置中的UUID本身不授予账号访问权。首次alias绑定向G核验owner，已有绑定沿用已验证的同账号证明；重启恢复capture不恢复Token、不主动连接旧账号。

`sync.enabled`显式false优先；未配置在验证登录后自动绑定H。产品enable/disable只改变当前管理宿主当前已验证账号的sync_account_preferences.enabled；没有binding时也能保存选择，Desktop与公网Server管理员均不写宿主YAML；其他账号、同账号其他宿主继续同步。部署operator显式false禁止本管理服务所有账号连接H，优先级最高；原status观察到禁用时关闭所有已安装H handle，产品接口不能覆盖。恢复部署允许后仍读取每个账号的原选择。关闭只停止H，保留事务capture/outbox、G RPC连接与源FileService。G注册失败和H同步状态独立；登录成功不被注册/同步故障改写，切账号/登出停止前账号连接与原资源回调凭据。

Computer定义只由中心G原AgentNodeService创建/变更，Desktop节点表是当前账号metadata副本，不复制credential/lease，也不颁发新租约。原`create/update/rename/setStatus/rotateCredential`在非中心管理宿主通过同agent-client Node SDK转给G；rename/setStatus为窄RPC，保留未修改字段。UUID到embedded的映射只在宿主内部，公开Session/Project/Computer API仍返回同一真实UUID；目录、配置、执行继续走原Worker RPC SDK。

实时活跃状态复用原`session/activeTurnChanged`通知，消息队列复用`messageQueue/changed`，Timeline保持原事件/游标。Desktop原Controller/IPC/SDK转发这些通知；已打开的空闲Session持续订阅，因此其他协调者启动新Turn时可实时看到进展。列表按原query刷新，未另建Session视图存储。Worker恢复通过`desktop/worker/ready`清除先前的`desktop/worker/failed`显示状态，不编造Timeline事实。

产品界面：ComputerRail只切换Computer、溢出菜单列完整列表，顶部独立管理图标打开右侧完整Computer管理页；当前工作视图沿用单Agent的Session/Project/设置，不放注册、同步开关或Computer管理块。“本机”相对当前连接的管理宿主，由后端localComputerId决定；Desktop与talos-server Web一致，不能只按客户端是否Electron推断。原侧栏标题下轻量显示“已连接到Computer名称”；浅色灰白、深色黑灰，共用现有theme tokens。管理型Desktop仅同步当前验证账号的核心metadata与Session；公网G保留完整注册目录及各账号业务数据，HTTP/SDK读取仍按账号权限过滤。文件正文始终只走源FileService。

## Computer节点详情

原NodeInfo新增可选`systemInfo: NodeSystemInfo`，结构固定：`os: linux | macOs`、`osVersion?`、`architecture`、`hostname?`、`cpuModel?`、`logicalCpuCount`、`memoryBytes`、`boardModel?`、`networkInterfaces: {name, macAddress}[]`、`graphics: string[]`、`workerVersion`。无IP、凭据、序列号或任意JSON扩展字段。空/未知的可选字段在UI隐藏，系统采集错误不影响执行身份；展示自报告信息不用于授权。

Worker通过原RegisterParams的可选systemInfo上报，G验证固定结构/限长、与注册租约事务更新原agent_nodes.system_info列；SQLx编码typed JSON并按原Computer事实同步。租约/心跳仍为实时状态，不把健康状态变成CRDT权威。本机未云注册时由管理组装引用同Worker采集结果填充原Computer视图；其他端不重采集远端信息。原PB、原SDK、sync生成投影同源更新，无新详情协议/存储。

采集实现：Talos `crates/infra/system-info` 独立lib `talos-system-info::collect()`；使用最新[sysinfo 0.39.6](https://docs.rs/sysinfo/0.39.6/sysinfo/)和[wgpu 30.0.1](https://docs.rs/wgpu/30.0.1/wgpu/)（仅std/Metal/Vulkan，无窗口、设备申请或着色器编译）。不自写/proc/sysfs/system_profiler解析；GPU无驱动/容器未透传时为空。sysinfo给出Linux cgroup可见内存上限；不声明物理宿主配置。每个Worker启动采集一次，注册复用；本机未注册展示同一份信息。

## Bounded tool output records

Sync uses the current canonical tool-result schema, including optional
`BashOutput.outputLog`, `FileRead.nextByteOffset` and nullable `total_lines`.
The log reference and bounded preview remain ordinary Session history facts.
The Session's existing immutable Computer binding determines execution and file
access from every controller; Sync does not copy the log body, rewrite its path
or create a separate cross-machine log service. Older results without these
optional fields retain their original semantics. Matching public SDK/types and
service support must precede use by Talos; closed older schemas reject these
fields rather than preserving them.
