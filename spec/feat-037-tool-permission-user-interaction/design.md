# Tool Permission 与用户交互设计

Status: draft
Owner: agent
Created: 2026-08-11
Updated: 2026-08-11

## 1. 结论

本设计在 `feat-036-unified-tool-sandbox` 已有权限解析、permit、macOS/Linux provider 和文件系统 authorizer 之上，补齐产品层的 Session 权限模式、Sandbox 审批、Bash 风险判断、`AskUserQuestion`、Timeline 恢复以及 TUI/WebUI/Desktop 交互。

核心决定如下：

1. 配置文件中的 `tools.sandbox_policy.user_permission_mode` 只提供 Session 默认值；首期不提供 UI 配置编辑和运行时全局配置修改。
2. TUI、WebUI 和 Desktop 操作的是 Session 权限模式，不直接操作底层 sandbox mode、filesystem policy、network policy 或 provider。
3. `FullAccess` 不触发 Sandbox 审批，不调用 Bash 风险模型，并明确走无隔离执行；不可被用户覆盖的治理硬拒绝仍然直接拒绝。
4. `AskForApproval` 由确定性权限和风险规则决定审批，不为同一个审批决定额外调用模型。不能证明安全的 Bash 命令可以直接进入审批。
5. `ApproveForMe` 对确定性安全或危险结果直接处理，只对风险不明确的 Bash 命令调用模型。模型必须返回结构化 `CommandRiskAssessment`：`Safe` 在确定性权限已经允许的前提下直接执行，`Risky`、`Unknown`、模型失败或非法输出进入用户审批。
6. Bash 风险模型只提供风险事实，不作权限决定。`Safe` 允许 Runtime 继续既有 permission/sandbox 执行链，但不是 grant；模型不能授予额外权限、覆盖确定性审批或硬拒绝、关闭 Sandbox 或扩大 approval grant。
7. Sandbox 审批由 Runtime 生成固定枚举选项，只支持 `AllowOnce`、`Deny` 和 `Cancel`；不允许自由文本、客户端自造选项或任意权限组合。
8. `AskUserQuestion` 是模型驱动的语义提问。模型提供 2-3 个互斥选项，客户端自动增加 `Other` 自定义输入；回答作为原 tool call 的结果进入模型上下文并继续同一个 Turn。
9. Sandbox 审批、语义提问和 Dynamic Tool 输入统一为 `PendingUserAction -> WaitingUserAction -> Timeline -> Resume` 生命周期，但使用不同的强类型参数、响应和恢复逻辑。
10. 现有 `AskUserQuestion` 的进程内 `latest_question`、立即返回 `awaiting_user_response` 和仅测试可达的 `submit_question_answer` 是未闭环实现。新链路完成时必须 move-and-delete，不保留第二套等待机制。

```text
feat-036-unified-tool-sandbox
  permission resolver + permit + provider + enforcement
                         |
                         v
feat-037-tool-permission-user-interaction
  session mode + risk gate + approval/question + UI + resume
```

## 2. 背景与问题

`feat-036` 已经让本地 Bash/Grep 和文件工具在执行前进入 `ToolSandbox::evaluate()`，并在允许后通过 invocation-bound permit 执行。macOS/Linux 受限模式能够进入 enforcing provider，`FullAccess` 明确进入 Noop。

当前产品链路仍不完整：

- 三套环境配置都默认使用 `full_access`，TUI/WebUI/Desktop 没有 Session 权限模式控制。
- `ToolContext::with_sandbox_user_permission_mode()` 已存在，但生产调用链没有注入 Session 选择。
- `ToolSandboxDecision::RequiresApproval` 当前被直接转换成 `PermissionRejection`，不存在用户审批、grant 和恢复。
- Orchestrator 当前把 `potentially_unsafe` 固定为 `false`，`ApproveForMe` 没有真实危险命令判断来源。
实现已经收口为一条持久化 UserAction 链：Question、SandboxApproval 和 DynamicToolResult 共享 Service-owned lifecycle、Store CAS、ActiveTurn、Timeline recovery 和 typed response。WebUI、Desktop 和 TUI 通过现有协议与 SDK 消费同一事实；无后端语义的 Settings safe read/write 开关和写死模式展示已不再作为权限入口。

后续实现不得为审批和问题分别新增 manager、全局 map 或客户端私有状态，否则会重新产生多套等待/恢复机制，并让断线恢复、幂等和工具结果注入分叉。

### 2.1 Codex 参考边界

本设计参考 `../codex` 已验证的交互分层，但不复制其 Session 内存实现：

- Codex 的 `request_user_input` 由模型生成结构化问题和 2-3 个选项，客户端自动提供 Other；回答作为 function tool result 返回模型。
- Codex 的 command/file approval 由 Runtime 生成 typed request 和 `available_decisions`，客户端只能返回固定 `ReviewDecision`，不会把自由文本解释为授权。
- 两类请求共享 elicitation pause/resume 生命周期，但保持不同 request/response 类型和不同安全语义。
- Codex 使用 pending callback/oneshot 恢复进程内调用；Talos 已有 ActiveTurnStore、Timeline recovery 和 tool continuation，因此复用 Talos 的持久化主干，不迁移另一套内存 Session coordinator。
- Codex 的 session approval cache、exec/network policy amendment 和自动 reviewer 不进入首期，避免在 grant scope 未稳定前暴露半成品能力。

## 3. 目标

1. 让用户在每个 Session 上选择 `AskForApproval`、`ApproveForMe` 或 `FullAccess`，并由 TUI/WebUI/Desktop 共享同一 API 和状态。
2. 让非 Full Access 模式下超出有效权限基线或被识别为危险的操作进入真实审批，而不是直接失败。
3. 让批准结果绑定到原 invocation，重新执行 policy evaluation，并且只恢复原 tool call 一次。
4. 为 `ApproveForMe` 增加 bounded、结构化、fail-closed 的 Bash 模型风险判断。
5. 让 `AskUserQuestion` 真正暂停 Turn，接收选项或自定义回答，并把回答作为 tool result 反馈给模型。
6. 将现有等待输入能力收敛为统一 `PendingUserAction`，复用 ActiveTurnStore、Timeline、recovery 和 resume，不新增平行 elicitation framework。
7. 删除被目标链路取代的未闭环问题状态、伪等待结果、重复 DTO 和无生产调用方。
8. 确保 UI 只展示服务端发布的模式、审批选项和 `PendingUserAction`，不在客户端重复推导权限或风险。

## 4. 非目标

1. 不提供 YAML Sandbox 配置编辑页面，不扩展当前 Config API 管理 Sandbox 默认值。
2. 不实现 `AllowForSession`、持久化命令规则、network amendment 或自定义 approval policy。
3. 不让用户通过自由文本授予 Sandbox 权限。
4. 不让 Bash 风险模型直接执行命令、产生 grant、修改 Session 模式或替代确定性 governance。
5. 不实现 host-level network enforcement、managed proxy、完整 sensitive-path policy 或 Windows Sandbox provider。
6. 不把 MCP、remote tool 或 dynamic tool 宣称为本地 OS sandboxed；它们继续遵循 `feat-036` 的 enforcement boundary。
7. 不新增通用 workflow/form engine。首期只在已有 Dynamic Tool 输入之外增加 `Question` 和 `SandboxApproval` 两种 user action variant。
8. 不在本 Spec 编写实现阶段；阶段拆分和进度状态属于后续独立 Plan。

## 5. 命名与领域边界

语义提问对象不保留 `Model` 前缀。模型只提供问题参数，Runtime 负责 user action 和 tool invocation 身份：

```rust
pub struct QuestionParams {
    pub header: String,
    pub question: String,
    pub options: Vec<QuestionOption>,
}

pub struct QuestionOption {
    pub id: String,
    pub label: String,
    pub description: String,
}

pub enum QuestionAnswer {
    SelectedOption { option_id: String },
    FreeForm { text: String },
}
```

`QuestionAnswer` 必须二选一，不能同时提交 option 和 free-form。模型不创建 `Other` option；TUI/WebUI/Desktop 根据 `QuestionParams` 固定增加 `Other` 输入入口。首期始终支持 Other，删除现有 `allows_free_form` 输入字段，避免模型参数和客户端行为出现两套开关。

Sandbox 审批参数使用独立对象；action 和 call identity 不在参数内重复定义：

```rust
pub struct SandboxApprovalParams {
    pub tool_name: String,
    pub reason: SandboxApprovalReason,
    pub presentation: SandboxApprovalPresentation,
    pub available_decisions: Vec<SandboxApprovalDecision>,
}

pub enum SandboxApprovalDecision {
    AllowOnce,
    Deny,
    Cancel,
}
```

所有 pending user action 只使用两个运行时身份：

- `action_id` 是 response、compare-and-set 和幂等身份，由 Runtime 生成。
- `call_id` 只绑定原始 tool invocation，由模型调用产生并由 Runtime 保存。
- `item_id` 是后端 Timeline item 身份；客户端只可据此挂载 inline action panel，不能用 `call_id` 搜索、合并或合成 Timeline item。

不再定义独立 `question_id` 或 `approval_id`。Question 恢复逻辑将 `QuestionAnswer` 转成模型可见的 `QuestionToolResult`；`SandboxApprovalDecision` 进入 Runtime permission decision。两者不能转换为彼此，也不能共用自由 JSON response。

`UserAction` 只作为领域和模块名称，不再额外定义同名 struct。跨层等待对象统一为 `PendingUserAction`，ActiveTurn 状态沿用 `WaitingUserAction`，关联字段沿用 `current_action_id`；不引入 `Interaction` 或其他同义根概念。

## 6. 模块职责

| 模块 | 本 Spec 中的职责 | 禁止承载 |
| --- | --- | --- |
| `talos-tools-sandbox` | 消费 requested permissions、risk facts 和 grant，返回 Allowed/RequiresApproval/NeedsRiskAssessment/Denied | 调用 LLM、维护 UI、保存 user action |
| `talos-tools` | `AskUserQuestion` schema/validation；Bash invocation facts；把 pending outcome 返回 Kernel | 全局 question store、平台隔离、Session persistence |
| `talos-agent-kernel` | 识别 typed pending outcome；停止和恢复原 tool call；把 Question answer 写成 tool result | UI、平台 provider、自由文本权限解析 |
| `talos-service::user_action` | response validation、状态转换、幂等 resume 和类型专属恢复编排 | SQL、migration、DB transaction、Record 定义、客户端展示规则 |
| `talos-service` 其他模块 | Session mode、Sandbox approval projection 和 Timeline 编排 | 绕过 `user_action` 模块修改 action 状态 |
| Bash risk assessor | 对 bounded command facts 返回结构化风险判断 | grant、Sandbox execution、Session mutation |
| `talos-store::session::user_action` | 定义 `UserActionRecord`、`UserActionStore` 和原子 CAS；保存 response/state/version | 状态转换决策、tool result、LLM prompt、approval policy |
| ActiveTurnStore | 保存 `WaitingUserAction` 和 `current_action_id`，不保存完整 action record | action response、状态转换和恢复编排 |
| Timeline/Gateway/SDK | 发布和恢复统一 DTO，接收强类型 response | 根据命令文本重新判断权限 |
| TUI/WebUI/Desktop | 展示 Session mode 和服务端提供的 `PendingUserAction`；提交允许的 response | 自造审批选项、修改 requested permissions、客户端风险分类 |
| macOS/Linux provider | enforce 已解析的 effective policy | 用户模式、风险判断、审批和 UI |

核心对象 owner：

- `talos-types` 只定义跨 Service、Gateway、SDK 和客户端复用的产品对象：`PendingUserAction`、`UserActionParams`、`UserActionResponse`、`UserActionState` 以及 Session mode。Timeline snapshot 直接引用这些对象，不再复制同构 DTO。
- `talos-store::session::user_action` 是 user action 持久化的唯一模块 owner。它定义 `UserActionRecord`、`UserActionStore`、查询和 CAS；对应 schema migration 仍使用 `talos-store` 现有统一 migration 机制。Record 不向协议、SDK 或 UI 暴露。
- `talos-service::user_action` 只依赖 `UserActionStore` API。它不能直接访问 database handle、拼 SQL、定义 migration、重复定义 Record，或在 Service 内维护第二份持久化状态。
- `talos-service::user_action::state` 定义纯状态转换校验；Store 只原子执行已校验的 expected state/version -> next state，不决定业务上是否允许迁移。
- `talos-agent-kernel` 定义 continuation 和 resume 私有对象；这些对象不能成为 Gateway DTO。
- `talos-tools-sandbox` 定义 permission/risk/grant 对象；Question params 不进入该 crate。
- `talos-service` 是 `ToolApprovalRequest -> SandboxApprovalParams` 的唯一 mapper。它只做 bounded projection，不重新决策权限；内部 requested permissions 和 risk assessment 保存在 continuation/store owner 中，不要求 `talos-types` 依赖 `talos-tools-sandbox`。

依赖方向：

```text
TUI / WebUI / Desktop
          |
          v
Gateway / SDK
          |
          v
TalosService::user_action -- UserActionStore / ActiveTurnStore / Timeline
          |
          v
AgentKernel -- Tools / BashRiskAssessor
          |
          v
talos-tools-sandbox
          |
          v
talos-sandbox-spi -> platform provider
```

## 7. Session 权限模式

配置文件只提供默认值。有效模式按以下顺序解析：

```text
SessionConf.user_permission_mode
  -> absent: tools.sandbox_policy.user_permission_mode
  -> absent: FullAccess compatibility default
```

目标 Session 对象：

```rust
pub struct SessionConf {
    pub model_selection: Option<ModelSelection>,
    pub user_permission_mode: Option<UserPermissionMode>,
}
```

约束：

- Session 创建时不必复制默认值到 override；`None` 表示继续继承配置默认值。
- Gateway/SDK 返回 `configured_default_mode` 和 `effective_mode`，客户端不自行猜测 fallback。
- Session 只允许在没有 active Turn 时修改模式。首期不定义运行中切换的竞态语义。
- 修改 Session 模式不写入 YAML，也不影响其他 Session。
- `PermissionMode::Plan/Default` 是计划工具约束，和 `UserPermissionMode` 无关，不能互相映射。

生产注入只有一条链路：

```text
TalosService resolves effective mode
  -> TalosSession / AgentRunConfig
  -> ToolContext
  -> RegistryOrchestrator
  -> talos-tools-sandbox
```

只有 `TalosService` 合并 Session override、配置默认值和兼容默认值。后续层只消费已解析的 `UserPermissionMode`，不得再次读取 YAML 或实现 fallback。

## 8. 权限模式与审批矩阵

### 8.1 AskForApproval

`AskForApproval` 是确定性审批模式。它不调用 Bash 风险模型来决定是否询问用户。

自动允许：

- workspace 内满足有效 policy 的读取和写入。
- 已被确定性规则证明安全、且不请求额外权限的 sandboxed Bash 命令。
- 不需要网络和额外 filesystem roots 的本地进程。

进入审批：

- 请求开启当前 policy 未授予的网络访问。
- 请求读取或写入 workspace 外路径。
- 请求关闭 Sandbox、`DangerFullAccess` 或其他额外 filesystem capability。
- 被确定性规则识别为危险或不能证明安全的 Bash 命令。

直接拒绝而不审批：

- governance maximum 明确禁止的权限。
- provider 不支持、不可用或 setup 失败。
- 非法 request、grant mismatch、sandbox bypass 或协议校验失败。

### 8.2 ApproveForMe

`ApproveForMe` 自动执行普通受限操作，并为危险操作请求用户审批。

自动允许：

- workspace 内满足 effective policy 的文件操作。
- policy 已允许的普通网络访问。
- 被确定性规则证明安全的 sandboxed Bash 命令。
- 模糊 Bash 命令经结构化模型判断为 `Safe`，且没有确定性 permission escalation。

进入审批：

- 确定性规则识别为危险的 Bash 命令。
- 模糊 Bash 命令的结构化模型结果为 `Risky` 或 `Unknown`。
- 风险模型超时、不可用、反序列化失败或违反 schema。
- 请求 workspace 外 filesystem access、Sandbox bypass 或 Full Access。

确定性 permission/governance 结果优先于模型。只有在确定性权限已经允许、且唯一未决事实是命令风险时，模型返回 `Safe` 才直接继续 sandbox evaluate 和执行；它不能覆盖任何已经要求审批或拒绝的事实。

### 8.3 FullAccess

`FullAccess`：

- 不触发 Sandbox 用户审批。
- 不调用 Bash 风险模型。
- filesystem 为 full access，network enabled，local sandbox capability disabled。
- 本地执行使用 `NoopBackend`。
- governance hard deny 仍然直接拒绝；hard deny 不是可由用户批准的 Sandbox 审批。

## 9. Bash 风险判断

### 9.1 决策顺序

`talos-tools` 只负责从 Bash invocation 构造 bounded command facts 和确定性 `Safe/Dangerous/Ambiguous` 风险事实。用户模式、permission、governance 和“是否需要调用模型”的矩阵只由 `talos-tools-sandbox` 决定：

```text
Bash invocation + deterministic risk fact
  -> ToolSandbox::evaluate(request)
       Denied                 -> reject
       RequiresApproval       -> publish approval
       Allowed(permit)        -> execute
       NeedsRiskAssessment    -> ApproveForMe only
          -> structured Bash risk assessor
          -> ToolSandbox::evaluate(request + assessment)
               Safe           -> Allowed(permit) -> execute
               Risky/Unknown  -> RequiresApproval
```

```rust
pub enum ToolSandboxDecision {
    Allowed(ToolSandboxPermit),
    RequiresApproval(ToolApprovalRequest),
    NeedsRiskAssessment,
    Denied(ToolSandboxDenial),
    External(ExternalSandboxEnforcement),
}
```

`NeedsRiskAssessment` 只能由 `ApproveForMe + Ambiguous` 产生。`AskForApproval + Ambiguous` 直接返回 `RequiresApproval`；`FullAccess` 直接按现有 Noop policy evaluate，不返回该 variant。模型风险判断不是 Sandbox SPI 或 platform provider 的职责，也不能由 AgentKernel/客户端复制模式矩阵。

### 9.2 结构化输出

```rust
pub struct CommandRiskAssessment {
    pub verdict: CommandRiskVerdict,
    pub categories: Vec<CommandRiskCategory>,
    pub summary: String,
}

pub enum CommandRiskVerdict {
    Safe,
    Risky,
    Unknown,
}

pub enum CommandRiskCategory {
    DestructiveFileSystem,
    CredentialOrSecretAccess,
    SystemConfiguration,
    PrivilegeOrSandboxBypass,
    RemoteCodeExecution,
    ProcessOrServiceControl,
    Unknown,
}
```

约束：

- 必须使用 provider 的结构化输出能力或严格 schema 解析，禁止从自然语言中猜测 verdict。
- 请求只暴露 `CommandRiskAssessment` 一个 tool、关闭 parallel tool calls，并明确要求 arguments 是符合 schema 的 JSON object；provider 不支持 named tool choice 时允许使用 `Auto`，但返回结果仍必须经过“恰好一个同名 tool call + 严格对象 schema”校验。
- 输入只包含 bounded command、cwd、parsed command facts 和当前 effective permission summary；不注入完整环境变量、文件内容、历史对话或无界日志。
- `summary` 只用于审批展示和 audit，不能作为权限判断条件。
- `Safe` 表示模型没有识别到需要用户确认的命令风险；当确定性权限已经允许时，Runtime 直接继续 sandbox evaluate 和执行，不生成审批。
- `Risky` 表示模型识别到危险，Runtime 必须生成用户审批；`Unknown` 和任何模型/schema failure 同样按审批处理。
- risk assessor 不返回具体 approval decision，不构造 `ToolApprovalGrant`。
- 首期不建立跨 Session 风险缓存、规则学习或后台复审服务。
- 明确安全和明确危险的命令不调用模型，避免无必要延迟和成本。

## 10. Sandbox 审批场景

首期审批原因只有以下四类：

```rust
pub enum SandboxApprovalReason {
    NetworkAccess,
    FileSystemAccessOutsideWorkspace,
    SandboxBypass,
    RiskyCommand,
}
```

路径级审批在 `talos-tools-sandbox` 内使用一套由 request 和 grant 共同引用的对象：

```rust
pub struct RequestedFileSystemAccess {
    pub operation: FileSystemOperation,
    pub paths: Vec<AbsolutePathBuf>,
}
```

`RequestedToolPermissions` 和 `ToolApprovalGrant` 增加 bounded `file_system_access`。路径必须在 tool request 构造时完成绝对化、归一化、去重和数量上限校验；grant 只能覆盖原 request 中的 path/operation。Service 将该对象一次性投影为 public `SandboxApprovalPresentation::FileSystem`，不得在 Gateway/客户端再次解析。Bash 无法可靠预测全部路径时不伪造 path scope，继续使用 invocation-bound capability，并在 presentation 中明确实际授权范围。

不为 provider failure、unsupported capability、setup error、非法 request 或 governance hard deny 创建审批。用户批准不能修复平台能力问题，也不能覆盖组织治理。

展示对象只包含当前场景需要的信息：

```rust
pub enum SandboxApprovalPresentation {
    Command {
        command: String,
        cwd: String,
        risk_summary: Option<String>,
    },
    FileSystem {
        operation: SandboxFileOperation,
        paths: Vec<String>,
    },
    Network {
        scope: String,
    },
}
```

`SandboxApprovalReason` 只用于稳定分类，所有用户可见细节只在 bounded presentation 中出现，避免 reason 和 presentation 重复字段。presentation 是产品 DTO，不暴露内部 `RequestedToolPermissions`、`CommandRiskAssessment` 或 SPI path 类型。

Reason 必须按实际 resolver 决策顺序生成：Sandbox bypass、workspace-external filesystem、network escalation、command risk。存在普通 `CommandRisk::Safe` 事实不能产生 `RiskyCommand`；客户端不得重新计算 reason。

如果当前 enforcement 只能做到本次命令全网络开关，UI 必须显示“允许本次命令访问网络”，不能声称只授权某个 host。展示范围必须与实际 grant/enforcement 一致。

## 11. 审批决策与 grant

所有 Sandbox 审批首期固定提供：

```text
AllowOnce
Deny
Cancel
```

语义：

- `AllowOnce`: 只批准当前 `action_id + call_id + requested permissions + operations`，生成一次性 `ToolApprovalGrant` 并重新 evaluate 原请求。
- `Deny`: 原操作不执行，生成结构化 tool rejection，模型可以在同一个 Turn 中选择其他方案。
- `Cancel`: 原操作不执行，并取消当前 Turn。

安全约束：

- 客户端提交的 decision 必须包含在该 request 的 `available_decisions` 中。
- approval response 通过 envelope 的 Session、Turn 和 action ID 匹配持久化 action；call ID 从服务端 action binding 读取，客户端不重复提交。重复、过期或跨调用 response 明确拒绝。
- grant 只能覆盖原 requested permissions，不能由客户端提交新的 path、network 或 operation。
- 文件工具应按本次请求涉及的 path/root 生成 scoped grant，不能因为单个 workspace 外文件自动授予全文件系统。
- Bash 无法可靠预测全部副作用时，`AllowOnce` 可以授予当前命令所请求的完整一次性 capability，但 UI 必须明确展示实际范围。
- approval 完成、拒绝、取消、Turn interrupt 或 Session close 后，pending response token 和一次性 grant 必须失效。

## 12. 统一 PendingUserAction

复用现有 ActiveTurn/Timeline 主干，不新增 `QuestionManager`、`ApprovalManager`、通用 form engine、consumer registry 或全局 responder map。除各 variant 自己的参数和响应类型外，共享生命周期只包含四个直接表达含义的对象：

```rust
pub struct DynamicToolResultParams {
    pub tool_name: String,
}

pub struct DynamicToolResult {
    pub output: String,
    pub success: bool,
}

pub struct PendingUserAction {
    pub action_id: String,
    pub call_id: String,
    pub item_id: Option<String>,
    pub state: UserActionState,
    pub params: UserActionParams,
}

pub enum UserActionParams {
    DynamicToolResult(DynamicToolResultParams),
    Question(QuestionParams),
    SandboxApproval(SandboxApprovalParams),
}

pub enum UserActionResponse {
    DynamicToolResult(DynamicToolResult),
    Question(QuestionAnswer),
    SandboxApproval(SandboxApprovalDecision),
}

pub enum UserActionState {
    Pending,
    ResponseAccepted,
    Resuming,
    ResumeFailed,
    ExecutionUnknown,
    Resolved,
    Cancelled,
}
```

不定义 `UserActionPayload`、`UserActionResponseValue`、`UserActionStateEvent`、`UserActionConsumer`、`UserActionManager` 或其他同义包装。`UserActionParams` 和 `UserActionResponse` 是强类型 tagged union；各 variant 保持自己的校验、恢复和结果语义。

### 12.1 状态模块

状态规则属于 `talos-service::user_action::state`，保持为独立、无 I/O 的模块：

```rust
fn validate_transition(
    current: UserActionState,
    next: UserActionState,
) -> Result<(), InvalidUserActionTransition>;
```

允许的状态边只有：

```text
Pending          -> ResponseAccepted | Cancelled
ResponseAccepted -> Resuming | Cancelled
Resuming         -> Resolved | Cancelled | ResponseAccepted | ResumeFailed | ExecutionUnknown
ResumeFailed     -> terminal
ExecutionUnknown -> terminal
Resolved         -> terminal
Cancelled        -> terminal
```

`Resuming -> ResponseAccepted` 只允许用于“已确认 dispatch 未开始且仍有 bounded retry”的场景。该模块不读取数据库、不发布 Timeline、不调用 Kernel、LLM 或 Sandbox。所有迁移使用穷尽匹配；首期不引入 FSM crate，也不编写通用状态机框架。

### 12.2 Store 边界

user action 持久化必须在现有独立 Store crate 中实现，目标模块为 `talos-store::session::user_action`。该模块定义唯一的数据库 Record 和 Store API；schema 变更接入 `talos-store` 现有统一 migration 入口：

```rust
pub struct UserActionRecord {
    pub session_id: String,
    pub turn_id: String,
    pub action: PendingUserAction,
    pub response: Option<UserActionResponse>,
    pub version: u64,
    pub updated_at: i64,
}

pub trait UserActionStore: Send + Sync {
    fn create(
        &self,
        record: UserActionRecord,
    ) -> impl Future<Output = TalosResult<UserActionRecord>> + Send;

    fn get(
        &self,
        session_id: &str,
        turn_id: &str,
        action_id: &str,
    ) -> impl Future<Output = TalosResult<Option<UserActionRecord>>> + Send;

    fn compare_and_set(
        &self,
        expected: &UserActionRecord,
        next: UserActionRecord,
    ) -> impl Future<Output = TalosResult<UserActionRecord>> + Send;
}
```

以上签名表示 Store 的最小能力，不要求新增 manager、registry 或 repository 包装层。具体异步 future、错误类型和 transaction 使用 `talos-store` 现有模式。

约束：

- `UserActionRecord` 只属于 Store 边界，不能导出到 Gateway、SDK、Timeline 或 UI。
- schema、migration、SQL、序列化、查询、CAS 和 DB transaction 只能由 `talos-store` crate 实现；user action 查询和写入逻辑收口在 `session::user_action`，Service 不得直接访问 database handle 或拼接 SQL。
- Store 通过 `session_id + turn_id + action_id + state + version` 执行原子 compare-and-set，并返回冲突或最新 Record；它不决定某条业务状态边是否允许。
- `talos-service::user_action` 先调用 `validate_transition()`，再调用 `UserActionStore::compare_and_set()`；不能在 Service 内重复定义 Record、维护内存状态或实现第二套持久化逻辑。
- ActiveTurnStore 只保存 `WaitingUserAction + current_action_id`。完整 params、response、state 和 version 只保存在 UserActionStore。
- DB 启动 migration 直接删除 `active_turns.pending_user_action_json` 及其中的历史 payload，并清理依赖该旧列、无法按新模型恢复的遗留 waiting state。旧数据不转换为 `UserActionRecord`，升级前尚未完成的 legacy Dynamic Tool 输入不保证恢复。
- migration 完成后只允许 `UserActionRecord + current_action_id` 新链路读写等待状态；禁止保留旧列、双读、双写或 legacy fallback。
- `UserActionStore::create()` 必须按 `session_id + turn_id + action_id` 幂等；相同 key 但内容不同返回 conflict。Record 创建成功但 ActiveTurn 更新失败时，不发布 `UserActionRequested`；该 Record 尚未激活，重试复用同一 key。首期不新增跨 Store transaction coordinator。
- terminal Record 按现有 idempotency/Timeline retention 生命周期保留，使重复 response 能返回既有状态。`version` 只用于 Store CAS，不由客户端设置。
- `SubmitUserActionResponse.state` 返回响应完成时的真实 durable state。并发提交同一 response 时，唯一 owner 继续 resume；幂等调用可能观察到 `ResponseAccepted` 或后续状态，客户端不得把提交成功硬编码为 `Resuming`。

### 12.3 生命周期

```text
Tool returns typed pending outcome
  -> AgentKernel persists continuation facts
  -> UserActionStore creates UserActionRecord(Pending)
  -> ActiveTurnStore records WaitingUserAction + current_action_id
  -> Timeline persists UserActionRequested, then pushes it to clients
  -> client submits typed UserActionResponse
  -> talos-service::user_action validates identity, variant and transition
  -> UserActionStore CAS Pending -> ResponseAccepted and stores response
  -> UserActionStore CAS ResponseAccepted -> Resuming
  -> Service directly dispatches the response to the variant-specific resume path
  -> persist tool/execution outcome
  -> UserActionStore writes Resolved/Cancelled/ResumeFailed/ExecutionUnknown
  -> Timeline persists UserActionFinished, then pushes it to clients
  -> only then clear current_action_id
```

状态约束：

- 同一 `action_id` 重复提交相同 response 返回既有 accepted 状态；不同 response 返回 conflict。只有一次 CAS 可以从 `Pending` 离开。
- `Question` 在成功 tool result 持久化后进入 `Resolved`；`Deny` 在 tool rejection 持久化后进入 `Resolved`；`Cancel` 在 Turn cancellation 持久化后进入 `Cancelled`。
- dispatch 明确未开始且仍有 bounded retry 时，状态从 `Resuming` 回到 `ResponseAccepted`，继续使用已保存 response，不重新询问用户；重试耗尽后进入 terminal `ResumeFailed` 并使 Turn failed。
- process 已启动或文件 mutation 可能已发生后失去确认，进入 `ExecutionUnknown`，终止当前 Turn 并向用户显示明确错误；不得自动重新执行。
- host 在 `Resuming` 时崩溃，无法证明 dispatch 是否开始，恢复时直接进入 `ExecutionUnknown`；首期不自动重放。
- 当前实现“先清 pending，再调用 resume”的顺序必须移除。Store 中的 response、state 和 version 是恢复事实源，进程内 channel 不是。
- 正常路径由 Service 在 durable acceptance 后直接 dispatch；重启恢复路径只继续 `ResponseAccepted`，`ResumeFailed` 是 terminal，不新增后台轮询器或消息队列。
- 本 Spec 保证 accepted response 最多触发一次可能开始副作用的 execution dispatch；明确未开始的 dispatch failure 可以 bounded retry。对于 host crash 后不可观测的外部副作用，只能报告 `ExecutionUnknown`，不虚假承诺 exactly-once execution。

`ToolDispatchOutcome::Pending` 已经是通用外形，但 `ToolPending::Dynamic`、`PendingDynamicToolRequest`、`WaitingDynamicInput` 和 `GeneralTurnResume::DynamicToolResult` 仍绑定 dynamic tool。实现时只泛化真正进入共享主干的对象，不复制新的 parallel outcome/resume framework。

首期 ActiveTurn 对外仍只暴露一个当前 `PendingUserAction`。工具执行模式在每个 Turn 开始或恢复时从 effective Session permission mode 和配置的 `tool_parallelism` 派生：`AskForApproval` 与 `ApproveForMe` 始终使用 `Sequential`，`FullAccess` 仅在 `tool_parallelism > 1` 时使用 `Parallel`。同一 Turn 内该运行配置保持不变，Session mode 只能在没有 active Turn 时修改。

非 Full Access Turn 即使模型返回多个 tool call，也必须按模型顺序执行。一个 batch 出现 pending 后不得启动后续调用，未执行调用由原 Turn continuation 保留；当前 action 完成后才允许发布下一个 action。Kernel 内部 pending 表示必须与单一 `current_action_id` 一致，检测到多个 pending 时应明确失败，不能发布多个事件后由 Service 或客户端选择。本 Spec 不实现 approval queue 或任意并行表单系统。

continuation 必须自包含模型原始整批 `tool_calls`、已完成前缀的 `tool_messages` 和尚未执行的 `deferred_tool_calls`。每次 resume 只执行当前调用及 deferred 后缀；如果再次 pending，必须累计已完成的 tool message 并原样携带整批 tool call。最后恢复模型时，provider context 必须包含一条原始 assistant tool-call message，以及按原始顺序排列且一一对应的全部 tool result message。Kernel 不得从 Timeline、历史摘要或客户端状态反推这些协议事实，否则连续审批后模型可能重复调用已经完成的工具。

## 13. AskUserQuestion 目标行为与清理

目标调用：

```text
Model calls AskUserQuestion
  -> validate one question and 2-3 mutually exclusive options
  -> ToolDispatchOutcome::Pending(Question)
  -> WaitingUserAction
  -> UI adds Other free-form choice
  -> user submits selected option or free-form text
  -> resume original AskUserQuestion call
  -> QuestionAnswer becomes successful tool result
  -> model receives the answer and continues the same Turn
```

当前 `Tool::execute_payload()` 只能返回 `ToolOutputPayload`，`LocalToolHandler` 会无条件包装为 `Executed`。首期只增加一个默认兼容扩展点，不修改每个现有 tool：

```rust
trait Tool {
    async fn execute_payload(
        &self,
        input: Value,
        context: ToolContext,
    ) -> TalosResult<ToolOutputPayload>;

    async fn execute_outcome(
        &self,
        input: Value,
        context: ToolContext,
    ) -> TalosResult<ToolDispatchOutcome> {
        self.execute_payload(input, context)
            .await
            .map(ToolDispatchOutcome::executed)
    }
}
```

`LocalToolHandler` 统一调用 `execute_outcome()`。`AskUserQuestion` 覆盖该方法：首次调用没有 accepted answer 时返回 typed pending；恢复调用从 `ToolContext` 读取与当前 call ID 绑定的 typed `QuestionAnswer`，再由 `execute_payload()` 生成 `QuestionToolResult`。因此现有 tools 继续只实现 `execute_payload()`，Question 也不需要 unreachable/dead payload 方法。禁止在 handler/registry 中按 `AskUserQuestion` 名称特判，也禁止把 Question 伪装成 Dynamic Tool。

当前实现返回 `awaiting_user_response: true` 的 success JSON，导致 Timeline 把 `AskUserQuestion` 标记为 `Completed`，同时只在展开详情中显示 `status: pending`。这是错误的生命周期，不是可通过增加一个 JSON renderer 修复的展示问题。目标实现必须满足：

- 发布 Question 后，原 tool call 保持未完成，Turn 进入 `WaitingUserAction`，用户可见状态为 `Waiting for input`。
- Pending Question 不能产生成功 tool result，也不能显示为 `Completed`。
- 用户选择 option 或提交 Other 文本后，Runtime 接受 `QuestionAnswer`、恢复原 tool call，并把回答生成成功 tool result。
- 只有 tool result 已写入且 Turn 已恢复后，Question tool call 才转为 `Completed`。
- Question 的 raw arguments 和协议 payload 可以保留在折叠 diagnostics 中，但不能充当主要用户交互，也不能要求用户从 JSON 中理解或提交答案。

目标 `AskUserQuestion` 一次只提出一个问题。首期不复制 Codex 的多问题表单和 auto-resolution；需要多个问题时，模型可以在收到上一个回答后再次调用工具。

新链路完成时必须删除：

- `SessionTaskState.latest_question`。
- `TaskStateRuntime::ask_question()`。
- `TaskStateRuntime::latest_question()`。
- `TaskStateRuntime::submit_question_answer()`。
- `StructuredQuestionStatus` 和 `StructuredQuestionRecord`。
- `allows_free_form` tool input 字段；Other 是首期统一产品行为。
- `awaiting_user_response: true` 的立即成功结果。
- 只验证进程内记录的旧测试和旧 E2E 断言。

保留并迁移：

- `AskUserQuestionTool` 名称、可见性和 `requires_user_interaction` 标记。
- header/question/options schema 和 2-3 options 校验。
- option id/label/description 的非空和去重校验。
- 原有 Dynamic Tool continuation 的持久化、幂等和 resume 基础设施。
- `TaskStateRuntime` 中与 Todo 和 Plan Mode 相关的现有职责；本 Spec 不顺手重构这些能力。

Question 成功结果使用稳定结构，不返回任意字符串：

```rust
pub struct QuestionToolResult {
    pub question: String,
    pub answer: QuestionToolResultAnswer,
}

pub enum QuestionToolResultAnswer {
    SelectedOption { option_id: String, label: String },
    FreeForm { text: String },
}
```

该结果只包含模型继续推理需要的 question 和用户答案，不注入 user action/store 状态。`action_id` 只用于 Runtime 幂等和 Timeline 关联。

## 14. 协议、推送与恢复

`talos-types::PendingUserAction` 是唯一公开 user action 对象，Rust/TS schema 必须同步生成。Service 通过 `UserActionStore` 读取 Record，并只把其中的 canonical `PendingUserAction` 放入 Active Turn snapshot；`UserActionRecord` 本身不会越过 Store/Service 边界。刷新和断线重连据此恢复 waiting/submitting/resume-failed/execution-unknown UI，客户端不从本地状态猜测服务端阶段。

Timeline 直接引用 canonical `PendingUserAction`，不复制 `TimelineUserActionParams` 等同构 DTO。首期只增加两个直接事件：

```rust
pub enum TimelineUserActionEvent {
    Requested { action: PendingUserAction },
    Finished {
        action_id: String,
        state: UserActionState,
        response: Option<UserActionResponse>,
    },
}
```

`Requested` 使客户端定位并渲染等待中的 panel；`Finished` 保存 bounded、只读 terminal history。Question 和 SandboxApproval 写入 accepted response，使刷新后仍能展示最终答案或审批决定；DynamicToolResult 的 `response` 固定为 `None`，完整自由文本 output 继续由既有 tool result 保存。执行错误继续使用既有 tool/Turn error，不新增通用 outcome/error 包装。

统一 application service 入口遵循 Request/Response 命名，不使用 `Payload`、`Value` 包装，也不把所有 variant 退化成 `output: String`：

```rust
pub struct SubmitUserActionRequest {
    pub session_id: String,
    pub turn_id: String,
    pub action_id: String,
    pub response: UserActionResponse,
}

pub struct SubmitUserActionResponse {
    pub action_id: String,
    pub state: UserActionState,
}
```

App Protocol 方法使用 `userAction/submit`。HTTP、JSON-RPC 和 Embedded SDK 可以保留各自 transport envelope，但必须直接引用 canonical `UserActionResponse`，不能复制 Question/Approval DTO，也不能退化成 `output: String`。

Service 验证：

- 通过 `UserActionStore::get()` 按 Session、Turn 和 action ID 读取 Record；Service 不直接查询数据库。
- Record 为 `Pending` 时，ActiveTurn 必须是 `WaitingUserAction` 且 `current_action_id` 匹配；terminal/accepted Record 的幂等查询不依赖 ActiveTurn 仍处于等待态。
- response variant 与 params variant 匹配。
- Question option ID 存在，free-form 非空且有长度上限。
- Sandbox decision 在 `available_decisions` 内。
- 同一 action 的相同 response 重复提交返回当前 server state；不同 response 返回 conflict，不能恢复两次。
- 只有 `Pending` 接受新 response。`ResponseAccepted/Resuming` 只返回当前状态；`ResumeFailed/Resolved/Cancelled/ExecutionUnknown` 返回 terminal 状态，客户端提交 response 不触发重新 dispatch。

### 14.1 Push + Pull Recovery

客户端通知复用现有 Timeline 混合链路，不新增 UserAction channel、event bus 或消息队列：

```text
UserActionStore persists state
  -> TimelineStore appends event
  -> Tokio broadcast publishes live event
  -> per-subscription bounded Tokio mpsc
  -> HTTP: Axum SSE
     Desktop: app-server JSON-RPC notification over stdio
     TUI: Embedded SDK mpsc receiver
```

服务端必须先持久化 PendingUserAction 和 Timeline event，再发布 live event。Push 只负责低延迟通知，不是事实源；发送失败、客户端断开或订阅 lagged 不影响已持久化的等待状态。

Pull 只用于首次进入和恢复：HTTP/SDK 使用现有 `timeline.recover`，返回 Timeline replay 和 ActiveTurn 的当前 `PendingUserAction` 投影；订阅通过 event ID cursor 从恢复位置继续。WebUI、Desktop 和 TUI 不轮询 user action endpoint，也不维护客户端私有事实源。

客户端响应通过 `userAction/submit` request 提交，不通过 Timeline 反向推送。Service 在 response 已由 UserActionStore durable CAS 后返回 accepted state，并直接调用类型专属恢复路径；后续模型输出继续通过既有 Timeline stream 推送。进程重启时，Service 从 UserActionStore 读取已接受且允许重试的 Record 恢复，不新增后台 polling worker。

现有 `SubmitDynamicToolResultRequest { output: String }` 可以继续服务外部 Dynamic Tool 的兼容入口，但内部应转换成同一 `UserActionResponse::DynamicToolResult`；Question 和 SandboxApproval 不经过该自由字符串 API。

## 15. WebUI 与 Desktop

WebUI/Desktop 只共享一个 `UserActionPanel` 外壳，负责 waiting、submitting/resuming、resume-failed、execution-unknown、resolved 和 recovered 状态、禁用重复提交以及 Timeline 定位。`QuestionPanel` 和 `SandboxApprovalPanel` 分别渲染强类型 params，不引入通用 JSON form renderer。

### 15.1 Session mode

Chat composer 工具栏靠近 model selector 显示：

```text
[ Model: GPT-5 ] [ Permissions: Ask for approval v ]
```

下拉菜单固定三项并显示简短语义。选择写入 Session override；active Turn 期间控件 disabled。切换到 `FullAccess` 需要明确确认，但不改变全局配置。

Desktop 复用同一个 WebUI 组件、SDK DTO 和 response API，不新增 Desktop 私有 permission state。

Settings Permissions 中未接后端的 safe read/write 本地开关已删除。Session mode 是产品唯一权限入口，不能再增加冲突的全局本地开关。

### 15.2 Sandbox approval

审批显示在对应 tool call 的 Timeline 位置，并在 composer 上方保留当前 blocking user action 入口。按 presentation variant 展示 command/cwd、filesystem operation/paths 或 network scope。

Inline panel 只按服务端 `PendingUserAction.item_id` 关联对应 Timeline item。`item_id` 缺失时客户端仍可展示 blocking action 入口，但不得用 `call_id` 推断位置或创建 `pending-<call_id>` 伪 item。

按钮完全来自服务端：

```text
[Allow once] [Deny] [Cancel]
```

没有 Other、文本输入、权限复选框或客户端编辑 requested permissions。提交后 panel 变为只读 history，记录 approved once、denied 或 cancelled。

### 15.3 Question

Question 必须使用专用交互 panel，不能交给通用 `ToolResultRenderer` 显示 pending JSON：

- panel 位于对应 `AskUserQuestion` tool call 的 Timeline 位置，直接展示 header、question 和模型提供的 2-3 个选项。
- 客户端固定追加 `Other`。选择 Other 后显示 bounded text input 和明确提交操作；空文本不能提交。
- 选择普通 option 或提交 Other 后，panel 进入 submitting 状态，阻止重复提交；服务端接受后自动恢复同一个 Turn，不要求用户再发送一条普通消息。
- 等待回答时 tool 状态显示 `Waiting for input`，不能显示 `Completed`；主 composer 不接受与当前 Question 无关的普通发送。
- 主 composer 被阻塞时仍保留现有 Cancel Turn 控件。取消 Turn 是 Runtime control，不伪造成 Question option 或 free-form answer。
- transport failure 或服务端尚未接受 response 时，保留问题和用户当前选择并允许重试同一 request；stale/conflict 不得伪装成功。服务端已接受 response 后若恢复进入 terminal `ResumeFailed`，客户端只能展示失败并结束当前 Turn，不能通过重复提交再次 dispatch。
- 提交成功后 panel 变成只读问题/回答 history，显示实际选择的 label 或自定义文本；回答经原 tool result 进入模型上下文，随后继续显示模型后续输出。
- 页面刷新、断线重连和 Desktop 恢复必须从 ActiveTurn snapshot 重建同一可交互 panel，而不是退化成 raw JSON details。

## 16. TUI

### 16.1 Session mode

- 欢迎卡片和状态栏显示服务端返回的 effective mode，删除写死的 `YOLO mode`。
- `/permissions` 打开标题固定为 `Choose what Talos is allowed to do` 的三项选择 overlay。
- active Turn 期间不能切换模式。
- 选择 `FullAccess` 需要确认。

### 16.2 Sandbox approval

使用 blocking bottom-pane overlay，展示服务端 presentation 和固定选项：

```text
Approval required

Command:
  cargo install cargo-nextest

Requested access:
  Network access for this command

> 1. Allow once
  2. Deny
  3. Cancel
```

数字键、方向键和 Enter 提交；Esc 只能映射为 `Cancel` 或保持等待，绝不能隐式批准。

### 16.3 Question

Question overlay 展示 2-3 个模型选项和客户端追加的 Other。选择 Other 后复用现有 composer 的 plain-text 输入能力，但不启用 slash command、附件或其他主 composer 行为。Esc 不提交答案并保持 pending；用户需要结束时使用现有 Cancel Turn control。

TUI 和 WebUI 不各自实现 permission/risk mapping，snapshot 恢复后必须根据同一 PendingUserAction 重建等价 UI。

## 17. 失败、取消与恢复

- `Deny` 产生结构化 permission rejection，原操作没有副作用，模型可以继续当前 Turn。
- `Cancel` 取消当前 Turn，完成当前 user action 并清理一次性 grant。
- Turn interrupt、Session close、客户端断开和 runtime shutdown 都不能自动批准。
- 客户端断开时 UserActionRecord 保持在 UserActionStore，ActiveTurnStore 保留 `current_action_id`；重连通过 recovery snapshot 恢复。
- provider unsupported/setup/execution failure 是 Sandbox error，不转成 approval，也不自动切换 Noop。
- risk assessor failure 在 `ApproveForMe` 下转成审批，不转成 Safe，也不直接失败整个 Turn。
- Question response 无效时保持 pending，返回 validation error；不能把空答案作为成功 tool result。
- response accepted 后发生 crash 时，恢复必须依赖 UserActionStore 中的 response/state/version。`ResponseAccepted` 可以继续同一 accepted response；`Resuming` 恢复为 `ExecutionUnknown`，`ResumeFailed` 保持 terminal。不能仅依赖进程内 channel 或全局 map。
- 客户端只可重试尚未被服务端接受的同一 submit request。`ResumeFailed` 和 `ExecutionUnknown` 都是 terminal，必须结束当前 Turn 并保留可审计结果；不能重新开放原 panel 或重放原调用。

## 18. 关键不变量

1. 模型可以提出语义问题，但不能生成 Sandbox 审批选项。
2. 客户端可以选择服务端给出的审批决策，但不能提交新的权限范围。
3. Bash 风险模型返回风险事实：`Safe` 可让已经通过确定性权限检查的调用继续执行，`Risky`/`Unknown` 触发审批；它不能授予权限、拒绝 hard policy 或关闭 Sandbox。
4. `FullAccess` 不经过审批和风险模型；restricted mode 不能因 provider failure 自动进入 Full Access。
5. Question answer 进入模型上下文；Sandbox approval 进入 permission resolver。两者不互换。
6. 所有批准必须绑定原 invocation 并在执行前重新 evaluate。
7. UserActionStore 中的持久化状态是 UI/recovery 的事实源；React/TUI 本地状态不是事实源。
8. 新链路完成后只能存在一套 question pending/resume 实现。
9. Question、SandboxApproval 和 DynamicToolResult 共享 action identity、状态转换、CAS、Timeline 和 recovery；params validation、UI body、response dispatch 和结果语义保持类型专属。

## 19. 影响范围

- `talos-rs/crates/action/tools`: `AskUserQuestion`、Bash facts、typed `ToolPending`。
- `talos-rs/crates/action/tools-sandbox`: risk facts consumption、approval request/grant validation。
- `talos-rs/crates/orchestration/agent-kernel`: generic user action pending outcome、tool continuation 和 resume。
- `talos-rs/crates/orchestration/service`: Session mode，以及独立 `user_action` 模块中的 response validation、状态转换和 resume 编排。
- `talos-rs/crates/orchestration/store`: 独立 `session::user_action` 模块、`UserActionRecord`、`UserActionStore` 和 CAS；schema 变更接入现有 migration 入口。
- `talos-rs/crates/common/types` 与 `gateway/app-protocol`: Session、Timeline、response DTO/schema。
- Rust/TypeScript SDK: mode read/update、PendingUserAction 和 response types。
- `talos-app/web`: Session mode、approval/question panels、recovery。
- `talos-app/desktop`: 复用 WebUI 和 app-server request guard。
- `talos-rs/apps/tui`: permissions selector、approval/question overlay、recovery。
- `talos-e2e`: AskUserQuestion 真实回答/恢复和 Sandbox approval 产品链验收。

## 20. 验证策略

### 20.1 功能测试

- 三种 Session mode 的默认值、override、读取和空闲期修改。
- `AskForApproval` 不调用风险模型，并对 protected/ambiguous Bash 请求产生审批。
- `ApproveForMe` 只对 ambiguous Bash 调用风险模型；`Safe` 在确定性权限允许后直接执行，`Risky`/`Unknown`/error 进入审批。
- `FullAccess` 不调用风险模型、不产生审批并走 Noop。
- deterministic deny/approval 不能被模型 Safe 覆盖。
- `AllowOnce` grant 绑定 action/call/permissions/operation/path scope，重复或跨调用提交失败。
- `Deny` 返回 tool rejection 并继续；`Cancel` 取消 Turn。
- Question selected option 和 Other free-form 都能恢复原 tool call 并进入下一轮模型上下文。
- Question pending 期间 tool call 为 `Waiting for input` 而非 `Completed`，且不存在 success JSON 伪 pending 状态。
- 无效 option、空 free-form、错误 response variant 和 stale action ID 保持安全失败。
- CAS lifecycle 覆盖相同 response 幂等、不同 response conflict、resume failure 和 execution unknown 且不自动重复副作用。
- 状态测试穷尽允许边和禁止边，并覆盖 `Resuming` 崩溃恢复为 `ExecutionUnknown`、明确未 dispatch 时回到 `ResponseAccepted`。
- UserActionStore 功能测试覆盖 create/get/CAS、version conflict、response 持久化和 terminal Record 幂等读取；Service 测试通过 mock Store 验证状态规则，不连接数据库。
- DB migration 测试从包含历史 `pending_user_action_json` 数据的旧 schema 启动，验证旧列和无法恢复的 legacy waiting state 已删除，并验证迁移后的新 `UserActionStore` 可以正常创建和读取 Record。
- Session effective mode 只由 Service 解析一次并沿 AgentRunConfig/ToolContext 注入，后续层不存在第二套 fallback。
- 旧 question global state 和伪等待 API 删除后不存在第二条生产路径。

### 20.2 协议与恢复测试

- Rust/TS generated schema 对 `Question`、`SandboxApproval` 和 response tagged union 一致。
- active turn snapshot 能恢复 `Question` 和 `SandboxApproval` 两种新增 `PendingUserAction` variant。
- WebUI/Desktop/TUI 断线重连后显示同一问题或审批，不重复提交。
- accepted response 只离开 `Pending` 一次，并最多触发一次可能开始副作用的 dispatch；Timeline 与既有 tool/Turn history 能恢复最终 decision/answer/error。

### 20.3 产品 E2E

- WebUI 和 TUI 分别选择 Session mode，并验证后端 effective mode。
- `ApproveForMe` 中结构化模型判断危险 Bash 后出现固定审批选项，AllowOnce 后原命令只执行一次。
- `AskForApproval` 中 permission escalation 直接出现审批，不调用 risk assessor。
- Full Access 同一命令不出现审批且 audit 为 no isolation。
- `AskUserQuestion` 展示 2-3 个选项和 Other；两种回答都进入模型 tool result 并继续 Turn。
- WebUI/Desktop 中 pending Question 直接显示可交互 panel，不显示 `Completed` 或要求用户展开 raw JSON；刷新后仍可回答。
- Deny 无副作用且模型继续；Cancel 不执行命令并结束 Turn。

## 21. Open risks 与后续能力

1. Bash 风险判断依赖模型输出质量。首期通过 deterministic precedence、strict schema 和 unknown/error -> approval 限制风险，不承诺模型能证明绝对安全。
2. Bash 一次性 Full Access grant 的实际范围可能大于命令展示的显式路径，UI 和 audit 必须诚实描述；更细粒度 execve/path/network enforcement 属于后续 Spec。
3. Host crash 发生在 side effect 可能开始之后时无法证明执行结果；首期明确进入 `ExecutionUnknown` 并禁止自动重试，不承诺 exactly-once 外部副作用。
4. ActiveTurn 当前只引用一个 `current_action_id`；首期串行发布，未来如果需要并行审批队列，应由独立需求扩展 schema 和恢复语义。
5. `AllowForSession`、exec policy amendment、host network policy 和 persisted approval cache 参考 Codex，但首期不暴露半成品 API。
6. Windows provider 继续按 `feat-036` 返回明确 unsupported；本 Spec 只要求产品 UI 正确呈现错误，不实现 Windows enforcement。
