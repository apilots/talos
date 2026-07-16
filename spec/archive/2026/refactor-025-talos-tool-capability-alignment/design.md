# Talos Tool Capability Alignment 设计

Status: archived
Archived: 2026-07-14
Owner: agent
Created: 2026-06-15
Updated: 2026-06-15

## 背景

Talos 目标是实现接近 Claude Code 的代码协作体验。Claude Code 是本 spec 的能力参考来源，但 Talos 不复制其 planner、team/task 或 UI 概念。当前 tools 系统已经具备 `ToolRegistry`、`ToolRouter`、local / MCP / dynamic / collaboration tool source、deferred activation、tool execution envelope 等基础能力，也已经有 `Read`、`Edit`、`Write`、`Grep`、`Glob`、`Bash`、`WebSearch`、`WebFetch`、`TodoWrite`、`Agent`、`Skill` 等工具入口。

主要差距不在工具名，而在每个工具 runtime、权限语义、结果格式、UI 可表达性和上下文治理尚未达到稳定代码协作所需水平。

## 目标

1. 以 Claude Code 为参考，对齐 Talos 系统 tools 的能力边界和产品语义。
2. 将工具按能力分组补齐，而不是分散地堆工具名。
3. 明确每个工具需要增强的 runtime 行为、权限行为和返回格式。
4. 审视现有 tool execution result 是否足够表达 diff、progress、引用、长期任务和 artifact 结果。
5. 保持 WebUI、Desktop、TUI、SDK、replay 消费同一套 tools 结果定义，协议层只做 transport 映射。

## 非目标

- 不逐字复制 Claude Code 源码。
- 不复刻 Claude Code 的 planner、team/task 或 coordinator 概念；Talos 继续以 BuiltIn `general_turn` 为唯一主路径。
- 不引入独立意图分类器或硬编码用户输入前缀路由。
- 不把 HTTP / JSON-RPC 协议层变成业务逻辑层。
- 不为旧格式保留弱化版工具行为；当前项目未上线，优先最佳方案。
- 不在本 spec 中定义具体 UI 视觉稿，只定义 UI 必须能表达的结构化数据。
- 不在本轮实现 LSP、NotebookEdit、Agent task/team 语义；这些能力另起后续 spec。
- 不恢复阻塞式 tool approval waiting；风险控制通过 governance policy、permission decision、risk classification 和 sandbox 执行策略表达。

## 当前事实

| 领域 | 当前状态 | 结论 |
| --- | --- | --- |
| 工具注册 | 已有 `ToolRegistry` / `RegisteredToolSpec` / deferred selection | 可复用 |
| 文件工具 | 有 `Read` / `Edit` / `Write`，含 read-before-edit/write 检查 | 需要增强 runtime |
| 搜索工具 | 有 `Grep` / `Glob`，但 `Grep` 不是真 ripgrep | 需要重做搜索 runtime |
| Shell | 有 sessionized `Bash` / `TaskOutput` / `TaskStop` | 需要补安全语义、权限决策和风险分类 |
| Web | `WebSearch` 支持正式 search provider；`WebFetch` 简单 HTML text | provider 已接入，页面读取仍需增强 |
| Patch | config 有 `enable_apply_patch`，但无工具实现 | 必须补齐 |
| MCP / Skill | 有基础 MCP tool 和 `Skill` / `ToolSearch` | 需要 resource 与 lazy discovery |

## 工具分组与增强清单

### 1. 文件读取与编辑工具

| 工具 | 当前能力 | 需要补齐 |
| --- | --- | --- |
| `Read` | UTF-8 文本读取、行号、offset/limit、read state | blocked device、防大文件/token 爆炸、重复读 unchanged stub、路径建议、目录误读提示、读权限检查、为后续富媒体读取预留 artifact ref |
| `Edit` | exact replace、唯一性检查、read-before-edit、replace_all | 保留 CRLF/编码、相似路径建议、大文件保护、diff 结构、失败定位、secret guard、编辑后 read state 更新 |
| `Write` | 整文件写入、read-before-overwrite、创建父目录 | 创建/覆盖语义明确化、diff 结构、编码/换行策略、权限检查、禁止无请求写文档、冲突错误更可恢复 |
| `ApplyPatch` | 缺失 | 新增 freeform patch 工具，支持 add/update/delete/move、路径 guard、语法错误定位、结构化 diff、governance policy 和回滚友好错误 |

### 2. 代码搜索与路径发现工具

| 工具 | 当前能力 | 需要补齐 |
| --- | --- | --- |
| `Grep` | walkdir + regex 逐文件读 | 真 ripgrep runtime、`type`、`glob`、`-A/-B/-C`、`head_limit`、`offset`、multiline、默认 VCS/构建目录排除、超时与部分结果 |
| `Glob` | walkdir + globset | gitignore/ignore 支持、稳定排序、分页、默认排除噪音目录、大 repo 性能保护 |

### 3. Shell 与长期任务工具

| 工具 | 当前能力 | 需要补齐 |
| --- | --- | --- |
| `Bash` | shell spawn、session output、workdir、background | shell parser、只读命令分类、危险命令风险标注、prefix risk rule、写文件绕过检测、sandbox 真执行、超时/kill、环境快照 |
| `TaskOutput` | 读取后台输出 | progress metadata、offset/pagination、stdin 写入语义、owner session 校验一致化 |
| `TaskStop` | 停止后台任务 | 真实进程终止、子进程清理、状态可观测、重复停止幂等 |

### 4. Web 与外部知识工具

| 工具 | 当前能力 | 需要补齐 |
| --- | --- | --- |
| `WebSearch` | Firecrawl / Brave / Tavily provider adapter | recency、domain filters、result refs、citations、错误分类、限额、合规来源归因 |
| `WebFetch` | GET + HTML stripping + max chars | readability/Markdown、redirect 信息、domain permission、auth/private URL 提醒、prompt-based extraction、MIME 处理、内容持久化策略 |

`WebSearch` 必须对外暴露稳定的 `WebSearchRequest` / `WebSearchResponse` runtime schema。外部使用方只调用统一 search 接口，不感知具体 provider；provider 选择、API key、base URL、timeout 等只从 `AgentKernelConfig.tools.web_search` 进入 runtime。

配置形态：

```yaml
agent:
  kernel:
    tools:
      web_search:
        provider: firecrawl # firecrawl | brave | tavily
        firecrawl:
          api_key_env: TALOS_WEB_SEARCH_FIRECRAWL_API_KEY
          base_url: https://api.firecrawl.dev
          timeout_ms: 15000
        brave:
          api_key_env: TALOS_WEB_SEARCH_BRAVE_API_KEY
          base_url: https://api.search.brave.com/res/v1/web/search
          timeout_ms: 15000
        tavily:
          api_key_env: TALOS_WEB_SEARCH_TAVILY_API_KEY
          base_url: https://api.tavily.com
          timeout_ms: 15000
```

Provider 约束：

- `firecrawl` 是默认 provider，支持无 key free tier，也可通过配置或环境变量提供 API key。
- `brave` / `tavily` 通过 provider adapter 接入，API key 可来自配置值或环境变量。
- `recency_days`、domain allow/block、limit 在标准 request 中表达，由 provider adapter 映射到厂商参数；response 必须暴露 `provider`、`recency_days`、`freshness_applied` 和标准 hits。
- Tool 层只把标准 response 映射到 canonical `WebReference` / `Text` content items，不把 provider 原始响应作为 UI/SDK/replay 主路径。

### 5. MCP、Skill 与工具发现

| 工具 | 当前能力 | 需要补齐 |
| --- | --- | --- |
| `ToolSearch` | catalog substring 搜索并激活 deferred tool | BM25/metadata 搜索、namespace-aware、MCP/Skill/resource 统一发现、不可用原因、schema 摘要、激活结果 |
| `Skill` | 从 catalog 过滤 `skill_` entry | 文件系统 skill discovery、`SKILL.md` metadata、lazy load、skill prompt 注入、版本/缓存 |
| `ListMcpResources` | 缺失 | 列出 MCP resources，支持 server filter 和分页 |
| `ListMcpResourceTemplates` | 缺失 | 列出 resource templates，暴露参数 schema |
| `ReadMcpResource` | 缺失 | 读取 resource URI，支持文本/二进制/结构化结果 |

### 6. 计划与协作工具

| 工具 | 当前能力 | 需要补齐 |
| --- | --- | --- |
| `TodoWrite` | 基础任务状态 | 状态机约束、增量更新、UI 稳定展示、与 plan mode 一致 |
| `Agent` | spawn child agent、foreground/background | cwd 传递、工具权限继承、结果摘要、失败恢复；不在本 spec 扩展 team/task 生命周期 |
| `AskUserQuestion` | 基础提问 | 权限模式、阻塞/恢复协议、UI 表单结构化 |

### 7. 后续独立 spec 范围

以下能力不进入本轮交付范围，后续另起 spec：

- LSP tools：diagnostics、definition、references、symbols。
- NotebookEdit：cell insert/update/delete、cell-level diff。
- Agent task/team：多 agent 任务管理、team 生命周期、跨 agent 协调 UI。
- 富媒体 deep read：PDF/image/notebook 的完整解析和多模态渲染。

## Tool Result 格式审视

当前 `ToolResultEnvelope` 已能表达 `status`、`structured_content`、`error`、`pending`、`policy`，`ToolOutputPayload` 也已有 `content`、`success`、`structured_content`。本 spec 以现有 envelope 为落点重定义 canonical tool result schema，Phase 1 一次性迁移内核、协议、SDK、UI 和 replay，不保留旧格式双轨路径。

### Canonical Result Schema

Talos 必须提供唯一的 canonical tool result schema。所有客户端都消费同一套 `ToolResultEnvelope` / `ToolContentItem` 定义；WebUI、Desktop、TUI、SDK、replay 只能基于同一结构做渲染、格式化或持久化投影，不允许为不同端返回不同 result shape。

工具内部可以保留独有 runtime output，例如 `BashOutput`、`GrepOutput`、`ApplyPatchOutput`、`WebFetchOutput`。这些内部类型不能直接成为跨端协议；每个工具必须显式实现从 tool-specific output 到 canonical result schema 的转换。

```text
tool-specific runtime output
  -> canonical ToolResultEnvelope / ToolContentItem schema
      -> model compact projection
      -> WebUI renderer
      -> Desktop renderer
      -> TUI renderer
      -> SDK typed API
      -> replay storage
```

硬约束：

- Kernel 负责完整暴露工具结果结构、语义和原始内容，不负责决定最终视觉展示。
- 多端消费同一套 schema，端上差异只允许发生在 renderer / formatter，不允许发生在协议数据结构。
- `output: string` 不能作为工具结果主数据通道；简短文本只允许进入 `compact_summary` 或明确的 `Text` content item。
- 工具返回 JSON、diff、stdout/stderr、search matches、web references、MCP resources 时必须保留结构化字段，禁止先压成单行字符串再要求客户端反解析。
- 每个工具增强时必须同步定义 tool-specific output schema、canonical content item 映射、compact summary 和错误结构。

建议演进方向：

```rust
pub struct ToolResultEnvelope {
    pub status: ToolResultStatus,
    pub structured_content: Option<Value>,
    pub error: Option<ToolResultError>,
    pub pending: Option<ToolResultPendingMeta>,
    pub policy: Option<ToolResultPolicyMeta>,
    pub content_items: Vec<ToolContentItem>,
    pub compact_summary: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ToolResultStatus {
    Success,
    Failure,
    Pending,
    Rejected,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ToolTaskStatus {
    Queued,
    Running,
    Completed,
    Failed,
    TimedOut,
    Cancelled,
}

#[serde(tag = "type", rename_all = "snake_case")]
pub enum ToolContentItem {
    Text { text: String },
    Json { value: Value },
    Diff { files: Vec<ToolFileDiff> },
    SearchResults {
        query: Option<String>,
        matches: Vec<ToolSearchMatch>,
        offset: usize,
        limit: usize,
        total_matches: Option<usize>,
        more_available: bool,
        partial: bool,
    },
    BashOutput {
        session_id: Option<i64>,
        status: ToolTaskStatus,
        exit_code: Option<i32>,
        stdout: String,
        stderr: String,
        truncated: bool,
        elapsed_ms: Option<u64>,
    },
    WebReference {
        url: String,
        title: Option<String>,
        citation_id: Option<String>,
        snippet: Option<String>,
        source_domain: Option<String>,
        published_at: Option<String>,
        provider_rank: Option<usize>,
    },
    ArtifactRef { artifact_id: String, mime_type: Option<String>, label: Option<String> },
    McpResourceRef { server: String, uri: String, mime_type: Option<String> },
    Progress { message: String, elapsed_ms: Option<u64> },
}

pub struct ToolSearchMatch {
    pub path: String,
    pub line: u32,
    pub column: Option<u32>,
    pub text: String,
    pub before: Vec<String>,
    pub after: Vec<String>,
}
```

迁移与协议约束：

- `ToolResultStatus` 保留现有协议枚举 `Success` / `Failure` / `Pending` / `Rejected`，Phase 1 不做语义重命名。
- `content_items` 是 canonical result 的主数据字段；Phase 1 完成后工具结果不得只返回 `structured_content` 或 display string。
- `compact_summary` 是模型回灌和列表摘要字段，不承载完整工具结果。
- 模型回灌必须优先来自 `compact_summary` 和 selected `content_items` projection，不得直接把 `structured_content` 当作主回灌内容。
- `structured_content` 只允许作为 provider payload 或 MCP passthrough 的结构化附加字段，不参与 UI/SDK/replay 主路径。
- Phase 1 必须在 result builder 或 validator 层拒绝非 MCP passthrough 工具返回“`content_items` 为空但 `structured_content` 非空”的结果。
- app-protocol schema、TypeScript SDK、Rust SDK、UI domain type 和 replay storage 必须随 canonical schema 一次性同步更新。
- 外部协议 JSON 必须锁定为 `{ "type": "...", ...payload }` 的 flat shape，例如 `{ "type": "bash_output", "stdout": "...", "stderr": "..." }`，不能出现 Rust 内部包装形态。
- 未知 `ToolContentItem.type` 是 schema 错误，应 fail fast；新增 content item 类型必须同步更新 Rust enum、schema、SDK、UI/TUI renderer 和 fixture。
- `status`、`severity`、`kind`、`risk`、`task_status` 等协议字段必须优先定义强类型枚举，禁止用裸 `String` 表达可枚举状态。
- 大内容必须通过 `ArtifactRef` 表达，不能塞进模型上下文或 UI event。

### Tool-specific Schema 补齐规则

每个工具落地增强时，必须补齐以下四类定义：

| 定义 | 说明 |
| --- | --- |
| Input schema | 模型调用参数、默认值、限制、校验错误 |
| Runtime output schema | 工具内部真实结果结构，允许每个工具独有 |
| Canonical result mapping | 映射到 `ToolResultEnvelope` / `ToolContentItem` 的规则 |
| Compact summary | 面向模型回灌、列表展示和大输出裁剪的短摘要 |

各阶段不得只增强 runtime 而继续返回 display-only string。工具能力补齐必须同时补齐结果 schema，否则视为未完成。

必须覆盖的表达能力：

| 场景 | 需要的结构 |
| --- | --- |
| 文件编辑 | changed files、hunks、old/new snippet、是否 truncated |
| 搜索结果 | matches、path、line、match text、context lines、offset/limit、total_matches、more_available、partial |
| Web 搜索 | query、result refs、citations、domain filter、provider error |
| Web 抓取 | final_url、mime、title、markdown/text、truncated、auth warning |
| Bash | session_id、status、exit_code、stdout/stderr、truncated、elapsed_ms |
| MCP resource | uri、mime、resource type、server name |
| 大输出/富媒体 | artifact ref，而不是把大内容塞进上下文 |

## 执行流程

```text
model tool call
  -> ToolRouter normalize
  -> ToolRegistry lookup spec
  -> validate input
  -> permission / governance check
  -> runtime execute
  -> build ToolResultEnvelope
  -> persist large artifacts if needed
  -> stream UI event
  -> compact model-facing result
  -> feed next model round
```

## 核心 API 约束

不新增 `ToolRuntimeHandler` 作为主抽象，避免和现有 `Tool`、`ToolDefinitionProvider`、`ToolExecutorPort`、`ToolRegistry` 重叠。本轮只扩展现有边界：

- `Tool` 继续负责 schema、description、validate、permission 和 runtime dispatch。
- `ToolRegistry` / `ToolExecutorPort` 继续负责路由、选择、执行和上下文构建。
- `ToolOutputPayload` / `ToolResultEnvelope` 增加 typed content items。
- 各 runtime 返回更强的 structured result，由 registry 统一转成 envelope。

建议新增 artifact store 仅作为大输出持久化辅助：

```rust
pub trait ToolArtifactStore: Send + Sync {
    async fn put_text(&self, content: String, meta: ArtifactMetadata) -> TalosResult<String>;
    async fn put_binary(&self, bytes: Vec<u8>, meta: ArtifactMetadata) -> TalosResult<String>;
}
```

## 模块接口清单

| 模块 | 职责 | 主要接口 |
| --- | --- | --- |
| `talos_tools::registry` | 工具注册、选择、路由 | `ToolRegistry`, `ToolSelectionRequest` |
| `talos_tools::runtimes::fs` | 文件读写、patch、path guard | `ReadRuntime`, `EditRuntime`, `PatchRuntime` |
| `talos_tools::runtimes::search` | rg/glob 搜索 | `RipgrepRunner`, `GlobRunner` |
| `talos_tools::runtimes::shell` | shell 执行与任务生命周期 | `ShellRunner`, `TaskSessionManager` |
| `talos_tools::runtimes::web` | search/fetch provider | `WebSearchProvider`, `WebFetchClient` |
| `talos_tools::mcp` | MCP tools/resources/templates | `McpConnectionManager`, `McpResourceClient` |
| `talos_tools::output` | 统一结果 envelope | `ToolResultEnvelope`, `ToolContentItem` |
| `talos_tools::policy` | 权限决策、风险分类、sandbox | `ToolPolicyChecker`, `ToolGovernanceSpec` |
| `agent-kernel/general_turn` | tool loop 与结果回灌 | `execute_general_turn_tool_batch` |
| `gateway/app-protocol` | HTTP / app-server schema | generated request/response DTO |

## 分阶段落地

### Phase 1: Canonical Tool Result Schema

目标：定义并落地对外暴露的 canonical tool result schema。

- 演进 `ToolResultEnvelope` / `ToolOutputPayload`
- 新增 `ToolContentItem` typed schema
- 定义 tool-specific output 到 canonical schema 的转换约束
- 同步 app-protocol schema、Rust SDK、TypeScript SDK、UI domain type 和 replay storage

### Phase 2: 代码编辑闭环

目标：稳定读代码、搜代码、改代码，并补齐文件工具独有 schema。

- 增强 `Read` / `Edit` / `Write`
- 新增 `ApplyPatch`
- 统一 diff result
- 补文件工具单测和 06_tools e2e

### Phase 3: 搜索与执行闭环

目标：稳定定位代码和运行验证，并补齐 search/bash/task 独有 schema。

- `Grep` 切换 ripgrep
- 增强 `Glob`
- 增强 `Bash` / `TaskOutput` / `TaskStop`
- 补 sandbox / governance policy / risk classification 语义测试

### Phase 4: Web、MCP、Skill

目标：稳定获取外部知识和按需发现工具，并补齐 web/mcp/skill 独有 schema。

- 替换 `WebSearch` demo provider
- 增强 `WebFetch`
- 新增 MCP resource tools
- 增强 `ToolSearch` / `Skill`

### Phase 5: 多端投影与回归

目标：验证多端消费同一 canonical result schema。

- WebUI、Desktop、TUI、SDK、replay 使用同一 tool result 定义
- 各端只做 renderer / formatter，不引入协议分叉
- 大输出通过 artifact ref 或 compact summary 表达
- 补跨端 fixture、replay storage 和 e2e 回归

## 验收标准

- 每个工具都有 schema、runtime、权限、结果格式和 e2e 覆盖。
- 每个工具都有 tool-specific output schema，并能映射到唯一 canonical `ToolResultEnvelope` / `ToolContentItem`。
- WebUI、Desktop、TUI、SDK、replay 消费同一套 tool result schema，不存在按端返回不同 result shape。
- 工具结果保留原始结构和内容；JSON、diff、stdout/stderr、search matches、web references、MCP resources 不得只作为单行 text 暴露。
- `Read` 支持 blocked device 防护、size/token guard、offset/limit、重复读取 unchanged stub、路径错误建议。
- `Edit` / `Write` 保留换行/编码策略，执行 read-before-write 检查，返回结构化 diff 和可恢复错误。
- `ApplyPatch` 支持 add/update/delete/move，语法和匹配失败返回文件、hunk、原因和建议动作。
- `Grep` 使用真实 rg，支持 `type`、`glob`、context、`head_limit`、`offset`、timeout、partial result。
- `Glob` 支持 ignore 规则、稳定排序、limit/pagination 和默认噪音目录排除。
- `Bash` 支持命令分类、permission decision、risk classification、超时、真实 kill、后台 session owner 校验、输出分页。
- `TaskOutput` / `TaskStop` 支持幂等读取/停止、分页、状态和错误分类。
- `WebSearch` / `WebFetch` 有 provider abstraction、domain filter、recency、引用、redirect、错误分类和限额。
- `ToolSearch` / `Skill` / MCP resource tools 支持 lazy discovery、namespace-aware 搜索、不可用原因和激活结果。
- 工具结果能同时满足模型回灌、UI 展示、SDK 消费和 replay storage。
- 大输出不直接污染上下文；有 artifact ref 或 compact summary。
- WebUI 和 Desktop 共享同一 tool result 投影，不分叉产品行为。

## 后续 Specs

以下能力依赖本 spec 的 result envelope 和 artifact 能力，但不作为本轮验收条件：

- LSP code intelligence tools。
- Notebook read/edit tools。
- Agent task/team lifecycle。
- 富媒体 PDF/image/notebook deep read。
