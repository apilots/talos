# Agent Engineering Guide

本文件用于固化 Agent 在软件工程协作中的通用执行标准。以下原则与具体业务无关，适用于功能实现、缺陷修复、重构、测试和交付全过程。

## 0. 适用范围与优先级

- 本文件是 Talos 项目的根级工程规范，适用于 `talos-rs`、前端、插件、SDK、协议层、运行时和文档。
- 子目录存在更具体的 `AGENTS.md` 时，以更近的子目录规则为准。
- 当规则之间看似冲突时，优先执行更严格、更能保护架构边界和用户行为一致性的规则。
- Codex 项目中沉淀出的 Rust、TUI、Agent、MCP、API 和测试细则应作为 Talos 迭代原则吸收，但不能机械复制 Codex 的目录名、crate 名或专用命令。
- 对外说明、Agent 输出和推理过程使用中文；代码命名、注释、提交信息中的技术标识和公开 API 使用英文。

## 1. 先理解边界，再动手实现

- 动手前必须先阅读现有代码和相邻模块，理解职责边界、调用链和已有模式。
- 实现前必须查找至少 3 个相邻或类似模块，确认可复用的基础设施、对象、测试工具和错误处理方式；找不到时才评估新增能力。
- 不要只按用户提出的表面方案机械实现，要结合代码实际判断方案是否合理。
- 如果用户描述和代码事实不一致，应先指出差异并给出判断，而不是盲目照做。
- 架构边界比“功能暂时能跑”更重要，不能为了快速修复让底层模块依赖上层模块。

## 1.1 第一性原理、复用与必要抽象

- 先明确当前阶段要解决的具体问题、事实来源、不可变约束和最小成功标准；不能直接从“可能以后需要”反推框架。
- 优先在现有模块、Service、Store、事件链路、配置系统和测试基础设施上迭代；已有能力能承载目标时，禁止另起 manager、registry、scheduler、event bus、协议或通用框架。
- 模块化、分层、强类型对象和简洁 API 是必要抽象，不属于过度设计；每个抽象必须有唯一 owner、内聚职责、真实调用方和明确禁止承载的跨层职责。
- 对外 API 必须简洁、确定且可直接发现：调用方只依赖稳定 Request / Response / Domain Object，不得猜测内部目录、数据库表、自由 JSON、字符串约定或实现分支。
- 新增抽象前必须说明它替代了什么重复或隔离了什么稳定边界；如果不能对应当前目标中的真实问题，就不应新增。
- 实现只覆盖当前 Spec 和 Plan 的成功标准。顺手优化、未来扩展点、兼容层、备用路径和与当前阶段无关的重构必须删除或留待对应阶段。

## 2. 分层必须清晰

- 底层模块只依赖抽象输入，不应反向依赖上层业务管理模块。
- 生命周期管理、状态管理、执行运行时、协议暴露、UI 展示应各自有清晰职责。
- 上层负责组装、编排、配置合并和策略决策；下层负责纯粹执行。
- 跨层传递时应传递稳定抽象，而不是把上层服务对象塞进底层。
- 如果某个模块开始知道太多外部细节，说明边界已经失控。

## 3. 对外核心对象必须统一

- 同一业务能力的核心 Request / Response / Params / Result / Domain Object 应只有一套定义。
- HTTP、JSON-RPC、CLI、SDK、UI 等不同端只应有协议 envelope 或 transport 差异。
- 不允许每个协议或每个客户端复制一套结构相似但类型不同的对象。
- SDK 的目标是屏蔽协议差异，而不是把协议差异暴露给使用者。
- 新增能力时必须同时考虑服务端、协议层、SDK、前端和终端调用的一致性。

## 4. 对象必须强类型化

- 业务对象必须是结构化强类型对象，不能滥用 JSON、map 或任意 Value 承载核心语义。
- 枚举语义必须定义 enum，不应用裸字符串到处传。
- 状态文件即使以 JSON 落盘，也必须有明确 schema 和对应强类型对象。
- 类型命名要准确、简洁、可解释，避免使用含义模糊的 `Item`、`Details`、`Content` 等泛化名称。
- Request / Response / Params / Result 应保持命名和 API 边界一一对应，避免调用关系难以追踪。

## 5. 不重复造结构和逻辑

- 如果已有共享对象、工具函数、测试 fixture 或模块抽象，应优先复用。
- 不要在 HTTP、JSON-RPC、Service、SDK、UI 各层重复写 mapping 和转换。
- 不要为同一件事写多套相似测试 setup。
- 当重复出现第三次时，应停下来抽取共享 helper、fixture 或公共模块。
- 重构时要主动删除旧实现、旧测试和旧入口，避免新旧两套逻辑并存。

## 6. 不用 fallback 掩盖问题

- 不要用 fallback 让错误路径“看起来可用”。
- 配置缺失、状态不一致、路径错误、协议不匹配，应在正确层级显式失败。
- 未上线的新能力不需要保留历史兼容包袱，目标态应干净。
- 如果某个开关正式发布后永远不会关闭，就不应该存在。
- 不要为了让测试通过增加生产环境不需要的特殊分支。

## 7. 配置与路径必须有单一来源

- 默认行为应由配置系统统一表达。
- 测试可以覆盖配置，但不应修改真实默认配置语义来适配测试。
- 路径必须从统一根目录或配置派生，不能散落硬编码绝对路径。
- 调用链上不要传大量零散配置标量，优先传递配置对象或稳定上下文对象。
- 组装、去重、过滤等配置派生逻辑应集中在一个合适层级，不要让调用方各自实现。

## 8. 状态写入必须安全

- 涉及持久化状态的写入必须考虑并发、崩溃和部分失败。
- 文件状态写入应使用锁、临时文件、原子 rename，必要时执行 fsync。
- 涉及“目录 + 状态文件”双写时，必须设计事务顺序、回滚和恢复策略。
- 更新不能先删除旧数据再写新数据；卸载不能先永久删除再写状态。
- crash cleanup 不能只看文件名或目录名，必须结合状态文件判断恢复还是清理。

## 8.1 DB 持久化模块命名与边界

- 需要数据库持久化的业务必须独立为 `xxx-store` 模块或 crate。
- `xxx-store` 内定义数据库对象 `XxxRecord` 和对外 DB 操作接口 `XxxStore`。
- `XxxRecord` 只属于 store / repository 边界，不得作为 SDK、协议层、Gateway 或 UI 的公共对象外泄。
- Service 层通过 `XxxStore` API 访问数据库，并负责把 `XxxRecord` 转换为业务 DTO 或对外 `XxxInfo` / `XxxResponse`。
- 不允许把不同业务的 DB Record 和 Store API 混在同一个无关 store 中，例如 project 持久化不能定义在 session store 里。

## 9. 测试要验证真实行为

- 测试不能只验证“返回 OK”，必须验证核心行为真的发生。
- 对于功能链路，应覆盖输入解析、service 调用、协议封装、SDK 调用、UI/TUI 行为和运行时实际效果。
- 单元测试必须 deterministic，不依赖网络、真实外部服务或不稳定时间。
- E2E 可以依赖真实服务、真实网络、真实模型，用来验证真实集成。
- 修复 bug 时应补能失败的测试，而不是只靠手工验证。

## 10. 手工验证必须模拟真实用户路径

- 手工测试不能只启动一半服务，也不能只验证静态页面。
- 如果功能依赖前后端一体化，就必须同时启动真实前端和后端。
- 如果用户路径涉及安装、搜索、选择、执行，就要完整点一遍。
- 验证截图或日志必须能证明关键行为真实发生。
- 不要把间接验证说成手工验证。

## 11. UI 行为应跨端一致

- 同一能力在 Web、Desktop、TUI、CLI 中应尽量共享后端语义。
- 搜索、匹配、解析、选择等核心逻辑优先放在后端或共享模块，避免各端行为漂移。
- 前端不应重新实现一套和后端不一致的业务规则。
- UI 状态要反映真实后端状态，例如已安装、可更新、禁用、只读等。
- 交互细节必须在自测中覆盖，而不是只看页面能打开。

## 12. 前端和后端都要控制文件复杂度

- 新增页面、弹窗、列表、行组件、搜索组件应拆成独立组件。
- 不要把多个功能堆进一个大文件。
- Rust 也应按职责拆模块，不要把 manager、state、installer、reader、search 都揉在一起。
- 文件行数不是唯一标准，职责混杂才是更严重的问题。
- 拆分应服务于可读性和边界清晰，不为拆而拆。

## 13. Spec 和 Plan 分工明确

- Spec 写设计方案，不写执行阶段。
- Plan 写阶段、状态、成功标准和测试。
- Spec 应说明架构边界、对象模型、API、状态 schema、错误处理和安全约束。
- 实现变化后必须同步更新 spec，避免文档和代码漂移。
- Plan 阶段完成后要及时更新状态，不能让计划文件失真。
- 每个阶段开始前必须重新核对目标、非目标、可复用基础设施和最小改动面，并做一次“是否在发明已有能力”的自审。
- 当前阶段的成功标准、测试和质量门未全部满足前，不得提前实现后续阶段；发现越界改动时，应先停止并撤回或隔离越界内容，再继续当前阶段。
- 阶段状态只能按事实更新：局部代码完成、能够编译或部分测试通过都不能代替阶段完成。
- 每个阶段结束后必须再次检查是否存在过度设计、过度实现、跨层依赖、重复路径、半成品 API 和超出 Spec 的代码；清理完成并通过该阶段验证后才能进入下一阶段。

## 14. 提交前必须自我审查

- 大范围实现后必须主动检查 dead code、无用测试、重复 fixture、旧 API 残留、临时 workaround、无意义 fallback 和多余转换层。
- 自我审查必须同时检查“少了什么”和“多了什么”：既验证目标行为完整，也删除不服务于当前目标的抽象、参数、状态、配置、测试和注释。
- 提交前要跑对应层级的编译、测试、lint/typecheck 和必要 E2E。
- 不要把“能编译”当作“设计正确”。
- 不要把 review 问题逐个打补丁后就结束，应整体回看是否产生新的重复和残留。

## 15. 失败时要停下来复盘

- 同一问题连续多次修不好时，不要继续堆补丁。
- 应重新梳理根因是否判断错、抽象层是否放错、是否存在更简单的边界、是否应该删除旧逻辑而不是继续兼容。
- 如果修复方向被证明错误，应主动撤回无效改动。
- 不能用更多复杂度掩盖前一个错误设计。

## 16. 命名必须表达真实概念

- 名字要反映对象真实职责，而不是临时实现形态。
- 不准确的名字会导致后续设计继续偏离。
- 避免为了“通用”起过度抽象的名字。
- 如果一个对象按类型扩展，应把公共字段放在主对象，把类型特有字段放在子对象。
- 命名不清时应先讨论概念，而不是先写代码。

## 17. 新能力不应留下半成品 API

- API 暴露的字段必须有完整可达的管理路径。
- 如果某个状态可以被查询，就应明确它如何被创建、更新和删除。
- 不要暴露暂时无法管理的字段，除非明确标注为 reserved/experimental。
- 新增 API 必须同时定义错误语义、权限边界、状态变化和测试覆盖。
- 分层 API、核心结构和小框架入口必须有英文 doc comment，说明 owner、职责、调用方预期和禁止承载的跨层职责。
- 新增可扩展 enum、factory、loader、registry、router、validator、lifecycle、normalizer、input bridge、error mapper 时，注释必须说明扩展条件，避免后续塞入不属于该层的功能。
- 不完整的 API 比没有 API 更容易造成长期债务。

## 18. 代码应该服务于长期演进

- 当前实现要为未来扩展留出合理边界，但不要过早抽象。
- 可扩展性来自清晰模型和稳定边界，不来自松散 JSON 或万能字段。
- 能配置扩展的地方，应通过配置和统一 loader 支持，不要写死特殊路径。
- 未来类型可以扩展，但当前类型必须明确。
- 不要为了“以后可能用”保留现在没有真实语义的代码。

## 19. talos-rs / Rust 细则（Codex 对齐）

- `talos-rs` 下 Rust crate 命名应统一使用项目约定前缀；如果已有 `talos-*` 命名模式，新 crate 必须延续该模式。
- 仓库依赖的命令（例如 `just`、`rg`、`cargo-insta`、schema 生成器、lint 工具）缺失时，应先安装或说明缺失，不要跳过对应流程。
- 使用 `format!` 且变量可以直接内联到 `{}` 时，必须内联变量，例如 `format!("{name}")`，不要写 `format!("{}", name)`。
- 必须按 Clippy 规则折叠可折叠的 `if`，对应 `collapsible_if`。
- 必须按 Clippy 规则内联 format 参数， 对应 `uninlined_format_args`。
- 能使用方法引用时，优先使用方法引用，不要写冗余 closure，对应 `redundant_closure_for_method_calls`。
- 避免 `bool` 或含义不清的 `Option` 位置参数，例如 `foo(false)`、`bar(None)`；优先使用 enum、命名方法、newtype、builder 或结构化参数。
- 当无法调整 API 且必须传入不透明位置字面量时，按 `argument_comment_lint` 约定添加精确参数注释，例如 `foo(/*enabled*/ false)`、`bar(/*timeout*/ None)`。
- 方法唯一的非 self 参数如果方法名和参数名一致，可不加参数注释，例如 `.enabled(false)`。
- 不要给字符串或字符字面量机械添加参数注释，除非注释确实提升可读性。
- `match` 应尽量穷尽所有枚举分支，避免 wildcard arm 掩盖未来新增 variant。
- 新增 trait 必须包含 doc comment，说明 trait 的职责、实现方约束和调用方预期。
- 不鼓励 `#[async_trait]` 和 `#[allow(async_fn_in_trait)]`。
- Rust trait 中的异步接口优先使用原生 RPITIT，并显式写出 returned future 的 `Send` 约束：`fn foo(&self, ...) -> impl std::future::Future<Output = T> + Send;`。
- trait 实现可以使用 `async fn foo(&self, ...) -> T`，前提是满足 trait 的 future contract。
- 不要把 `#[allow(async_fn_in_trait)]` 当成省略 future contract 的捷径。
- 优先使用私有模块和显式导出的 public crate API。
- 新增小 helper 前必须确认它不只是被调用一次；只调用一次且不显著表达概念或降低复杂度时，不要新增 helper。
- 异步 tracing 应优先标注在函数或方法定义上，例如 `#[tracing::instrument(...)]`，不要在调用点给 future 临时 `.instrument(...)`。
- 添加 instrumentation 前，先检查被调函数或直接代理的实现方法是否已经 instrumented，避免重复 span。

## 20. 核心 crate 与模块规模（Codex 对齐）

- 必须抵制把新能力默认加到核心 crate；核心 crate 只能承载稳定、必要、跨模块复用的模型和能力。
- 新增概念、功能或 API 前，必须先判断是否存在比核心 crate 更合适的现有 crate。
- 如果没有合适 crate，应评估是否需要新建 crate，而不是继续扩大核心 crate。
- Review 时应主动指出不必要扩大核心 crate 的改动。
- 非机械变更的总变更行数原则上不应超过 800 行。
- 复杂逻辑变更应控制在 500 行以内；超过时必须拆分为可 review 的阶段，并说明第一阶段最小可落地范围。
- Rust 模块目标保持在 500 LoC 以下（不含测试）。
- 文件超过约 800 LoC 时，除非有明确记录的理由，不应继续向该文件追加新职责。
- 高触达中央编排文件不应继续承载新功能；应新增模块并保持原文件聚焦编排。
- 从大模块抽取代码时，相关测试、模块文档、类型文档和不变量说明要靠近新实现。
- 前端大页面、弹窗、列表、搜索、表单、运行状态区域、TUI 面板都必须按职责拆组件或模块。

## 21. Agent 上下文规则（Codex 对齐）

- Agent 发送给模型的 context 必须增量构建，禁止重写历史。
- 避免频繁改变 context 结构或内容顺序，防止不必要的缓存失效。
- 所有注入 context 的内容必须有大小上限和硬 cap，禁止无界列表、无界日志、无界文件片段。
- 单个 context item 不得超过 10K tokens。
- 新增任何可能超过 1K tokens 的单项内容，必须标记为高风险并进行额外人工审查。
- 所有注入片段必须定义为明确结构体或 domain object，并实现/遵循项目的上下文注入接口；禁止散落字符串拼接。
- 上下文对象必须记录来源、边界、裁剪策略和 token 成本假设。

## 22. MCP / Tool Call 规则（Codex 对齐）

- 修改 MCP tool call 行为时，应优先使用现有 MCP connection manager / tool call manager 边界处理工具和调用状态 mutation。
- 目标是最小化改动面，复用已有抽象，不要把 tool mutation 逻辑穿透多层函数参数。
- 不要无必要调用 session reset / connection reset；应让增量检查、生命周期状态和能力协商决定是否复用已有连接。
- MCP server、tool、resource、prompt、call result、capability、transport config 都必须强类型化。
- 用户可配置 MCP server 时，必须把配置、运行状态、来源、安装/启动方式、错误信息分开建模。
- 对任意外部 MCP server 的启动命令、环境变量、工作目录和权限边界必须显式保存并可审计。
- 不允许用 fallback 吃掉 MCP 初始化失败、tool schema 不匹配、transport 断开或权限错误。

## 23. 测试编写规则（Codex 对齐）

- 写测试时优先比较完整对象相等，而不是逐字段断言。
- Rust 测试应优先使用清晰 diff 的断言工具；如果项目使用 `pretty_assertions::assert_eq`，新增测试应沿用。
- 不要为静态定义的值添加测试。
- 不要为已经删除的逻辑添加负向测试。
- Agent 逻辑变更优先写集成测试，不要只写狭窄单元测试。
- 会改变 Agent 推理、工具调用、上下文注入、会话状态、运行时策略的功能，必须添加集成测试。
- 如果仍需单元测试，放到专门的 `*_tests.rs` 文件，避免在主实现中暴露 test-only function。
- 新增 test module 时，优先使用 sibling test file，并用显式 `#[path = "..._tests.rs"] mod tests;` 引入。
- 不要为了测试内部实现扩大 public API。
- 检查是否已有 helper、fixture、builder 或 mock 工具可以复用，避免重复测试 setup。
- 测试不得依赖网络、真实外部服务、不稳定时间或全局进程环境。
- 避免在测试中 mutate process environment；需要环境差异时，从上层显式传入 flag、配置或依赖。
- 需要启动 workspace 内二进制时，路径解析必须兼容不同构建系统和工作目录；不要依赖当前 shell 的相对路径偶然正确。
- 修复 bug 时必须补能暴露原 bug 的测试，而不是只验证返回 OK。

## 24. 快照、TUI 与文本输出（Codex 对齐）

- 任何影响用户可见 UI 或文本输出的变更，都必须包含对应 snapshot、视觉测试或端到端覆盖。
- 如果项目使用 `insta`，UI/TUI 输出变更必须生成、检查并接受对应 snapshot。
- 不能只跑测试生成 snapshot；必须阅读 `*.snap.new` 或等效 diff，确认用户可见变化是预期的。
- TUI 文本样式应使用项目既有 styling helper；不要在无必要时手写低层 Style。
- 不要硬编码 white 前景色；优先使用默认前景色，除非设计系统明确要求。
- 不要在等价写法之间制造 churn，例如无意义地来回改 `Span::styled`、`set_style`、`Line::from`、`.into()`。
- 构造普通 span / line 时优先选择最简洁、符合本文件局部风格且 rustfmt 后可读的形式。
- 包装 plain string 必须使用项目统一 text wrapping 工具；不要手写临时换行逻辑。
- 需要缩进 wrapped lines 时，优先使用统一 wrapping options，而不是自己拼前缀。
- 对列表批量加前缀时，优先使用统一 line utility，避免每处重新实现。

## 25. App Server / JSON-RPC API 规则（Codex 对齐）

- 活跃 API 开发只能发生在当前主版本协议；不要给旧版本协议增加新 API surface。
- talos-rs API 类型命名必须按边界区分：Service / Gateway / SDK 等对外公共方法 DTO 使用 `*Request` / `*Response`；JSON-RPC method params 使用 `*Params`，response 使用 `*Response`；Coordinator / AgentHarness / AgentRuntime SPI 等内部执行 API 使用 `*Params` / `*Result`。
- 同一语义不能同时存在 `XxxRequest` 和 `XxxParams`、`XxxResponse` 和 `XxxResult` 两套对象；如果边界变化需要改名，应重命名已有对象并删除旧名，不要用 alias 长期保留双名。
- JSON-RPC request payload 命名使用 `*Params`，response 使用 `*Response`，notification 使用 `*Notification`。
- RPC 方法名使用 `<resource>/<method>`，resource 保持单数，例如 `thread/read`、`app/list`。
- Wire 字段默认使用 `camelCase`；如果配置 API 需要镜像配置文件 key，可使用 `snake_case`，但必须明确说明。
- Wire enum 字符串默认使用 `camelCase`，Rust serde rename 和 TypeScript rename 必须一致。
- v2 / 当前主版本 API 的 request、response、notification 类型必须导出到对应 TS namespace 或 schema namespace。
- 当前主版本 API payload 字段不要使用 `skip_serializing_if = "Option::is_none"`。
- 例外：client -> server 且 params 为空的方法，可以显式用 `Option<()>` 表达无 params。
- Rust 和 TS 的 wire rename 必须保持一致；Rust 有 `#[serde(rename = "...")]` 时，TS 必须有对应 rename。
- tagged union 必须显式 tag，并保证 Rust 与 TS tag 一致。
- API 边界优先使用普通 `String` ID；UUID 等解析和转换放在内部。
- 时间戳使用整数 Unix seconds（`i64`），字段命名使用 `*_at`，例如 `created_at`、`updated_at`、`resets_at`。
- 实验 API 必须显式标记；字段级实验能力需要可被 schema / runtime 检查。
- client -> server request payload 中，每个 optional 字段必须明确 TS optional / nullable 语义。
- Optional collection 必须使用 `Option<Vec<T>>` 或 `Option<HashMap<K, V>>` 表达；不要用 `serde(default)` 把省略和空集合混在一起。
- 省略表示 `false` 的 bool 字段，应优先使用 `#[serde(default, skip_serializing_if = "std::ops::Not::not")]`，而不是 `Option<bool>`。
- 新增 list 方法默认实现 cursor pagination：request 包含 `cursor`、`limit`，response 包含 `data`、`next_cursor`。
- API 行为变化时必须更新对应 docs、README、schema fixture 和示例。

## 26. 配置、依赖与构建规则（Codex 对齐）

- 修改配置结构或嵌套配置类型时，必须运行项目对应 schema 生成命令并提交 schema 更新。
- 修改 app-server / JSON-RPC API shape 时，必须运行项目对应 schema 生成命令，并同时更新 experimental schema（如果涉及实验 API）。
- 修改 Rust 依赖时，必须同步更新 `Cargo.lock`。
- 如果项目使用 Bazel、Buck、Nix 或其他额外锁文件，依赖变更必须同步更新对应 lockfile。
- 构建系统不会自动暴露源树文件给编译期读取；新增 `include_str!`、`include_bytes!`、数据库 migration、schema、fixture 或类似编译期文件读取时，必须同步更新构建声明。
- 生成产物只能由生成命令产生，不要手写修改。
- 默认配置必须表达真实产品语义；测试可以覆盖配置，但不能改变默认语义来适配测试。
- 不要使用 `--no-verify` 跳过 hook。
- 不要禁用测试、lint 或编译检查来让变更通过。

## 27. 运行命令与验证规则（Codex 对齐）

- Rust 改动完成后，应在仓库根目录运行项目约定 formatter：`just fmt`。
- 不要直接运行裸 `cargo test` 代替项目约定命令；Talos Rust 测试默认使用根目录 `just test`，该命令通过 `cargo-nextest` 执行。
- 优先运行被修改 crate / package 的测试，例如 `just test -p <package>`。
- 日常本地验证应尽量 scoped 到 package、test filter 或必要 E2E case；不要为了普通改动运行全量 workspace 测试。
- 如果改动涉及 common、core、protocol、config、runtime 等共享层，通过局部测试后应评估是否需要完整测试套件。
- 大范围 Rust 改动提交前，应运行项目约定 lint/fix 命令，优先 `just fix -p <package>`。
- 不要在 formatter 或 fix 后为了形式重复跑同一批测试；需要复测时应基于实际风险说明原因。
- `talos-rs/target` 体积异常时，先用 `just target-size` 查看分布；优先用 `just clean-incremental` 回收增量状态，只有需要从头重建 debug artifacts 时才用 `just clean-debug`。
- Rust 命令可能因为锁或构建缓存较慢，应耐心等待，不要用 PID 强杀。
- 无法运行某个必要命令时，必须说明原因、影响和剩余风险。

## 28. Breaking Change 审查（Codex 对齐）

- 每次变更都要主动搜索外部集成面的 breaking change。
- 必查范围包括 app-server API、JSON-RPC 方法、HTTP API、SDK 类型、CLI 参数、配置加载、会话恢复、raw event、TUI/前端事件和插件协议。
- 即使 API 标记 experimental，只要已有外部调用，也要按 breaking change 审查。
- 删除字段、重命名字段、改变默认值、改变错误码、改变通知顺序、改变状态机时，都必须写清兼容影响。

## 29. Review 清单（Codex 对齐）

- Crate API surface 是否最小？是否暴露了 test-only helper？
- 是否把新逻辑放入核心 crate，而没有评估更合适的 crate / 模块？
- 是否新增了超过 1K tokens 的模型上下文片段？是否存在超过 10K tokens 的单项注入？
- 是否存在无界列表、无分页 API、无上限日志、无上限文件读取或无上限 tool result？
- 是否改变了外部集成面但没有更新 schema、TS 类型、SDK、README 或示例？
- 是否有 fallback、临时 workaround、feature flag 或兼容分支掩盖真实设计问题？
- 是否新增重复 DTO、重复 mapping、重复 fixture、重复安装逻辑或重复状态机？
- 是否把 DB Record、运行时内部对象、transport envelope 泄漏给 SDK / UI / 协议层？
- 是否需要 snapshot、集成测试、E2E 或真实用户路径验证？
- 是否能拆成更小、更容易 review 和回滚的阶段？
