# Agent 全链路消融收口计划

## 最终 ACK 等待层删除与单提交交付（2026-09-06）

### C1：删除重复执行等待
**目标**：删除 useSessionMessageStream 及其 Store 终态等待、AbortController 与伪取消；start/retry 直接调用既有 API，在 ACK 后完成请求。
**成功标准**：保留 ACK 错误、输入接管与真实停止；retry pending 不代表 cancelling，不再因等待整轮阻塞附件草稿清理。
**测试**：既有 streaming/composer 场景覆盖 ACK 先于终态、附件 retry 顺序、真实停止请求和快速下一 Turn。
**状态**：已完成。重复 hook 和只验证等待的测试删除，附件 retry 场景迁入既有 streaming 测试；前端 22/22 场景、lint/typecheck 通过，旧 barrel export 已随删除清理。

### C2：验证并压缩提交
**目标**：汇总当前重构收尾代码为 PR 唯一 commit，安全推送，不合并 PR。
**成功标准**：保留本地可恢复历史；仓库 hook 通过；dev YAML 的个人设置与配置锁留在本地，不混入提交。
**测试**：受影响前端场景，lint/typecheck，仓库 pre-commit/pre-push；不重复完整 E2E。
**状态**：已完成。全部收尾代码已压为相对 main 的一个 commit 并通过 force-with-lease 推送。pre-commit/pre-push 通过：Web 348/348、SDK 57 通过/1 个既有跳过、Desktop 24 通过/1 个既有跳过；前端 lint/typecheck/build/repo checks、Rust fmt-check/workspace check 通过。最后仅回填本验收记录，不重复代码验证。恢复分支 codex/pr15-before-final-ack-fix-20260906 保留之前 PR 提交；未合并 PR。dev YAML 设置及锁文件保留在本地，未提交。远端 CI 结果不以本地通过替代。

## 前后端删除实验（2026-09-06）

### A1：确认可删除项
**目标**：核对相邻实现和实际调用方，区分纯包装与必要边界。
**成功标准**：删除项不改变协议、持久化 schema 或用户场景；保留准入取消保护、队列 commit 屏障和清理确认。
**测试**：只读调用链。
**状态**：已完成。队列通知重复刷新会话查询；队列已持有独立准入任务但提交内部再次 spawn；checkpoint 纯转发；Subagent 将唯一末事件包装成数组再查找终态。

### A2：删除重复路径
**目标**：队列只管理自身 revision，执行状态仍由 Session 事件推进；队列准入复用 owned 实现；删除无语义转发和单元素数组转换。
**成功标准**：无新增抽象，无删去必要安全保障，无对外 API 变化。
**测试**：实现完成后定点运行前端 Session/Queue 场景、后端 Queue/恢复/Subagent 场景；fmt、lint/typecheck。
**状态**：已完成。前端 22/22、lint/typecheck 通过；后端恢复/Subagent/Queue 41/41，加队列派发与准入 4/4，共 45 个定点场景通过；just fmt、服务端构建通过。删除两例仅断言旧重复查询的测试，保留队列 revision、执行终态、恢复及真实行为覆盖；无新增测试框架。

### A3：行为验收
**目标**：真实页面确认队列自动派发与刷新行为，记录消融前后差异，不重跑完整 E2E。
**成功标准**：保留原有用户路径，删除仅证明内部重复刷新的机械测试，不削弱业务断言。
**测试**：定点页面及 diff 检查。
**状态**：已完成。重建后由 just dev-server + dev YAML 启动；真实 Web 输入 ABLATION_FIRST，运行中排队 ABLATION_SECOND，自动派发完成，刷新后两轮问题与回答完整、页面可输入。截图 talos-e2e/reports/manual_pr15_20260905/22_ablation_queue_completed.png。本轮未重复完整 E2E，未提交或推送。

消融结论：删除的是重复会话刷新 owner、嵌套任务和无业务意义的包装；保留 Provider SPI、Harness 生命周期、Service 持久化编排、Store 事务及队列 revision 协调。本次无协议/schema 变化。开发服务启动另有既有 npx Playwright MCP 缓存 ENOTEMPTY 错误，未修改全局 npm 缓存；不将本次 Bash/队列验收扩称为 MCP 可用性验收。

## 最终独立自审（2026-09-06）

范围：相对 main 的完整 PR；只处理有调用链/真实场景证据的冗余或缺陷，不进行命名润色、未来扩展或机械拆分。此前验收结果作为基线，不重复完整 E2E。

### F1：独立审查与最小改动面
**目标**：独立 subagent 核对 Provider/Harness/Service/Store；主 agent 核对 Gateway/SDK/Web 交接。
**成功标准**：每条问题对应真实 owner、调用场景及可删除/收敛方案；必要边界不强删。
**测试**：只读代码及既有测试，不运行测试。
**状态**：已完成。独立审查确认下列三项后端问题；主 agent 确认前端重复订阅/状态写入。保留必要分层，不追加低价值命名工作。

已确认前端问题：同 Session 的 Timeline 与 Queue 各自订阅 SSE/重连；send/retry 成功回调重复改执行状态，可覆盖已经自动派发的下一 Turn；cursor 过期/lagged 的重复恢复分支在 idle 时停止 Session 订阅。

独立后端审查：
- P1：claimed continuation 已提交 progress 后仍使用旧 Waiting checkpoint 和 admission，可能重放工具副作用并导致 sequence 冲突；必须在执行前拒绝旧材料重放，并复用唯一失败收尾。
- P1：start/compact/resume 在请求取消窗口遗留 Admitting；必须让完整产品流程与完成策略一起脱离请求 future 生命周期，不能只删除 map。
- P2：完整 Turn 读取被最早 10,000 条截断；用既有分页 API 固定 durable tail、增量归约，统一 checkpoint/history/用户响应/interrupt 读取。

### F2：消除重复 owner 并闭合场景
**目标**：保留既有事件与队列 revision 协调，合并 Session 订阅；终态仅由规范事件推进。汇总确认后的后端问题一并修复。
**成功标准**：无新增 manager、状态机、协议 DTO 或兼容路径；队列派发、重连与下一轮输入不被旧请求完成覆盖。
**测试**：实现集中完成后运行受影响场景、lint/typecheck；涉及后端时用 scoped just test/fmt。
**状态**：已完成。前端 33 个相关场景通过，后续收敛定点 16/16、订阅最终 9/9 通过，lint/typecheck 通过；后端 Service/Store 285/285、最终恢复/取消/Subagent 场景 32/32 通过。just fmt、diff-check 通过；scoped Clippy 无新增 warning，严格模式被既有 Project Store 的两处 &PathBuf warning 阻断，未扩展范围修改。

### F3：最终验收与交付
**目标**：定点页面/集成验证，更新唯一 Spec 和本记录；本轮改动留待 Review，不直接提交或合并，后续交付仍保留 PR 单提交。
**成功标准**：独立审查问题逐条有关闭证据，实际变更对应验证，无新增已知目标内缺口；不直接合并 PR。
**测试**：不重复完整 E2E；必要的定点场景和 diff 检查。未来提交时另执行仓库提交检查。
**状态**：已完成。定点真实 E2E 06_tools/http_turn_cancellation 通过，报告 e2e_report_20260906_084000.md；新后端已通过 just dev-server + dev YAML 启动，health 确认 auth_enabled=false。页面队列/刷新/Steer 验收与持久化对照通过，Spec 已同步。本次改动未提交或推送，不把之前单提交 PR 当成本轮已推送证据。

页面验收环境已恢复。新验收 Session 为 03114b23-a79b-419d-8457-4f491f4e7e9d；工具运行中刷新后当前输入和待发送消息均保留，第一轮完成后第二轮自动派发，标题生成、最终可输入状态正常。截图 19_final_queue_completed.png；18 记录第一轮结束交接时的队列状态，不作为运行中截图。原用户 Session 未修改。

同 Turn Steer：第三轮工具运行中入队后点击 Steer，更新输入即时显示；刷新后原输入与更新输入保留，最终回答 FINAL_STEER_UPDATED。截图 20_final_steer_live.png、21_final_steer_completed.png。Store 对照共三个 Turn，每轮一个 turnStarted/turnCompleted，active 数量为 0，无额外派发一轮。截图均位于 talos-e2e/reports/manual_pr15_20260905/。

收口结论：保留 Provider/Session、Harness、Service、Store 的必要边界；删除前端第二条 Session 订阅及重复终态写入，统一完整 Turn 分页读取。未新增协议对象、兼容路径、manager 或状态机。当前 Kernel 对已推进但没有安全 continuation 的恢复显式失败，不能承诺任意执行位置恢复或外部工具副作用 exactly-once。

## 合并前验收（2026-09-05）

### 阶段 A：完整 E2E 基线
**目标**：默认 E2E 全部执行一次，补齐默认入口未包含的 interaction、sandbox、installation 组；保留每个结果。
**成功标准**：所有场景都有通过、失败或环境阻塞证据，不能把未运行写成通过。
**测试**：just e2e；遗漏组各执行一次；失败后只定点复测。
**状态**：已完成。默认 31/31，补充 interaction 3/3、sandbox 11/11、installation 2/2，共 47/47。

### 阶段 B：回归修复与定点验证
**目标**：修复实际故障，优先恢复 Service 所有的 Inner Model Session 标题生成；不改用例迁就错误实现。
**成功标准**：故障原因为代码证据支撑，失败场景定点通过；无新增兼容层或重复概念。
**测试**：失败 E2E、必要场景回归、受影响代码的格式与 lint/typecheck。
**状态**：已完成。标题、输入投影、刷新恢复、滚动、模型菜单和吞吐口径已修复；回归与定点 E2E 通过。

### 阶段 C：真实前后端交互验收
**目标**：just dev-server 显式使用 dev YAML，just dev-web；独立 Talos Home，真实模型和无登录鉴权。
**成功标准**：对话流、标题、历史刷新/重连、Steer/队列/停止、等待输入、模型切换、compact/clear、文件与设置等本 PR 触达场景有实际操作证据及截图；不把 API 测试冒充页面手测。
**测试**：隔离 Chrome 真实操作；结果逐项记录。
**状态**：已完成。本 PR 核心会话交互已真实操作；外围页面仅按下方记录标注冒烟范围，不冒称所有页面功能穷尽验收。

### 阶段 D：统一 Spec 与单提交交付
**目标**：040 与 044 去重合并为唯一设计文档，更新引用；整个 PR 最终压缩成一个提交。
**成功标准**：设计与最终实现一致，验证记录准确，保留本地可恢复历史，不直接合并 PR。
**测试**：文档引用、git diff --check、提交前范围及生成产物检查；必要验证未通过不得宣称可合并。
**状态**：已完成。Spec 已合并且独立读者核对完成；本地和推送 hook 通过，PR 已压成一个提交并安全推送。完整历史保存在 codex/pr15-before-squash-20260906；没有合并 PR。远端 CI 独立显示运行结果，不用本地结果代替。

### 收尾阻断项：用户实测回归（其他事项完成后排查，最终提交前必须解决）

- [x] 工具运行中整页刷新后永久 Running：保留 Session `4bbca50e-6ee5-4401-8565-55c2d2ad081c`（http://127.0.0.1:5175/sessions/4bbca50e-6ee5-4401-8565-55c2d2ad081c），不删除或重建证据。对比 durable active、终态事件、恢复响应和浏览器订阅；修复后复现工具运行中刷新及自然完成。
- [x] 用户问题 2/4：发送后当前问题在执行过程中消失或延迟到终态才出现。用户在本次手测的 `FIX_ORIGINAL` 场景中再次确认；Steer 生效不能代表消息展示验收通过。对比提交内容、admission message ID、canonical history、实时投影及 history replacement，区分展示遗漏与实际数据丢失；确认发送后立即显示，运行中、Steer 后、终态及刷新后均不丢失、不闪烁、不重复。
- [x] 自动滚动停在旧问题处：确认发送时跟随新消息、用户在底部时跟随最新执行、主动上滚阅读时不抢滚动；以用户截图和真实页面验收。
- [x] 运行中刷新出现 `Context compacting... Please wait.` 阻塞整页，必须再次强刷才进入：核对请求错误和真实 maintenance/recovery 状态，不能将恢复互斥误报为压缩，也不能靠刷新重试掩盖。
- [x] 用户问题 5：核查截图中 `2610.79 tokens/s` 的指标定义与计算链路。追踪服务端统计、SDK 字段和 UI 计算；确认是否误计输入/缓存 tokens、时间单位是否一致、是否混入首 token 等待/工具耗时，以及多轮是否正确加权。排查时查阅主流推理性能指标的一手定义，明确输出生成速率、端到端吞吐与 TTFT 的区别，再选择符合当前产品展示的口径；不凭数值大小认定错误，不新增无消费方指标。使用可复算的实际采样及必要场景测试验证，最后与其他用户回归统一处理。

用户问题 4/5 截图证据：`/var/folders/zf/gm0hxhrj0wnfclwcb3vkjkd40000gn/T/codex-clipboard-a0e105fd-aaa8-4716-a32d-bcc93524b0b0.png`。上述问题已完成定位及修复，逐项证据见下方合并前验收记录。

提交检查和安全压缩已完成；本地验收通过不等于 PR 已合并。

## 合并前验收记录

### 实际修复与删除

- 新建 Session 的路由 initial prompt 在发送前消费，刷新不再重复提交；ActiveTurnRunning 不再误报为 ContextCompacting。
- 保留 Session cursor、跨 Turn 去重；pending user input 只由匹配 message ID 且已有内容的 canonical message 接管。删除首次 recovery query 与 SSE 同时写投影的重复路径。
- Session SSE 不因旧 Turn 终态停止；终态统一刷新 metadata、列表、active 与 History，删除前端标题轮询和多套 cache 合并逻辑。
- Service 恢复 Inner Model 标题生成，Store 原子条件防止覆盖手工标题；标题用量仍走既有 Profile owner。
- Kernel Provider 在 Steer 边界结束旧 AgentMessage，后续回答位于 canonical Steer input 之后，不额外制造 User message。
- 自动滚动收敛为跟随底部/主动阅读；新 Turn 清除旧指标。模型菜单悬停后点击不再反向关闭。
- tokens/s 只计算 output_tokens / execution seconds；不计算 input/cache tokens，不宣称纯解码速度。真实样本：663 / 4.504 = 147.20，旧错误值为 11759 / 4.504 = 2610.79。
- 040 与 044 合并为唯一 design.md，删除重复 Spec；独立读者仅发现 enum/tagged union 描述不精确，已修正，没有追加新架构。

### 自动化证据

- 完整基线仅一次：默认 E2E 31/31；补充第 13/14/15 组分别 3/3、11/11、2/2，总计 47/47。报告位于 talos-e2e/reports/e2e_report_20260905_{224434,225005,225006,225007}.md。
- 标题修复后仅复测 03_messages_http_sse，通过；报告 e2e_report_20260905_230127.md。断言标题确实生成，Title 用量实际使用 deepseek-v4-flash。
- Kernel Provider、Service、Store 297/297 场景通过；just fmt、相关 scoped Clippy 通过，保留已记录的既有 lint 告警。
- Web 第一轮受影响场景 116/118 通过；两处旧 fixture 缺少真实 admission message ID，补齐 accepted 状态后相关 33/33 通过，未削弱断言。
- 后续改动定点验证：消息/指标 23/23，模型菜单 4/4，重试 cursor 与实时输入 5/5。
- 不重复执行完整 E2E，不把自动化 API/SDK 测试写成手工页面测试。
- 首次 pre-push 的 Web 347/348：唯一旧页面断言绑定“首轮完成前 query disabled”。即时消费导航输入后应允许读取 Session；改为验证只发送一次，保留失败草稿、权限和再次发送断言，页面组 10/10 定点通过。没有回退生产逻辑。
- 最终 pre-push 通过：Web 348/348；SDK core 36/36、browser 7/7、app-server 14 通过/1 个既有跳过；Desktop 24 通过/1 个既有跳过；全前端 lint、typecheck、production builds、repo checks、Rust fmt-check 与 workspace check 通过。
- Git 压缩前后代码树一致；最终仅补验收记录和旧查询开关断言调整。相对 main 为一个提交，596 个文件、约 9,800 行净删除（含生成物、文档与测试）。

### 真实页面证据

环境：just dev-server + conf/talos-dev.yaml + 隔离 pr15-manual-review Talos Home；just dev-web，端口 5175；后端 health 确认 auth_enabled=false。截图目录：talos-e2e/reports/manual_pr15_20260905/。

| 场景 | 实测结果与证据 |
| --- | --- |
| 原用户 Session 刷新后卡 Running | 原 4bbca50e... 数据保留；activeTurn=null，四个 durable Turn 均 completed；页面恢复历史和可输入状态，17_original_session_recovered.png。工具错误展示不等于 Turn 仍在运行。 |
| 运行中输入与刷新 | RELOAD_OK 工具运行期间输入可见，一次刷新后自然完成；06_input_visible_running.png、07_after_refresh.png。 |
| 新会话首轮刷新与标题 | FIRST_REFRESH_OK 运行中刷新，完成后唯一首轮，不重复提交；自动标题显示，09_first_turn_refresh_result.png。 |
| FIFO 队列 | 运行中入队，前一 Turn 完成后自动派发。早期过晚点击 Steer 的尝试只算 FIFO，不算同 Turn Steer。 |
| 同 Turn Steer + 刷新 | clear 后唯一 Turn turn_6e2ebab1261f49868b9d3ad2789f2619，运行中两条输入可见，最终 CLEAR_STEER_OK；API 对照同一 Turn 两条 User 消息、一个最终回答、activeTurn=null。15_steer_live_inputs.png、16_steer_refresh_complete.png。 |
| 停止 | Bash sleep60 正在执行时停止，取消终态可见，后续新问题可执行；10_stop_result.png。当前 UI 使用 Failed 标签展示 Cancelled，未改成新的状态概念。 |
| WaitingForInput 刷新续接 | AskUserQuestion 蓝/绿选择，刷新后同一问题仍可回答，选择蓝色后完成；11_waiting_question.png、12_question_resumed.png。 |
| compact / clear | /compact 实际完成；/clear 仅清理验收 Session 04645aeb...，历史/计数归零、Light 设置保留，随后对话及 Steer 成功；13_compaction_result.png、14_cleared_session.png、15/16。原用户 Session 未清理。 |
| 模型配置/菜单 | 设置页读取 DeepSeek 默认模型；对话菜单实际将 High 切换为 Light 并保持，悬停+点击回归通过。 |
| 外围入口冒烟 | Files 空列表搜索/视图切换、Memories 空列表搜索、插件列表及测试插件禁用/重新启用、General/Models 设置读取正常。未把空列表冒烟当作完整上传、Memory 写入或项目 CRUD 页面验收；对应后端能力由完整 E2E 覆盖。 |

### 范围与剩余限制

- 本轮手工验证为 Web，不冒称启动过原生 Desktop/TUI；其协议/SDK 场景使用已有自动化验证。
- 当前模型配置禁用附件入口；未伪造附件页面上传结果。
- 旧归档 Timeline 不作为新 History 读取；具体迁移限制已在合并 Spec 明示。
- 截图 01–05 包含修复前证据；08 仅捕获加载画面，不作为恢复通过证据。Review 使用 06、07、09–17。
- 验收 Home 缺少 IDENTITY.md 的显式 warning 保留，不修改真实默认配置掩盖；服务保留供用户 Review。

## 前序消融阶段（历史记录）

## 目标与退出条件

上一轮完成了主体删除，但独立审查证明清理与恢复后处理尚未全链路闭环。本计划撤回“整体已完成”的判断，不重启架构设计，只收敛现有执行、持久化和产品完成路径。

1. 未确认 cleanup 的 Turn 不得经任何恢复入口提交终态。
2. 已提交的正常或恢复终态进入同一 Service 完成策略；失败清理 Profile，非 Cancelled 终态在释放 live 占用后派发队列。
3. Provider/AgentSession、Harness、Service、Store 保持现有边界，不新增状态机、manager、callback framework、持久化 Context 或协议对象。
4. 完成以下场景验证和一次限定范围的删除/边界核对即结束本轮，不增加纯命名或假想需求工作包。

## 现有 owner 与消融方向

| Owner | 保留职责 | 本轮收敛 |
| --- | --- | --- |
| Provider / AgentSession | 原生执行及资源 | SPI 不扩展 |
| Harness / run-control | 精确 Turn cleanup 与资源隔离 | 直接复用既有 interrupt outcome，不复制状态 |
| Service execution driver | 单写入者、durable checkpoint、终态 CAS | 删除吞掉恢复结果的分支；恢复使用真实 checkpoint 形成既有 settlement |
| TurnService | 产品中断、恢复互斥、统计与队列 | 正常完成/中断/恢复共用完成策略，不再各自遗漏后处理 |
| Store | 事实、索引、事务 | 不新增 schema 或重复持久化对象 |

## 阶段 1：统一恢复与完成链路

**目标**：一次性改完整条调用链，不逐个入口打补丁。

**实现范围**：
- 恢复写终态的入口显式检查实际 cleanup outcome；Running/Cancelling、Session acquisition 失败、writer 持久化失败都不能绕过。
- 外部 orphan 收尾复用现有 Session maintenance reservation，避免与 live writer/并发续接竞争；不重建 Context。
- 删除 interrupt 中重复的 durable tail 查询/恢复分支，保留自然完成竞争与 cleanup-unconfirmed 对外语义。
- 恢复成功返回真实 checkpoint 形成的 AgentRunSettlement，不伪造空 messages/metrics，也不返回“只有错误没有已提交结果”。
- 正常执行与恢复复用 Service 完成函数；失败用量清理与队列派发单一定义。
- start permit 拒绝、progress/settlement 持久化失败、显式 interrupt 和 restart recovery 都遵循同一收口规则；保留原错误诊断与 queue reservation 屏障。
- 不改动本轮前已有 Kernel、前端、Channels、migration 等修改，不提交代码。

**成功标准**：所有生产 recovery terminal 调用方均检查 cleanup；所有已提交 Turn 终态均有可达的产品后处理；没有第二条内部事件流、新结果 DTO 或第二套 active 索引。

**测试**：集中完成实现和场景用例后验证，不边写边重复跑测试。

**状态**：已完成（生产调用方核对及场景验证完成；不改 Provider SPI/Store schema）

## 阶段 2：场景验证与限定范围验收

**目标**：验证跨层交接，而非静态对象或机械断言。

**场景**：
- cleanup 未确认后重连/恢复：不产生终态、不删除 durable active。
- Waiting 中断：唯一取消终态、清理未完成 Profile、取消后不自动派队列。
- writer 错误恢复成功：唯一失败终态，释放 live 后继续队列；恢复失败仍显式失败。
- 正常完成、Provider 业务失败、续接累计用量及 compaction 保持既有语义。
- admission / recovery / continuation 互斥，不把本进程执行当成 orphan。

**验证**：受影响 Service、Agent、Store、Coordinator 和 Gateway 现有场景；just fmt、scoped Clippy、git diff --check；不修改生产校验迁就测试。

**完成门**：Spec 与实际 owner/API 对齐；一次检查全部 recovery 与 completion 调用方，无已知目标内缺口。未做真实模型 E2E/前后端手测时必须明确说明，不宣称整个 PR 完成端到端验收。

**状态**：已完成（402 个相关场景覆盖；首轮 401 通过，修正注入位置后的剩余场景定点通过）

## 验证记录

- 本轮集中验证七个 package：Service、Agent、Kernel Provider、Store、Coordinator、Gateway、HTTP Gateway，共 402 个测试；首轮编译漏 import 已修正，随后 401 个场景通过。
- 唯一场景失败源于故障注入误命中初始 User admission，未进入目标 progress 写入路径；改为注入 Provider MetricsUpdated 后，该场景通过，包含 permit/progress/terminal 三种故障阶段，没有放宽生产校验或失败断言。
- 定点复测：just test -p talos-service -E 'test(committed_failure_recovery_releases_execution_and_dispatches_the_queue)' --status-level fail --final-status-level fail，1/1 通过。仅测试 fixture 增补注入事件，其余生产代码未变，不重复执行同批次。
- just fmt、七个 package 的 scoped Clippy、git diff --check 成功；没有新增 lint 告警。既有 Project Store &PathBuf、Service module inception、TurnService 构造参数数量、FS 测试持锁跨 await 告警保留，不扩范围修复。
- 删除核对：旧 recover_progress_commit_failure、settle_from_durable_checkpoint、interrupt_recovery_failure 及内部 Timeline 转发链均无残留。恢复终态写入只有 writer failure 与外部 recovery 两类调用方，都传递实际 cleanup outcome；两类结果进入同一完成策略。
- 完成策略按实际业务职责移到 turn/service/completion.rs，仍是 TurnService 的方法，不新增 owner、trait、DTO、通道或状态集合。
- 外部行为修正：durable admission 已成功时，start permit 拒绝不再把已接受请求返回成未接受错误；失败通过后续 canonical Turn 结果表达，与幂等重放一致。HTTP/JSON-RPC/SDK 字段和方法不变。
- 本轮未执行真实模型 E2E、全量 workspace 或前后端手测；本记录只证明本次限定的跨层场景，不承诺整个 PR 已完成端到端验收。
- 没有提交、推送或覆盖本轮前已有改动。
