# Talos Skills System 设计

Status: draft
Owner: agent
Created: 2026-06-17
Updated: 2026-06-19

## 背景

Talos 已有基础 `Skill` tool 入口、`PromptBuilder`、`ToolRegistry` 和通用 tool 执行路径。前一版 026 试图同时覆盖 plugin package、MCP、capability catalog 和 skill discovery，抽象过宽，容易把 Skill、MCP、Tool、Plugin 做成一套新的平行体系。

本 spec 收敛为专门的 Skills 系统设计：

1. Skill 如何被 Talos 发现。
2. Skill metadata 如何进入 UI / CLI / PromptBuilder。
3. 用户显式选择或提到 Skill 后，如何加载完整 `SKILL.md`。
4. Skill 与 Tool / MCP / Plugin 的边界如何保持简单。

MCP 不在本 spec 中设计。MCP server、MCP tool exposure、MCP resource、`rmcp` 接入和 deferred tool search 后续单独开 spec。

## 核心判断

Skill 是可移植 instruction package，不是 Talos 私有 workflow engine，也不是函数工具。

```text
Skill teaches the model how to work.
Tool performs an action for the model.
```

因此 Talos Skills 第一版只处理：

- 扫描 `SKILL.md`
- 解析轻量 metadata
- 渲染 available skills summary
- 解析用户显式 `$skill` / structured skill selection
- 按需读取完整 `SKILL.md`
- 注入 `PromptBuilder`
- 记录 skill 注入事件

Talos 不把 Skill 注册成 `ToolRegistry` runtime，不把 Skill 编译成 DAG，不解释执行 Skill 内的步骤。Skill root 下除 `SKILL.md` 外的所有文件和目录都视为 opaque bundled resources；模型只有在 `SKILL.md` 明确要求且任务需要时，才通过普通 tools 读取或执行，仍然走正常 tool 权限、sandbox 和审计。

本 spec 与 `feat-027-plugin-marketplace-skill-install` 属于同一个未正式发布迭代。实现时不需要兼容旧 Skills lifecycle 行为，不允许为 `skills.install` / `skills.uninstall` / `skills.enable` / `skills.disable` 做 fallback、转发或降级实现。

## 设计原则

- `SKILL.md` 是 Skill 本体，必须保持可被其他 code agent 理解。
- Skill 是 instruction / workflow document，不是 model-callable function tool。
- Skill 不拥有执行特权；所有读取、执行、网络、MCP 调用都必须走宿主已有 tools。
- 第一版不引入 `requires` / `dependencies` / capability mapping 作为 Talos 核心协议。
- 可选宿主 metadata 只能增强 UI 和产品体验，不得成为 Skill 加载前置条件。
- `ToolRegistry` 只管理可执行 tools，不管理 Skill package，不扫描 `SKILL.md`。
- `PromptBuilder` 是 Skill 注入的唯一入口，调用方不拼接 Skill prompt 细节。
- Skill 目录、frontmatter 和 bundled resources 保持可移植，优先沿用通用 Agent Skills 结构。
- 项目、用户、系统、插件来源统一归一成 `SkillMetadata`，但不改变 Skill 本体格式。

## 非目标

- 不设计 MCP tool/resource。
- 不设计 plugin marketplace。
- 不设计 hooks、LSP、background daemons。
- 不把 Skill 注册成 `skill_xxx` runtime tool。
- 不做 Talos 私有 workflow DSL。
- 不做自动 dependency install。
- 不要求 Skill 声明 Talos tools 或 host capability。
- 不让 Skill 绕过 existing tool permissions / sandbox。
- 不复制其他宿主 metadata 的全部行为；Talos 只识别可选 `agents/talos.yaml` 中的轻量 UI metadata。

## Skill 目录结构

第一版只强约束 `SKILL.md` 入口。Skill root 下其他文件和目录均作为 opaque bundled resources 保留，不做目录名白名单。

```text
skills/
  pr-review/
    SKILL.md
    references/
    scripts/
    assets/
    examples/
    templates/
```

`SKILL.md` 必须包含 YAML frontmatter：

```markdown
---
name: pr-review
description: Review pull requests with repository-specific context.
---

# Pull Request Review

Use this skill when reviewing a code change...
```

规则：

- `name` 必填，建议 kebab-case。
- `description` 必填，用于 available skills summary 和 UI 展示。
- `metadata` 可选解析，用于 UI / debug 展示。
- unknown frontmatter fields 忽略，不透传为业务对象。
- unknown frontmatter fields 不能导致加载失败。
- 只有 invalid YAML、缺少 `name`、缺少 `description` 才进入 `SkillLoadError`。
- `SKILL.md` 正文可以引用相对路径下的任意 bundled resources，例如 `references/`、`scripts/`、`assets/`、`examples/`、`templates/`、`core/`。
- Talos 不预读、不解释、不执行 bundled resources。
- 模型只有在 `SKILL.md` 明确要求且任务需要时，才通过 `Read`、`Bash` 等现有 tools 访问 bundled resources。
- 相对路径以 skill root 为基准。

## 可选宿主 Metadata

为保持 Skill 本体可移植，Talos 可以读取可选宿主 metadata：

```text
skills/
  openai-docs/
    SKILL.md
    agents/
      talos.yaml
```

第一版策略：

- 可以解析 `agents/talos.yaml` 中的 UI metadata，例如 display name、short description、icon、default prompt。
- 未知字段不进入结构化业务对象。
- 不强制要求存在该文件。
- 不执行 `dependencies`。
- 不因为 metadata 缺失或无法识别而禁用 Skill。

Talos 宿主 metadata 使用 `agents/talos.yaml`。优先保持 `SKILL.md` 本体可移植，宿主 metadata 只能增强 UI 和调试体验。

## Skill 来源

Skills 模块不拥有安装根，也不直接决定本地目录布局。它只消费上层组装好的 runtime roots，并统一归一成 `SkillMetadata`。

```text
Plugin
  ${TALOS_HOME}/plugins/installs/<install-id>/skills/<skill-name>
  # derived from enabled PluginInstall records

External
  ~/.agents/skills
  ~/.codex/skills
  other configured read-only skill roots
  # provided by Plugin Manager config, unmanaged by Talos lifecycle

Project
  <workspace>/.talos/skills
  <workspace>/.agents/skills
  # only when workspace trust allows it

System
  bundled / cached Talos skills
```

Scope 第一版定义：

```rust
pub enum SkillScope {
    Plugin,
    External,
    Project,
    System,
}
```

目标态来源规则：

- `Plugin` skill 来自 Plugin Manager 的 enabled `PluginInstall`，是第一版主要来源。
- `External` roots 来自 Plugin Manager 的 `plugins.roots` 配置数组，只作为三方只读 catalog source。
- `~/.agents/skills` 需要作为常见只读 root 支持，用来加载用户已经在其它 agent 宿主里安装的 skills。
- `~/.codex/skills` 或其它目录也可以通过同一数组配置。
- `Project` roots 必须等待 workspace trust 策略允许后才能进入 runtime roots。
- `System` roots 只用于 bundled / cached Talos skills。
- 只有具备 `PluginInstallId` 的 plugin-owned skill 承诺 enable/disable。
- unmanaged/read-only roots 不提供 lifecycle，也不写入 enable/disable 状态。

优先级建议：

```text
Project > Plugin > External > System
```

同名 Skill 不应静默覆盖。Talos 需要保留 `root`、`path`、`scope`、`source_plugin`、`source_install`，并在显式选择时优先用 structured path 解析；仅纯文本 `$skill-name` 命中多份同名 Skill 时，应选择最高优先级并产生 warning。

## 总体架构

```text
        skill roots / plugin skill roots
                    |
                    v
           +-------------------+
           | SkillsLoader      |
           | - scan roots      |
           | - parse metadata  |
           | - collect errors  |
           +---------+---------+
                     |
                     v
           +-------------------+
           | SkillCatalog      |
           | - metadata        |
           | - disabled paths  |
           | - source/scope    |
           +---------+---------+
                     |
             per turn|
                     v
       +-------------+--------------+
       |                            |
       v                            v
+-------------------+       +-------------------+
| SkillResolver     |       | SkillRenderer     |
| - explicit $skill |       | - available list  |
| - structured path |       | - budget control  |
+---------+---------+       +---------+---------+
          |                           |
          v                           v
+-------------------+       +-------------------+
| SkillInjector     |       | PromptBuilder     |
| - read SKILL.md   +------>| - summaries       |
| - build injection |       | - active skills   |
+-------------------+       +-------------------+
```

## 与 Tool / MCP / Plugin 的关系

### Tool

Skill 不进入 `ToolRegistry`。

```text
Skill system:
  discovery + prompt injection

Tool system:
  execution + permission + result
```

一轮执行示例：

```text
User: use $pr-review to review this diff
    |
    v
SkillResolver selects pr-review
    |
    v
SkillInjector reads SKILL.md
    |
    v
PromptBuilder injects skill instructions
    |
    v
Model follows instructions and calls normal tools:
Read / Grep / Bash / Write / ...
```

### MCP

MCP 不属于本 spec。Skill 可以在自然语言里建议使用某个 MCP server 或 tool，但 Talos 026 不解析、安装或激活 MCP dependency。MCP 的 tool exposure、resource list/read、`rmcp` adapter 和 `ToolSearch` 统一在单独 spec 中处理。

### Plugin

Plugin 是 Skill 的上层安装管理来源。Plugin package 可以包含 `skills/`，Talos Skills 系统从 Plugin Manager 提供的 enabled skill roots 扫描 `SKILL.md`。

本 spec 不设计 plugin install / enable / disable。Skills 只消费 `PluginManagerService` 派生的 `plugin_skill_roots`，并在 catalog 对象里保留 `source_plugin` / `source_install`。

## 对外 API 收敛

面向多端产品 API 时，Skills 模块只通过 `SkillsService` 暴露一个 facade。调用方不感知 root 扫描、frontmatter 解析、同名消歧、禁用状态、metadata 解析和文件读取细节。

Prompt 注入不走 `SkillsService`，而是由 `talos-skills::SkillProvider` 提供给 `PromptBuilder`。

对外 request / response 对象统一命名为 `XRequest` / `XResponse`，不使用 `Params` 表达业务请求对象。HTTP / JSON-RPC / SDK 只包装同一组 shared core objects。

对外 API 分两类：

```text
Catalog:
  skills.refresh
  skills.list
  skills.suggest
  skills.read

Turn:
  skills.resolve
```

安装、更新、卸载、启用、禁用属于 Plugin Manager 管理面，由 `plugin.*` API 统一负责。

典型调用链：

```text
TUI/Web/Desktop input "$"
  -> skills.suggest(query)
  -> user selects suggestion
  -> UI sends structured UserInput::Skill { name, path }

Agent turn:
  -> PromptBuilder.skills(&dyn SkillProvider, request)
  -> talos-skills builds available summary + selected SKILL.md injections
```

## 核心 API 伪代码

```rust
pub struct SkillsService;

impl SkillsService {
    pub async fn refresh(&self, ctx: SkillContext) -> TalosResult<SkillSnapshot>;

    pub async fn list(&self, ctx: SkillContext) -> TalosResult<Vec<SkillSummary>>;

    pub async fn suggest(
        &self,
        ctx: SkillContext,
        query: SkillSuggestQuery,
    ) -> TalosResult<Vec<SkillSuggestion>>;

    pub async fn read(
        &self,
        ctx: SkillContext,
        selector: SkillSelector,
    ) -> TalosResult<SkillDocument>;

    pub async fn resolve(
        &self,
        ctx: SkillContext,
        input: SkillResolveInput,
    ) -> TalosResult<SkillResolution>;
}
```

核心 service / catalog DTO：

```rust
pub struct SkillContext {
    pub user_id: UserId,
    pub workspace_root: Option<PathBuf>,
    pub plugin_skill_roots: Vec<PluginSkillRoot>,
    pub configured_roots: Vec<SkillRoot>,
    pub project_roots: Vec<SkillRoot>,
    pub system_roots: Vec<SkillRoot>,
}

pub struct PluginSkillRoot {
    pub plugin_id: PluginId,
    pub install_id: PluginInstallId,
    pub root: PathBuf,
}

pub struct SkillRoot {
    pub root: PathBuf,
    pub scope: SkillScope,
    pub read_only: bool,
}

pub enum SkillSelector {
    ByPath(PathBuf),
    ByName(String),
}

pub struct SkillSummary {
    pub name: String,
    pub description: String,
    pub short_description: Option<String>,
    pub path: PathBuf,
    pub scope: SkillScope,
    pub source_plugin: Option<PluginId>,
    pub source_install: Option<PluginInstallId>,
    pub enabled: bool,
}

pub struct SkillSuggestQuery {
    pub query: String,
    pub limit: usize,
}

pub struct SkillSuggestion {
    pub name: String,
    pub description: String,
    pub short_description: Option<String>,
    pub path: PathBuf,
    pub scope: SkillScope,
    pub source_plugin: Option<PluginId>,
    pub source_install: Option<PluginInstallId>,
}

pub struct SkillDocument {
    pub summary: SkillSummary,
    pub contents: String,
}

pub struct SkillResolveInput {
    pub text: String,
    pub selected: Vec<SkillSelector>,
}

pub struct SkillResolution {
    pub selected: Vec<SkillSummary>,
    pub warnings: Vec<SkillWarning>,
}

pub struct SkillSnapshot {
    pub skills: Vec<SkillSummary>,
    pub errors: Vec<SkillLoadError>,
}
```

Prompt-facing trait / DTO 由 `talos-skills` 暴露，供 `talos-prompt` 和 `agent-kernel` 使用，对齐现有 tools prompt 注入模式：

```rust
pub trait SkillProvider: Send + Sync {
    fn prompt_context(&self, request: SkillPromptRequest) -> TalosResult<SkillPromptContext>;
}

pub struct SkillPromptRequest {
    pub text: String,
    pub selected: Vec<SkillSelector>,
    pub budget: SkillPromptBudget,
}

pub struct SkillPromptContext {
    pub available_summary: String,
    pub injections: Vec<SkillPromptInjection>,
    pub warnings: Vec<SkillPromptWarning>,
}

pub struct SkillPromptInjection {
    pub name: String,
    pub root: PathBuf,
    pub path: PathBuf,
    pub contents: String,
}
```

## 共享协议 API

`SkillsService` 是 Desktop、TUI、WebUI、CLI、HTTP / JSON-RPC 的唯一业务入口。各 transport 只做 envelope 和 serialization mapping。

| API | 调用方 | 用途 | 返回 |
| --- | --- | --- | --- |
| `skills.refresh` | Settings / CLI / watcher | 重新扫描 Skill roots | `SkillSnapshot` |
| `skills.list` | Settings / CLI / WebUI / Desktop / TUI | 展示 Skill 列表 | `Vec<SkillSummary>` |
| `skills.suggest` | WebUI composer / Desktop composer / TUI composer | `$` mention 候选 | `Vec<SkillSuggestion>` |
| `skills.read` | Settings / CLI / detail view | 查看完整 `SKILL.md` | `SkillDocument` |
| `skills.resolve` | API / CLI debug | 从用户输入解析 selected skills | `SkillResolution` |

对端使用约束：

- WebUI 和 Desktop 必须复用同一 `useSkillSuggestions` / composer picker 产品逻辑。
- TUI 也调用 `skills.suggest`，只负责终端渲染和键盘交互。
- UI 选择候选后必须向 turn input 写入 `SkillSelector::ByPath`，不要只写 `$name`。
- Agent turn 不经过 `SkillsService` 拼 prompt，而是让 `PromptBuilder` 消费 `talos-skills::SkillProvider`。
- protocol 层不得重新实现排序、过滤、同名消歧、禁用状态或 workspace trust。

## SkillsService 与 talos-skills 分层

`SkillsService` 是 application/service 层 facade，负责把多端请求、用户身份、workspace、config、plugin roots 和 trust policy 组织成一次 Skill use case。除安装、启停、卸载这类管理动作外，Skill 语义逻辑应下沉到 `talos-skills`。`talos-skills` 是 action domain crate / module，负责 Skill 文件系统语义、catalog、suggest、resolve、render 和 injection 数据转换，并额外暴露 prompt-facing provider trait 给 `talos-prompt` 使用。

外部调用方只能依赖 `SkillsService`：

```text
TUI / WebUI / Desktop / CLI / HTTP / JSON-RPC
        |
        v
application-service::SkillsService
        |
        v
talos-skills internal modules
```

Agent prompt 注入路径不经过 `SkillsService`，而是对齐现有 tools 模式：

```text
agent-kernel
        |
        v
talos_prompt::PromptBuilder
        |
        v
talos_skills::SkillProvider
```

内部依赖方向：

```text
application-service
  - owns auth/user/workspace context
  - owns config access
  - owns plugin manager integration
  - calls SkillsService facade

SkillsService
  - builds SkillContext
  - receives plugin_skill_roots derived from PluginManagerService
  - enforces application-level auth / workspace trust / config policy
  - delegates refresh/list/suggest/read/resolve to talos_skills domain APIs
  - returns stable DTOs to application-service

talos-skills
  - owns Skill catalog and disabled-state semantics
  - owns skill sorting, filtering, same-name resolution, and `$` suggestion
  - owns SKILL.md parsing, reading, summary rendering, and injection assembly
  - exposes SkillProvider for PromptBuilder
  - does not know HTTP / JSON-RPC / WebUI / Desktop / TUI
  - does not know session protocol envelopes
  - does not call PromptBuilder directly
  - does not execute tools
```

方法到内部模块映射：

| `SkillsService` API | 内部调用 | 说明 |
| --- | --- | --- |
| `refresh` | `catalog::refresh` | 扫描所有有效 roots |
| `list` | `catalog::list` | 返回 enabled/disabled 状态 |
| `suggest` | `catalog::suggest` | `$` picker 候选 |
| `read` | `catalog::read_document` | 查看完整 `SKILL.md` |
| `resolve` | `catalog::resolve` | 解析 structured selection 和 `$name` |

其中 `catalog::*` 是 `talos-skills` 对 service 层暴露的 domain facade。它内部再组合 `loader`、`suggest`、`resolver`、`render`、`injection`。这样 `SkillsService` 不关心 Skill 内部实现细节，也不会把 list/suggest/resolve 逻辑留在 application-service。

`PromptBuilder` 可以依赖 `talos-skills` 暴露的稳定 prompt-facing trait/types，类似当前依赖 `talos-tools::ToolDefinitionProvider`。它不依赖 `SkillsService`：

```text
PromptBuilder.skills(provider, request)
        |
        v
SkillProvider.prompt_context(request)
        |
        v
SkillPromptContext {
  available_summary,
  injections,
  warnings
}
```

## 内部模块清单

| 模块 | 职责 | 第一版接口 |
| --- | --- | --- |
| `talos_skills::model` | Skill domain types | `SkillMetadata`, `SkillScope`, `SkillLoadError` |
| `talos_skills::catalog` | 面向 `SkillsService` 的 domain facade | `refresh`, `list`, `suggest`, `read_document`, `resolve` |
| `talos_skills::loader` | 扫描 roots 并解析 `SKILL.md` metadata | `load_skills_from_roots` |
| `talos_skills::resolver` | 从 user input 解析显式 Skill 选择 | `resolve_explicit_skills` |
| `talos_skills::suggest` | 为 TUI/Web `$` mention 提供候选 Skill | `suggest_skills` |
| `talos_skills::injection` | 读取完整 `SKILL.md` 并生成注入块 | `build_skill_injections` |
| `talos_skills::render` | 渲染 available skills summary | `render_available_skills` |
| `talos_skills::prompt` | 面向 PromptBuilder 的稳定 trait/types | `SkillProvider`, `SkillPromptRequest`, `SkillPromptContext` |
| `talos_skills::watcher` | UI/CLI 可选刷新机制 | `watch_skill_roots` |
| `talos_prompt::PromptBuilder` | 通过 `SkillProvider` 注入 Skill prompt context | `skills`, `skill_prompt_context` |
| `application-service` | 暴露共享 skills API | `skills.refresh`, `skills.list`, `skills.read`, `skills.suggest`, `skills.resolve` |
| HTTP / JSON-RPC protocol | 多端共享 envelope，不承载 Skill 业务逻辑 | `skills.*` request/response mapping |
| WebUI/Desktop client | 复用同一 composer picker 产品逻辑 | `useSkillSuggestions` |
| TUI client | 调用同一后端 suggest API 渲染候选 | `$` completion |

## 配置

```yaml
skills:
  enabled: true
  max_skill_md_bytes: 65536
  max_available_summary_bytes: 8192

plugins:
  roots:
    - kind: skill
      path: ~/.agents/skills
      read_only: true
    - kind: skill
      path: ~/.codex/skills
      read_only: true
```

配置规则：

- `skills.enabled` 控制 Skills catalog / prompt injection 是否启用。
- `plugins.roots` 是 Plugin Manager 统一维护的本地 plugin roots 配置数组。
- `plugins.roots[*].kind = skill` 的只读 roots 会进入 Skills catalog，并使用 `SkillScope::External`。
- `plugins.roots` 不代表 Talos lifecycle ownership；这些目录不支持 `plugin.update` / `plugin.uninstall` / `plugin.enable` / `plugin.disable`。
- Plugin skill roots 不写在 `skills` 配置里，而是由 `PluginManagerService` 根据 enabled `PluginInstall` 派生。
- project skills 后续可受 workspace trust 控制，进入 `SkillContext.project_roots`。
- 单个 `SKILL.md` 必须有大小上限。
- available skills summary 必须有 prompt budget。

## 状态存储

Skills 状态只记录 plugin-owned skill 的启停状态，不负责安装目录管理。

```text
${TALOS_HOME}/plugins/skills/
  skills-state.json
```

`skills-state.json` 使用 `talos-types::skills::SkillStateFile`：

```rust
pub struct SkillStateFile {
    pub version: u32,
    pub disabled: BTreeSet<PluginSkillStateKey>,
}

pub struct PluginSkillStateKey {
    pub install_id: PluginInstallId,
    pub relative_skill_md_path: String,
}
```

该 schema 由 Plugin Manager spec 统一定义。External / Project / System roots 不写入该文件，也不承诺 enable/disable。

## Prompt 注入策略

Talos prompt 中分两类 Skill 内容：

### 1. Available Skills Summary

每轮可注入轻量 summary：

```text
Available skills:
- pr-review: Review pull requests with repository-specific context.
- pdf: Create, edit, inspect, and verify PDF documents.
```

规则：

- 只包含 name、description、source hint。
- 不包含完整 `SKILL.md`。
- 按 budget 截断。
- 同名冲突显示最优先项，并可附 warning。

### 2. Active Skill Instructions

当用户显式选择或提到 Skill 时，才读取完整 `SKILL.md`：

```text
<skill name="pr-review" path=".../SKILL.md">
Skill root: .../pr-review
Relative paths mentioned by this skill are relative to the skill root.
Do not read bundled resources unless the skill instructs you to and the task needs them.

...full SKILL.md...
</skill>
```

规则：

- 完整正文进入 PromptBuilder 的动态段。
- 注入顺序保持用户选择顺序。
- 注入失败时给模型和 UI 明确 warning。
- `SkillPromptInjection` 必须包含 `root` 和 `path`。
- Prompt 必须说明 Skill 中提到的相对路径以 skill root 为基准。
- Prompt 必须说明不要自动读取 bundled resources，除非 Skill 明确要求且任务需要。
- 不自动读取 bundled resources。

## 显式选择规则

第一版只支持显式选择，不做复杂模型驱动自动选择。

支持输入：

- 文本 mention：`$pr-review`
- 结构化 selection：`UserInput::Skill { name, path }`
- 未来 UI 选择 Skill 时必须传 path，避免同名歧义。

解析规则：

- structured path 优先。
- `$name` 仅匹配 enabled skills。
- 同名多匹配时按 Project > Plugin > External > System 选中，并记录 warning。
- disabled skill 不注入。
- 找不到 skill 时不报 fatal，向模型/用户暴露 warning。

## Skill Mention Picker

TUI 和 WebUI 必须支持在输入框中键入 `$` 后列出可用 Skills，让用户选择具体 Skill，而不是要求用户记住名称。

触发规则：

- 输入 `$` 时打开 Skill 候选列表。
- 输入 `$pr` 时按 query 过滤候选。
- 空 query 时展示 enabled skills 的默认排序列表。
- 选择候选后，输入框插入稳定 mention，并同时保留 structured selection。

候选展示格式：

```text
$pr-review  Review pull requests with repository-specific context.
$pdf        Create, edit, inspect, and verify PDF documents.
```

候选数据结构：

```rust
struct SkillSuggestion {
    name: String,
    description: String,
    short_description: Option<String>,
    path: PathBuf,
    scope: SkillScope,
    source_plugin: Option<PluginId>,
    source_install: Option<PluginInstallId>,
    enabled: bool,
}
```

排序与过滤：

- 只展示 enabled skills。
- 默认排序与解析优先级一致：Project > Plugin > External > System。
- query 匹配 `name`、`description`、`short_description`。
- 同名多来源候选需要显示 source hint，例如 `project` / `plugin:<install-id>` / `external` / `system`。
- UI 选择后必须发送 path-based structured selection，避免同名歧义。

API 边界：

```text
skills.suggest(query, limit, cwd)
  -> Vec<SkillSuggestion>
```

TUI/WebUI 可以本地缓存 `skills.list` 结果做前端过滤，但服务端仍应提供 `skills.suggest`，保证 CLI、WebUI、Desktop 和未来 SDK 复用同一排序、过滤和权限规则。

TUI、WebUI、Desktop 都必须通过共享 application/service API 获取候选，不各自实现一套 Skill 排序、过滤、权限和同名解析逻辑。WebUI 与 Desktop 共享同一份 WebView composer 代码，只允许 transport 差异；Desktop 不得复制一套 Skill picker 产品逻辑。

共享 API 需要同时覆盖多端：

```text
application-service
  skills.refresh
  skills.list
  skills.read
  skills.suggest
  skills.resolve
        |
        +-- CLI/TUI transport
        +-- HTTP / JSON-RPC
        +-- WebUI client
        +-- Desktop WebView client
```

`skills.suggest` 是产品语义 API，不是某个 UI 组件的私有 helper。它负责：

- scope / source 优先级排序。
- enabled / disabled 过滤。
- query 匹配规则。
- 同名候选 source hint。
- workspace trust / project root 约束。
- 返回 path，供 UI 生成 structured skill selection。

## 事件与可观测性

第一版只记录轻量事件，不做 dependency enforcement。

```text
SkillCatalogLoaded {
  total,
  errors,
  roots
}

SkillInjected {
  name,
  path,
  scope,
  source_plugin,
  source_install,
  bytes
}

SkillPromptInjectionFailed {
  name,
  path,
  error
}
```

这些事件用于 UI、日志和调试，不改变执行权限。

## 安全边界

- `SKILL.md` 是文本输入，不是可信代码。
- Skill root 下除 `SKILL.md` 外的文件和目录都是 opaque bundled resources。
- Bundled resources 不自动读取、不自动解释、不自动执行。
- Skill 中提到的任何命令都必须通过普通 tools 执行。
- 普通 tools 继续遵循现有 permission、approval、sandbox。
- Project skills 需要纳入 workspace trust 策略。
- 解析路径必须防止越过 skill root。
- 可选 metadata 中的 icon/resource 路径必须限制在 skill root 或 plugin asset 目录内。

## 第一版验收标准

- 可以扫描 Plugin Manager 派生的 plugin skill roots。
- 显式配置的 unmanaged roots 可以作为 read-only catalog source 进入列表。
- 可以解析 `name`、`description`、path、scope。
- 可以宽松解析 frontmatter：unknown fields 不失败，invalid YAML 或缺必填字段返回 `SkillLoadError`。
- 可以读取 `skills-state.json`，且 disabled Skill 不出现在 `$` 候选和 prompt injection 中。
- PromptBuilder 可以注入 available skills summary。
- Active Skill injection 包含 skill root 和 `SKILL.md` path。
- Prompt 明确 Skill 相对路径以 skill root 为基准。
- TUI/WebUI 输入 `$` 后可以列出 enabled skills 候选。
- Skill 候选显示 `$name` 和 description。
- UI 选择 Skill 后通过 structured path selection 注入，避免同名歧义。
- TUI、WebUI、Desktop 使用同一 `skills.suggest` 后端语义。
- WebUI 与 Desktop 不分叉 Skill picker 产品行为。
- 用户输入 `$skill-name` 后可以注入完整 `SKILL.md`。
- structured skill selection 可以按 path 精确注入。
- 同名 skill 有确定优先级和 warning。
- Skill 不注册到 `ToolRegistry`。
- Skill 注入不自动读取或执行 bundled resources。
- CLI/WebUI/Desktop/TUI 通过同一 application service 获取 `skills.*` 能力。
- MCP 相关逻辑不在本 spec 中实现。

## 风险

| 风险 | 处理 |
| --- | --- |
| Skill 系统变成私有 workflow engine | 第一版只做 `SKILL.md` 发现和注入，不做 DSL |
| Skill 绑定 Talos tools | 不设计 `requires` / Talos tool dependency |
| prompt 膨胀 | summary 和 full injection 分开，均受 budget 控制 |
| Skill 执行越权 | Skill 不执行，scripts 走现有 tools 和 sandbox |
| 同名 Skill 歧义 | 保留 path/source，structured selection 优先 |
| Project skills 不可信 | 纳入 workspace trust，默认保守 |
| Skill 本体被宿主绑定 | 保持 `SKILL.md` 主体与目录结构可移植；Talos 专属 UI metadata 使用可选 `agents/talos.yaml`，不强依赖 |

## 结论

Talos Skills 第一版应该是一个轻量、可移植的 instruction loading system：

```text
Skill roots provide SKILL.md.
SkillsLoader builds metadata.
SkillRenderer summarizes available skills.
SkillResolver selects explicit skills.
SkillInjector reads full SKILL.md.
PromptBuilder injects instructions.
Existing tools execute actions.
```

这条边界保持 Skill 通用，避免 Talos 私有 workflow 和 dependency mapping，同时为后续 plugin skills、UI 展示、MCP 独立 spec 留出扩展空间。
