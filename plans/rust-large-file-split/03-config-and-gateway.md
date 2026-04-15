## 阶段 1: Freeze Infrastructure Entry Boundaries
**目标**: Define the stable split boundaries for `config/src/lib.rs` and `gateway/http/src/server.rs` before moving implementation details.
**成功标准**: The target module trees for config loader/paths/validate/domains and HTTP state/errors/auth/router/handlers/lifecycle are fixed, and low-value fragments like logging config are explicitly kept inline unless they genuinely expand.
**测试**: `cargo test -p talos-config` and `cargo test -p talos-gateway-http`
**状态**: 已完成

### Baseline Line Counts

Current oversized file sizes before any split work:

- `config/src/lib.rs`: `2511`
- `gateway/http/src/server.rs`: `1569`

Files already at acceptable size (no split needed):

- `gateway/http/src/lib.rs`: `7` (facade — stays unchanged throughout)

### Frozen Export Surfaces

`config/src/lib.rs` current public surface (all types are directly exported from crate root):

```
// Functions
pub fn default_timezone() -> String
pub fn parse_timezone(input: &str) -> TalosResult<TalosTimezone>
pub fn set_talos_home(path: impl Into<PathBuf>) -> TalosResult<()>
pub fn resolve_talos_home() -> PathBuf
pub fn resolve_runtime_path(path: &str) -> PathBuf
pub fn sanitize_agent_name(agent_name: &str) -> String
pub fn generate_default_config() -> String

// Core types
pub type TalosTimezone = chrono_tz::Tz
pub struct Config { timezone, agent, gateway, channels, logging, storage, llm }
pub struct TalosPaths { talos_home, conf_dir, config_file, logs_dir, log_file, data_dir, sqlite_db }

// Domain types (all pub)
AgentConfig, AgentKernelConfig, AgentKernelBindingConfig
AgentKernelBootstrapPromptConfig, AgentKernelPromptConfig, BootstrapPromptWarningMode
AgentKernelMemoryConfig
AgentKernelMcpConfig, McpServerConfig, McpTransportConfig
ToolsRuntimeConfig, ToolApprovalPolicy, ToolSandboxPolicy, ToolSandboxModeConfig
LocalExecToolConfig, FileToolConfig, McpToolConfig, DynamicToolConfig, DynamicToolTimeoutBehavior
GatewayConfig, LlmConfig, ProviderConfig, ProviderModelConfig
ModelInputKind, ModelCostConfig, ModelCompatConfig, ThinkingFormat
ChannelsConfig, ChannelConfig, TelegramChannelsConfig, TelegramAccountConfig
TelegramGroupConfig, TelegramTopicConfig
StorageBackendConfig, DatabaseType
PlannerMode, PlannerSelection, PlanReActPolicy, ExecutionMode
```

`gateway/http/src/lib.rs` current public surface (must stay unchanged):

```
pub use server::{HttpServer, ServerLifecycle, ServerLifecycleState};
```

### Precise Boundary Notes — `config/src/lib.rs`

| Block | Lines | Target module |
|---|---|---|
| Imports + `TALOS_HOME_OVERRIDE` static | 27–36 | `paths.rs` |
| `default_timezone`, `parse_timezone` | 88–100 | `lib.rs` (public API, keep at root) |
| `TalosPaths` struct + impl | 102–186 | `paths.rs` |
| `set_talos_home`, `resolve_talos_home`, `resolve_runtime_path`, `sanitize_agent_name`, `normalize_path` | 188–251 | `paths.rs` |
| `DatabaseType`, `StorageBackendConfig` + defaults | 253–315 | `domains/storage.rs` |
| `AgentConfig`, `PlannerMode`, `PlannerSelection`, `PlanReActPolicy`, `ExecutionMode` + defaults | 317–473 | `domains/agent.rs` |
| `AgentKernelConfig` struct | 475–557 | `domains/kernel.rs` |
| `ToolsRuntimeConfig`, all tool policy types + defaults | 559–668 | `domains/tools.rs` |
| `AgentKernelMcpConfig`, `McpServerConfig`, `McpTransportConfig` | 694–735 | `domains/tools.rs` (merge with tools) |
| Kernel default functions | 737–803 | `domains/kernel.rs` |
| `BootstrapPromptWarningMode`, `AgentKernelBootstrapPromptConfig`, `AgentKernelPromptConfig` + defaults | 805–958 | `domains/prompt.rs` |
| `AgentKernelMemoryConfig` + defaults | 960–1101 | `domains/memory.rs` |
| `AgentKernelBindingConfig` + `Default for AgentKernelConfig` | 1104–1149 | `domains/kernel.rs` |
| `GatewayConfig` + defaults | 1151–1197 | `domains/network.rs` |
| `ChannelsConfig`, `ChannelConfig`, all Telegram types + defaults | 1199–1380 | `domains/network.rs` |
| `LlmConfig`, `ProviderConfig`, `ProviderModelConfig`, `ModelInputKind`, `ModelCostConfig`, `ModelCompatConfig`, `ThinkingFormat` + defaults | 1382–1537 | `domains/network.rs` |
| `impl Config { talos_paths, apply_derived_paths, current_provider_config, provider_config }` | 1538–1572 | `lib.rs` (keep on Config) |
| `impl Config { load, load_from_path, save, default_path, resolve_env_placeholders, resolve_value_placeholders, expand_env_in_string }` | 1574–1700 | `loader.rs` |
| `impl Config { validate_runtime }` | 1702–1990 | `validate.rs` |
| `REQUIRED_CONFIG_PATHS`, `FORBIDDEN_CONFIG_PATHS`, `validate_required_paths`, `validate_forbidden_paths`, `has_json_path`, `is_unresolved_placeholder` | 1992–2094 | `validate.rs` |
| `generate_default_config` | 2096–2231 | `loader.rs` |
| `#[cfg(test)] mod tests` | 2233–2511 | `tests.rs` |

### Precise Boundary Notes — `gateway/http/src/server.rs`

| Block | Lines | Target module |
|---|---|---|
| Imports | 1–31 | distributed to each submodule |
| `ApiErrorBody`, `ApiError`, `IntoResponse impl` | 35–63 | `server/errors.rs` |
| `HttpApiState` struct + constructors | 65–118 | `server/state.rs` |
| `ServerLifecycleState`, `ServerLifecycle` | 120–175 | `server/lifecycle.rs` |
| `HttpServer` struct + builder methods + `run` + `run_with_shutdown` + `listen_addr` | 177–327 | `server/mod.rs` |
| `lifecycle_gate_middleware`, `auth_middleware`, `mock_auth_result` | 329–404 | `server/auth.rs` |
| `health_handler`, `list_models_handler` | 406–419 | `server/misc.rs` |
| `CreateSessionRequest`, `create_session_handler`, `list_sessions_handler`, `get_session_handler`, `get_session_turns_handler`, `patch_session_handler`, `close_session_handler`, `cancel_active_turn_handler` | 421–608 | `server/sessions.rs` |
| `session_message_handler`, `stream_session_message`, `process_session_message`, `PatchSessionRequest`, `ToolApprovalRequest`, `DynamicToolResultRequest` | 610–868 | `server/messages.rs` |
| `validate_text_input`, `validate_model_supported`, `validate_auth_context`, `talos_event_name`, `shutdown_signal` | 870–1002 | `server/misc.rs` |
| `#[cfg(test)] mod tests` | 1004–1569 | `server/tests.rs` |

---

## 阶段 2: Split `config/src/lib.rs` Into Domain Modules
**目标**: Move `lib.rs` into `paths.rs`, `loader.rs`, `validate.rs`, `domains/{agent,kernel,tools,prompt,memory,network,storage}.rs`, and `tests.rs`, keeping `Config` in `lib.rs` as facade root.
**成功标准**: `lib.rs` holds only `Config` struct + base impl + submodule declarations + re-exports; all domain types re-exported through facade; `default_timezone` remains pub at crate root; behavior unchanged.
**测试**: After each moved submodule run `cargo test -p talos-config`; after completing this phase run `cargo test -p talos-config`
**状态**: 已完成

### Target Layout

```
config/src/
  lib.rs              (~300 lines)  Config struct, Default, apply_derived_paths, talos_paths,
                                    current_provider_config, provider_config;
                                    default_timezone, parse_timezone (pub — keep at root);
                                    mod declarations + pub use re-exports
  paths.rs            (~155 lines)  TalosPaths, TALOS_HOME_OVERRIDE, set_talos_home,
                                    resolve_talos_home, resolve_runtime_path, sanitize_agent_name,
                                    normalize_path
  loader.rs           (~390 lines)  impl Config { load, load_from_path, save, default_path,
                                    resolve_env_placeholders, resolve_value_placeholders,
                                    expand_env_in_string }; generate_default_config
  validate.rs         (~390 lines)  impl Config { validate_runtime }; REQUIRED_CONFIG_PATHS,
                                    FORBIDDEN_CONFIG_PATHS; validate_required_paths,
                                    validate_forbidden_paths, has_json_path, is_unresolved_placeholder
  domains/
    mod.rs            (~60 lines)   pub use submodules — no logic
    agent.rs          (~250 lines)  AgentConfig, PlannerMode, PlannerSelection, PlanReActPolicy,
                                    ExecutionMode + defaults
    kernel.rs         (~200 lines)  AgentKernelConfig, AgentKernelBindingConfig + defaults
    tools.rs          (~220 lines)  ToolsRuntimeConfig, ToolApprovalPolicy, ToolSandboxPolicy,
                                    ToolSandboxModeConfig, LocalExecToolConfig, FileToolConfig,
                                    McpToolConfig, DynamicToolConfig, DynamicToolTimeoutBehavior,
                                    AgentKernelMcpConfig, McpServerConfig, McpTransportConfig + defaults
    prompt.rs         (~200 lines)  BootstrapPromptWarningMode, AgentKernelBootstrapPromptConfig,
                                    AgentKernelPromptConfig + defaults
    memory.rs         (~145 lines)  AgentKernelMemoryConfig + defaults
    network.rs        (~400 lines)  GatewayConfig, ChannelsConfig, ChannelConfig,
                                    TelegramChannelsConfig, TelegramAccountConfig,
                                    TelegramGroupConfig, TelegramTopicConfig,
                                    LlmConfig, ProviderConfig, ProviderModelConfig,
                                    ModelInputKind, ModelCostConfig, ModelCompatConfig,
                                    ThinkingFormat + defaults
    storage.rs        (~70 lines)   DatabaseType, StorageBackendConfig + defaults
  tests.rs            (~278 lines)  #[cfg(test)] mod tests
```

### Dependency Direction Constraints

```
paths.rs         → external only (talos_error, std)
domains/storage.rs  → paths.rs (default_storage_connection_string calls TalosPaths::resolve())
domains/agent.rs    → external only (talos_error, serde)
domains/tools.rs    → external only (serde, std::collections)
domains/prompt.rs   → uses crate::default_timezone (for AgentKernelConfig.timezone field default)
domains/memory.rs   → paths.rs (default_kernel_memory_workspace_dir calls TalosPaths::resolve())
domains/kernel.rs   → domains/{agent,tools,prompt,memory} (nested sub-configs)
domains/network.rs  → external only (serde, talos_types for ModelInputKind/ModelCostConfig/ModelCompatConfig)
loader.rs        → paths.rs, validate.rs
validate.rs      → paths.rs
lib.rs           → paths.rs, loader.rs, validate.rs, domains/*
```

### Serde Default Function Conventions

- `default_true()` — trivial 1-liner; **duplicate** as a private `fn` in each domain module that uses it
- `default_timezone()` — public API; stays in `lib.rs`; reference in domain serde attrs as `"crate::default_timezone"`
- All other `default_*` functions — private module-local; move with the domain type that uses them

### Recommended Migration Order

1. Create `domains/storage.rs` — move `DatabaseType`, `StorageBackendConfig` (no inbound deps from production code beyond same file)
2. Create `domains/agent.rs` — move `AgentConfig`, `PlannerMode`, `PlannerSelection`, `PlanReActPolicy`, `ExecutionMode`
3. Create `domains/tools.rs` — move all tool policy types + `AgentKernelMcpConfig`, `McpServerConfig`, `McpTransportConfig`
4. Create `domains/prompt.rs` — move prompt config types (references `crate::default_timezone`)
5. Create `domains/memory.rs` — move memory config (references `paths::TalosPaths` for workspace default)
6. Create `domains/kernel.rs` — move `AgentKernelConfig` + `AgentKernelBindingConfig` (depends on steps 2–5)
7. Create `domains/network.rs` — move all gateway, channel, telegram, LLM types
8. Create `domains/mod.rs` — thin re-export facade
9. Create `paths.rs` — move `TalosPaths`, path helpers, `TALOS_HOME_OVERRIDE`
10. Create `validate.rs` — move `validate_runtime` + path validation helpers
11. Create `loader.rs` — move load/save methods + `generate_default_config`
12. Create `tests.rs` — move `#[cfg(test)] mod tests`
13. Shrink `lib.rs` — keep only Config struct + base impl + re-exports + module declarations

---

## 阶段 3: Split HTTP Server By Protocol Responsibility
**目标**: Move `server.rs` into `server/{mod,state,lifecycle,errors,auth,misc,sessions,messages,tests}.rs`, separating middleware, lifecycle, and handler domains.
**成功标准**: Route registration in `mod.rs` is the only place referencing handler names; auth middleware is no longer embedded in request handlers; SSE and sync message handling live in `messages.rs`.
**测试**: After each moved submodule run `cargo test -p talos-gateway-http`; after completing this phase run `cargo test -p talos-gateway-http`
**状态**: 已完成

### Target Layout

```
gateway/http/src/
  lib.rs              (7 lines, UNCHANGED)
  server/
    mod.rs            (~150 lines)  HttpServer struct, builder methods, run, run_with_shutdown
                                    (Router wiring referencing all handlers), listen_addr
    state.rs          (~55 lines)   HttpApiState struct + new + new_with_lifecycle
    lifecycle.rs      (~80 lines)   ServerLifecycleState enum, ServerLifecycle struct
    errors.rs         (~45 lines)   ApiErrorBody, ApiError + IntoResponse impl
    auth.rs           (~90 lines)   auth_middleware, lifecycle_gate_middleware, mock_auth_result
    misc.rs           (~90 lines)   health_handler, list_models_handler, validate_text_input,
                                    validate_model_supported, validate_auth_context,
                                    talos_event_name, shutdown_signal
    sessions.rs       (~200 lines)  CreateSessionRequest, PatchSessionRequest,
                                    create/list/get/get_turns/patch/close/cancel handlers
    messages.rs       (~280 lines)  session_message_handler, stream_session_message,
                                    process_session_message, ToolApprovalRequest,
                                    DynamicToolResultRequest, submit_tool_approval_handler,
                                    submit_dynamic_tool_result_handler
    tests.rs          (~565 lines)  #[cfg(test)] mod tests
  server.rs → DELETED
```

### Dependency Direction Constraints

```
errors.rs    → external only (axum, serde)
lifecycle.rs → external only (tokio::sync::watch, std::sync::atomic)
state.rs     → lifecycle.rs (holds Arc<ServerLifecycle>); external (axum, talos_auth, talos_types)
auth.rs      → state.rs, errors.rs
misc.rs      → state.rs, errors.rs (uses HttpApiState via State extractor)
sessions.rs  → state.rs, errors.rs (uses HttpApiState, ApiError)
messages.rs  → state.rs, errors.rs (uses HttpApiState, ApiError)
mod.rs       → state.rs, lifecycle.rs, auth.rs, misc.rs, sessions.rs, messages.rs
tests.rs     → mod.rs, state.rs, lifecycle.rs (uses HttpApiState::new_with_lifecycle)
```

### Key Visibility Rules

- `HttpApiState` — keep `pub(super)` (used only within `server/` subtree, not exported from crate)
- `ServerLifecycle`, `ServerLifecycleState` — keep `pub` (exported from `lib.rs`)
- `HttpServer` — keep `pub` (exported from `lib.rs`)
- `ApiError`, `ApiErrorBody` — `pub(super)` (internal to `server/` subtree)
- Handler fns (`create_session_handler` etc.) — `pub(super)` (called only from `mod.rs` router)
- Auth fns — `pub(super)`

### Recommended Migration Order

1. Create `server/errors.rs` — move `ApiErrorBody`, `ApiError` (no local deps)
2. Create `server/lifecycle.rs` — move `ServerLifecycleState`, `ServerLifecycle` (no local deps)
3. Create `server/state.rs` — move `HttpApiState` struct + constructors (depends on lifecycle.rs)
4. Create `server/auth.rs` — move middleware fns + `mock_auth_result` (depends on state.rs)
5. Create `server/misc.rs` — move health/models handlers + validators + `talos_event_name` + `shutdown_signal`
6. Create `server/sessions.rs` — move session CRUD handlers + request structs
7. Create `server/messages.rs` — move message + stream + tool handlers + request structs
8. Create `server/tests.rs` — move `#[cfg(test)] mod tests` with explicit imports
9. Create `server/mod.rs` — `HttpServer` struct + run + `run_with_shutdown` (Router wiring) + module declarations
10. Delete `server.rs` — activate directory module

---

## 阶段 4: Verify Infrastructure Behavior And Cleanup
**目标**: Remove transitional code, verify config loading behavior and HTTP API behavior, and keep both entry files as thin facades.
**成功标准**: `config/lib.rs` and `gateway/http/server/mod.rs` are both facade-oriented, no dead code remains, and infrastructure behavior matches the pre-split baseline.
**测试**: `cargo test -p talos-config`, `cargo test -p talos-gateway-http`, and `cd talos-rs && tests/integration/run_e2e.sh`
**状态**: 已完成

### Latest Verification Snapshot

- `cargo test -p talos-config`: PASS
- `cargo test -p talos-gateway-http`: PASS
- `tests/integration/run_e2e.sh`: PASS (`8/8`)
