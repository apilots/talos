# 统一 Tool Sandbox 设计

Status: draft
Owner: agent
Created: 2026-08-06
Updated: 2026-08-14
Related: `archive/2026/refactor-025-talos-tool-capability-alignment`, `feat-035-mcp-server-management-reliability`

## 背景与问题

Talos 当前已经有 `talos-tools-sandbox` crate，并且配置层已经暴露 `tools.sandbox_policy.default_mode`。工具注册和编排层也能根据工具 governance 计算 `ToolExecutionPlan`。但当前实现仍停留在“计划”阶段：

1. `RegistryOrchestrator::resolve_pending()` 计算出 `ToolExecutionPlan` 后没有把 plan 传入 runtime 执行。
2. `BashCommandRequest` 暴露 `sandbox_permissions`，但 bash runtime 仍直接构造 `tokio::process::Command` 并 `spawn()`。
3. `Read`、`Write`、`Edit`、`Glob`、`Grep` 等文件工具直接使用本地路径解析、`tokio::fs` 或本地进程，没有统一文件系统沙箱边界。
4. 现有 `ToolSandboxMode` 只有模式枚举，缺少用户权限模式、审批策略、文件系统、网络、workspace roots、平台能力和 platform provider 的统一领域模型。
5. Linux 和 macOS 可以较早实现 OS sandbox；Windows 暂不实现，但如果抽象中不保留 Windows 平台语义，后续会被 Unix-only 假设绑住。

本设计的目标是把 sandbox 从 bash 专属字段提升为所有本地 tool runtime 共享的新增执行能力。它应作为独立 sandbox library，通过标准执行接口接入现有 tool runtime，而不是散落在某个工具内部的条件分支。这里的“可接入”指 sandbox capability 可以由用户权限模式解析后开启或关闭：受限模式进入强制隔离路径，Full Access 明确关闭隔离并走无沙箱路径。它不是用户可安装的 plugin 机制，也不是让调用方动态拼装 provider。第一版先建立统一抽象和可测试的数据流，再逐步接入 macOS/Linux provider 和文件工具。Windows 暂不落地隔离执行，但必须作为一等平台进入配置、能力探测和错误语义。

## 设计结论

1. `talos-tools-sandbox` 是唯一面向 Talos runtime 的 public facade 和架构 owner。它内部包含两个内聚业务：Talos permission decision，以及跨平台 sandbox execution facade。
2. 用户可见的权限模式不能直接等同于底层 sandbox mode。用户模式先解析为 `ApprovalPolicy`、`ToolPermissionProfile` 和 `SandboxCapability`，runtime 执行只消费解析后的 effective policy。
3. 新增内部 crate `talos-sandbox-spi`，定义平台 provider 必须实现的标准隔离执行协议。SPI 只包含归一化 execution request/result、capability 和 provider error，不包含任何 tools-domain 权限对象。
4. 新增 `talos-macos-sandbox`、`talos-linux-sandbox` 和 `talos-windows-sandbox`。三个 crate 只依赖 `talos-sandbox-spi`，分别完成本平台隔离机制和独立平台测试，不依赖 `talos-tools-sandbox`。
5. Talos runtime 通过 `ToolSandbox` 的统一 API 获取 permission decision、validated permit、进程执行、受控文件系统和 diagnostics，不直接构造 `ToolSandboxContext`、SPI request 或选择 provider。
6. `RegistryOrchestrator` 只负责 governance constraint 和 plan，不直接合并最终权限或拼平台命令。`talos-tools-sandbox` 内的 permission engine 解析 effective policy，`SandboxExecutionCompiler` 再把 validated context 编译成 SPI `SandboxExecutionRequest`。
7. Bash、grep 这类进程型工具通过 permit 绑定的统一进程 API 启动子进程，不在各 runtime 中写平台分支。
8. 文件工具必须通过 permit 绑定的 `ToolFileSystemHandle` 访问文件系统，禁止 runtime 直接传递或伪造 sandbox context，也禁止继续裸用 `tokio::fs` 承载核心读写语义。
9. 第一阶段文件工具可使用进程内 path authorizer enforce workspace-write/read-only；需要 deny-read 或强隔离时，目标态必须升级为 sandboxed file helper 子进程。
10. macOS 和 Linux provider 作为首批实现目标。Windows provider 首版返回 `Unsupported`，但 enum、SPI implementation、diagnostics、tests 和错误文案必须完整保留。
11. `NoopBackend` 是 `talos-tools-sandbox` 内的一等本地执行策略，只表达明确无沙箱执行；它不是 platform SPI provider。外部隔离通过 `ExternalSandboxEnforcement` 建模，也不进入 platform SPI。
12. MCP 和外部 dynamic tools 不默认承诺本地 OS sandbox enforce。它们消费 sandbox metadata、approval 和 server capability；只有 Talos 自己控制的本地进程和文件 helper 才能强 enforce。
13. `FullAccess` / `DangerFullAccess` 表示 sandbox capability 明确关闭并走 `NoopBackend`。它不是 macOS/Linux/Windows provider 初始化失败后的 fallback。
14. 现有 `retry_without_sandbox` 不进入目标执行模型。任何从受限模式到 Full Access 的变化都必须重新解析 effective policy，不能由 provider failure 自动触发。
15. 首期验收以本 spec 的 “Initial capability boundary” 为准，不要求一次完成 deny-read、sandboxed fs helper、MCP sandbox 或 Windows enforcement。

## 目标

1. 提供一套跨平台 sandbox 抽象，覆盖 Linux、macOS、Windows placeholder、no sandbox 和 external sandbox。
2. 定义稳定的 `talos-sandbox-spi`，让平台 crate 在不知道用户模式、approval、tool governance 和 workspace 产品语义的前提下实现隔离执行。
3. Bash/local exec 通过统一 `ToolSandboxPermit::spawn()` 启动；manager 和 platform command transformation 保持 facade 内部。
4. 文件类工具通过 permit 绑定的统一 `ToolFileSystemHandle` 访问本地文件系统，runtime 不直接持有或传递内部 `ToolSandboxContext`。
5. 支持 Codex 风格的用户权限模式映射，并把它拆解成 approval、filesystem、network 和 sandbox capability 四个执行维度。
6. 支持 network restricted/enabled 作为 permission profile 的一部分，即使首版只有部分 provider 能 enforce。
7. 支持平台能力探测和显式 unsupported error，不用 fallback 掩盖未生效的沙箱。
8. Windows 暂不实现隔离执行。首期只提供 crate、SPI platform type、placeholder provider、provider selection、unsupported diagnostics 和 tests；未来 Windows provider 由独立 Spec 设计，允许 workspace-internal SPI 演进而不影响 runtime-facing facade。
9. 保持 tool registry、tool runtime、tools sandbox facade、platform SPI、platform provider、filesystem helper 的职责边界清晰。
10. 让后续 approvals 能基于 path、command 和 network capability 合并成同一套 additional permissions。
11. 能在 tool audit、diagnostics 和后续 UI 中说明每次 tool call 的隔离来源：本地 OS sandbox、进程内 authorizer、外部环境声明或无隔离。
12. 在仓库内 `talos-e2e` 项目中新增 `14_sandbox` group，在 macOS 本机和本地 Ubuntu Docker 中验证同一套首期验收标准。

## 非目标

1. 不在本 spec 中实现 Windows restricted token 或 elevated backend。
2. 不实现 Docker sandbox provider。Ubuntu Docker 只作为本机 Linux provider 的 E2E 运行环境，不属于产品执行 provider。
3. 不实现 CPU、内存、进程数、磁盘配额或 cgroup 资源治理。
4. 不改变 MCP 协议实现，不把第三方 MCP server 默认搬入本地 sandbox。
5. 不新增用户可见的权限 UI；审批和设置页面可以在后续 spec 中设计。
6. 不实现完整 fs helper 的所有操作作为第一阶段强制目标；第一阶段只要求抽象能承载 helper。
7. 不通过静默降级把 requested sandbox 变成 unsandboxed 执行。
8. 不为每个工具新增一套私有 sandbox 参数或私有 path allowlist。
9. 首期不迁移 Codex 的 managed network proxy、bundled bubblewrap、legacy Landlock fallback、Windows sandbox 或完整 split filesystem policy。

## Initial capability boundary

首期目标是形成一条可运行、可关闭、可审计并可在 macOS/Linux 上真实验收的最小闭环。后续能力可以沿同一抽象增量接入，但不得成为首期完成条件。

| Capability | macOS/Linux first release | Enforcement source | First-release acceptance |
| --- | --- | --- | --- |
| Bash, `rg`, and local child processes | OS sandbox | `local_os` | Required |
| Process filesystem `read_only` | Seatbelt or bubblewrap | `local_os` | Required |
| Process filesystem `workspace_write` | Seatbelt or bubblewrap | `local_os` | Required |
| Process network `restricted` / `enabled` | Seatbelt or Linux namespace/seccomp | `local_os` | Required |
| `Read` / `Write` / `Edit` / `Glob` | `AuthorizingToolFileSystem` | `in_process_authorizer` | Required |
| Full Access | `NoopBackend` | `none` | Required as positive control |
| deny-read for built-in file tools | `SandboxedToolFileSystem` + fs helper | `local_os` | Follow-up |
| stdio MCP local process sandbox | MCP launcher integration | `local_os` | Follow-up |
| HTTP/remote MCP | External enforcement metadata | external or `none` | Follow-up |
| Windows restricted execution | Placeholder only | `unsupported` | Type and diagnostics tests only |

首期不得把 `AuthorizingToolFileSystem` 标记为 `local_os`。它能对 Talos 内建文件工具执行稳定的路径授权，但不能阻止任意本地子进程绕过进程内检查。需要 deny-read 或内建文件工具的 OS 强隔离时，再接入 fs helper。

## Crate architecture and ownership

目标依赖关系：

```text
Talos runtime / talos-tools
  -> talos-tools-sandbox
       -> talos-sandbox-spi
       -> talos-macos-sandbox
       -> talos-linux-sandbox
       -> talos-windows-sandbox

talos-macos-sandbox
talos-linux-sandbox
talos-windows-sandbox
  -> talos-sandbox-spi
```

禁止以下依赖：

```text
Talos runtime -> talos-sandbox-spi
Talos runtime -> platform sandbox crate
platform sandbox crate -> talos-tools-sandbox
talos-sandbox-spi -> talos-tools-sandbox
```

各 crate 的唯一职责：

| Crate | Owner responsibility | Must not own |
| --- | --- | --- |
| `talos-tools-sandbox` | Public facade；permission decision、`ToolSandbox`、`ToolSandboxPermit`、permit-bound process/filesystem API、internal context/compiler/manager、Noop/External、provider assembly 和 error mapping | Seatbelt profile、bubblewrap/seccomp、Windows token 实现 |
| `talos-sandbox-spi` | Internal platform contract；归一化 isolation request/result、provider capability、provider error、`SandboxProvider` trait | 用户权限模式、approval、tool governance、session/workspace 产品语义、Noop/External、tool filesystem API |
| `talos-macos-sandbox` | `SandboxProvider` 的 Seatbelt 实现和 macOS 平台测试 | Talos policy resolution、tool runtime integration |
| `talos-linux-sandbox` | `SandboxProvider` 的 bubblewrap/seccomp 实现、Linux helper binary 和 Linux 平台测试 | Talos policy resolution、tool runtime integration |
| `talos-windows-sandbox` | 首期 placeholder provider 和 unsupported tests；后续 Windows enforcement | Talos policy resolution、tool runtime integration |

`talos-sandbox-spi` 是 workspace-internal crate，设置 `publish = false`。它承载实现标准，但不自行决定产品语义，也不是 Talos runtime 的 public API。SPI 的对象语义、不变量和演进方向由 `talos-tools-sandbox` 作为架构 owner 定义；独立 crate 只用于切断 platform implementation 对 tools domain 的依赖。`talos-tools-sandbox` 是唯一向上暴露的 sandbox library，并通过 target-aware static assembly 组装 host provider，不引入运行时动态 plugin registry。

### Two cohesive businesses in talos-tools-sandbox

`talos-tools-sandbox` 是一个 crate 和一个 runtime facade，但内部必须明确拆成两个业务域，不能把 permission resolution、provider orchestration 和 OS implementation 混在同一个 manager 中。

#### Permission decision domain

负责回答“本次 tool invocation 被授予什么权限，是否允许继续”：

```text
UserPermissionMode
  + ToolGovernanceConstraints
  + RequestedToolPermissions
  + ApprovalGrant
  + SandboxConfig
  -> EffectiveToolPermissionPolicyResolver
  -> ToolSandboxDecision
       - Allowed(ToolSandboxPermit)
       - RequiresApproval(ToolApprovalRequest)
       - Denied(ToolSandboxDenial)
       - External(ExternalSandboxEnforcement)
```

该业务拥有用户三种权限模式、approval、governance constraint、requested/granted permission、effective policy、policy precheck、context validation 和 permission audit。它不调用 Seatbelt、bubblewrap、seccomp 或 Windows API，也不把 provider capability 当成业务授权结果。

`RegistryOrchestrator` 只输出 tool governance constraints 和 request facts。最终权限合并必须由本域唯一完成，避免 registry、runtime 和 provider 各自解析一套权限。

#### Sandbox execution facade

负责回答“如何用已经批准且 validated 的权限执行”：

```text
ToolSandboxPermit
  -> process API / filesystem API
  -> SandboxManager
  -> SandboxExecutionCompiler
  -> SandboxProvider capability preflight
  -> SandboxProvider prepare contract
  -> facade process lifecycle
  -> unified process result, error, diagnostics and audit
```

该业务拥有 runtime-facing `ToolSandbox` API、permit-bound execution、Noop/External routing、SPI compilation、provider selection、capability preflight orchestration、provider error mapping、process lifecycle、filesystem gateway 和 enforcement metadata。OS capability 的实际探测逻辑仍由 platform provider 实现。

两类 precheck 必须使用不同概念：

- `PolicyPrecheck` 是纯业务判断，决定 allowed、requires approval 或 denied，不访问 OS。
- `ProviderPreflight` 是执行准备，探测 host 是否能 enforce 已批准的 isolation spec，不得反向授予或扩大权限。

### Architecture invariants

1. Talos runtime 只调用 `talos-tools-sandbox::ToolSandbox` 和 permit-bound API，不能直接构造 effective context、SPI request 或调用 platform provider。
2. `talos-tools-sandbox` 决定是否隔离、隔离哪些能力以及如何记录 enforcement；SPI 和 platform crate 不参与业务授权决策。
3. `SandboxExecutionCompiler` 是 tools-domain 到 SPI 的唯一转换点；新增权限维度必须先在 facade 形成清晰语义，再决定是否扩展 SPI。
4. `talos-sandbox-spi` 只稳定表达 platform implementation 真正需要的隔离事实，不能成长为第二套 permission domain。
5. platform provider 只决定如何在本 OS enforce 已给定约束；无法 enforce 时必须显式失败，不能修改约束或切换 Noop。
6. `NoopBackend` 和 `ExternalSandboxEnforcement` 由 facade 单独建模，永远不伪装成 platform provider。
7. provider 采用编译期依赖和 host factory 静态组装；trait injection 只用于内部解耦和测试，不定义用户 plugin ABI。

### Permission and isolation separation

“平台不关心权限”指平台 crate 不理解权限为何被授予，不读取用户模式、approval、tool arguments、workspace config 或 governance。平台仍必须接收已经编译完成的隔离约束，否则无法生成 OS policy。

```text
UserPermissionMode / approval / governance / config
  -> EffectiveToolPermissionPolicy
  -> ToolSandboxContext
  -> SandboxExecutionCompiler
  -> SandboxExecutionRequest (SPI)
       - absolute readable roots
       - absolute writable roots
       - absolute denied roots
       - restricted or enabled network
       - command / cwd / allowlisted env
       - helper and platform runtime requirements
  -> SandboxProvider
  -> PreparedSandboxCommand
```

`SandboxExecutionCompiler` 是 tools-domain 到 platform SPI 的唯一翻译边界。平台 crate 不得接收 `ToolSandboxContext`、`ToolPermissionProfile`、`ToolSandboxMode`、`SandboxCapability` 或 `ApprovalPolicy`。SPI request 中的 paths 必须已经绝对化、规范化并完成 workspace/project 特殊语义展开。

### Object ownership and reuse

统一对象不等于所有层只使用一套 DTO。对象 owner 由语义决定：相同语义必须复用，不同阶段或不同职责的语义必须显式转换，不能用结构相似替代边界判断。

| Object family | Owner | Reuse and exposure rule |
| --- | --- | --- |
| `UserPermissionMode`、approval、governance、requested/granted permission、effective policy | `talos-tools-sandbox` permission domain | 不进入 SPI 或 provider |
| `ToolSandboxRequest`、`ToolSandboxDecision`、`ToolSandboxPermit`、public denial/error/audit | `talos-tools-sandbox` facade | Talos runtime 的稳定 API |
| `ToolSandboxContext`、`SandboxExecutionCompiler`、`SandboxManager`、`PreparedToolCommand` | `talos-tools-sandbox` internal | 字段 private；可以组合 SPI objects，但不向 runtime 暴露 raw SPI request |
| `AbsolutePathBuf`、`SandboxStdin`、`SandboxProviderPlatform`、`SandboxIsolationCapability` | `talos-sandbox-spi` | facade 可通过 `pub use` 选择性 re-export，不能复制等价 enum/newtype |
| `SandboxExecutionRequest`、command/isolation specs、provider capability/support/error、`PreparedSandboxCommand` | `talos-sandbox-spi` | compiler 构造，provider 直接消费；runtime 不直接使用 |
| Built-in file-tool `TalosFsHelperRequest/Response` | `talos-tools-sandbox` internal | 只服务 `SandboxedToolFileSystem`，不加入 platform SPI |
| Seatbelt rules/profile | `talos-macos-sandbox` private | 只由 SPI objects 转换产生，不向上暴露 |
| Bubblewrap args、seccomp program、Linux sandbox-launcher helper wire DTO | `talos-linux-sandbox` private | 只服务 Linux implementation；不得复制一套 public sandbox request |
| Token、job、ACL/firewall launch objects | `talos-windows-sandbox` private | 后续 Windows implementation 使用，不向上暴露 |

Facade 选择性复用稳定 SPI value objects 时，必须从 `talos-tools-sandbox` re-export，保持 runtime 的 Cargo/API 入口仍然只有 facade：

```rust
pub use talos_sandbox_spi::{
    SandboxStdin,
    SandboxIsolationCapability,
    SandboxProviderPlatform,
};
```

以下转换是有意边界，不属于重复 DTO：

```text
ToolPermissionProfile
  -> FileSystemIsolationSpec / NetworkIsolationSpec

ToolProcessCommand
  -> validated SandboxExecutionCommand

SandboxProviderError
  -> TalosSandboxError
```

前者表达 Talos 已授予的业务权限，后者表达 provider 必须 enforce 的规范化事实；即使字段相似，也不能合并为同一个类型。相反，platform crate 如果重新声明自己的 public command、filesystem policy、network policy、capability 或 provider error，则属于禁止的重复定义。

## 当前实现事实

当前实现已经形成以下闭环：

- `talos-sandbox-spi` 唯一拥有 platform-neutral provider contract、execution request、capability、prepared command 和 provider error。
- macOS、Linux 和 Windows placeholder crate 只实现 SPI；它们不依赖 Talos permission、Session、approval 或 UI 对象。
- `talos-tools-sandbox` 已拥有 permission resolver、validated context、runtime facade、provider assembly、execution compiler、permit、filesystem handle、audit 和 diagnostics。
- `RegistryOrchestrator` 在真实 dispatch 前生成 `ToolSandboxRequest`，只有 `Allowed` decision 才绑定 invocation-scoped permit；approval request 不是 grant。
- Bash 和 Grep 通过 permit-bound process API 启动；Read、Write、Edit 和 Glob 通过 `ToolFileSystemHandle` 执行。
- Full Access 显式解析为 disabled Sandbox 和 Noop enforcement；受限模式的 provider、helper、probe、support 或 prepare 失败保持 fail-closed。
- runtime 统一提供规范化 workspace/project roots，具体工具和 platform provider 不再推导 Session 路径。

因此本 Spec 后续章节描述的是已落地架构及其持续约束，不是尚未接入的并行方案。

## 对 Codex 的参考判断

Codex 的可借鉴点不是目录名，而是边界：

```text
PermissionProfile
  -> SandboxManager
  -> platform sandbox implementation
  -> command/helper execution
```

对文件工具，Codex 不是让每个 tool 自己判断路径，而是通过带 `FileSystemSandboxContext` 的 filesystem trait 收口。需要平台 sandbox 时，文件操作被封装成 helper 请求，helper 子进程在 OS sandbox 中执行真实读写。

Talos 应采用同类边界，但用自己的对象命名和 crate 布局：

```text
ToolPermissionProfile
  -> ToolSandboxContext
  -> SandboxExecutionCompiler
  -> SandboxExecutionRequest
  -> SandboxProvider
  -> PreparedSandboxCommand

ToolFileSystem
  -> DirectToolFileSystem
  -> AuthorizingToolFileSystem
  -> SandboxedToolFileSystem
  -> fs_helper
```

## Codex code migration assessment

Codex 的本地代码可以作为迁移来源，但迁移单位必须是独立机制和测试语义，不能把 Codex crate 直接作为 Talos runtime dependency。Codex sandbox 实现依赖自己的 protocol、`PathUri`、absolute path、network proxy、exec-server 和 packaging；直接依赖会破坏 Talos 的领域边界并引入大量首期非目标。

### Licensing boundary

- Codex repository 声明 Apache-2.0；Talos workspace 声明 MIT。
- 选择性复制或修改 Codex source 时，实施变更必须保留来源记录、Apache-2.0 license/NOTICE 要求，并在 Talos 的第三方声明中登记。最终分发方式仍需项目 owner 复核。
- 每次源码迁移必须记录 Codex source commit、原始文件路径和 Talos 目标文件，不能长期依赖一个持续变化的 `../codex` working tree 作为隐式来源。
- Codex vendored bubblewrap source 使用 GNU Library General Public License v2。首期不复制、静态链接或分发该 bundled binary，而是在 Linux 环境使用受支持的 system `bwrap`。
- 如果未来需要随 Talos 分发 bubblewrap，必须单独设计 binary separation、source availability、NOTICE/license packaging 和升级流程，不能作为普通 Rust source migration 顺手引入。

### Migration decision matrix

| Codex area | Decision | Talos target | Scope |
| --- | --- | --- | --- |
| `sandboxing::manager` | Rewrite against Talos types | `talos-tools-sandbox::manager` | Reuse selection and fail-closed semantics；manager 只消费 Talos effective context 并调用 SPI provider |
| Codex permission-to-platform transformation | Rewrite | `talos-tools-sandbox::compiler` | 输出 `talos-sandbox-spi::SandboxExecutionRequest`，不复用 Codex protocol objects |
| Seatbelt base policies and profile builder | Selectively port | `talos-macos-sandbox` | 消费 SPI request；保留 fixed executable path、filesystem/network policy generation 和 focused tests |
| Linux `no_new_privs` and restricted-network seccomp | Selectively port | `talos-linux-sandbox` helper inner stage | 消费 SPI request；适配 provider error；显式支持 `x86_64` 和 `aarch64` |
| Linux bubblewrap argument generation | Port a reduced subset | `talos-linux-sandbox` | Read-only root、writable roots、network namespace、cwd、env 和 system `bwrap` preflight only |
| Codex bundled bubblewrap launcher/vendor tree | Do not port in first release | None | Use system `bwrap`; return unsupported when unavailable |
| Legacy Landlock fallback | Do not port in first release | None | Avoid maintaining a second Linux enforcement path |
| Managed proxy and proxy routing bridge | Do not port in first release | Follow-up spec | Initial network policy is restricted or enabled, without proxy-only mode |
| `ExecutorFileSystem` interface pattern | Adapt | `talos-tools-sandbox::ToolFileSystem` | Use Talos paths/results and object-safe boxed futures |
| `local_file_system::resolve_existing_path` | Adapt | `talos-tools-sandbox::file_system::authorization` | Source commit `a26bc337cf61cbca9cd9ac25b25c88c32186eb20`; retain nearest-existing-ancestor semantics, add fail-closed broken-symlink handling, and do not import `PathUri` or executor DTOs |
| fs helper request/response and runner pattern | Selectively port later | `talos-tools-sandbox::SandboxedToolFileSystem` and Talos helper | Helper command 仍通过 compiler/SPI/provider 进入 OS sandbox |
| Codex filesystem and sandbox tests | Port scenarios, not fixtures verbatim | crate tests and `talos-e2e/scenarios/14_sandbox` | Recreate positive/negative controls with Talos tool dispatch |
| Windows sandbox implementation | Do not port in this spec | Placeholder only | Preserve type/config/error contract |

### macOS migration boundary

首期 macOS provider 可以适配以下 Codex 机制：

1. 默认拒绝的 Seatbelt base profile 和固定 `/usr/bin/sandbox-exec` 启动路径。
2. readable roots、writable roots 和 restricted/enabled network 对应的 profile sections。
3. shell、dynamic library、PTY 和基础系统服务所需的 platform runtime defaults。
4. 真实执行生成 profile 并验证 outside write denied、workspace write allowed 的测试。

首期迁移不携带 Codex managed proxy、Codex-specific protected metadata names、unreadable glob translation 或 remote-environment policy，除非首期 capability matrix 明确要求。

### Linux migration boundary

首期 Linux helper 可以适配以下 Codex 机制：

1. Outer helper -> bubblewrap -> inner helper -> target command 的执行顺序。
2. 从 workspace 外发现 system `bwrap` 并执行 capability preflight。
3. Read-only root、显式 writable-root bind mounts、cwd preservation、user/PID/network namespace，以及 Talos process lifecycle 所需的 signal forwarding。
4. Inner stage 的 `PR_SET_NO_NEW_PRIVS` 和 restricted-network seccomp filter。
5. workspace write、outside write denial、network restriction、symlink escape 和 unsupported user namespace 的聚焦测试。

Provider 将 bounded launcher request 写入 outer helper stdin。请求中保留目标命令的 `SandboxStdin`；outer helper 解码后使用 Linux `memfd` 恢复 fd 0，再 `exec` system bubblewrap，因此 inner helper 和 target 继承原始 closed/inline stdin 语义。该机制只属于 Linux provider 私有协议，不扩展 SPI，也不创建临时状态文件。

首期迁移排除 Codex synthetic mount registry、protected-create monitor、nested split-policy carveouts、managed proxy routing、bundled bwrap 和 legacy Landlock。只有 Talos 出现首期 policy 无法表达的真实需求时，才继续评估这些能力。

### fs helper migration boundary

Codex 的 fs helper 架构适合后续迁移到 Talos：parent 序列化 typed operation，通过同一个 `SandboxManager` 启动 helper，发送一个有大小上限的 request，接收一个有大小上限的 response，并且只在 helper 内执行真实 filesystem operation。

Talos 应适配该结构，而不是导入 Codex exec-server types：

```text
ToolFileSystem operation
  -> TalosFsHelperRequest
  -> sandbox helper process
  -> DirectToolFileSystem inside OS sandbox
  -> TalosFsHelperResponse
```

Talos-specific constraints：

- helper executable path 通过 `SandboxRuntimePaths` 注入，调用方不假设特定 Talos binary layout；
- request/response enums 使用 Talos filesystem DTOs 和 `TalosError` categories；
- stdin、stdout、base64 payload、walk result 和 process lifetime 都有 hard limits；
- helper environment 使用 allowlist，不继承完整 parent environment；
- request payload 不携带第二份 sandbox context，避免 recursive sandbox dispatch；
- helper setup 或 protocol failure 不 retry direct filesystem operation。

该迁移是后续能力，必须在首期 `AuthorizingToolFileSystem` 路径完成并由 `14_sandbox` E2E group 覆盖后再实施。

## 统一框架伪代码

### 接入语义

Sandbox library 的接入点是 Talos 现有 tool execution path，而不是新的用户插件系统：

```text
existing tool runtime
  -> talos-tools-sandbox::ToolSandbox.evaluate()
  -> ToolSandboxDecision
  -> ToolDispatchContext binds ToolSandboxPermit for this invocation
  -> local ToolContext receives the same ephemeral permit at handle boundary
  -> permit.spawn() / permit.file_system()
```

`ToolContext` 当前承载 session identity、project root、kernel config 和共享 session state；`ToolDispatchContext` 当前承载一次 dispatch。为避免大范围替换现有 `Tool` trait，同时保证 permit 不进入共享 `SessionToolState`，首期按以下 ownership 接入：

1. `ToolDispatchContext` 是 invocation-bound permit 的 owner；Registry 每次 dispatch 都从无 permit 状态开始，不能继承上一个调用的 permit。
2. `ToolHandler::pre_execute()` 完成现有输入和权限预检后，新增 sandbox request extraction；Bash 提取 process/network 请求，文件工具提取并规范化全部访问路径。
3. `ToolSandbox::evaluate()` 返回 allowed permit 后，Registry 将 permit 绑定到当前 `ToolDispatchContext`；RequiresApproval、Denied 和 External 不进入本地 handler execution。
4. `LocalToolHandler::handle()` 在调用现有 `Tool::execute_payload()` 时，把 permit 作为 `ToolContext` 的 private ephemeral execution field 一并转交。该字段不属于 session state、不能由公共 builder 构造，并在下一次 Registry dispatch 入口被清除。
5. Bash 和内建文件工具只通过 `ToolContext` 的 crate-private sandbox accessor 获得 permit-bound process/filesystem API；对 `talos-tools` 外部仍不公开 manager、SPI 或 raw context。

这保留了现有 runtime API 的最小改动面，同时 permit 自身继续校验 invocation identity 和 operation kind。是否启用强制隔离由用户权限模式解析后的 effective `SandboxCapability` 决定：

```text
SandboxCapability::Enabled
  -> sandbox capability enabled
  -> SandboxManager must select an enforcing local provider

SandboxCapability::Disabled
  -> sandbox capability disabled
  -> SandboxManager selects NoopBackend

SandboxCapability::External
  -> local sandbox capability not applicable
  -> do not enter local process executor
  -> record ExternalSandboxEnforcement metadata
```

因此 “可接入/可关闭” 的边界在 effective permission policy 和 runtime context，而不是让每个工具或用户动态选择一套 provider 插件。

目标调用链：

```text
tool runtime
  -> talos-tools-sandbox facade
       -> EffectiveToolPermissionPolicy / ToolSandboxContext
       -> SandboxManager
            -> NoopBackend, when sandbox capability is disabled
            -> SandboxExecutionCompiler, when sandbox capability is enabled
                 -> talos-sandbox-spi::SandboxExecutionRequest
                 -> selected SandboxProvider
                      - MacosSandboxProvider
                      - LinuxSandboxProvider
                      - WindowsSandboxProvider placeholder
                 -> talos-sandbox-spi::PreparedSandboxCommand

external tool runtime
  -> ExternalSandboxEnforcement
  -> remote/server capability and audit metadata
```

Tool runtime 不直接选择平台 provider，也不直接拼 `sandbox-exec`、Linux helper 或 Windows token 参数。runtime-facing public API 由一个内聚的 `ToolSandbox` facade 提供：

```rust
pub struct ToolSandbox {
    permission_engine: ToolPermissionEngine,
    execution: Arc<ToolSandboxExecution>,
}

pub struct ToolSandboxRequest {
    pub invocation: ToolInvocationIdentity,
    pub user_mode: UserPermissionMode,
    pub governance: ToolGovernanceConstraints,
    pub requested_permissions: RequestedToolPermissions,
    pub approval_grant: Option<ToolApprovalGrant>,
    pub environment: ToolSandboxEnvironment,
}

pub enum ToolSandboxDecision {
    Allowed(ToolSandboxPermit),
    RequiresApproval(ToolApprovalRequest),
    Denied(ToolSandboxDenial),
    External(ExternalSandboxEnforcement),
}

pub struct ToolSandboxPermit {
    context: Arc<ToolSandboxContext>,
    execution: Arc<ToolSandboxExecution>,
    scope: ToolSandboxPermitScope,
}

pub(crate) struct ToolSandboxPermitScope {
    invocation: ToolInvocationIdentity,
    allowed_operations: BTreeSet<ToolSandboxOperationKind>,
}

pub(crate) enum ToolSandboxOperationKind {
    SpawnProcess,
    FileSystem,
}

pub struct ToolSandboxDiagnostics {
    pub providers: Vec<ToolSandboxProviderDiagnostic>,
}

pub struct ToolSandboxAuditEvent {
    pub invocation: ToolInvocationIdentity,
    pub stage: ToolSandboxAuditStage,
    pub outcome: ToolSandboxAuditOutcome,
    pub user_mode: UserPermissionMode,
    pub effective_policy: Option<EffectiveToolPermissionPolicy>,
    pub enforcement: Option<ToolSandboxEnforcement>,
    pub error_category: Option<TalosSandboxErrorCategory>,
}

pub trait ToolSandboxAuditSink: Send + Sync {
    fn record(&self, event: &ToolSandboxAuditEvent);
}

pub struct ToolSandboxProviderDiagnostic {
    pub platform: SandboxProviderPlatform,
    pub provider_name: String,
    pub status: ToolSandboxProviderStatus,
    pub capabilities: Vec<SandboxIsolationCapability>,
}

pub enum ToolSandboxProviderStatus {
    Available,
    Unsupported { reason: String },
    Failed { reason: String },
}

pub enum ToolSandboxEnforcement {
    None,
    InProcessAuthorizer,
    LocalOs {
        platform: SandboxProviderPlatform,
        provider_name: String,
    },
    External {
        source: ExternalEnforcementSource,
    },
}

impl ToolSandbox {
    pub fn evaluate(
        &self,
        request: ToolSandboxRequest,
    ) -> TalosResult<ToolSandboxDecision>;

    pub fn diagnostics(&self) -> TalosResult<ToolSandboxDiagnostics>;
}

impl ToolSandboxPermit {
    pub fn spawn(
        &self,
        command: ToolProcessCommand,
    ) -> ToolExecutionFuture<'_, RunningToolProcess>;

    pub fn file_system(&self) -> ToolFileSystemHandle;
}

pub struct ToolProcessCommand {
    pub program: PathBuf,
    pub args: Vec<String>,
    pub env: BTreeMap<String, String>,
    pub cwd: PathBuf,
    pub stdin: SandboxStdin,
}

impl ToolDispatchContext {
    pub(crate) fn bind_sandbox_permit(
        self,
        permit: ToolSandboxPermit,
    ) -> TalosResult<Self>;

    pub(crate) fn into_tool_context(self) -> ToolContext;
}

impl ToolContext {
    pub(crate) fn sandbox_permit(&self) -> TalosResult<&ToolSandboxPermit>;
}
```

`ToolSandboxPermit` 是一次 validated permission decision 的不可伪造 capability object。它绑定原始 tool invocation、允许的 operation kinds 和 effective policy，不能跨 tool call 复用。它的字段保持 private，runtime 不能自行替换 context、扩大 roots、切换 provider、用 filesystem-only permit 启动进程，或把受限 permit 转成 Full Access。`ToolSandbox::evaluate()` 只有在 permission decision 为 allowed 时才构造 permit；approval 产生新 grant 后必须重新 evaluate。

Public facade 的最小 surface 是 `evaluate`、permit-bound `spawn`、permit-bound filesystem handle、`diagnostics` 和 application-assembly 注入的 audit sink。实际 process/filesystem result、audit event 和 diagnostics 使用统一 `ToolSandboxEnforcement`，不能在执行前仅根据期望策略声称 `local_os` 已生效。默认 audit sink 写入 tracing；持久化、队列和 UI projection 不属于首期 sandbox library。`SandboxManager`、`SandboxExecutionCompiler`、`ToolSandboxContext`、provider registry、SPI request/result 和 provider error 都是 crate-internal implementation details。

Facade 内部的进程型执行通过 `ToolProcessExecutor` 进入 `SandboxManager`。由于 executor 需要以 `Arc<dyn ...>` 注入，接口必须保持 dyn-compatible；这里有意使用 boxed future，而不是在 trait method 上返回 RPITIT：

```rust
pub type ToolExecutionFuture<'a, T> =
    Pin<Box<dyn Future<Output = TalosResult<T>> + Send + 'a>>;

pub(crate) trait ToolProcessExecutor: Send + Sync {
    fn spawn<'a>(
        &'a self,
        command: ToolProcessCommand,
        sandbox: &'a ToolSandboxContext,
    ) -> ToolExecutionFuture<'a, RunningToolProcess>;
}

pub(crate) struct SandboxedToolProcessExecutor {
    sandbox_manager: Arc<SandboxManager>,
}

pub(crate) struct PreparedToolCommand {
    program: PathBuf,
    args: Vec<String>,
    env: BTreeMap<String, String>,
    cwd: PathBuf,
    stdin: SandboxStdin,
    enforcement: ToolSandboxEnforcement,
}

impl ToolProcessExecutor for SandboxedToolProcessExecutor {
    fn spawn<'a>(
        &'a self,
        command: ToolProcessCommand,
        sandbox: &'a ToolSandboxContext,
    ) -> ToolExecutionFuture<'a, RunningToolProcess> {
        Box::pin(async move {
            let prepared = self
                .sandbox_manager
                .prepare_command(command, sandbox)?;
            spawn_process(prepared).await
        })
    }
}
```

`SandboxManager` 是 facade 内唯一的本地执行路由。它根据 validated context 决定进入 Noop 还是 enforcing provider；runtime 不选择 provider，provider failure 也不能切换到 Noop：

```rust
pub(crate) struct SandboxManager {
    compiler: SandboxExecutionCompiler,
    providers: Vec<SandboxProviderRegistration>,
    noop: NoopBackend,
}

pub(crate) struct SandboxProviderRegistration {
    provider: Arc<dyn SandboxProvider>,
    snapshot: SandboxProviderSnapshot,
}

impl SandboxManager {
    pub(crate) fn prepare_command(
        &self,
        command: ToolProcessCommand,
        sandbox: &ToolSandboxContext,
    ) -> TalosResult<PreparedToolCommand> {
        match sandbox.capability() {
            SandboxCapability::Disabled => {
                self.noop.prepare_command(command, sandbox)
            }
            SandboxCapability::Enabled => {
                let request = self.compiler.compile(command, sandbox)?;
                let platform = request.platform();
                let registration = self.select_provider(platform)?;
                registration
                    .provider
                    .support(&request, &registration.snapshot)
                    .require_supported()
                    .map_err(map_provider_error)?;
                let prepared = registration
                    .provider
                    .prepare(request)
                    .map_err(map_provider_error)?;
                Ok(PreparedToolCommand::local_os(
                    prepared,
                    platform,
                    registration.provider.name(),
                ))
            }
            SandboxCapability::External => {
                Err(TalosSandboxError::InvalidLocalExecutionContext)
            }
        }
    }
}
```

`SandboxExecutionCompiler` 是唯一能同时看到 tools-domain context 和 SPI request 的对象。它负责路径绝对化、workspace 语义展开、环境变量 allowlist、runtime requirements 和非法组合校验：

```rust
pub(crate) struct SandboxExecutionCompiler;

impl SandboxExecutionCompiler {
    pub(crate) fn compile(
        &self,
        command: ToolProcessCommand,
        sandbox: &ToolSandboxContext,
    ) -> TalosResult<SandboxExecutionRequest> {
        sandbox.require_enforcing_local_context()?;

        Ok(SandboxExecutionRequest {
            platform: sandbox.provider_platform()?,
            command: compile_command(command, sandbox)?,
            file_system: compile_file_system(sandbox.permissions())?,
            network: compile_network(sandbox.permissions())?,
            runtime: compile_runtime_requirements(sandbox)?,
        })
    }
}
```

SPI 只描述隔离执行事实，不描述这些约束来自哪个用户模式或审批结果。`AbsolutePathBuf` 是 SPI 自有的 validated newtype，构造时拒绝相对路径：

```rust
pub struct AbsolutePathBuf(PathBuf);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SandboxProviderPlatform {
    MacosSeatbelt,
    LinuxBubblewrap,
    WindowsRestrictedToken,
    WindowsElevated,
}

pub struct SandboxExecutionRequest {
    pub platform: SandboxProviderPlatform,
    pub command: SandboxExecutionCommand,
    pub file_system: FileSystemIsolationSpec,
    pub network: NetworkIsolationSpec,
    pub runtime: SandboxRuntimeRequirements,
}

pub struct SandboxExecutionCommand {
    pub program: AbsolutePathBuf,
    pub args: Vec<String>,
    pub env: BTreeMap<String, String>,
    pub cwd: AbsolutePathBuf,
    pub stdin: SandboxStdin,
}

pub struct FileSystemIsolationSpec {
    pub readable_roots: Vec<AbsolutePathBuf>,
    pub writable_roots: Vec<AbsolutePathBuf>,
    pub denied_roots: Vec<AbsolutePathBuf>,
}

pub enum NetworkIsolationSpec {
    Restricted,
    Enabled,
}

pub struct SandboxRuntimeRequirements {
    pub private_tmp: bool,
}

pub struct PreparedSandboxCommand {
    pub program: AbsolutePathBuf,
    pub args: Vec<String>,
    pub env: BTreeMap<String, String>,
    pub cwd: AbsolutePathBuf,
    pub stdin: SandboxStdin,
}

pub enum SandboxIsolationCapability {
    FileSystemRead,
    FileSystemWrite,
    DenyRead,
    NetworkRestricted,
    PrivateTmp,
}

pub enum SandboxRequestSupport {
    Supported,
    Unsupported {
        capability: SandboxIsolationCapability,
        reason: String,
    },
}

pub trait SandboxProvider: Send + Sync {
    fn name(&self) -> &'static str;
    fn supports_platform(&self, platform: SandboxProviderPlatform) -> bool;
    fn probe(
        &self,
        platform: SandboxProviderPlatform,
    ) -> Result<SandboxProviderCapabilities, SandboxProviderError>;
    fn support(
        &self,
        request: &SandboxExecutionRequest,
        snapshot: &SandboxProviderSnapshot,
    ) -> SandboxRequestSupport;
    fn prepare(
        &self,
        request: SandboxExecutionRequest,
    ) -> Result<PreparedSandboxCommand, SandboxProviderError>;
}
```

`SandboxProvider` 是 workspace 内部 SPI，不是 runtime 或用户可扩展的 plugin API。平台 crate 可以拒绝自身无法 enforce 的规范化约束，但不能重新解释权限、扩大 user-data roots、修改 network policy，或决定是否允许 Full Access。provider 只能附加其固定、受测试且可诊断的 platform runtime defaults，例如系统 library roots 和 `/dev/null`。

`SandboxProvider::name()` 只提供稳定 diagnostics/audit identifier，不参与 provider selection 或权限判断。选择依据只能是 validated `SandboxProviderPlatform` 和 capability probe。

### SPI validation contract

`SandboxExecutionRequest` 只能通过 validated constructor 创建，不提供可绕过校验的 public struct literal。校验分两层，避免 provider 重复解释公共语义：

- Facade compiler 负责把 program、cwd 和 policy roots 转成绝对规范路径，展开 workspace 语义，解析 executable，并构造 allowlisted environment。
- SPI constructor 负责验证绝对路径、集合冲突、portable limits 和结构组合；provider `prepare()` 只重查会受 TOCTOU 影响的平台事实，例如 cwd 仍为目录、program 仍可启动和 helper 仍可用。

路径和 root 规则固定如下：

1. Empty readable/writable roots 是合法的 default-deny policy。Talos restricted runtime 默认把 host file-system roots 显式编译为 readable roots，把 workspace/project roots 编译为 writable roots；denied roots 用于收窄 read access。Compiler/provider 只补充平台固定 runtime roots，不能自行扩大 user-data write access。
2. 所有 path 必须绝对化并执行 lexical normalization；存在的路径解析 symlink，尚不存在的 write target 使用 nearest existing ancestor 的 canonical path 加 unresolved suffix。文件操作前 authorizer 必须再次解析 parent，拒绝 symlink escape。
3. 同权限的重复 root 和被父 root 完全覆盖的 child root 去重；不同权限的父子 root 保留，以表达 carve-out。
4. Effective precedence 固定为 denied > writable > readable；writable 隐含 readable。任何与 denied root 相同或位于其下的访问都拒绝。
5. Policy root 必须指向存在目录；具体 write target 可以不存在。Compiler 解析后的 program 必须是绝对路径并指向普通可启动文件，cwd 必须是存在目录。
6. Validation failure 一律返回 `SandboxProviderError::InvalidIsolationSpec`，不能截断输入、丢弃 root 或降级为较宽策略。

Portable hard limits 是：roots 每类最多 256 个；args 最多 4096 个且 UTF-8 payload 合计最多 1 MiB；env 最多 256 项，key 最多 256 bytes、value 最多 32 KiB、合计最多 256 KiB；inline stdin 最多 1 MiB。超限在 SPI constructor 中 fail fast。平台可以声明更小的 capability limit，但不能静默截断。

### Provider preflight lifecycle

Provider 在 `ToolSandbox` assembly 时恰好执行一次 `probe()`，结果以 immutable `SandboxProviderSnapshot` 存入 registration。并发 invocation 只读 snapshot，不触发 probe；`support(request, snapshot)` 必须是无 I/O、无 mutation、只依赖这两个参数的 deterministic operation。Probe failure 也缓存为 failed snapshot并进入 diagnostics，requested sandbox 随后 fail closed。

首期不实现 TTL、后台刷新或每调用探测。配置或 helper 路径变化时，由 application assembly 重建 `ToolSandbox` 才产生新 snapshot。Diagnostics 返回当前 snapshot，不能把未经 probe 的期望能力报告为 available；`prepare()` 仍需报告 assembly 后发生的 executable/helper/OS 状态变化。

### Helper protocol ownership

Linux sandbox-launcher wire protocol 完全归 `talos-linux-sandbox` 私有模块所有，只接受 SPI request 的单向转换结果。协议使用 serde typed envelope、显式 `protocol_version`、1 MiB request/response hard limit，并要求 parent/helper 的 major protocol version 完全一致；版本不匹配、decode failure 或超限均返回 provider setup error，不能执行原命令。兼容性测试位于 Linux provider crate。

Built-in file-tool helper 继续由 `talos-tools-sandbox` 私有拥有，使用独立协议和 limits；两种 helper 不共享 wire DTO，也不把 wire fields 加入 SPI。

### Shared conformance fixture ownership

共享 provider contract scenarios 由 `talos-sandbox-spi` 的非默认 `provider-contract-tests` feature 拥有。该 module 只暴露 provider implementer 的 contract runner、canonical request fixtures 和 expected SPI objects；三个平台 crate 仅在 dev-dependencies 中启用，production dependency graph 不包含 test support。平台 crate 不复制 fixture，也不为测试暴露 private OS translation 类型。

### Provider conformance contract

`talos-macos-sandbox`、`talos-linux-sandbox` 和 `talos-windows-sandbox` 必须遵守同一组 conformance rules：

1. 实现 `talos-sandbox-spi::SandboxProvider`，直接消费 `SandboxExecutionRequest` 并返回 SPI result/error。
2. 不定义第二套 public command、filesystem policy、network policy、capability、support、prepared result 或 provider error。
3. 可以把 SPI objects 转换为 provider-private OS objects，但转换必须单向且不能扩大 user-data roots 或 network access。
4. Provider constructor 只接收平台运行依赖，例如 helper path 或 fixed executable path；不得接收用户模式、approval、governance 或 `ToolSandboxContext`。
5. Provider capability 不足、preflight 失败或 prepare 失败必须返回 SPI error，不得直接启动原命令或选择 Noop。
6. Platform runtime defaults 必须是集中定义、可诊断并经过测试的固定集合，不能成为任意 user-data allowlist。
7. macOS/Linux 真实实现和 Windows placeholder 都运行共享 SPI contract tests；各 crate 再补充自己的 private translation 和真实 OS integration tests。
8. Provider public API 仅包含 constructor/config、SPI implementation 和必要 diagnostics constants；Seatbelt、bubblewrap/seccomp、helper wire、token/job 等类型保持 private。

首期 conformance 结果：macOS 和 Linux 对 capability matrix 中的 required cases 返回 supported 并真实 enforce；Windows 对 enforcing request 稳定返回 `UnsupportedCapability`。这三个 crate 的差异只能体现在 capability 和私有 OS translation，不能体现在 Talos runtime API 或重复 DTO。

文件型工具不直接持有内部 `ToolFileSystem` 或 sandbox context，而是从当前 invocation 的 `ToolContext` 获取 permit-bound `ToolFileSystemHandle`：

```rust
pub async fn run(
    request: &ReadFileRequest,
    context: &ToolContext,
) -> TalosResult<ReadFileResponse> {
        let bytes = context
            .sandbox_permit()?
            .file_system()
            .read_file(&request.path)
            .await?;
        render_read_file_response(bytes)
}
```

这套伪代码的约束是：runtime 只能使用当前 `ToolContext` 的 crate-private permit-bound API，不能获得内部 sandbox context、manager、provider 或 SPI request，也不能绕回裸 `tokio::process::Command` 或 `tokio::fs` 实现核心行为。

## 隔离体现

Sandbox 的核心职责是解决 tool execution permission 问题，而不是只给 UI 或 prompt 增加一个状态标签。隔离必须体现在真实执行边界上：

1. 文件能读哪里。
2. 文件能写哪里。
3. 是否允许网络访问。
4. 子进程能否绕过 Talos 的路径检查。
5. 执行失败时能区分权限拒绝、平台不支持和 sandbox 初始化失败。
6. 审计中能记录本次 tool call 的 enforcement source。

### Process isolation

进程型工具的隔离点是子进程启动前的 command transformation：

```text
bash / rg / local process tool
  -> ToolSandboxPermit.spawn()
  -> internal ToolProcessExecutor / SandboxManager
  -> NoopBackend when sandbox is explicitly disabled
  -> SandboxExecutionCompiler when sandbox is enabled
  -> selected SandboxProvider through talos-sandbox-spi
  -> PreparedSandboxCommand
  -> OS sandboxed child process
```

在 `read_only` 或 `workspace_write` 下，compiler 必须先把权限编译成规范化 isolation spec，provider 再把普通命令包装进对应平台的隔离机制中。体现为子进程不能读取 denied roots，不能写 workspace 外路径，network restricted 时不能联网，并且只能看到 request 明确传入的 cwd、env、args 和 allowed roots。

在 `danger_full_access` 下，执行明确走 `NoopBackend`。这表示 sandbox 未开启，不得在 diagnostics、audit 或 UI 中声明为 sandboxed。

### File operation isolation

文件型工具的隔离点是 `ToolFileSystem`：

```text
Read / Write / Edit / Glob
  -> ToolSandboxPermit.file_system()
  -> ToolFileSystemHandle
  -> internal ToolFileSystem
  -> AuthorizingToolFileSystem or SandboxedToolFileSystem
  -> real filesystem operation
```

迁移期可以用 `AuthorizingToolFileSystem` 做 canonical path checks，先 enforce read-only 和 workspace-write 的路径边界。目标态需要 `SandboxedToolFileSystem`：

```text
tool runtime
  -> ToolSandboxPermit.file_system()
  -> fs_helper request
  -> fs_helper child process
  -> SandboxManager
  -> OS sandbox
  -> real filesystem operation
```

`deny-read` 这类能力不能只依赖进程内检查长期承载。只要存在绕过 `ToolFileSystem` 的裸文件读取路径，deny-read 就不成立。

### External isolation

External tool、remote environment 和部分 MCP server 的执行位置不在 Talos 本地进程控制范围内。此时 Talos 只能记录和传递 sandbox metadata、approval state、capability declaration 和 enforcement source。只有当执行发生在 Talos 控制的本地子进程或 fs helper 中，才能声明 `local_os` 强隔离。

## 核心对象模型

### User permission mode

用户可见的权限模式是产品语义，不能直接作为底层 sandbox provider input 或 filesystem policy 使用。它需要先解析成审批策略、执行权限和 sandbox capability：

```text
UserPermissionMode
  -> ApprovalPolicy
  -> ToolPermissionProfile
  -> SandboxCapability
  -> ToolSandboxDecision
       - Allowed(ToolSandboxPermit)
       - RequiresApproval
       - Denied
       - External
  -> internal ToolSandboxContext / SandboxManager / Noop
```

目标模型：

```rust
pub enum UserPermissionMode {
    AskForApproval,
    ApproveForMe,
    FullAccess,
}

pub struct EffectiveToolPermissionPolicy {
    pub approval: ApprovalPolicy,
    pub permissions: ToolPermissionProfile,
    pub sandbox: SandboxCapability,
}

pub struct ApprovalPolicy {
    pub require_for_network: bool,
    pub require_for_workspace_external_write: bool,
    pub require_for_potentially_unsafe: bool,
}

pub enum SandboxCapability {
    Enabled,
    Disabled,
    External,
}
```

Codex 风格模式的目标映射：

```text
AskForApproval
  -> approval: required for network and workspace-external writes
  -> filesystem: workspace write by default
  -> network: restricted until approved
  -> sandbox: enabled for local execution

ApproveForMe
  -> approval: required only for detected unsafe actions
  -> filesystem: workspace write by default
  -> network: policy-dependent effective permission
  -> sandbox: enabled for local execution unless the effective permission escalates to FullAccess

FullAccess
  -> approval: not required
  -> filesystem: danger full access
  -> network: enabled
  -> sandbox: disabled
  -> execution strategy: NoopBackend for local execution
```

这个映射说明：Full Access 是用户权限模式，不是 OS sandbox provider。它解析后的结果是 no-sandbox 执行；受限模式解析后的结果才进入 enforcing provider。

### Effective policy resolution

`EffectiveToolPermissionPolicyResolver` 是用户模式、配置、tool governance、单次请求和 approval grant 合并的唯一 owner。具体工具和 platform provider 不得各自重复合并权限。

解析顺序：

1. Session `UserPermissionMode` 和全局 sandbox config 生成 base policy。
2. Tool governance 可以收窄权限或声明所需能力，但不能自行授予 Full Access。
3. Tool request 中的 escalation/additional permissions 只表示请求，不表示已经授权。
4. 已批准的 grant 只能与本次 request 取交集，再合并到 base policy；首期未接入 approval runtime 时，额外权限请求按 governance escalation 稳定返回 `RequiresApproval` 或 `Denied`，不得自动重试为 unsandboxed execution。
5. Resolver 先产生 `ToolSandboxDecision`；只有 local allowed decision 才构造并验证 `ToolSandboxContext`，再封装为 `ToolSandboxPermit`。
6. Permit 执行进程或 helper 时，`SandboxExecutionCompiler` 把内部 context 编译成不含 tools-domain 语义的 SPI request。
7. `SandboxManager` 只检查 host capability 并选择 provider。provider unsupported 或 setup failure 不能反向扩大 effective policy。

现有 `retry_without_sandbox` 不保留为执行层 fallback。目标模型使用显式 escalation 语义：

```rust
pub enum ToolEscalationPolicy {
    Forbidden,
    RequiresApproval,
}
```

如果受限执行失败，原 tool call 以 sandbox error 结束。需要升级权限时，必须由 approval 结果产生新的 effective policy 和新的 execution attempt；这不是 provider 内部 retry。Full Access session 则从一开始就解析为 `SandboxCapability::Disabled`，不经历 sandbox failure。

### Permission profile

`ToolSandboxMode` 是执行侧的粗粒度权限入口，不是用户可见权限模式的完整表达。执行前必须解析成强类型 profile：

```rust
pub struct ToolPermissionProfile {
    pub file_system: FileSystemSandboxPolicy,
    pub network: NetworkSandboxPolicy,
}

pub enum FileSystemSandboxPolicy {
    ReadOnly(FileSystemScope),
    WorkspaceWrite(WorkspaceWritePolicy),
    DangerFullAccess,
}

pub struct FileSystemScope {
    pub readable_roots: Vec<PathBuf>,
    pub denied_roots: Vec<PathBuf>,
}

pub struct WorkspaceWritePolicy {
    pub readable_roots: Vec<PathBuf>,
    pub writable_roots: Vec<PathBuf>,
    pub denied_roots: Vec<PathBuf>,
}

pub enum NetworkSandboxPolicy {
    Restricted,
    Enabled,
}
```

设计约束：

- `DangerFullAccess` 是显式权限，不是 provider unsupported 的 fallback。
- `denied_roots` 一旦进入 effective policy，不能通过 unsandboxed fallback 绕过。
- `workspace_roots` 从 session/project/workspace config 派生，不能由具体工具各自猜测。
- 后续 approval 只修改 effective policy，不修改原始 config default。

### Sandbox context

Facade 内部使用同一类 validated context；runtime 只能通过 `ToolSandboxPermit` 间接使用：

```rust
pub(crate) struct ToolSandboxContext {
    capability: SandboxCapability,
    mode: ToolSandboxMode,
    permissions: ToolPermissionProfile,
    cwd: PathBuf,
    workspace_roots: Vec<PathBuf>,
    target: LocalSandboxTarget,
}

pub(crate) enum LocalSandboxTarget {
    Noop,
    Provider(SandboxProviderPlatform),
}
```

`SandboxProviderPlatform` 直接复用 SPI 定义，不在 facade 再声明一套等价 platform enum。`WindowsRestrictedToken` 和 `WindowsElevated` 首版不实现 enforcing provider，但必须存在于 SPI 模型中，并由 placeholder 对两者返回明确 unsupported。当前 Windows host assembly 固定选择 restricted-token placeholder；Elevated 的产品选择入口和真实 enforcement 由未来 Windows Spec 定义。facade-facing unsupported error 包含 requested platform、requested user mode 和 missing capability。

`ToolSandboxContext` 必须通过 validated builder 构造，避免以下非法组合进入 runtime：

- `SandboxCapability::Enabled` 只能搭配 managed/restricted permission profile 和 `LocalSandboxTarget::Provider`。
- `SandboxCapability::Disabled` 只能搭配 `DangerFullAccess` 和 `LocalSandboxTarget::Noop`。
- `SandboxCapability::External` 不构造本地 `ToolSandboxContext`，而是进入 `ExternalSandboxEnforcement`。
- `ToolSandboxMode::UseDefault` 必须在 resolver 内完成解析，不能进入 effective context。

每个 allowed local decision 内部都必须携带一个 validated `ToolSandboxContext`，并由 `ToolSandboxPermit` 封装。Full Access 也生成 permit，只是 target 为 `LocalSandboxTarget::Noop`；不使用 `Option` 或“缺少 context”隐式表达权限。

`SandboxExecutionCompiler` 负责生成运行目标命令所必需的标准 runtime requirements，例如 sandbox-private temporary directory，并把 helper invocation 也视为普通 `ToolProcessCommand` 编译。Talos runtime 负责把 host file-system roots 编译为显式 readable roots；workspace/project roots 仍是产品拥有的 writable roots。该规则不枚举 HOME 子目录、Cargo、Rustup、Python、Node、package manager prefix 或隐藏目录，并由 denied roots 收窄敏感 read access。provider 可以添加平台固定的系统 executable/library roots 和 `/dev/null` 等运行条件，但不得增加任意 user data write access。两类隐式访问都必须集中定义并接受 contract test，不能由具体工具自行追加。`SandboxExecutionCommand.env` 表示经过 allowlist 构造的完整环境，不默认继承 Talos 进程中的全部变量；program 和 cwd 在进入 SPI 前必须解析为绝对路径。

### Platform SPI execution contract

`talos-sandbox-spi` 定义平台实现的最小稳定标准。SPI 输入已经完成 tools-domain policy compilation，平台 provider 只负责能力探测和 OS command transformation：

```rust
pub struct SandboxProviderCapabilities {
    pub file_system: FileSystemIsolationCapabilities,
    pub network: NetworkIsolationCapabilities,
    pub helper_required: bool,
}

pub enum SandboxProviderError {
    UnsupportedCapability {
        capability: SandboxIsolationCapability,
        reason: String,
    },
    ProbeFailed {
        reason: String,
    },
    InvalidIsolationSpec {
        reason: String,
    },
    PrepareFailed {
        reason: String,
    },
}
```

SPI 明确禁止出现 `UserPermissionMode`、`ApprovalPolicy`、`ToolSandboxMode`、`ToolPermissionProfile`、`ToolSandboxContext`、session id、project id、workspace label 和 governance event。`AbsolutePathBuf` 保证 platform provider 不必重新猜测 cwd-relative path；所有集合在进入 SPI 前必须去重、规范化并检查冲突。

`SandboxManager` 负责选择 provider、检查 support、映射 provider error 和产出 diagnostics。runtime 不直接知道 `sandbox-exec`、bubblewrap、seccomp、restricted token 等平台细节。SPI 只表示 Talos 能够准备并启动的本地 enforcing process，因此不包含 Noop 或 external execution。

Facade assembly 仍需提供 helper 的可执行路径，但该配置属于 `talos-tools-sandbox`：

```rust
pub struct SandboxRuntimePaths {
    pub macos_sandbox_executable: Option<PathBuf>,
    pub linux_helper_executable: Option<PathBuf>,
}
```

`SandboxRuntimePaths` 由 app/server assembly 解析并注入 facade。`macos_sandbox_executable` 允许可信 assembly 覆盖固定 Seatbelt executable，`linux_helper_executable` 通过静态组装传给 `LinuxSandboxProvider` constructor；两者都不进入每次 SPI request。任何 crate 都不能猜测 workspace `target` 目录，或依赖当前 working directory 偶然找到 helper。

目标静态组装形态：

```rust
pub fn build_sandbox_manager(
    runtime_paths: SandboxRuntimePaths,
) -> TalosResult<SandboxManager> {
    let assembly = build_host_providers(&runtime_paths);

    Ok(SandboxManager::new_with_unavailable(
        host_platform(),
        assembly.providers,
        assembly.unavailable,
    ))
}

#[cfg(target_os = "linux")]
fn build_host_providers(
    runtime_paths: &SandboxRuntimePaths,
) -> HostProviderAssembly {
    assemble_linux_provider(runtime_paths)
}
```

组装失败被保存在 provider diagnostics 中，不能折叠为通用 `unconfigured`，也不能切换 Noop。macOS 和 Windows 使用对应 target-specific dependency 与 factory。测试可以注入 fake provider，但生产组装没有按用户输入动态加载 provider 的入口。

### Tool file system

Runtime 只看到绑定 permit 的 handle：

```rust
pub struct ToolFileSystemHandle {
    context: Arc<ToolSandboxContext>,
    file_system: Arc<dyn ToolFileSystem>,
}

impl ToolFileSystemHandle {
    pub fn read_file<'a>(
        &'a self,
        path: &'a Path,
    ) -> ToolExecutionFuture<'a, Vec<u8>>;

    pub fn write_file<'a>(
        &'a self,
        path: &'a Path,
        contents: Vec<u8>,
    ) -> ToolExecutionFuture<'a, ()>;
}
```

Handle 内部调用 crate-private filesystem interface，调用方不能替换 context：

```rust
pub(crate) trait ToolFileSystem: Send + Sync {
    fn read_file<'a>(
        &'a self,
        path: &'a Path,
        sandbox: &'a ToolSandboxContext,
    ) -> ToolExecutionFuture<'a, Vec<u8>>;

    fn write_file<'a>(
        &'a self,
        path: &'a Path,
        contents: Vec<u8>,
        sandbox: &'a ToolSandboxContext,
    ) -> ToolExecutionFuture<'a, ()>;
}
```

实际接口需要覆盖 metadata、read directory、walk、copy、remove、create directory 和 atomic write。上面只是最小示意。

实现分层：

```text
ToolFileSystem
  DirectToolFileSystem
    - only accepts DangerFullAccess
  AuthorizingToolFileSystem
    - in-process canonical path checks
    - first implementation target
  SandboxedToolFileSystem
    - serializes operation to fs_helper
    - helper executes inside OS sandbox
    - target state for deny-read enforcement
```

`DirectToolFileSystem` 必须拒绝需要 sandbox enforce 的 context，不能“看起来能用”。

## 执行流

### Local exec

```text
ToolInvocation
  -> RegistryOrchestrator.plan()
  -> ToolSandbox.evaluate()
       -> EffectiveToolPermissionPolicyResolver
       -> PolicyPrecheck
       -> ToolSandboxDecision::Allowed(ToolSandboxPermit)
  -> BashRuntime / process runtime
  -> ToolSandboxPermit.spawn()
  -> internal ToolProcessExecutor
  -> talos-tools-sandbox::SandboxManager
       -> NoopBackend, or
       -> SandboxExecutionCompiler
       -> talos-sandbox-spi::SandboxExecutionRequest
       -> host SandboxProvider
       -> talos-sandbox-spi::PreparedSandboxCommand
  -> PreparedToolCommand + enforcement metadata
  -> tokio::process::Command::spawn()
```

Bash runtime 只负责 shell 参数、TTY、输出截断、session lifecycle 和 timeout。sandbox command transformation 不属于 bash runtime。

Restricted local sandbox execution uses a non-login shell. It must rely on the
sanitized inherited environment and must not source user startup files outside
the effective readable roots. Full Access keeps the existing login-shell
behavior. Restricted execution preserves the host HOME value and grants host
file systems read-only access so user-installed toolchains can load transitive
runtime files; only a sandbox-private TMPDIR and workspace/project roots are
writable. Hidden directories have no special policy and follow the same root
rules. Platform
providers may grant narrowly scoped read access to immutable OS shell runtime
files, such as macOS `/private/var/select/sh`, but must not add tool-specific
user directories.

### File tool

```text
Read / Edit / Write
  -> ToolSandbox.evaluate()
  -> ToolSandboxDecision::Allowed(ToolSandboxPermit)
  -> ToolSandboxPermit.file_system()
  -> ToolFileSystemHandle
  -> internal ToolFileSystem
  -> AuthorizingToolFileSystem or SandboxedToolFileSystem
  -> local fs operation or fs_helper
```

### Grep and Glob

`Glob` 如果是进程内 walk，应走 `ToolFileSystemHandle.walk()`。`Grep` 如果继续调用 `rg`，应作为进程型 tool 走 `ToolSandboxPermit::spawn()`；如果后续改为 library search，也必须走 permit-bound filesystem handle。不能让 search 工具成为读取策略绕过点。

## 平台策略

### Noop

目标内部执行策略：`NoopBackend`。

职责：

- 只服务 `DangerFullAccess` 或明确 disabled sandbox 的执行。
- 不做任何隔离声明，不返回 `sandboxed=true` 类 diagnostics。
- 如果 context 要求 read-only、workspace-write、deny-read 或 network restricted，必须返回 `SandboxBypassDenied` 或 `UnsupportedCapability`，不能直接执行。

`NoopBackend` 的存在是为了把“sandbox capability 明确关闭”建模清楚，而不是给 platform provider 失败时提供 fallback。

如果用户配置或 tool approval 明确进入 `DangerFullAccess`，sandbox capability 视为关闭，`NoopBackend` 是唯一正确本地执行策略。如果请求的是 `read_only`、`workspace_write` 或任何包含 denied roots/network restricted 的 profile，`NoopBackend` 不能被选择。

### macOS

目标 provider：`talos-macos-sandbox::MacosSandboxProvider`。

职责：

- 只消费 SPI `SandboxExecutionRequest`，根据 `FileSystemIsolationSpec`、`NetworkIsolationSpec` 和 runtime requirements 生成 seatbelt profile。
- 只调用固定路径 `/usr/bin/sandbox-exec`，不从 workspace 或普通 `PATH` 查找替代程序。
- 把 command 包装为 Seatbelt profile 执行。
- 支持 read-only、workspace-write 和 network restricted/enabled 能力探测。
- 独立完成 profile generation 和真实 Seatbelt integration tests，不依赖 tool runtime fixture。

首版必须明确记录系统不支持 `sandbox-exec` 或 profile 生成失败的错误，不允许静默 unsandboxed。

### Linux

目标 provider：`talos-linux-sandbox::LinuxSandboxProvider`。

职责：

- 只消费 SPI `SandboxExecutionRequest`，通过独立 Linux helper 执行 sandbox；helper path 由 facade static assembly 注入 provider constructor。
- 首期固定使用 system bubblewrap 构造只读根文件系统和显式 writable roots。
- restricted network 使用 bubblewrap network namespace，并在 inner stage 应用 `PR_SET_NO_NEW_PRIVS` 和 seccomp。
- capability preflight 检查 system `bwrap`、所需参数、user namespace 和 container `/proc` 能力；缺失时返回明确 unsupported/setup error。
- 首期不提供 Landlock fallback，也不提供 bundled bubblewrap。两者如需引入必须单独评估能力、分发方式和许可证。
- 独立完成 bwrap command generation、helper protocol 和真实 Ubuntu integration tests，不依赖 tool runtime fixture。

本地 Linux provider 验证使用 `tests/docker/ubuntu-24.04.Dockerfile`。镜像固定 Ubuntu 24.04 digest、Rust 1.88.0、`just` 1.53.0 和 `cargo-nextest` 0.9.100；运行命令为：

```bash
docker build \
  -f talos-rs/crates/action/linux-sandbox/tests/docker/ubuntu-24.04.Dockerfile \
  -t talos-sandbox-ubuntu24 .
docker run --rm --privileged \
  -e CARGO_TARGET_DIR=/tmp/talos-target \
  -v "$(dirname "$PWD"):/workspace" \
  talos-sandbox-ubuntu24
```

`--privileged` 只向 inner bubblewrap 提供 namespace/mount 前置能力；container 保持默认网络，network enabled positive control 必须先成功。当前 macOS/Apple Silicon 验证 native `aarch64` functional cases，并使用 `x86_64-unknown-linux-gnu` cross-check 验证另一条 seccomp cfg 编译路径。

Linux provider 不应把 fallback 作为成功。如果请求了首期未支持的 deny-read/split policy，或者当前 kernel/container 不能 enforce，应返回 `SandboxProviderError::UnsupportedCapability`。preflight 可以根据已知 container 限制显式选择 `--no-proc` 等受支持参数，但这属于 capability-based configuration，不是 unsandboxed fallback。

### Windows

目标 provider placeholder：`talos-windows-sandbox::WindowsSandboxProvider`。

首版行为：

- `SandboxProviderPlatform::WindowsRestrictedToken` 和 `SandboxProviderPlatform::WindowsElevated` 进入 SPI 类型系统。
- 当前 Windows host assembly 选择 `WindowsRestrictedToken`；`WindowsElevated` 仅作为 reserved SPI variant 由 placeholder contract 覆盖，不提供产品 requested-level 入口。
- provider 对两个 Windows SPI variant 的所有 request 返回 `UnsupportedCapability`，错误明确说明 Windows sandbox provider is not implemented。
- tests 覆盖两个 placeholder variant 和 facade unsupported error construction。

本 Spec 不定义 Windows token、job object、ACL、firewall、spawn hook 或 cleanup 方案。未来实现必须由独立 Windows sandbox Spec 完成，并继续实现 `SandboxProvider`；在此之前 crate 不包含半成品 enforcement path。

### External enforcement

目标模型：`ExternalSandboxEnforcement`，它不是 `SandboxProvider` 的实现。

```rust
pub struct ExternalSandboxEnforcement {
    pub source: ExternalEnforcementSource,
    pub declared_capabilities: ExternalSandboxCapabilities,
    pub evidence: Option<String>,
}

pub enum ExternalEnforcementSource {
    RemoteEnvironment,
    ServerDeclared,
    None,
}
```

职责：

- 表示执行位置不在 Talos 本地进程控制范围内，例如 remote environment、外部 connector 或声明自身隔离能力的 MCP server。
- 只生成 metadata、audit record 和 capability declaration，不进入 `ToolProcessExecutor`，也不构造 `SandboxExecutionRequest`。
- 必须记录 enforcement source，例如 `remote_environment`、`server_declared` 或 `none`。
- 不能把 metadata 传递成功等同于本地 OS sandbox 已生效。

调用方必须能区分“外部环境声明支持 sandbox”和“Talos 本地强制执行 sandbox”。这一区分需要进入 audit、diagnostics 和后续 UI 展示。

## External tools and MCP

MCP、plugin dynamic tools 和外部 connector 不能默认假设由 Talos 本地 OS sandbox enforce：

1. stdio MCP server 是外部子进程，可在后续通过 MCP launcher 获取 `ToolSandboxPermit` 并调用统一 spawn API；MCP runtime 不直接接入 `SandboxManager`，该能力也不属于 ToolRegistry。
2. streamable HTTP MCP 和 remote tools 不在本机执行，Talos 只能传递 sandbox metadata、approval state 和调用来源。
3. 不支持 sandbox metadata 的外部 server 不能声明为强 sandboxed。
4. Tool audit 需要记录外部 tool 的 sandbox enforcement source：`local_os`、`remote_environment`、`server_declared`、`none`。

## 配置模型

现有配置：

```rust
pub struct ToolSandboxPolicy {
    pub default_mode: ToolSandboxModeConfig,
}
```

首期产品配置为：

```rust
pub struct ToolSandboxPolicy {
    pub user_permission_mode: ToolUserPermissionModeConfig,
    pub default_mode: ToolSandboxModeConfig,
    pub network: ToolSandboxNetworkConfig,
    pub platform: ToolSandboxPlatformConfig,
}

pub enum ToolUserPermissionModeConfig {
    AskForApproval,
    ApproveForMe,
    FullAccess,
}

pub enum ToolSandboxNetworkConfig {
    UseDefault,
    Restricted,
    Enabled,
}

pub struct ToolSandboxPlatformConfig {
    pub linux_helper_executable: Option<PathBuf>,
}
```

配置语义：

- `user_permission_mode` 是产品级默认授权模式，invocation/session context 可以显式覆盖；planner mode 不能充当授权模式。
- `FullAccess` 是兼容现有产品行为的默认值，并明确产生 `SandboxCapability::Disabled`；其他模式进入受限 policy resolution。
- host provider 由 facade 按编译目标静态选择，runtime 不直接选择；Linux helper 路径由 application assembly 注入。
- Windows crate 保留 placeholder，调用稳定返回 unsupported，不提供半成品 enforcement。
- 修改配置 schema 时必须同步生成 Rust/TS schema 和文档。

## 错误语义

Facade 对 runtime 暴露以下稳定错误：

- `UnsupportedPlatform`: 当前 OS 没有对应 provider。
- `ProviderUnavailable`: 当前 OS 有 provider 定义，但 application assembly 未提供所需 helper/provider。
- `UnsupportedCapability`: provider 存在但不能 enforce requested capability。
- `PolicyDenied`: policy 明确拒绝请求。
- `SandboxSetupFailed`: provider probe、profile、helper 或 command 准备失败。
- `SandboxExecutionFailed`: sandboxed process 启动或运行失败。
- `SandboxBypassDenied`: 调用方试图用 direct FS 或 unsandboxed exec 执行需要 sandbox 的 context。

Facade 同时通过 `TalosSandboxErrorCategory` 提供稳定分类：unsupported platform、provider unavailable、unsupported capability、setup failed、execution failed、bypass denied、filesystem denied/failed、invalid context 和 permission denied。SPI provider 只返回 `SandboxProviderError`，由 `talos-tools-sandbox` 在唯一 mapper 中转换为上述错误。组装 diagnostics 必须区分未配置、构造失败和 probe 失败；provider capability error 必须保留 requested platform、user mode 和 missing capability。平台 provider 不得构造 `PolicyDenied` 或 approval error，因为它不知道业务授权语义。不得把 setup failed、unsupported 和 policy denied 都映射成普通 tool execution failed。用户和 audit 需要能判断“没权限”和“平台能力未实现”的差异。

## Closed implementation gates

首期实现前的五个 design gates 已关闭：

1. Facade 沿真实 `RegistryOrchestrator -> ToolHandler -> Tool runtime` 链路接入；permit 由 `ToolDispatchContext` 绑定，并作为 `ToolContext` 的 private ephemeral execution field 转交，不进入共享 session state。
2. SPI 使用 validated constructor，并采用本 Spec 的 root precedence、normalization、symlink ownership、program/cwd contract 和 portable hard limits。
3. Provider 在 facade assembly 时 probe 一次并保存 immutable snapshot；`support()` 是 deterministic pure decision，刷新通过重建 facade 完成。
4. Linux launcher wire 归 Linux provider 私有，使用 versioned bounded serde protocol；built-in fs helper 协议归 facade 私有。
5. Shared provider fixtures 归 SPI 的非默认 `provider-contract-tests` feature，平台 crate 仅在测试依赖中启用。

这些决定是阶段 2-4 的实现约束。实现中如发现现有调用链事实与决定冲突，必须先修订 Spec 和 Plan，不能通过新增并行 context、重复 DTO 或 fallback 绕过。

### Deferred to a future Windows sandbox spec

Windows 首期没有 launch-lifecycle TODO，也不阻塞 macOS/Linux 实现。未来独立 Spec 负责选择 Windows isolation mechanism、扩展或调整 workspace-internal SPI、定义 capability matrix 和真实 E2E；该工作不能改变 `ToolSandbox::evaluate()`、`ToolSandboxPermit::spawn()` 和 runtime-facing error/enforcement 语义。

### Follow-up capability TODOs

1. 完成 `SandboxedToolFileSystem` 和 bounded fs helper 后，再把 built-in file tools 的 enforcement source 从 `in_process_authorizer` 提升为 `local_os`。
2. stdio MCP 只有通过同一个 permit/compiler/SPI/provider 路径启动后，才能声明 `local_os`；remote MCP 继续使用 external enforcement metadata。
3. managed proxy、split filesystem policy、Landlock、bundled bubblewrap 和资源配额必须分别由后续 capability/spec 驱动，不提前扩展 SPI。
4. Session 权限模式、Bash 结构化风险判断、Sandbox 审批、`AskUserQuestion`、Timeline resume 和 TUI/WebUI/Desktop 交互由 `feat-037-tool-permission-user-interaction` 统一设计；本 Spec 继续只拥有 permission resolver、permit、execution facade、SPI 和 provider enforcement。

## Impacted areas

- `talos-rs/crates/action/tools-sandbox`
- `talos-rs/crates/action/sandbox-spi`，新增 workspace-internal SPI crate
- `talos-rs/crates/action/macos-sandbox`，新增 macOS provider crate
- `talos-rs/crates/action/linux-sandbox`，新增 Linux provider/helper crate
- `talos-rs/crates/action/windows-sandbox`，新增 Windows placeholder provider crate
- `talos-rs/crates/action/tools`
- `talos-rs/crates/infra/config`
- `talos-rs/crates/common/fs` 仅在复用 atomic/local filesystem primitive 时受影响；sandbox policy 和 `ToolFileSystem` ownership 仍属于 `talos-tools-sandbox`
- Tool audit / event sink
- Prompt assembler 中 sandbox 状态展示
- App server / SDK schema，如果 sandbox policy 或 diagnostics 对外暴露
- MCP runtime launcher，作为后续接入点
- `talos-e2e` project，新增 `scenarios/14_sandbox` scenario group

## 增量接入边界

以下顺序只描述架构依赖关系，不作为 implementation plan 或阶段状态来源。真正实现时仍应在单独的 `IMPLEMENTATION_PLAN.md` 中拆分阶段、状态和验证进度。

### 抽象与 plan 数据流

建立 `ToolSandbox` facade、request/decision/permit、`ToolPermissionProfile`、internal `ToolSandboxContext`、`LocalSandboxTarget`、`SandboxExecutionCompiler`、`SandboxManager` 和 facade error。建立 `talos-sandbox-spi` 的 request/result、platform/capability/error 和 `SandboxProvider` trait，并使用 fake provider 验证 contract。`RegistryOrchestrator` 产出的 constraints 必须能进入 `ToolSandbox::evaluate()`，但暂不改变实际执行行为。

验证重点：

- `ToolSandboxModeConfig` 到 effective profile 的映射确定。
- compiler 输出不包含 tools-domain 类型或 workspace 产品语义。
- crate dependency check 确保 runtime 不依赖 SPI/platform，platform 只依赖 SPI。
- Windows platform 能被编译、选择 provider 并返回 unsupported。
- `DangerFullAccess` 不构造 SPI request。
- governance event 记录 effective sandbox mode。

### Bash/local exec 接入

Bash runtime 改为只调用 `ToolSandboxPermit::spawn()`。facade 内部通过 `ToolProcessExecutor` 消费 `PreparedToolCommand`；macOS/Linux provider 分别在独立 crate 提供最小可运行实现，Windows provider 对 enforcing request 返回 unsupported。

验证重点：

- read-only/workspace-write 下 bash 命令经过 compiler、SPI 和 provider 启动。
- requested escalation 在未授权时变成 `RequiresApproval` 或 `PolicyDenied`；只有新的 effective policy 才能映射到 `DangerFullAccess`。
- provider unsupported 不会静默直接 spawn 或切换到 Noop。
- stdout/stderr/session lifecycle 行为保持不变。

### ToolFileSystem 与文件工具收口

引入 public `ToolFileSystemHandle` 和 crate-private `ToolFileSystem`，把 `Read`、`Write`、`Edit`、`Glob` 的核心文件访问迁移到 permit-bound handle。第一版内部使用 `AuthorizingToolFileSystem` 做 canonical path checks。

验证重点：

- read-only 拒绝 write/edit。
- workspace-write 只允许写 workspace roots。
- denied roots 不能被 symlink 或 `..` 绕过。
- 工具行为测试比较完整 response，不只验证 OK。

### Sandboxed file helper

实现 `fs_helper` 子进程，把文件操作请求序列化给 helper，并通过 `SandboxManager` 将 helper 放进 OS sandbox。

验证重点：

- helper 进程受到和 exec tool 一致的 filesystem/network policy。
- deny-read 由 OS enforce。
- helper JSON request/response 有大小上限和错误分类。
- helper 不继承不必要的环境变量。

### MCP launcher 接入点

为 stdio MCP launcher 增加可选 sandbox context。第一版只支持本地 stdio MCP，HTTP/remote MCP 只记录 sandbox metadata。

验证重点：

- ToolRegistry 不拥有 MCP lifecycle。
- MCP runtime snapshot 暴露 sandbox enforcement source。
- 不支持 sandbox 的外部 server 不被标记为强隔离。

## Verification strategy

自动化验证分层：

- `talos-tools-sandbox` 单元测试：policy mapping、context validation、compiler output、provider selection、provider error mapping、Noop routing。
- `talos-sandbox-spi` contract tests：规范化 DTO、capability/support 和 provider error contract；SPI public surface 不含 tools-domain 类型。
- 各 platform crate 独立集成测试：同一组 SPI request scenarios 转换为真实平台隔离命令并验证副作用。
- `talos-tools` 集成测试：registry constraints 到 `ToolSandbox::evaluate()`，以及 allowed permit 到 runtime 的传递。
- bash runtime 测试：facade executor 接入、escalation、unsupported platform；runtime 不断言 provider 内部参数。
- file runtime 测试：read/write/edit/glob 在 read-only/workspace-write 下的行为。
- symlink/path traversal 测试：确认 canonicalization 后 enforce。
- schema 测试：配置 shape 变更后同步 Rust/TS schema。
- 依赖边界测试或 workspace metadata check：runtime 只依赖 `talos-tools-sandbox`，platform crate 只依赖 `talos-sandbox-spi`。
- 平台集成测试：macOS/Linux 分别验证真实 OS sandbox；Windows 只验证 unsupported diagnostics，直到 provider 实现。

### Sandbox E2E group

Sandbox 的平台验收统一放在仓库内 `talos-e2e` project 的 `14_sandbox` group，并通过 Talos 根目录入口运行：

```bash
just e2e-group 14_sandbox
```

测试不依赖真实 LLM。scenario 必须通过真实 `ToolInvocation -> RegistryOrchestrator -> ToolSandbox.evaluate() -> ToolSandboxPermit -> spawn/file handle -> compiler/SPI/provider 或 internal ToolFileSystem` 路径发起调用，避免模型输出不确定性影响权限验收。

Runner matrix：

| Host | macOS cases | Linux cases | Expected behavior |
| --- | --- | --- | --- |
| macOS developer machine | Native | Ubuntu Docker | Both required for local acceptance |
| Linux developer/CI machine | Not applicable | Native Ubuntu-compatible environment | Linux required |
| Windows | Not applicable | Not applicable | Only pure platform-selection/unsupported tests until implemented |

macOS cases 使用本机构建的 Talos binary 和 `/usr/bin/sandbox-exec`。Linux cases 使用固定版本的 Ubuntu image 或 Talos-owned Dockerfile，在 container 内安装/构建 Talos 和 system `bubblewrap`，并执行同一套 platform-neutral scenario assertions。Docker 只是 Linux host fixture；被测进程仍必须报告 `LinuxSandboxProvider` 和 `local_os` enforcement。

`14_sandbox` group 至少包含以下 cases：

1. `provider_preflight`: facade selection, helper discovery, provider capability report and enforcement source are correct。
2. `process_runtime`: private TMPDIR is writable and user-installed tools execute with host-wide read access and no tool-specific allowlists。
3. `process_filesystem`: workspace read succeeds; read-only write fails; workspace-write succeeds inside writable roots and fails outside them。
4. `escape_attempts`: `..`, symlink traversal and a nested child process cannot write outside effective writable roots。
5. `network_policy`: a deterministic local TCP endpoint is reachable with network enabled and unreachable with network restricted。
6. `file_tools`: `Read`、`Write`、`Edit` and `Glob` are exercised through real tool dispatch and `ToolFileSystemHandle`；outside writes fail under the authorizer。
7. `full_access_control`: Full Access performs an operation denied by the restricted profile and audit reports `NoopBackend` / enforcement source `none`。
8. `no_silent_fallback`: a valid Sandbox context deliberately points at an unavailable trusted provider/helper executable；provider preflight fails without executing the original command, switching to Noop or creating the sentinel output file。

E2E assertions 必须同时验证 result/error categories 和外部副作用。正常的 shell positive control 还必须验证 stdout/stderr，禁止把 shell startup 阶段的 sandbox denial 当作成功结果。只有 target 保持不变，并且 audit/diagnostics 能识别 enforcing provider 时，denied operation 才算通过。测试必须包含 positive controls，避免把 host permission、缺少 command 或 container configuration 造成的失败误判为 sandbox 生效。

Linux network tests 不能用 `--network none` 启动外层 Docker container，否则 denial 来自 Docker 而不是 Talos。Enabled control 必须先能够访问 test endpoint，随后 restricted denial 才能证明 Talos sandbox enforcement 生效。

Ubuntu fixture 必须定义准确的 image、architecture policy、system `bwrap` 安装方式和必要 container options。Scenarios 运行前，preflight 检查 user namespace、bubblewrap arguments、seccomp 和 `/proc` behavior。配置的验收环境缺少必要能力时，group 必须带 `UnsupportedCapability` diagnostics 失败，不能静默 skip 或 unsandboxed 执行。

手工验证：

- 在 macOS 下执行 HOME/PATH 中的用户工具、读取 workspace 文件、写 workspace 文件并尝试写 workspace 外文件。
- 在本地 Ubuntu Docker 中执行同等命令，并确认 denial 来自 Talos Linux provider 而不是 Docker 外层限制。
- 在 Windows 下请求 read-only/workspace-write，确认返回明确 unsupported，而不是直接执行。
- 对 `Read`、`Write`、`Edit`、`Glob` 走真实用户路径验证权限表现。

## Open risks and follow-ups

1. macOS `sandbox-exec` 在不同系统版本上的可用性需要实测。
2. Linux system bubblewrap 对 user namespace、container seccomp 和 `/proc` 的要求需要由 preflight 与 Ubuntu E2E 持续验证。
3. 进程内 authorizer 不能完整替代 OS deny-read，只能作为迁移阶段能力。
4. `rg` 类搜索工具可能通过子进程读取文件，必须尽早接入 `ToolSandboxPermit::spawn()`，否则会成为 read policy 绕过点。
5. Windows provider 仅为 placeholder；真实 enforcement、capability 和 launch lifecycle 由后续独立 Spec 定义。
6. fs helper 会引入 JSON 协议和子进程生命周期，需要严格限制 request/response 大小和环境变量。
7. 外部 MCP 的 sandbox enforcement source 必须对用户和 audit 诚实展示，不能只传 metadata 就宣称本地强隔离。
8. 选择性迁移 Codex Apache-2.0 source 和未来可能分发 bubblewrap 时，必须持续维护第三方来源与许可证声明。
