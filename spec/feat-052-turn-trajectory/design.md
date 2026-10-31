# Turn 执行历史补充与 Tracing 查询视图

Status: active
Created: 2026-10-29
Revised: 2026-10-30
Implementation model: history-backed execution supplements

## 1. 本次重写的决策

**执行时补齐已有历史缺失的事实；Tracing 查询并展示这份历史。**

本规范替代此前独立录制通路、两张观察业务表、逐秒完整输出快照、独立 revision/录制状态和前端实时轮询的设计。实施阶段和实际验证记录由本地 ignored 的根目录 IMPLEMENTATION_PLAN.md 管理，不作为公开合同的一部分。

保留已经迭代形成的 Session、Turn、事件、用户归属、对话反馈、等待/继续和结算机制。Talos 不增加 Step、Span Tree、tracing 框架或服务。

### 1.1 不变量

1. 唯一用户身份沿用 `Session.user_id`；不增加另一套 owner 身份，不建设用户 ID 同步。授权、历史领取、附件和删除沿用原所有者。
2. 原 `AgentEvent` / `AgentEventPayload` / `TimelineEvent` JSON、sequence、event_seq、History DTO、消息和工具 DTO、Sync 事件格式不变。
3. 补充信息落在已有持久事件记录的**私有可空字段**，不是向严格公开事件 JSON 塞入新属性。旧 reducer 和旧客户端继续处理原事实。
4. 原对话继续边执行边反馈；Tracing 首版只展示 History 已结束的 Turn，包括原失败和取消结果。等待用户输入不算已结束。
5. 旧数据未保存的信息为 null；已知空内容可为空字符串或空集合。不按当前配置、最终历史、文本相似度或原汇总补造请求。
6. 可选执行补充失败不改变原业务结果、不取消工具、不参与恢复决策。内容读取失败与内容未保存分开处理。
7. 先接通新的历史补充读写，再移除旧实现；最终只有一个新写入口和一个查询事实源，不长期双写/双读。

## 2. 目标、范围和非目标

Tracing 在现有 Session 中提供按 Turn 连续展开的执行账本：用户输入、实际上下文、阶段反馈、公开思考、模型请求与响应、工具调用与结果，以及真实耗时和 Token。可查看完整已保存详情、原始工具参数，连续阅读和复制已加载内容。

首版覆盖 Kernel 主模型循环及最终回答，普通响应和流式响应均覆盖。复用公开 reasoning、`assistantText.phase`、executionSummary、工具 progress、原消息/block 状态。辅助模型调用、全量 Skill Gate/Memory 排序、跨 Subagent 因果树、全局看板、定价、逐 token 持久化、自动回放、外部导出和跨管理端详情 Sync 不在首版范围。

保证的是实际请求的可检查规范化语义，不保证逐字节 HTTP 报文、认证头、Provider 私有签名或模型内部状态。 Prompt 的 `SnapshotMessage.reasoning` 按顺序保留实际回传的公开 text/summary；缺失为 null，旧快照缺字段按 null 读取，不保存签名和加密块。该字段参与现有完整值比较，正文详情与 Raw/复制共用同一快照。附件保留当时的文件引用和描述，不复制二进制，也不读取当前文件冒充历史内容。

## 3. 既有基础及 DSH 参考

| 现有所有者 | 本次职责 |
| --- | --- |
| [事件和 Timeline](../../talos-rs/crates/orchestration/agent-api/src/event.rs) | 原执行事实及顺序保持不变；私有补充字段不进入公开事件 |
| [History](../../talos-rs/crates/orchestration/agent-api/src/history.rs) | 终态 Turn、原消息、结果和指标的权威；不新增另一份业务 Turn |
| [Kernel 请求](../../talos-rs/crates/orchestration/agent-kernel/src/general_turn/step_request.rs) / [工具处理](../../talos-rs/crates/orchestration/agent-kernel/src/general_turn/tool_batch.rs) | 在实际请求、尝试和工具结果准备边界提供缺失事实 |
| [Provider](../../talos-rs/crates/common/llm/provider/src/protocol_provider.rs) | 保留最终绑定及语义转换后的请求事实；复用原编码器 |
| [结算写入所有者](../../talos-rs/crates/orchestration/service/src/turn/agent_execution/timeline_writer.rs) / [Store](../../talos-rs/crates/orchestration/store/src/session/agent_execution/settlement.rs) | 原 waiting/terminal 生命周期；组织可选历史补充写入，不增加第二个结算 owner |
| [Upgrade](../../talos-rs/crates/app/talos-upgrade/src/database_migration.rs) / [SQL migration](../../talos-rs/crates/orchestration/store/src/migration.rs) | 仅加法可空字段迁移；不重写原事件 |
| Service / HTTP / App RPC / SDK | 同一类型化查询业务逻辑，独立 Tracing API；transport 不另算指标 |
| 前端 `features/tracing` | 只读 History 与详情查询；复用现有主题、组件、i18n、文件预览 |

DSH 的关键是 Session event sourcing：模型可见内容进入原 Session 日志，轨迹是日志投影。其 `request/header` 在请求前保留配置/工具；模型尝试保留计时流、输出和用量；`tool/call` 的原始参数通过 callId 关联 `tool/result`。参考实现：[Session](https://github.com/deepseek-ai/deepseek-harness/blob/f3803c96974ad4a94195fb97e5c637e9c2a3afe3/packages/core/session/src/types.ts)、[请求构建](https://github.com/deepseek-ai/deepseek-harness/blob/f3803c96974ad4a94195fb97e5c637e9c2a3afe3/packages/core/agent-loop/src/agent.ts)、[Trajectory](https://github.com/deepseek-ai/deepseek-harness/blob/f3803c96974ad4a94195fb97e5c637e9c2a3afe3/packages/client/ui-trajectory/README.md)。借鉴其事实归属和投影，不复制 Cordis 或 Step。内部工具对象的长期归档也不是 DSH 的统一保证。

## 4. 数据归属：复用、引用、补充

| 数据 | 保存/读取方式 |
| --- | --- |
| Session/Turn/user 身份、原业务状态、最终结果 | 只读原 Session/History；补充字段不复制 |
| 用户消息、阶段总结、公开思考、最终回答、工具进度 | 只读原消息/block/事件 |
| 原工具名、结构化参数、产品工具结果 | 只读原工具 block；缺少的模型原始参数文本单独补充 |
| 原 Turn 总耗时、modelRounds、工具成功失败数和汇总 Token | 只读原 metrics；不由观测结果覆盖，不重复累加 |
| 每次模型调用的模型/连接、最终绑定参数、工具目录 | 执行侧补充；同 Turn 中完全相同的配置/工具目录保存一次并引用 |
| 实际模型输入 | 保存真实最终规范化快照；按完整类型相等复用已有正文，不推测原历史映射 |
| 每次尝试实际输出、finishReason、ApiError、Token、计时 | 可准确关联原消息的部分复用原历史；失败/重试或语义不同部分保留实际快照 |
| 工具调用的产生请求/尝试、原始参数文本 | 补充稳定关联；不从解析后的 JSON 重新 stringify 冒充原文 |
| 模型可见工具结果 | 与原工具结果语义相同则引用；summary/wrapper/裁剪等真实差异保存快照 |
| 附件 | 复用原 fileId/授权；补充当时 kind/name/MIME/size/可用性，缺失留空 |

### 4.1 首版去重边界

直接复用原 History 的消息、反馈、工具执行结果和指标；模型请求的最终语义输入使用真实快照。当前组装/Provider 转换链没有保留可证明的原 canonical message/block 身份和派发时的来源 sequence 上界，因此首版不伪造 `historyRef` 或构造无调用方的引用模型。

在同一 Turn 内，按**完整类型值相等**复用 Tools、Parameters 及真正相等的整份 Prompt、Response、preparedResult；比较包括角色、顺序、来源、附件和缓存提示，不只比较文本。消费请求的工具结果通过 modelRequestId/messageIndex 指向该次已保存 Prompt，不再保存第三份正文。旧逐秒 Response 前缀不进入新存储。

不同请求的 Prompt 可能包含相同历史文本；不同的 Prompt 仍各自保存，不引入消息片段池、差分压缩或通用内容寻址平台。首版的去重不等于所有输入零重复。保留当时真实输入比用最终 Session 反推更重要。

若未来真实组装所有者提供稳定来源 ID，才可增加同一授权 Session 内的精确历史引用：来源 Turn 的 sequence 上界受真实派发时已提交前缀限制，不能拿不同 Turn 的局部 sequence 直接比较。那时也必须在最终规范化后证明完整语义一致；任何不能确认的部分继续使用真实快照。本轮不改变原事件/恢复模型来勉强建立映射。

### 4.2 来源与工具关联

上下文来源由原组装/注入所有者随消息在进程内携带，最终裁剪和 Provider 转换后核验；恢复后不确定就留空。保留现有 kind/source/admission 语义，不能把合成 user-role 上下文展示为真人用户。

工具使用原 callId 关联原工具 block，再关联产生它的 modelRequestId/callRecordId。工具的模型准备版本与后续请求真正消费版本分开：消费位置指向该次请求的有序输入，不再单存第三份消费正文。未知关联留空，不由相邻行推断。未调用工具的定义仍属于当时工具目录。

## 5. 持久化：已有事件的可空执行补充

### 5.1 唯一锚点与私有字段

在已有 `agent_events` 增加 `execution_capture_json TEXT NULL`，仅由执行历史 Store 访问。锚点为原 Turn 的 `TurnStarted` 持久事件（原执行 sequence=1）；不插入新事件、不占新 sequence、不修改 `payload_json`。等待/继续沿用同一 Turn 锚点；不使用最终会被删除的 `agent_active_turns` 承载历史。

该列解码为一个**类型化、有界的 Turn 执行补充对象**：请求/尝试、实际快照、共享工具目录/参数、工具关联和执行时钟段。不复制 user_id、业务 Turn 状态、全量消息或原 metrics，不保存录制状态、实时 revision、每秒响应版本和重复 owner 配额账本。

对象中的 modelRequestId/callRecordId 与既有 callId 仅为关联身份，不形成新的 Step 或生命周期。正文引用稳定于已保存对象；API payloadId 是该对象内内容的读取定位，不需要独立 payload 表或跨版本 UUID 日志。完整类型在 owning crate 定义，公开查询投影与存储封装分离。

### 5.2 写入、可见性与成本

- 派发前冻结真实最终输入、工具定义和允许公开的最终参数；不能到 Turn 结束再根据当前配置重建。
- 流式响应在有界内存中拼装一次，保留首末输出端点；每次尝试结束/失败/中断形成一份已观察输出，不再逐秒复制整个前缀。
- 请求结束在 invocation 的有界内存内冻结一次事实；复用原执行传输，在当前 invocation 返回 waiting/end/普通失败之前一次提交补充，不等待整场 Session 结束。执行中不额外开启后台录制任务；进程强制退出前尚未提交的部分允许缺失。
- 暂停前持久保存前段补充，恢复只追加新的执行事实。不同 invocation 的 ID 不冲突，已保存部分不覆盖；人工等待不计入模型生成耗时。
- 正文使用已有认证连接的有界资源传输；invocation 等这次可选调用成功或最终放弃后返回原结果，无需给原 settlement envelope 追加引用或正文。原始大正文不能直接加入 mandatory settlement 帧或主事件队列，也不能成为原结算成功条件。替换旧 Probe/Upload 业务，复用连接/授权/分块机制，不新建独立录制服务。
- 原结算 owner 在不改变主事务的前提下，先确认此次可选补充交接已成功或已最终放弃，再提交并发布终态历史；主事务失败仍按原恢复路径处理，补充本身不构成业务成功条件。补充写入单独提交、有统一上限且不重试；失败记录内部诊断，缺失为 null。终态发布后不继续后台补写，避免“历史已可见、快照稍后才到”导致前端必须轮询。Store 的运行时补充写入必须在同一写事务检查原 Turn 尚无终态；终态提交后拒绝补写。调用方 RPC 超时不等于远端未提交，不能仅靠本地停止等待承诺封口。这个原终态谓词保证迟到写入不会改变已发布详情，不新增录制封口状态机。
- 冷恢复没有新的补充数据就保留旧片段；崩溃前未提交信息允许缺失。不存在第二套 tracing 恢复/协调框架。

一列并非零成本：增量保存会重写有界对象，复用现有每 Turn 32 MiB、每个逻辑正文 4 MiB 上限；单次 invocation 的完整补充对象最多 32 MiB，通过每帧最多 64 KiB 的资源流交接，进程内缓冲也必须按字节限制；不新增自动调参。实现不得先无限构造正文再检查大小。读取 History、回放和 Sync 的 `SELECT *` / `RETURNING *` 必须收窄为原字段，避免普通操作读取大正文。页面按需读详情；删除 Session 时原事件行与其字段一起删除，不能再依赖下一轮的孤儿清理探测。需要验证大对象读写与删除的实际成本，不能只用表数证明轻量。

## 6. 升级和旧数据

### 6.1 普通历史升级

通过现有 SQLx migration/Upgrade 启动顺序增加可空列。旧行自然为 NULL；原 Session、消息、事件内容、时间、序号和恢复输入不回填不重放。Upgrade SQL 版本账本不复制到另一套任务系统。

已上线/main 的迁移保持原样，不修改版本、脚本或 checksum。本 PR 只新增 `agent_events.execution_capture_json TEXT`，不新增 tracing 侧表，不回填旧历史。新安装和旧版本升级都走已有 SQLx/Upgrade 顺序，重复启动幂等。

### 6.2 未发布中间实现的清理

本 PR 未合并的旧侧表迁移、录制链路、专用导入 Upgrade 任务、临时字段兼容别名及对应镜像测试全部移除，不作为产品兼容目标。开发验收库按测试资料归档，不能为了复用它们而给产品增加转换、双读或忽略 SQLx 校验的逻辑。

兼容性只针对已上线 Session/Turn/Event 和公开业务协议。既有用户绑定、附件、消息、事件、序号和恢复路径保持原样；缺失 Prompt、工具补充信息、模型实际输入输出、计时或 Token 都留空，不推断也不补造。

## 7. Tracing API：独立查询，同一事实源

保留本 PR 的方法及路由位置，不把正文塞入 `session/history` 或原事件通知：

| 业务 | JSON-RPC | HTTP |
| --- | --- | --- |
| 能力 | `trajectory/capabilities` | GET `/v1/trajectory/capabilities` |
| Turn 账本 | `trajectory/readTurn` | POST `/v1/trajectory/turn/read` |
| 请求详情 | `trajectory/readRequest` | POST `/v1/trajectory/request/read` |
| 工具详情 | `trajectory/readTool` | POST `/v1/trajectory/tool/read` |
| 正文按需读取 | `trajectory/readPayload` | POST `/v1/trajectory/payload/read` |

Service 校验原 Session 权限、Turn 归属和原终态；Store 读取历史补充，原消息/工具事实由既有 History 投影提供。HTTP/RPC/SDK 共用同一个类型化结果，不各自合并或计算一遍。旧历史从未保存该事实时返回字段 null；已保存事实的内容损坏或引用解析失败须返回可见统一错误，不能以 null 掩盖。不存在的关联返回原统一错误，不能跨 Turn 找一个相似记录顶替。

### 7.1 新查询合同的裁剪边界

以下为当前查询合同。沿用现有 request/model/tool/timing/snapshot 业务字段，生成所有客户端与 schema，禁止手写第二份类型：

| 对象 | 保留/简化 |
| --- | --- |
| Turn 查询 | identity、clockSegments、data、nextCursor；无补充时 data=null。身份只含原 Session/Turn，业务状态来自原 History |
| 模型请求/调用记录 | modelRequestId、ordinal、purpose、connectionId、modelId、callRecords；每条调用记录保留真实结局、timing/usage/finishReason/error，不从业务 Turn 成败推断调用结果 |
| 请求详情 | prompt/tools/parameters 均为 PayloadRef 或 null；outputs 为已知调用记录的数组，每项为 callRecordId 与 response（PayloadRef 或 null）；缺少 Prompt 不隐藏其他内容 |
| 工具详情 | callId、sourceModelRequestId/sourceCallRecordId、真实 intervals、preparedResult（PayloadRef 或 null）、usages；原工具名/参数/执行结果来自原历史 |
| 正文引用 | PayloadRef={payloadId: string, complete: boolean 或 null}；引用本身为 null 才表示没有正文，禁止再用 payloadId=null 表达第二套缺失。空文本/空目录仍有有效引用；complete=true 为已确认完整，false 为实际片段，null 为未知；不保留 CaptureRef.state |
| 分页 | 保留 cursor/limit、data/nextCursor；游标绑定原 Turn 终态身份与固定内容位置，无实时 revision 失效重试 |
| 能力 | 描述当前管理端是否支持该查询；不是执行前录制探测，也不为每 Turn 增设 capability 生命周期 |

完整性是已保存内容的事实，不是录制生命周期。不能根据 error=null、finishReason=null、Turn 成功/失败推断 complete；Provider 已完整返回后业务仍可能失败。因取消、错误或上限而保存的实际片段必须标为不完整，不能展示为完整快照。

本 PR 新增 Tracing 合同统一使用最终字段与生成类型；不保留已移除中间实现的状态、别名或转换分支。原 Session/Turn/events 协议保持兼容。

详情分页依然有界，按不可变的已结束 Turn 补充读取；不为每条结果反复解码整场 Session。已有业务失败与真实部分输出可检查，查询网络错误仍使用现有错误契约，与“历史没保存字段”分开。

## 8. 计时和 Token 口径

- Turn 条上的整体耗时、状态、模型轮次和总 Token 直接取原 History metrics。缺失显示 `—`，不累加 LLM+Tools 冒充墙钟。
- 每次模型调用从真实派发到结束使用执行侧单调时钟；首输出取第一个有效文本/reasoning/tool-call 增量，TTFT 为派发到首输出，生成跨度为首输出到末输出。不把网络 chunk 或心跳当输出。
- 失败/重试各自保留端点和 Provider 实际用量；未返回 usage 为 null，不能分摊原 Turn Token。usage 整体可空；存在时沿用现有 TokenUsage 的 input/output/total/cache 字段及未知值约定，不改变这些字段的 null 性、不新增现 DTO 没有的 reasoning Token，也不在 UI 用输入+输出重算 Provider 总量。
- 工具真实执行、等待和已知排队分开。执行与并行工作量可重叠，不相加为整轮墙钟。
- 时钟段按原执行节点/恢复 invocation 区分；只能在同段比较单调端点。不同段只按保存的墙钟起点近似对齐，不能视为精确校时；人工等待与跨节点误差不混入模型耗时。
- 页面会话级数字只描述明确已加载范围；N 个模型请求不叫 N 个 Turn。所有已加载 Turn 均有原历史条；Session 概览包含全部已保存时序，详情和原指标明确标记所属 Turn。

## 9. 前端结构与交互

保留现有 Session 页的“对话 / Tracing”入口；对话的实时逻辑不动。Tracing 的轮次仅来自 `SessionHistoryResponse.data`，不追加 active/live 占位 Turn，不因缺少补充隐藏旧 Turn。

布局沿用既有设计：顶部范围/加载范围说明；选中 Turn 原指标；整个 Session 的紧凑执行时间线；左侧连续 Turn 账本；右侧选中事实的详情。每个 Turn 条均展示原状态及整体耗时。反馈和公开思考在原顺序连续可读，不重复显示成另一套记录。

保留请求详情的概览、Prompt、Tools、Response、Raw、Timing，以及工具执行结果/原始参数/模型消费结果。内容尚未加载、历史缺失和网络读取错误分别表达；正常数据不打“已录制”标签。原始参数保留空白和换行；复制范围写清，包含实际已加载的跨页关联请求详情，不以当前元数据页为全部已加载内容。

保留 `features/tracing` 的查询 hooks、纯模型选择器和按职责拆分的组件，使用项目主题 token、共享 Tabs/按钮/图标与中英 i18n。保留浅深主题的字体层级、tab 选中/hover/focus-visible、行对齐和窄屏布局；删除仅服务录制状态的组件和翻译，避免重做整套 UI。

查询只在页签可见及相应 Turn/详情需要时运行；不轮询执行过程，也不维护 active 轨迹或可变正文版本缓存。新终态通过原 History 失效刷新进入列表，不再维护第二个通知流或固定结束后追尾窗口。概览读取全部历史 Turn 的分页时序元数据并与账本共用缓存；完整 Prompt、工具参数、结果等正文只在详情需要时读取。

## 10. 最终职责与复用边界

- 原 Session/Turn、用户绑定、业务状态、反馈、工具执行结果与结算由已有历史负责；Tracing 不建立第二套生命周期。
- 执行边界只补充真实模型输入/输出、原始参数、计时与关联；等待/恢复使用同一原 Turn 累积补充，终态后只读。
- 正文结构只由 Rust `TrajectoryPayload` 定义。历史 Store 在读取正文、分页前使用该类型验证保存的数据；损坏返回统一错误，缺失返回原空值语义。验证不重新序列化正文，原始参数文本不变。
- SDK 组装有界分页并解析 JSON，保留身份、编码、字节限额、游标进度、取消与传输错误检查；不手写第二套完整字段/枚举校验，不新增 schema 解释器或校验框架。
- TypeScript 类型和 JSON Schema 由已有生成命令从 Rust 导出；HTTP/RPC/SDK 复用同一业务接口，不分别定义数据结构。
- 内部执行消息与公开历史快照保持边界，二进制、签名和加密续写数据不进入公开快照。前端只呈现真实已保存数据，来源未知不猜测。

## 11. 验证策略与风险

验证原历史升级后 Session/Turn/messages/events/sequence 完全一致，新增字段 NULL，重复启动幂等；确认新库和升级库均不创建 tracing 侧表。公开事件旧 fixture 应原样可读，旧 DTO 不出现私有新列。

通过可控模型执行覆盖：多 Turn、多次模型请求、真实 Prompt 转换、原始参数、工具结果后处理、失败重试、等待/恢复与取消。比较最终实际请求和历史重建结果，不写只比较自身序列化的镜像测试。单独验证补充写失败不影响原结算、终态可见时补充已定案、删除后不能附加复活。

HTTP/RPC/embedded 使用相同 fixture 检查同义结果及权限；前端用后端真实历史检查全部轮次、逐 Turn 耗时、详情和跨页复制，并在浅深主题及窄屏确认交互。使用最终构建生成的真实多轮历史验证新采集，已有历史单独用于旧数据兼容验证。具体运行结果仅进根目录 ignored output 和 plan。

需在实施中证明的边界：真实请求快照与最终 Provider 输入一致；私有大字段的事件查询/删除成本；远端有界交接与暂停数据保全；已上线旧库升级与空字段行为。这些是实施验收项，不能用“设计简单”代替验证。

Tracing 命名统一：模型请求使用 ModelRequestTrace/modelRequestId，实际调用记录使用 ModelCallRecord/callRecordId/callRecords。这些新 Tracing 对象在本 PR 内统一更新 Rust、HTTP/RPC、生成 SDK 和 UI，不保留未发布中间名称别名；原 Session/Turn 业务请求字段不改。


概览沿用 Input / LLM / Tools 三泳道与时间轴，在一张固定高度图中按时序展示整个 Session 的全部历史 Turn。元数据通过现有 readTurn 接口读取所有页并共用查询缓存，Prompt、模型输出和工具正文仍按需读取；每页读取前检查取消信号。保留原概览在上、账本和详情在下的布局，不增加总耗时条形图、逐 Turn 导航标签或第二张单轮下钻图。区间标识所属 Turn，直接查看具体模型或工具详情；账本原 Turn 展开和选择保持原行为。

Session 概览默认按原 Turn 顺序连接执行段，明确标识“轮间空闲已折叠”，只去除 Turn 之间的显示空闲；各 Turn 内的恢复段、并行和等待间隔保留。取消勾选“折叠轮间空闲”后，按各 ClockSegment.startedAt 的已保存墙钟起点近似对齐，显示轮次间隔。这是保存到秒的时间戳分辨率，不代表不同执行节点已经校时，不能保证跨节点一秒准确度。区间长度和详情耗时仍取执行侧真实 interval/elapsedMs，不拿绝对时间差重算。LLM 图条按“首输出前 / 首输出后”分色，后者包含首输出至调用结束；详情的生成跨度单独使用首输出至末输出，不能与图条后段混用。缺时序的旧 Turn 不推算虚拟区间，原事实仍在账本；恢复段使用各自已保存起点。概览始终三条泳道，不随 Turn 数量增加纵向布局。调用密集时使用原生横向滚动和固定泳道标签，宽度不再强压在屏幕内；图条不设百分比最小宽度，真实区间比例不变，短事件只保留像素级点击目标。仅挂载可视范围及一屏预留范围的图条，并有界保留最后交互 Turn 以恢复关闭详情后的焦点。折叠 Turn 仅挂载摘要，展开后使用原 History 消息，不依赖补充数据成功。历史与时序查询共用稳定派生结果，详情切换不重算全 Session 正文；不引入额外框架或持久化缓存。

RPC integration: `trajectory/capabilities` accepts no parameters. Explicit JSON
`null` becomes an absent optional value when the JSON-RPC envelope is decoded;
the existing no-parameter dispatch branch must retain null instead of defaulting
to an empty object. Verify the envelope-to-typed-request boundary, not only a
mock SDK invocation.
