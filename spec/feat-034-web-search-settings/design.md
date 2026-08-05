# Web Search 配置管理设计

Status: implementing
Owner: agent
Created: 2026-08-04
Updated: 2026-08-05
Related: `refactor-038-llm-model-selection`, `archive/2026/refactor-018-runtime-env-bootstrap`, `archive/2026/refactor-025-talos-tool-capability-alignment`

## 背景与问题

Talos 已经具备完整的 Web Search 运行时配置和 Provider adapter。`AgentKernelConfig.tools.web_search` 可以选择 Firecrawl、Brave 或 Tavily，并分别配置 `api_key`、`base_url` 和 `timeout_ms`。`api_key` 使用 `${TALOS_WEB_SEARCH_...}` 占位符引用 `.env` 中的密钥值。`WebSearch` 工具执行时从当前 Kernel 配置读取选中的 Provider 和运行时环境，不需要新增搜索协议、Provider registry 或第二套工具运行时。

当前缺少的是配置管理入口：

1. Settings 没有 Web Search 页面，用户只能手工修改 YAML 和 `.env`。
2. 三个 Search Provider 的 API key 需要分别管理，但明文密钥不能写入 YAML 或通过配置读取 API 返回。
3. 现有 Config API 名称通用，实际 Request、Response、Service 和 Store 只支持 `llm`。
4. Models Settings 已经实现了 Provider tabs、API key 脱敏、draft 保存和错误展示，但组件命名和业务逻辑都属于 LLM，不能直接扩展成同时承载 Web Search 的大组件。
5. 当前 Agent 执行使用启动时加载的 `RuntimeEnv` 快照。只写入 `.env` 而不刷新运行时配置，会出现 Settings 显示保存成功、实际 Web Search 仍使用旧 key 的不一致。

本能力应在现有配置、Store、Gateway、SDK 和 Settings 模式上做最小扩展。后端统一配置管理边界，前端复用 Provider 配置的展示原语，但不新增平行配置对象、通用表单框架或新的运行时管理系统。

## 设计结论

1. Web Search 继续使用现有 `WebSearchToolConfig`、`WebSearchProviderConfig` 和 `WebSearchProviderRuntimeConfig`，不定义一套平行的 Settings DTO。
2. 现有 Config API 扩展为同时管理 `llm` 和 `web_search`，不新增 WebSearch 专用的业务 API。
3. `ConfigValue` 增加 `WebSearch(WebSearchToolConfig)`；配置读取响应改为按配置类型区分的 tagged response。
4. 现有 LLM 配置 Store 直接泛化为唯一的强类型 Config Store，同时读写 `llm` 和 `web_search`，不新增 WebSearch Store 或 Manager。
5. `SettingsModelsEditor` 重命名为 `SettingsLlmConfigEditor`。Web Search 新增独立的 `SettingsWebSearchConfigEditor`。
6. 前端只抽取 `SettingsProviderTabs`、`SettingsProviderKeyField` 和 `SettingsConfigSaveFooter` 等无业务语义的共享组件。
7. 配置保存后无需重启进程，从下一次 Turn 开始使用新的 Web Search Provider 和 API key；已经开始的 Turn 保持原配置快照。

## 目标

1. Settings 增加与 Models 同级的 Web Search 配置入口。
2. 支持选择 Firecrawl、Brave 或 Tavily 作为当前 Web Search Provider。
3. 支持分别设置、保留和清除三个 Provider 的 API key。
4. 支持编辑三个 Provider 已有的 `base_url` 和 `timeout_ms`。
5. 复用现有 Config API、配置对象、`.env` Secret Store、YAML 写入和运行时 reload 链路。
6. Web、Desktop 和 TypeScript SDK 使用同一配置语义。
7. 配置读取不返回明文 API key，配置更新不把明文 API key 写入 YAML。
8. 保存成功后，新 Turn 能使用更新后的 Provider 和运行时环境，不需要 reset Session 或重启服务。
9. 保持 LLM 和 Web Search 前端业务组件职责明确，不通过条件分支构造万能 Provider Editor。

## 非目标

1. 不新增 Firecrawl、Brave、Tavily 之外的 Search Provider。
2. 不实现动态 Provider registry、Provider 安装或自定义 Provider adapter。
3. 不修改 `WebSearchRequest`、`WebSearchResponse`、搜索结果引用或 Provider wire 协议。
4. 不实现 Provider fallback、自动切换或重试其他 Provider。
5. 不新增表单 schema、字段 registry、通用 Settings manager 或配置事件总线。
6. 不把 LLM model list、protocol、reasoning 等业务逻辑抽入共享 Provider 组件。
7. 不在首版增加网络探测式的 Test Connection API。
8. 不在本 Spec 中增加 TUI 或 CLI 配置页面；它们可以通过统一 SDK/API 在后续独立接入。

## 当前实现事实

### Web Search 配置与运行时

现有配置对象已经覆盖本能力需要的全部持久化字段：

```rust
pub struct WebSearchToolConfig {
    pub provider: WebSearchProviderConfig,
    pub firecrawl: WebSearchProviderRuntimeConfig,
    pub brave: WebSearchProviderRuntimeConfig,
    pub tavily: WebSearchProviderRuntimeConfig,
}

pub enum WebSearchProviderConfig {
    Firecrawl,
    Brave,
    Tavily,
}

pub struct WebSearchProviderRuntimeConfig {
    pub api_key: Option<String>,
    pub base_url: Option<String>,
    pub timeout_ms: u64,
}
```

`WebSearchTool` 从 `ToolContext.kernel_config()` 获取 `tools.web_search` 和 `runtime_env`。Provider adapter 已经统一消费这些对象，因此 Settings 保存后的目标仍然是更新同一份 Kernel 配置，不引入 UI 专用运行时对象。

### Config API

现有 Config API 具备通用入口：

```text
config.get
config.update
config.reload
```

但当前实现仍绑定 LLM：

- `ConfigGetResponse.config` 是 `LlmConfig`。
- `ConfigValue` 只有 `Llm(LlmConfig)`。
- `ConfigService` 只接受 `llm` path。
- `RuntimeConfigReloadHook` 只接收 `LlmConfig`。
- HTTP `/v1/llm` 和 SDK `client.llm` 是 LLM 专用投影。

因此本轮应扩展现有 Config 边界，而不是再复制一个 Web Search 配置管理栈。

### Secret 持久化

现有 Models 配置保存已经具备以下语义：

- 新明文 key 写入 `${TALOS_HOME}/.env`。
- YAML 只保存环境变量引用。
- 配置读取返回 `********`，不返回明文。
- `********` 表示保留已有 key。
- 空字符串表示清除 key。
- 旧 YAML 中存在的明文 key 会迁移到 `.env`。

Web Search 使用相同语义和现有 `EnvSecretStore`，不新增 Secret 对象或第二种更新协议。

### Settings Models

现有 `SettingsModelsEditor` 同时包含：

- LLM 默认 Provider。
- 默认 Model。
- Provider tabs。
- API key。
- Base URL 和 Protocol。
- Model list 编辑。
- 保存状态和错误展示。

其中只有 Provider tabs、API key field 和保存 footer 是 Web Search 可复用的 UI 语义。默认模型、协议和模型列表仍属于 LLM。

## 设计原则

### 复用已有能力

本能力只扩展现有事实来源：

```text
WebSearchToolConfig
  -> Config Store
  -> Config Service
  -> Gateway / SDK
  -> Settings editor
  -> existing WebSearch runtime
```

禁止新增以下平行路径：

- `WebSearchSettingsConfig`
- `WebSearchProviderSettings`
- `WebSearchSettingsManager`
- `WebSearchConfigStore`
- Settings 专用 Provider registry
- WebSearch 专用 YAML writer
- WebSearch 专用 `.env` parser
- 前端重新定义的 Search Provider 配置对象

只有 transport 必需的 Request/Response envelope 可以新增或调整；业务配置本体必须复用 `WebSearchToolConfig`。

### 共享展示原语，不共享业务 Editor

LLM 与 Web Search 都有 Provider tabs 和 secret 输入，但业务字段和验证不同。共享组件只接收展示和交互参数，不读取 Config API，不拥有业务 draft，也不判断配置类型。

不得出现以下设计：

```tsx
<SettingsProviderConfigEditor type="llm" />
<SettingsProviderConfigEditor type="webSearch" />
```

如果实现依赖大量 `type === "llm"` 或 `type === "webSearch"` 分支，说明业务边界已经混合，应拆回两个 Editor。

## 配置模型

### YAML

配置结构保持不变：

```yaml
agent:
  kernel:
    tools:
      web_search:
        provider: firecrawl
        firecrawl:
          api_key: ${TALOS_WEB_SEARCH_FIRECRAWL_API_KEY}
          base_url: https://api.firecrawl.dev
          timeout_ms: 15000
        brave:
          api_key: ${TALOS_WEB_SEARCH_BRAVE_API_KEY}
          base_url: https://api.search.brave.com/res/v1/web/search
          timeout_ms: 15000
        tavily:
          api_key: ${TALOS_WEB_SEARCH_TAVILY_API_KEY}
          base_url: https://api.tavily.com
          timeout_ms: 15000
```

Settings 保存只替换：

```text
agent.kernel.tools.web_search
```

其他 `agent.kernel.tools` 字段、Agent 配置、LLM 配置和未知但允许保留的 YAML section 不得被覆盖。

### `.env`

默认 Secret key 保持现有命名：

```dotenv
TALOS_WEB_SEARCH_FIRECRAWL_API_KEY=...
TALOS_WEB_SEARCH_BRAVE_API_KEY=...
TALOS_WEB_SEARCH_TAVILY_API_KEY=...
```

确定性规则：

1. YAML 中 `api_key` 保存为对应 Provider 的 `${TALOS_WEB_SEARCH_...}` 占位符。
2. Settings 保存明文 API key 时，后端写入 `.env` 中对应 Provider 的固定环境变量名。
3. YAML 中已有明文 `api_key` 时，首次读取或保存通过现有迁移规则写入 `.env`，随后 YAML 只保留占位符。
4. Runtime process env 的优先级继续高于 `.env`。Settings 写入 `.env` 后，如果同名 process env 存在，运行时仍使用 process env，并在 UI 中只展示“已配置”，不返回来源和值。

### Secret 输入语义

配置读取返回的 `WebSearchToolConfig` 沿用 Models 的脱敏规则：

| `api_key` 值 | UI 与更新语义 |
| --- | --- |
| `Some("********")` | 已配置；未修改时保留 |
| `None` | 未配置；未修改时保持未配置 |
| `Some("")` | 更新时清除 `.env` 中对应 key |
| `Some(new_value)` | 更新时把新值写入 `.env` |

后端必须在生成响应前再次清理配置对象，任何 Config、Gateway、SDK、日志和错误消息都不能包含明文 key。

## 统一 Config API

### Config path

新增强类型配置路径：

```rust
#[serde(rename_all = "camelCase")]
pub enum ConfigPath {
    Llm,
    WebSearch,
}
```

API path 表达业务配置资源，不暴露 YAML 内部路径。`webSearch` 映射到 `agent.kernel.tools.web_search` 的规则只属于 Config Store。

### Config value

扩展现有 `ConfigValue`：

```rust
#[serde(tag = "type", content = "value", rename_all = "camelCase")]
pub enum ConfigValue {
    Llm(LlmConfig),
    WebSearch(WebSearchToolConfig),
}
```

`ConfigUpdateRequest` 不再同时携带字符串 path 和 tagged value。更新目标由 `ConfigValue` 唯一确定，避免 `path = llm` 与 `value = webSearch` 之类不一致组合。

### Config response

当前 `ConfigGetResponse` 和 `ConfigUpdateResponse` 的字段完全重复且绑定 LLM。目标态使用一个共享的 tagged response：

```rust
#[serde(tag = "type", rename_all = "camelCase")]
pub enum ConfigResponse {
    Llm {
        config: LlmConfig,
        model_readiness: ModelReadiness,
        provider_keys: BTreeMap<String, ProviderKeyStatus>,
        provider_capabilities: BTreeMap<String, ProviderProtocolCapabilities>,
    },
    WebSearch {
        config: WebSearchToolConfig,
    },
}
```

`config.get` 和 `config.update` 都返回 `ConfigResponse`。Web Search 首版不增加额外 readiness DTO；三个 Provider 都通过脱敏后的 `api_key` 表达是否已经配置 key，运行时错误继续由现有 Web Search 边界返回。

### API 行为

```text
config.get
  input:  ConfigGetRequest { path: ConfigPath }
  output: ConfigResponse

config.update
  input:  ConfigUpdateRequest { value: ConfigValue }
  output: ConfigResponse
```

示例：

```json
{
  "path": "webSearch"
}
```

```json
{
  "value": {
    "type": "webSearch",
    "value": {
      "provider": "brave",
      "firecrawl": {
        "api_key": "********",
        "base_url": "https://api.firecrawl.dev",
        "timeout_ms": 15000
      },
      "brave": {
        "api_key": "new-secret",
        "base_url": "https://api.search.brave.com/res/v1/web/search",
        "timeout_ms": 15000
      },
      "tavily": {
        "api_key": null,
        "base_url": "https://api.tavily.com",
        "timeout_ms": 15000
      }
    }
  }
}
```

API 字段继续镜像配置文件 key 时使用 `snake_case`；Config envelope 和 enum variant 使用 `camelCase`。

### HTTP 与 SDK

App Server、Embedded SDK 和 Gateway 继续复用同一 Config Service。TypeScript SDK 增加通用配置入口：

```ts
client.config.get("llm")
client.config.get("webSearch")
client.config.update({ type: "llm", value: llmConfig })
client.config.update({ type: "webSearch", value: webSearchConfig })
```

现有 `client.llm` 可以在迁移期间作为 SDK convenience facade 调用统一 Config API，但不能拥有独立的保存、脱敏或 reload 逻辑。所有 Web、Desktop 和测试调用方迁移完成后，应评估是否删除该 facade；不得新增对应的 `client.webSearchConfig` 平行业务 facade。

HTTP 应提供统一配置资源投影，具体 path 仍映射到同一 Config Service。现有 `/v1/llm` 如果因已发布客户端需要暂时保留，只能作为 transport adapter 委托统一 Config API，不能继续拥有独立 Store 或 Service 分支。

## Store 与 Service

### Store 边界

`talos-config` 继续拥有配置文件和 `.env` 写入。现有 `LlmConfigStore` / `YamlLlmConfigStore` 直接泛化为唯一的 `ConfigStore` / `YamlConfigStore`，不并列增加 `WebSearchConfigStore`：

```rust
pub trait ConfigStore: Send + Sync {
    fn load_llm(&self) -> TalosResult<LlmConfig>;
    fn load_llm_read(&self) -> TalosResult<LlmConfigRead>;
    fn save_llm(&self, config: &LlmConfig) -> TalosResult<LlmConfigRead>;

    fn load_web_search_read(&self) -> TalosResult<WebSearchToolConfig>;
    fn save_web_search(
        &self,
        config: &WebSearchToolConfig,
    ) -> TalosResult<WebSearchToolConfig>;
}
```

这些方法共享同一个 Store 内部的少量私有实现：

- 读取 YAML 根 mapping。
- 读取和替换指定强类型 section。
- 读取、批量更新和写入 `.env`。
- 识别 `********`、环境变量引用、空字符串和新明文 key。
- 使用现有 `talos-fs` 原子 replace 和 durability 语义。
- 在同一配置文件锁内完成 read-modify-write，防止 Models 与 Web Search 并发保存互相覆盖。

Store 只显式支持当前两个配置 section，不接受任意 YAML path、自由 JSON 或动态类型注册。现有 `LlmConfigManager` 这类仅转发 Store 方法的薄层不得再复制一份 Web Search 实现；实现时应直接泛化或删除该薄层。

### 写入顺序与失败

保存前先构造并校验完整候选 YAML 和 `.env` 内容，然后在同一锁内写入。运行时 reload 只在两个文件都成功写入后发生。

如果第二个文件写入失败，必须恢复第一个文件的旧内容；恢复失败时返回包含 YAML path、env path 和恢复状态的明确内部错误。不得返回成功、静默使用部分配置或触发 runtime reload。

### Config Service

现有 `ConfigService` 持有唯一 `ConfigStore`，按 `ConfigPath` 和 `ConfigValue` 调用对应强类型方法：

```rust
match request.value {
    ConfigValue::Llm(config) => config_store.save_llm(&config),
    ConfigValue::WebSearch(config) => config_store.save_web_search(&config),
}
```

Service 负责：

- 验证 Config variant。
- 调用同一 Config Store 的对应强类型方法。
- 重新读取脱敏后的配置。
- 触发统一 reload hook。
- 返回对应 `ConfigResponse`。

Service 不直接解析 YAML、不直接操作 `.env`，也不把 `WebSearchProviderRuntimeConfig` 转换为另一套业务配置。

## 运行时 Reload

### 目标语义

保存 Web Search 配置后：

1. 使用统一配置 loader 重新加载 YAML。
2. 重新合并 process env 和 `${TALOS_HOME}/.env`。
3. 校验新的 `WebSearchToolConfig`。
4. 更新 Session Service 持有的 base config。
5. 新 Turn 从更新后的 Session config 获取 `tools.web_search` 和 `runtime_env`。

不执行：

- Session reset。
- MCP connection reset。
- Tool registry 重建。
- 正在执行 Turn 的配置替换。
- Search Provider fallback。

### RuntimeEnv 所有权

`runtime_kernel_config_for_session` 不能无条件用 Agent 启动时的 `RuntimeEnv` 覆盖 Session 中已经由最新 base config 生成的环境快照。运行时应以当前 Turn 构建时的有效 Session config 为输入。

直接使用 Agent Harness 的调用方仍必须显式提供有效 Kernel config；不得让 `WebSearchTool` 自行读取磁盘 `.env`，否则工具执行层会反向依赖配置文件位置并破坏配置 owner。

### 生效时间

每个 Turn 在开始时取得确定的配置快照：

- 保存前已开始的 Turn 继续使用旧配置。
- 保存成功后开始的新 Turn 使用新配置。
- 同一 Session 不需要关闭或重建，下一次从 Session Service 读取时自然获得新的 base config。

UI 保存成功提示应说明配置从新 Turn 生效，不声称正在运行的调用已经切换。

## Settings 前端设计

### 路由与导航

增加：

```text
/settings/web-search
```

`web-search` 是与 `models`、`mcp-servers` 相同层级的 `SettingsSection`。导航展示名为 `Web Search`，归入 Extensions 分组。

Web 与 Desktop 共享同一 React 页面和 SDK 调用，不创建 Desktop 专用配置页面。

### 组件职责

目标结构：

```text
SettingsModelsSection
  -> SettingsLlmConfigEditor

SettingsWebSearchSection
  -> SettingsWebSearchConfigEditor

provider-config/
  SettingsProviderTabs
  SettingsProviderKeyField
  SettingsConfigSaveFooter
```

具体约束：

- `SettingsModelsEditor` 重命名为 `SettingsLlmConfigEditor`。
- `SettingsModelsSection` 保留现有名称，因为它表达用户可见页面。
- `SettingsLlmConfigEditor` 继续拥有默认 Provider、默认 Model、Protocol 和 Model list。
- `SettingsWebSearchConfigEditor` 拥有 Search Provider、Base URL、Timeout 和 API key draft。
- 共享组件不调用 API、不创建 React Query、不保存业务 draft、不解析 Provider capabilities。
- 暂不抽取通用 `useSettingsConfigEditor` hook；只有出现第三个行为一致的调用方时再评估。

### Web Search 页面

页面包含：

1. 当前启用 Provider 选择。
2. Firecrawl、Brave、Tavily tabs。
3. 每个 Provider 的 API key 输入和配置状态。
4. Base URL。
5. Timeout。
6. 未保存状态、保存按钮和后端错误。

不展示：

- Provider 原始 wire 参数。
- 模型、Reasoning 或 Protocol 字段。
- 自动 fallback 设置。
- 任意 Provider 添加入口。

### Secret 前端状态

查询缓存只能保存后端返回的脱敏配置。用户输入的新 API key 保存在当前 Provider 表单的局部 state 中：

- 不写入持久化 store。
- 不写入 URL、日志或错误文本。
- mutation 完成后立即清除。
- 保存失败时可以保留输入供用户修正，但不得输出到 console。
- 切换 Provider 时保持各 Provider 尚未保存的本地输入，离开页面时清除。

## 校验与错误语义

配置更新必须显式校验：

- `provider` 只能是 Firecrawl、Brave 或 Tavily。
- `base_url` 存在时必须是合法 HTTP 或 HTTPS URL。
- `timeout_ms` 必须大于零；首版不增加产品级上限。
- Config API 收到不支持的 path 或 variant 时返回 `InvalidRequest`。
- YAML root 或目标 section 类型错误时返回包含配置路径的内部错误。
- `.env` 写入、恢复或 reload 失败时返回包含失败阶段的错误。

缺少当前 Provider API key 不阻止 Talos 启动，也不触发 fallback。Web Search 实际执行时继续使用现有明确错误，Settings 通过脱敏字段展示未配置状态。

## 权限边界

Web Search API key 是服务级运行时 Secret，与 LLM Provider API key 使用同一配置管理权限。

- auth-disabled 的本地运行模式可以读写配置。
- 启用认证后，配置读取和更新必须经过统一 Config Gateway 的管理权限检查。
- 普通用户不能读取或更新 Models / Web Search 全局配置。
- Web、Desktop 和 app-server 调用面都必须复用同一 Gateway 权限语义。
- 权限检查只实现一次并同时覆盖 LLM 与 Web Search，不能在两个 Store 或两个 HTTP handler 中复制。

如果现有 Config Gateway 尚未完整执行该权限，本能力必须在统一边界补齐，而不是让 Web Search 复制现有缺口。

## 影响范围

| 区域 | 变化 |
| --- | --- |
| `talos-config` | 将 LLM Store 泛化为唯一 Config Store，增加 Web Search 配置读写和 Secret 脱敏 |
| `talos-types` | `ConfigPath`、扩展 `ConfigValue`、统一 `ConfigResponse` |
| `talos-service` | Config 分派、Web Search 保存、统一 reload |
| `talos-base` | 重新加载完整配置并刷新 Session base config |
| Gateway / App Protocol | Config path/value/response 扩展 |
| Rust / TypeScript SDK | 通用 Config client 和生成类型 |
| Web / Desktop | Web Search Settings、Models Editor 重命名、共享展示组件 |
| Schema / docs | App Protocol schema、TypeScript binding、配置文档和示例 |
| 手工自测 | 使用隔离 `TALOS_HOME` 验证真实 Settings 保存、脱敏回读和配置文件落盘 |

## Breaking Change 审查

本设计可能改变 Config API 的 response shape，并删除 `ConfigUpdateRequest.path`。实现前必须确认当前 App Protocol 和 SDK 是否已有外部调用。

目标处理：

1. 当前主版本协议统一使用 `ConfigPath`、`ConfigValue` 和 `ConfigResponse`。
2. Rust、TypeScript schema、SDK、Web、Desktop、TUI/CLI 内部调用方和测试 fixture 同步更新。
3. 不长期保留 `ConfigGetResponse`、`ConfigUpdateResponse` 与 `ConfigResponse` 三套相同语义对象。
4. HTTP `/v1/llm` 如需兼容已发布客户端，只作为薄 transport adapter；兼容窗口和删除条件必须在实现 Plan 中明确。
5. Config YAML 结构和 Web Search 工具输入输出不发生 breaking change。

## 验证策略

### 配置与 Secret

- 读取三个 Provider 配置并比较完整 `WebSearchToolConfig`。
- 分别验证新 key、`********`、空字符串和未修改 key 的行为。
- 验证明文 YAML key 迁移到 `.env` 且响应中不存在明文。
- 验证只替换 `agent.kernel.tools.web_search`，其他 YAML section 保持不变。
- 验证 Models 与 Web Search 并发保存不会丢失 YAML 或 `.env` 更新。
- 验证写入第二个文件失败时恢复旧内容并且不触发 reload。
- 验证 process env 优先于 `.env`。

### Config API 与 SDK

- `config.get(llm)` 返回 `ConfigResponse::Llm`。
- `config.get(webSearch)` 返回脱敏的 `ConfigResponse::WebSearch`。
- 两种 `ConfigValue` 分别调用同一 Config Store 的对应强类型方法。
- schema 和 TypeScript binding 包含 `ConfigPath::WebSearch`、`ConfigValue::WebSearch` 和 `ConfigResponse::WebSearch`。
- HTTP、App Server、Embedded SDK 和 Browser SDK 对相同配置返回一致语义。
- API response、日志和错误中不包含提交的明文 key。

### Runtime

- 保存 Provider 选择后，新 Turn 使用新的 Provider adapter。
- 保存 API key 后，新 Turn 能从重新加载的 `RuntimeEnv` 取得新值。
- 同一 Session 的下一 Turn 无需 reset 或重建。
- 保存期间已经开始的 Turn 保持原配置快照。
- 缺 key 时返回现有明确 Web Search 配置错误，不切换其他 Provider。

### Web 与 Desktop

- Settings 导航包含 `/settings/web-search`。
- Models 页面继续渲染 `SettingsLlmConfigEditor`。
- Web Search 页面显示三个 Provider tabs、当前 Provider、Base URL、Timeout 和 key 状态。
- 新 key 不进入 Query cache，保存成功后输入被清空。
- 切换 Provider 不丢失当前页面内尚未保存的其他 Provider draft。
- 保存错误可见且不包含 secret。
- Web 与 Desktop 使用同一 Settings 组件和 Config SDK。

### 质量门

- 运行项目约定的 Rust formatter、目标 package 测试和 lint。
- 运行 App Protocol schema 生成命令并检查生成 diff。
- 运行 Settings 相关前端测试、typecheck 和 lint。
- 不运行 Talos E2E；本能力通过 scoped 单元/集成测试与真实前后端手工自测完成验证。
- 搜索并删除旧组件名、重复 Config response、重复 secret helper 和临时兼容分支。

## 风险与后续

1. Config API response 泛化是潜在 breaking change，必须先完成调用面搜索并明确兼容窗口。
2. YAML 与 `.env` 是两个文件，无法获得操作系统级跨文件原子事务；实现必须通过同一锁、候选内容预校验和失败恢复保证可恢复性。
3. process env 高于 `.env`，用户在 Settings 更新同名 key 后可能观察不到运行时值变化。UI 不泄露来源，但错误和文档需要说明优先级。
4. 如果未来增加第四个 Search Provider，应先扩展现有配置 enum、runtime adapter 和 Settings tabs；不能引入自由 JSON 或仅前端可见 Provider。
5. 如果第三个 Settings 能力也复用相同 draft/save 行为，再评估共享 hook；当前只共享展示原语。
