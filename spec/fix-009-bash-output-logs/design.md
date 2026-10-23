# Bash 大输出落盘与工具结果限制

Status: active
Owner: talos-tools / Agent Kernel
Created: 2026-10-22
Authorization: implementation, review and local verification requested; accepted defaults retained. Open a PR after local checks pass. Merge and deployment are not authorized.

## 1. 问题与范围

一次 Grep 的结构化结果绕过普通 content 的大小限制，使最新工具交换本身超过模型上下文预算。工作上下文维护保留最新完整交换，无法通过删除旧消息消除这个问题。Bash 另有完整 stdout/stderr 在内存和失败结果中传播的问题。

本方案以 Bash 为主要交付对象：把已经存在的 `ToolResultPolicy.max_inline_bytes`、`preview_lines`、`persist_large_output` 接入实际执行，完整大输出由执行 Computer 的文件保存，模型只收到受限预览与可读取引用。同时修复所有工具共用的模型入口限制，并完成 Grep 与大日志读取闭环。

目标：

- 小 Bash 输出正常内联；大输出保存、可检索、可续读，成功/失败/超时/取消路径一致。
- 进程输出量不决定内存占用；模型结果和流式展示都有明确边界。
- Workspace、用户、Session、Turn、tool call 和执行 Computer 的现有归属保持一致。
- 明确日志配额、保留、崩溃恢复、清理、写入失败和引用过期语义。

非目标：修改自动压缩算法；恢复 TaskOutput/TaskStop；新增通用 Artifact 平台、产品数据库日志表或对象存储；自动同步/上传 Worker 日志；新建日志浏览器、下载 API；改变 Bash 命令超时或进程树取消契约。使用现有工具和 UI 展示受限内容及引用。

本设计中的默认值已由用户接受，并接入配置与工具实现。验证记录保留在仓库根目录的任务计划及忽略的 `output/` 中。

## 2. 修复前的架构事实

| 现有位置 | 已确认行为 | 实施含义 |
| --- | --- | --- |
| `action/tools/src/spec.rs`、`tools/bash.rs` | Bash 策略为 32 KiB、60 行、允许持久化；策略目前只登记到工具规格 | 复用策略类型，补消费路径 |
| `infra/config/src/domains/tools.rs` | `tools.bash.max_output_bytes` 默认 64 KiB | 作为 Bash 预览字节预算的额外上限，不充当落盘配额 |
| `action/tools/src/runtimes/bash.rs` | reader 累积完整 stdout/stderr 和合并 output；响应克隆完整 stdout/stderr | 在读管道阶段改为有界缓冲/落盘，不能等执行结束才截断 |
| `tools/bash.rs`、`runtimes/bash.rs` | `max_output_tokens: Some(8_000)` 实际直接作为字节数使用 | 移除 Bash 的隐含 8,000 字节覆盖；内部参数明确使用字节单位 |
| `registry/output_limits.rs` | 通常只限制 content；成功 content_items 未受限 | 在规范化结果与模型投影边界同时保护 |
| `agent-kernel/src/general_turn/tool_batch.rs` | 成功序列化 content_items；失败额外序列化 envelope、structured | 模型投影不得复制完整原始结果或重复预览 |
| `agent-provider-kernel/src/provider.rs` | 将用户 Workspace 根目录 `EffectiveWorkspace.root` 注入有效 kernel config | 直接使用此目录，不从 CWD、项目目录、HOME 或 Agent 名重新推导 |
| `infra/config/src/workspace.rs` | `EffectiveWorkspace.root` 已解析为 `<workspace_root_prefix>/<user_id>` | 不再拼接 user_id，不写入用户源码仓库 |
| `agent-kernel/src/tool_execution.rs` | workspace root/project root 进入沙箱环境；远程工具在选定 Computer 执行 | 日志与 Read/Grep 在同一执行 Computer；不依赖控制面的本地路径 |
| `agent-provider-kernel/src/artifacts.rs` | `sessions/<session>/YYYY-MM-DD.jsonl` 存 Provider transcript/summary，clear 只处理这些日文件 | 输出日志独立于 transcript，不能扩张 summary store 的职责 |
| `service/src/access/service.rs` | Session 删除的 preflight 会 clear/close Worker，之后才提交业务删除 | clear 不能顺带永久删除日志，避免业务提交失败后丢失证据 |
| `runtimes/fs_read.rs` | 先全量读取；超过 2 MiB 直接拒绝；unchanged 判断不区分读取范围 | 需要真实分段 I/O，且不同范围读取不能被误判为无变化 |
| `runtimes/fs_search.rs` | 全量收集 rg JSON 后分页；匹配行/context 没有总字节边界 | 在生产与投影两处限额，不能只限制匹配条数 |


## 3. 职责与数据流

```text
Config + BashTool.result_policy()
  -> effective Bash output policy
  -> existing Bash runtime / owned child process
       stdout + stderr readers
         -> existing capture mutex -> one ordered output collector
              -> bounded preview
              -> spill writer + typed local metadata
         -> bounded existing progress events
  -> bounded BashOutput + output log reference
  -> registry canonical-result limiter
  -> Kernel model projection + final serialized bytes/token guard
  -> model

Read/Grep -> same Computer + same sandbox -> referenced log -> bounded result
```

- `talos-config` 拥有默认值、校验和用户 Workspace 根目录解析；工具层直接消费 `EffectiveWorkspace.root` 及其已注入的有效配置，不重复拼接 user_id，不散传不相关标量。
- `talos-tools` 的现有 Bash runtime 拥有 collector、writer、预览、文件记录及单 Workspace 的文件维护能力。周期调度、跨 Workspace 发现与轮转归 Worker；不由 Bash 调用触发。可拆成 `runtimes/bash/output.rs` 等具体模块；不新增通用 manager/store trait。
- Registry 拥有规范结果的统一限制。Kernel 拥有模型投影及最终消息预算；两个边界共用限制契约和元信息，不各写一套摘要算法。
- 原有进程、reader、waiter、timeout、取消任务继续由 BashSession 管理；writer 完成是本次工具返回的必要条件，不引入脱离任务组的后台写线程。InvocationGuard 在调用方 future 被强制 abort 时移除运行时 BashSession、终止所属进程、取消 timeout/waiter 和后续读取；现有 reader 继续完成已经接收的写入，并由最后一个 reader 提交 interrupted 记录后释放日志锁。
- 协调层与 UI 只接收有界预览/引用，不搬运整份日志；日志不是产品 Timeline 正文，也不写进 transcript/summary JSONL。

## 4. 配置与生效语义

### 4.1 接通现有三个策略字段

| 字段 | 建议语义 |
| --- | --- |
| `max_inline_bytes = 32 * 1024` | Bash 模型结果的总字节上限，包含 stdout/stderr 预览、状态、路径、提示、JSON 转义开销 |
| `preview_lines = 60` | 超限时最多显示最后 60 个逻辑行；不足 60 行也必须服从字节/token 上限；不是 stdout/stderr 各 60 行 |
| `persist_large_output = true` | 输出不能完整内联时保存完整原始输出；仅因行数超过预览阈值也会触发落盘 |

小输出定义为原始输出及结果封装均可在字节、行数和 token 预算内完整表示。只要任一条件不满足，就进入截断/落盘路径。collector 同时检查 stdout/stderr 的实际 JSON 序列化大小，控制字符转义和无效 UTF-8 替换带来的膨胀也会触发落盘。零值不表示无限。

有效字节上限为 `min(tool policy.max_inline_bytes, tools.bash.max_output_bytes, agent.kernel.tool_result_max_bytes, 10_000)`。Bash 在启动命令前拒绝小于 4,096 bytes 的有效预算，从有效预算中为状态、引用和封装预留 2,048 bytes，再选择预览；Kernel 对实际 JSON 序列化结果继续复核。默认值下预览可用预算为 7,952 bytes，并继续受 60 行限制。全局配置的通用输出下限为 256 bytes；Bash 的最低可用预算在工具准入时额外检查。

三个策略字段目前是工具规格，不是用户 YAML；第一版保持此边界，由 Bash 的现有声明生效。`tools.bash.max_output_bytes` 仍供用户收紧上限。以后确有独立用户配置需求，再在同一配置 owner 暴露这些字段，不复制默认值。修正内部 max_output_tokens 的错误单位，检查现有 runtime 测试；它不是公开 Bash 输入参数。

`persist_large_output = false` 时仍受所有预览限制，结果明确说明省略内容未保存，文件引用为空。不得保留全量内存作为替代。

### 4.2 独立的存储策略

`tools.bash.output_log` 使用以下有类型的配置对象；部分配置对象中未指定的字段沿用默认值，未知字段和不合法配额明确拒绝：

| 建议字段 | 建议默认值 | 语义 |
| --- | ---: | --- |
| `max_file_bytes` | 64 MiB | 每个输出正文文件最多保存的原始字节数 |
| `max_workspace_bytes` | 5 GiB | 当前执行 Computer 上，一个用户 Workspace 根目录内的日志总预算，包含正文、metadata 和活动预留 |
| `cleanup_interval_secs` | 3,600 秒 | Worker 后台维护间隔，启动时立即执行首轮；配置成功重载后唤醒任务使用新配置 |
| `retention_days` | 7 | 从输出捕获终态时间计算，不随 Read 或 compact 延长 |

不增加任意 root 配置。存储目录直接使用 `EffectiveWorkspace.root` 下的 `tool-output` 子目录；持久卷部署沿用 Worker 的 Workspace 挂载。有限的小 metadata 开销纳入配额并预留；配置校验要求用户 Workspace 日志预算可以容纳一份最大正文及其元数据。零值不表示无限。

这些值与内联上限无关。单文件限额到达后继续排空管道并更新尾部预览，停止追加正文，明确标记文件只保存了前缀；不得滚动覆盖并继续称为完整输出。

## 5. Bash 流式捕获与结果对象

### 5.1 捕获流程

1. 从可信 ToolContext 取得已解析的用户 Workspace 根目录、session/turn/call 身份及有效策略；根目录沿用 `EffectiveWorkspace.root` 注入值，不再拼接 user_id。生产调用缺少持久化必需身份时在启动命令前失败，测试显式提供隔离目录。
2. stdout/stderr readers 各以 4 KiB 固定大小读取原始 bytes，通过现有 BashSession 的独立 capture Mutex 交给单 collector。两条 reader 最多各持一块，collector 持锁等待 writer 时自然背压；退出/取消状态使用另一把锁。Progress 使用跨 chunk 的 UTF-8 decoder，预览按 stream 标签保留原始 bytes 后统一解码，文件保存原始 bytes。
3. 小输出仅在有界尾部 ring 中。第一次超过字节、行数或序列化预览预算时，创建独占日志目录，写 Capturing metadata，在裁剪 ring 前补写已缓存 bytes，然后继续顺序追加。状态和引用有预留空间；最终模型入口仍实施硬上限，不能通过原始 structured 内容绕过。
4. `output.log` 记录 collector 接收顺序的合并 stdout/stderr；不承诺还原两个 OS 管道的绝对发生顺序。ring 保留 stream 标签，stdout/stderr 的预览共享一份总预算。第一版不额外复制两份完整分流日志。
5. writer 通过有界缓冲自然背压；不得积累无限待写队列。预览限制不会停止命令；已有 timeout/cancel 仍能打断背压中的执行。写失败按下文转换为捕获故障后继续 drain。
6. 两个 reader 都结束时，由最后一个 reader flush/close 日志并原子提交终态 metadata；工具返回还要等待命令状态确认。reader drain 超过既有关闭期限时只取消后续管道读取，等待已接收 bytes 的 spill/backfill/append 完成，不在持久化中间 abort reader。此时捕获标记 interrupted。timeout 已经认领终止时必须等待其 TERM/KILL 收尾，不提前 abort 该任务。需要声明终态持久化时同步文件及目录；不逐 chunk fsync。命令 timeout/cancel 独立终止子进程；结果仍需等待进行中的文件 I/O 安全收尾，timeout 不充当磁盘 I/O 的截止时间。

内存目标为 `O(preview budget + bounded reader/writer queues)`，不再存储或克隆完整 stdout、stderr、merged output。任何供程序使用的结构化返回也只包含有界数据和引用；程序返回数据也不设置额外的超大内联通道。

### 5.2 有类型的结果

沿用 `TalosContentItem::BashOutput` 的命令状态、exit_code、stdout/stderr、elapsed_ms，新增可空的 `outputLog: BashOutputLog` 元信息，同步 PB、SDK、schema 和客户端消费者。新对象采用 camelCase：

- `path`、`relativePath`：执行 Computer 的绝对路径和相对于已包含用户身份的 Workspace 根目录的路径。
- `totalBytes`、`storedBytes`、`totalLines`：实际接收和保存数量；崩溃恢复只确认已保存字节，不编造未知总行数。
- `captureStatus: capturing | complete | quotaLimited | interrupted | writeFailed`：与命令退出状态独立。
- `expiresAt`：Unix 秒，终态时重置为当前时间加保留天数；内部 I/O 诊断仍通过 TalosError 私有 source/context 处理。

这里的 snake_case 名称用于 Rust/config 或模型投影示意；新增产品 wire 字段按现有 API 规范使用 camelCase，并明确 null/optional 语义，不直接复制示意文本为传输协议。

`truncated` 仅表示模型预览省略了内容；不能用它暗示文件也被截断。命令失败不等于日志不完整；取消后若全部已收到 bytes 成功写入，日志捕获仍可为 complete，命令状态保留 cancelled。

模型投影生成执行 Computer 上的绝对可读路径，因为 Read/Grep 的普通相对路径可能相对于项目 CWD。持久对象保留相对于用户 Workspace 根目录的引用，在同一节点直接以 `EffectiveWorkspace.root` 解析，不再拼接 user_id。参考形态：

```text
status: failed; exit_code: 1
output_truncated: true; capture: complete
total_bytes: 5280000
full_output_path: <workspace_root_prefix>/<user_id>/tool-output/<session-key>/<turn-key>/<call-key>-<output-id>/output.log
expires_at: <timestamp>
<bounded tail preview>
Search this file with Grep, then use Read with offset/limit. The command has finished.
```

提示、路径和元信息也计入总预算。非零退出不把完整 structured/envelope 再序列化给模型。UI/模型错误的命令状态保持同源；不要从提示文本反推业务状态。

### 5.3 Progress

保留现有 ToolProgressEmitter -> ToolProgressDelta -> 同一 Bash block 路径。按有限 chunk 合批/节流，并对每次工具累计发往 Timeline 的输出设预算；建议不超过该工具有效内联字节预算。预算用尽后只发一次“后续输出见日志”状态，并保留最终状态事件。日志捕获继续完整进行。

Progress 内容与最终预览分别有界，不能把每个滚动快照重复写入 Timeline。最终结果提供尾部，早期 progress 可提供开头。该行为需要 Web/TUI 的 live/replay 回归，不能只测试最终模型消息。

## 6. 存放位置与生命周期

### 6.1 目录

统一使用以下命名：

- `workspace_root_prefix`：Workspace 配置的 `root_prefix` 经现有配置解析后的物理前缀，尚未包含 user_id。
- 用户 Workspace 根目录：`EffectiveWorkspace.root = <workspace_root_prefix>/<user_id>`，已经包含 user_id。
- Bash 日志根目录：`EffectiveWorkspace.root.join("tool-output")`。实际代码直接使用 `EffectiveWorkspace.root` 或其现有有效配置注入值，禁止再次拼接 user_id，也不重新由前缀和用户身份计算根目录。

物理目录布局：

```text
<workspace_root_prefix>/<user_id>/
  sessions/<session>/YYYY-MM-DD.jsonl       # existing provider state
  tool-output/
    workspace.lock                        # cross-process quota/maintenance lock
    quota.json                            # bounded durable reservation accounting
    <session-key>/<turn-key>/<call-key>-<output-id>/
      output.log
      metadata.json
      capture.lock
```

默认 `workspace_root_prefix` 为 `$TALOS_HOME/workspace-talos`，因此用户 Workspace 根目录为 `$TALOS_HOME/workspace-talos/<user_id>`；自定义 Workspace 使用现有配置解析后的 `root_prefix`。这些展开仅用于说明目录布局，实际代码始终复用 `EffectiveWorkspace.root`。

- 文件在选定执行 Computer，不在 Web/协调者/仓库目录，不用 OS 临时目录；Worker 重启后路径与终态日志继续有效。
- ID 必须使用已有安全 ID 校验或无碰撞编码；不对不可信 ID 做丢字符后可能碰撞的 sanitize。output-id 由执行端生成，同一 call 重试也不覆盖之前输出。
- Unix 目录 0700、文件 0600；独占创建且拒绝符号链接，Windows 遵循现有私有文件权限能力。写入 confined 到用户 Workspace 根目录的 tool-output 子树。
- Read/Grep 继续走现有沙箱/身份准入。第一版不宣称同一用户 Workspace 内不同 Session 之间新增访问隔离，也不宣称 shell 可写 Workspace 中的日志有防篡改保证。
- 原始输出只留执行节点；Sync/Timeline 同步的是引用和受限预览。另一台管理节点查看历史不意味着其本机拥有文件。Computer 不可用时明确报告不可用，不能在管理宿主尝试同名路径。

### 6.2 生命周期规则

| 事件 | 日志处理 |
| --- | --- |
| 命令完成/失败/超时/取消 | writer 收尾并提交捕获结果；保留到 TTL 或配额淘汰；释放运行句柄不删除日志 |
| Turn 结束、新 Turn、compact、Session close | 不删除；历史引用仍有用，compact 保留引用及是否完整 |
| Worker 正常退出 | 先沿既有取消/drain 收尾，flush 已捕获内容；不清空已完成日志 |
| Worker 崩溃/SIGKILL | 下次后台维护该用户 Workspace 时，仅对可取得 capture lock 的遗留 Capturing 记录执行恢复；标为 interrupted，保留确认的部分输出，不伪造 exit code |
| TTL 到达 | 该引用进入可清理状态；下一次用户 Workspace 日志维护做有界清理，不承诺空闲节点精确在第 7 天执行删除 |
| 配额不足 | 在用户 Workspace 根目录级跨进程锁内，优先清理过期，再淘汰最旧的终态日志；禁止淘汰 writer 正在使用的日志 |
| 所有预算均被活动 writer 占用 | 新日志无法取得预留时返回 UsageLimitExceeded 捕获错误；不生成伪引用、不无限排队、不杀已启动命令；预览及命令状态照常返回 |
| Session clear/delete | 第一版不立即删日志；沿同一 TTL/配额回收，避免 preflight clear 成功但业务删除失败时丢日志 |
| 文件已淘汰/移除 | Read 返回既有 PathNotFound 等 typed 错误；不自动重跑原命令，不修改已提交 Timeline |

维护由共享 `TalosWorker` 的执行运行时持有，使用 common `talos-guard-task`（`crates/common/guard-task`）。独立、Docker 和嵌入式 Worker 注册一个 `bash_output_cleanup`，启动立即执行完整首轮，之后默认每小时一轮。任务与 Session、Turn、Bash 调用和注册连接无关；离线或无人调用 Bash 也检查本机全部已配置前缀下已有日志的用户 Workspace。控制面不扫描远程 Computer 的文件。发现过程不跟随符号链接、不创建从未产生过日志的用户目录；配置移除或改变根目录后，旧扫描范围不再执行下一批。单个范围失败隔离上报，健康范围继续。

common 导出 `GuardTasks`、`GuardTaskSpec`、`GuardTask`、`GuardTaskContext`、`GuardTaskHandle`，只依赖 Tokio/cancellation 与统一错误和日志。task 定义唯一名称、周期、run_on_start 和一轮 `run(context)`；业务自行观察取消并完成安全批次。框架负责唯一注册、周期、手动唤醒、取消与共享幂等 join、异常监督及统一日志，不依赖 Worker/Config/Workspace/Bash/quota。每个任务串行运行，周期不堆积补跑；运行期间配置唤醒合并为随后一轮。panic 由框架报告并以 1 秒至 60 秒指数退避重新执行，正常业务错误报告后保持周期；停止取消等待/退避并等待当前安全操作完成，不强制 abort 正在提交的磁盘操作。Worker 在构造成功后注册，在释放工具资源前 shutdown；clone 共享同一 owner。
普通 Bash 不触发扫描、回收或崩溃恢复。首次落盘只取得 `workspace.lock` 并读取常量大小的 `quota.json`，持久化最大文件正文加 4,096 bytes 的预留后才创建日志。新建日志根可直接初始化空账本；已存在但账本缺失的目录只能由后台重建，落盘准入最多有界等待初始化并在未就绪时明确失败，不能隐式扫描或绕过配额。metadata 与账本收尾更新在同一 Workspace 锁内；失败可保守多计，不得少计。日志创建失败时，仅在确认尚未写入正文、已安全移除已知初始化文件后，才回滚本次预留；发现未知文件或清理失败则报告错误并保留预留。收尾无法确认实际文件大小时保留完整预留，由后续成功的后台扫描校准，不能按零字节释放。后台先在锁内明确调整单份终态 metadata 的占额，再增量更新总账；只读校准累计已验证 metadata 的 reservation_bytes，与后续删除扣减使用完全相同的计费单位。大小无法确认时保留完整预留，不能只缩小总账。5 GiB 是每用户 Workspace 的硬预留上限，不是软回收水位；配额不足不会在 Bash 内联触发清理，也不自动重跑已执行命令。所有 Agent 使用执行节点统一的日志生命周期策略，显式 Agent 覆盖不能提高共享目录配额。该账本约束内置日志 writer 的协作分配；拥有同一文件系统完整写权限的用户仍能手工改写日志或账本，不能把它当成抵御宿主文件所有者篡改的安全边界，也不限制其他 Bash 文件写入。

每小时一轮是完整遍历，4096 只是批次上限。Worker 以 FIFO 等待队列和最多 8 个活动 Workspace 的窗口限制目录句柄；每个 Workspace 保留内存 `ReadDir`/DFS 游标，扫描、清理小批后轮转。每批最多 4096 目录项、128 份日志或 250ms，等待队列最多 128 个 Workspace。发现也分批计入全局预算。全局 30 秒预算用尽时让出约 1 秒，保留游标继续同一轮，不等下一小时、不并发重叠。后台对 Workspace 锁只 try-lock；忙时轮转并短暂让出，不占用前台的 2 秒等待。预算是协作式，不能中断阻塞内核 I/O/fsync。重启或 panic 可丢弃内存游标，从新一轮重新遍历；无需 SQLite、持久任务队列或索引。有限静态树中的错误/活动/未知条目不会阻止尾部健康日志被访问；扫描数量超限本身不得淘汰未过期日志。实际配额压力才允许按当前遍历顺序淘汰已确认归属的终态日志，以容纳一份最大文件预留；不承诺全局按年龄排序。

清理在 Workspace 锁内先验证预期文件和 metadata，再尝试 capture 锁，避免误判活跃 writer。先保存 `cleanup=deleting` 意图，再删除正文、capture lock、最后 metadata，确认完成后才扣除已知占额；中断后的 deleting 记录下轮继续。日志文件分批回收，但保留空调用目录直到 TTL/计费遍历结束，避免 APFS 等文件系统在保留迭代器时删除子目录而跳过后续条目。之后独立分批 `rmdir` 空目录；仅在确实移除空目录时再做一遍空目录收尾，直到无进展。该收尾不再处理 TTL/覆盖账本，不递归删除未知数据。

`quota.json` 保存 `{version: 2, used_bytes, revision}`，仍是唯一实时账本。reserve、finalize、恢复、删除及目录裁剪在同一 Workspace 锁内更新/使 revision 失效；即使使用量 ABA 回到原值也不能发布过期校准。缺失、损坏或旧格式账本不准入新 spill，后台先以独立只读完整遍历尝试恢复，再进行 TTL 清理；正常有效账本直接进入分批清理。清理后再独立只读完整校准；仅在所有记录可信且起止 revision 相同时发布总额。任何不完整扫描不得覆盖 used_bytes。并发写入或读失败只延后校准，不重置 TTL 游标；持续活跃时允许保守多计。删除成功但扣账前崩溃会多计；metadata 已移除，不能重复扣减其他日志的占额，完整可信校准可恢复。

通用 `talos::guard_task` 记录 INFO 启动、轮次开始/结束及停止，包含 task_name、interval、trigger、成功/取消与耗时；监督异常包含退避日志。Worker 的 `talos::output_logs` 记录业务轮次、预算让出、计费延后及扫描/删除/恢复/活动/失败计数、批次、Workspace 数、回收字节与耗时，DEBUG 记录每批进展，不记录正文。记录/Workspace 错误由 Worker 携带范围上下文报告，公共 runner 负责未处理的整轮错误和 panic；同一错误只有一个报告 owner。
跨进程并发使用同一用户 Workspace 根目录时，内存 Mutex 不够：该目录内的日志配额/淘汰和每个活跃 capture 使用文件锁。为新 writer 预留该文件最大正文加 4,096 bytes metadata 开销；实际 pretty JSON 每份不超过 2,048 bytes，以容纳原子替换时同时存在的新旧记录。结束后改计实际保存大小，防止多个命令各自通过配额检查后共同超限。终态 blocking metadata 任务拥有完整 writer 和 capture lock，即使等待者取消，也要在提交结束后才释放锁。清理只依据已验证身份 metadata、捕获状态和锁，不因目录看起来像旧文件就递归删除。

立即随 Session 删除日志涉及 clear/delete 意图区分、业务提交后的执行节点清理及离线恢复；明确列为后续扩展，不在本次工具修复中引入跨节点删除协议。对要求立即擦除的部署，TTL 回收不等于删除时擦除承诺。

### 6.3 磁盘故障

文件创建、写入或 flush 失败不允许回退为无限内存。停止落盘并继续有界 drain，保留已知 exit_code 和预览，返回 typed 捕获失败及 `capture=write_failed`，不承诺完整文件、不中途重跑命令。命令 exit_code=0 但承诺的持久化失败时，工具结果是失败，命令退出事实仍为 0；错误不可默认建议重跑有副作用命令。

正常单文件配额截断是显式的完整性降级，保留命令成功/失败；意外 I/O 故障是工具捕获失败。flush/sync 失败时仍尝试提交 writeFailed 元数据，storedBytes 以文件实际大小校准，不能把 Tokio 已接受但尚未成功写入的字节当成保存量。管道读取失败保留 typed 捕获错误，但另一条管道已接收的 bytes 仍继续保存；只有 writer 自身失败才停止继续写盘。命令启动前的错误仍由 TalosError -> ToolDispatchFailure 路径交给 Kernel 报告。命令已执行后的捕获错误必须同时保留命令状态/输出，因此由 Bash runtime 的最后一个 reader 在捕获收尾时报告一次原始 TalosError，再将受限 ApiError 与结果一起返回，Kernel 不重复报告；即使调用方已被强制取消，报告 owner 仍存在。没有打开成功的文件不生成可读取的伪引用。

## 7. 统一模型输出限制

- `registry/output_limits.rs` 限制完整 canonical content_items 集合的序列化大小，覆盖 Text、Json、SearchResults、FileRead、BashOutput 及 MCP 成功/失败内容；旧 content 和 structured 通道也必须有界，超大的 structured 重复投影被移除。重建错误对象后再次检查实际 JSON 大小，并保留 error_kind/retryable 等分类，不能把权限拒绝降为可重试运行失败。旧程序调用的多投影 payload 不是模型消息；它仍可能包含多份各自有界的表示，不能声称其总序列化大小等于模型预算。
- 字节裁剪发生在有语义的字段/记录边界；Json 保持合法对象，超大字段不能直接切断 JSON 字符串。无法有意义裁剪的 JSON 改为明确的省略说明和有限元数据；本期不承诺为任意 MCP 数据自动落盘。
- Kernel `build_model_tool_result_text` 使用一份显式模型投影，包含必要状态、有限预览和引用；不得顺便复制完整 structured/envelope。产品结果与模型投影仍来自同一 typed 事实。Bash 非零退出、超时和取消显式提供简短错误，不以整份 response JSON 作为错误正文；捕获故障携带 retryable=false。最后一道裁剪继续保留 Bash stdout/stderr 尾部，并同步 truncated；若 Read 正文再被裁剪则清空不再可信的 byte cursor，Search 页缩小时标记 more_available。
- 最后按实际序列化后的单个 tool message 复核大小；仍超限则缩减可选字符串和数组，资源路径/状态/类型/错误码保持完整。不可进一步缩减的巨大不透明对象转换为显式 `{"truncated":true}`，禁止发送超限请求。Bash 通过执行前最低预算检查和状态预留，使正常的引用能够保留。
- 遵守现有 Context rules：单个注入项不超过 10K tokens。可用时采用匹配模型的计数器；没有 tokenizer 时使用明确记录的保守 UTF-8 字节上界，不能用 chars/4 声称硬保证。因而实际预览可能少于 32 KiB/60 行。
- 现有 working-context budget 继续负责批量与历史总预算。新增回归覆盖多个各自合规的工具结果同时返回；不把 per-tool 限制误认为整轮必定不超预算。当前原子 ToolResultBudgetState 只有累计计数，不冒充可用的额度预留机制。
- 超过 1K tokens 的新注入项属于现有架构定义的高风险上下文，准备限额证据并在合并/发布前走人工 review；这不要求设计阶段新增审批。

## 8. Grep 与大文件读取

### 8.1 Read

保留普通 `path`、1-based `offset`、`limit`。大于 2 MiB 的文件以及所有显式 byte-range 请求改用 sandbox filesystem capability 内的 streaming/range read，不再因正文总长超过 2 MiB 拒绝有限范围。小文件沿用已有的有界全量读取与 freshness hash，以保持 Edit/Write 的读后写守卫；unchanged 快捷返回补充范围检查。不得通过裸文件系统 I/O 绕过沙箱授权。

- 默认仍读取 200 行，最多 2,000 行，并受统一字节/token 上限；回传实际范围及可继续的位置。
- 大文件以 4 KiB buffer 跳过 offset 前内容，line 扫描上限 64 MiB。正文预算按剩余模型字节的 1/6 保守预留 JSON 转义开销，避免最终限制再次剪掉正文后 cursor 已经越过未展示字节。行内停止时给出 byte cursor，不能把下一行当作继续位置。
- 为超长单行和大日志提供 typed byte-range 模式：建议可选 `byte_offset`、`max_bytes`，与 line offset/limit 互斥；按原始 bytes 推进 cursor，UTF-8 decoder/边界处理保证显示文本有效。命名和互斥条件同步工具 schema。
- line 模式对单行截断给出明确标记及 byte 读取入口。byte 模式返回 `next_byte_offset`，不能用输出字符数推导文件偏移。
- 在未扫描 EOF 时，total_lines 必须为可空/未知，不能编造精确总行数或为得到总数扫描整个文件。更新 FileRead typed model、PB/SDK/UI。
- unchanged 判断必须考虑文件身份/版本和请求范围；第一版可对 streaming/log 路径直接关闭该快捷判断。不同 offset/limit 必须返回新范围，不能仅因路径内容 hash 相同就返回 unchanged。
- 第一版优先支持已完成日志；正在写入的日志只读取打开时确认的 byte 范围，不声称快照之后不存在更多输出。

### 8.2 Grep

复用现有 rg 与 SearchResults，不另加日志专用搜索工具。对日志路径同样支持 pattern/offset/limit。

- 流式解析 rg 结果，限制每个匹配行、before/after context、整页序列化大小；巨型 minified 行也不能绕过预算。只保存需要的页，不累积全部匹配正文。
- 原始 rg JSON frame 的读取/解析也必须有界，不能用无限 read_line 再截断 text。第一版若单条 frame 无法在有界解析器内处理，停止此次搜索并返回明确的 typed 限制错误，提示缩小搜索范围或用 Read byte-range 查看；不能静默丢事件或伪造精确总数。
- `files_with_matches` 和 `count` 的模型投影真正只含路径/计数，不继续带完整匹配正文。
- 保留现有精确 total_matches 语义时，可继续 drain 并计数、丢弃非本页内容；不要直接提前 kill 后仍返回精确总数。超时保留 partial，未来若提前停止需显式引入未知总数契约。
- 每条原始 rg frame 上限 256 KiB，匹配文本上限 500 bytes，context 最多 20 行，整页为有效模型上限减 1,024 bytes 后最多 8,192 bytes。页截断后的 offset 按实际返回记录推进。若连路径和最小单条记录也放不下，返回 UsageLimitExceeded，不能返回零条但 next offset 不变形成循环。
- 大搜索结果以缩小范围或下一页检索为主，本期不为 Grep 默认持久化整个搜索集；完整 Bash 日志已经是可搜索来源。

## 9. 变更位置与交付顺序

| Owner | 预计改动 |
| --- | --- |
| `talos-config` | 有效输出策略/路径、output_log 默认值与校验、配置模板及配置 API |
| `talos-tools` | Bash collector/writer/日志维护、实际消费 ToolResultPolicy、统一 typed limiter、Read/Grep 有界 I/O |
| `tools-sandbox` | 如现有 filesystem handle 不支持，补授权后的 streaming/range 能力，不扩大原授权 |
| `talos-agent-kernel` | 成功/失败共享模型投影、最终限额、progress 累计限额、批量上下文回归 |
| common types / agent-pb / agent-protocol / SDK / Web / TUI | Bash 引用、FileRead 游标/未知总行数的同源对象及 live/replay 消费 |
| `talos-guard-task` (common) | 周期任务定义、注册、监督、取消、安全 join 与通用日志 |
| Worker composition | 实现日志清理业务、发现与轮转、注册/配置唤醒/关闭；不扩张 summary store |

可部署切片按“统一入口保护 -> Bash 真实落盘及日志读取 -> Grep 生产端有界化”拆分。Bash 的引用发布必须与可读取能力一起交付；不能把只返回无法读取路径的版本标为闭环完成。执行阶段和状态由仓库根目录的任务专属 IMPLEMENTATION_PLAN.md 维护，不覆盖仓库根目录另一任务的计划。

## 10. 验证策略与验收标准

以下为可复用验证标准；实际执行结果只记录于任务计划和忽略的 output，不从本清单推断通过。

- 策略：小输出不建日志；恰好等于/超过字节与行数边界；JSON 转义、中文/emoji、长单行、空输出；persist=false；用户设置低于默认值时真正生效。
- Bash：多 MiB 混合 stdout/stderr 的 0/非零退出、timeout、cancel；文件内容与捕获顺序一致；末尾错误出现在预览；内存缓冲/队列长度保持固定上界；成功和失败模型消息都受限。
- 存储：独占路径、并发 call/进程、重启恢复、写/flush 故障、64 MiB 限额、workspace 配额预留与淘汰；活动文件不被 GC；TTL 后仍可返回可理解的缺失错误；clear/delete preflight 失败不提前丢日志。
- Read：超过 2 MiB 日志按 line/byte 续读；不同范围不误报 unchanged；单行超过预算仍可完整逐段定位；UTF-8 边界；截断/增长/删除时 cursor 语义明确。
- Grep：超长匹配/context、精确 count、路径模式不泄漏正文、partial、分页推进、没有非必要全量正文缓冲。
- 后台任务：不同周期、启动首轮、重复名称、配置唤醒、串行执行、长轮跳过过期 tick、panic 退避、共享/取消中的 shutdown。
- 全量清理：超过 4096 项且混合活动、未过期、未知及损坏条目，尾部过期日志同轮回收；多 Workspace 轮转、全局让出续扫、空目录裁剪不影响 TTL/计费遍历。
- 配额：部分校准不发布，revision ABA/并发 reserve/finalize/delete 延后校准；未知大小预留经校准后删除不误扣其他日志；删除与扣账崩溃间隙保守多计并恢复。
- 集成：从实际 Bash/Read/Grep 经 Registry、Kernel 到捕获到的 Provider 请求断言大小与结构；验证失败 envelope 不带回原始日志；多工具批次以及 compact/恢复后引用仍可定位；不调用真实付费模型做单元测试。
- 跨边界：更新类型生成物，Web/TUI live/replay 展示状态及引用；现有 06_tools 下补 Bash 大输出 -> Grep 定位 -> Read 续读的代表性 gated E2E。真实远程 Computer 的路径归属与持久卷重启验收单列，未运行不能称通过。

执行时采用项目命令：`just test -p talos-guard-task`、`just test -p talos-worker`、`just test -p talos-tools`、`just test -p talos-tools-sandbox`（若改动）、`just test -p talos-agent-kernel`、`just test -p talos-agent-provider-kernel`（若改动）、`just test -p talos-config`；对应 scoped check/clippy、`just fmt-check` 和 `pnpm --dir talos-app check:repo`。前端/schema 改动另跑受影响生成与类型检查，E2E 使用现有 runner 的精确 case selector，不跑无关全量集。

## 11. 已知限制与后续扩展

- 默认值为 7 天、64 MiB/文件、5 GiB/用户 Workspace、每小时后台维护；用户可通过同一 output_log 配置调整有限配额。
- 日志是有限保留的执行节点文件，不是永久审计档案。配额淘汰可能早于 TTL，结果必须说明这一点。
- 第一版无 Session 删除即刻擦除、跨 Computer 日志迁移、Sync 文件复制、浏览器下载。上述需求需要已有 Service/Worker 授权路径中的独立协议设计。
- 启动及每小时扫描全部已配置且已有日志的用户 Workspace；大轮次跨批次和调度预算继续，实际删除可能晚于 TTL。变化中的目录项可留待下一轮；不承诺文件系统一致快照或反复崩溃下的持久进度。
- 普通 line offset 从文件头扫描可能有 I/O 成本，长日志后续页优先使用 byte cursor。本期不引入持久化行索引。
- 不把上下文总量、磁盘错误、命令退出码和输出完整性混成一个 truncated 标志；每项必须能单独验证。
