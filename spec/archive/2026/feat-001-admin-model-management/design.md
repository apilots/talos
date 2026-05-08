# Admin 模型管理功能设计方案

## 1. 背景与现状

当前 Admin > Models 页面只展示扁平的模型列表（7 个模型，provider 全标 anthropic，全是 HEALTHY），
Add model 按钮无实现，无法编辑/删除，无法配置 Provider 的 API Key / Base URL / enabled 开关。

目标：Admin 界面直接管理 `talos.yaml` 中的 `llm.*` 配置，保存后运行时热更新生效。

---

## 2. 整体分层

对齐现有 `talos-auth` 的 Store → Service → Handler 三层结构：

```
talos.yaml (llm: section)
      │
      ▼
LlmConfigStore           ← crates/infra/config/src/store.rs
  YamlLlmConfigStore       读写 yaml 的 llm: 段，保留 ${VAR} 原始值
      │
      ▼
LlmConfigService         ← crates/infra/config/src/service.rs
  LlmConfigManager         get / update，直接复用 LlmConfig
      │
      ▼
HTTP Handlers            ← crates/gateway/http/src/server/admin_llm.rs
  GET/PUT /v1/admin/llm    写 yaml + 热更新 HttpApiState
```

**所有 API 类型直接复用 `talos_config` 已有定义**，不新增视图/patch 对象：

| 用途 | 复用类型 |
|------|---------|
| GET 响应体 / PUT 请求体 | `LlmConfig` |
| 单个 Provider 配置 | `ProviderConfig` |
| 单个模型配置 | `ProviderModelConfig` |
| 模型成本 | `ModelCostConfig` |
| 兼容性 | `ModelCompatConfig` |
| 输入类型 | `ModelInputKind` |

---

## 3. Store 层

**文件**：`crates/infra/config/src/store.rs`

```rust
pub trait LlmConfigStore: Send + Sync {
    /// 从 yaml 读取 llm: 段，不展开 ${VAR}（raw 原始值）
    fn load_raw(&self) -> TalosResult<LlmConfig>;

    /// 只替换 yaml 中的 llm: 节点，其余段保持原样
    fn save(&self, llm: &LlmConfig) -> TalosResult<()>;
}

pub struct YamlLlmConfigStore {
    config_path: PathBuf,
}
```

`save()` 实现策略：
1. 读取 yaml 文件为 `serde_yaml::Value`（raw，不解析 `${VAR}`）
2. 将 `&LlmConfig` 序列化为 `serde_yaml::Value`
3. 替换 root map 中的 `llm:` 节点
4. 写回文件

其余段（gateway、channels 等）保持原始字符串，`${JWT_SECRET}` 等占位符不会被展开后写死。

---

## 4. Service 层

**文件**：`crates/infra/config/src/service.rs`

```rust
pub trait LlmConfigService: Send + Sync {
    /// 返回 raw LlmConfig（含 ${VAR} 占位符，不展开）
    fn get(&self) -> TalosResult<LlmConfig>;

    /// 全量替换 llm: 段并写回 yaml，返回写入后的 LlmConfig
    fn update(&self, llm: LlmConfig) -> TalosResult<LlmConfig>;
}

pub struct LlmConfigManager {
    store: Arc<dyn LlmConfigStore>,
}
```

`update()` 逻辑：
1. `store.save(&llm)`
2. 返回 `llm`（即写入后的值，供 handler 热更新用）

服务层无业务逻辑的特殊 merge，前端负责把完整 `LlmConfig` 发回来（包括未修改字段）。

---

## 5. HTTP Handler 层

**文件**：`crates/gateway/http/src/server/admin_users.rs`（追加到已有文件，与 sessions/projects admin handler 并列）

handler 使用强类型响应，不用 `Json<Value>`：

```rust
pub(super) async fn get_admin_llm_handler(
    State(state): State<Arc<HttpApiState>>,
    actor: Extension<RequestIdentity>,
) -> Result<Json<LlmConfig>, ApiError> {
    require_admin(&actor)?;
    let llm = state.llm_config_service.get().map_err(map_admin_error)?;
    Ok(Json(llm))
}

pub(super) async fn put_admin_llm_handler(
    State(state): State<Arc<HttpApiState>>,
    actor: Extension<RequestIdentity>,
    Json(llm): Json<LlmConfig>,
) -> Result<Json<LlmConfig>, ApiError> {
    require_admin(&actor)?;
    let new_llm = state.llm_config_service.update(llm).map_err(map_admin_error)?;
    state.reload_llm(&new_llm).map_err(map_admin_error)?;
    let result = state.llm_config_service.get().map_err(map_admin_error)?;
    Ok(Json(result))
}
```

**路由注册**（`mod.rs`）：

```rust
.route("/v1/admin/llm", get(get_admin_llm_handler).put(put_admin_llm_handler))
```

---

## 6. HttpApiState 变更

**文件**：`crates/gateway/http/src/server/state.rs`

```rust
pub(super) struct HttpApiState {
    // 已有字段...
    pub(super) models: Arc<RwLock<Vec<ModelInfo>>>,        // Arc<Vec<>> 改为支持热更新
    pub(super) llm_config_service: Arc<dyn LlmConfigService>,
}

impl HttpApiState {
    /// 热更新：用新 LlmConfig 重建 provider + model catalog
    pub(super) fn reload_llm(&self, new_config: &LlmConfig) -> TalosResult<()>;
}
```

`reload_llm()` 内部复用 `talos-server/src/lib.rs` 中已有的 `resolve_model_catalog()` 和 provider 工厂逻辑。

---

## 7. 接线（talos-server bootstrap）

**文件**：`crates/app/talos-server/src/lib.rs`

```rust
let config_path = TalosPaths::resolve().config_file;
let llm_store = Arc::new(YamlLlmConfigStore::new(config_path));
let llm_config_service: Arc<dyn LlmConfigService> = Arc::new(LlmConfigManager::new(llm_store));

HttpServer::new(..., llm_config_service)
```

---

## 8. API 示例

### GET /v1/admin/llm

直接返回 `LlmConfig`（raw yaml 值，含 `${VAR}` 占位符）：

```json
{
  "provider": "anthropic",
  "model": "kimi-k2.5",
  "anthropic": {
    "api_key": "${BAILIAN_CODINGPLAN_API_KEY}",
    "base_url": "https://coding.dashscope.aliyuncs.com/apps/anthropic/v1",
    "enabled": true,
    "models": [
      {
        "id": "kimi-k2.5",
        "name": "Kimi K2.5",
        "context_window": 128000,
        "max_tokens": 8192,
        "reasoning": true,
        "input": ["text", "image"],
        "cost": { "input": 0, "output": 0, "cache_read": 0, "cache_write": 0 },
        "compat": {}
      }
    ]
  },
  "openai":  { "enabled": false, "api_key": null, "base_url": null, "models": [] },
  "bailian": { "enabled": true, "api_key": "${BAILIAN_API_KEY}", "base_url": "...", "models": [] },
  "ollama":  { "enabled": false, "api_key": null, "base_url": "http://localhost:11434", "models": [] }
}
```

### PUT /v1/admin/llm

请求体为完整 `LlmConfig`，前端把 GET 拿到的对象修改后整体发回：

```json
{
  "provider": "anthropic",
  "model": "kimi-k2.5",
  "anthropic": {
    "api_key": "${BAILIAN_CODINGPLAN_API_KEY}",
    "base_url": "https://coding.dashscope.aliyuncs.com/apps/anthropic/v1",
    "enabled": true,
    "models": [
      { "id": "kimi-k2.5", "name": "Kimi K2.5", "context_window": 128000, ... },
      { "id": "new-model",  "name": "New Model",  "context_window": 32768, ... }
    ]
  },
  "openai":  { ... },
  "bailian": { "api_key": "sk-new-literal-key", ... },
  "ollama":  { ... }
}
```

---

## 9. Frontend UI

### 页面结构

```
AdminModelsSection
├── 全局默认栏
│   ├── Active Provider [Anthropic ▾]
│   └── Default Model   [Kimi K2.5  ▾]
│
├── Provider 标签页
│   ├── [Anthropic ●] [OpenAI] [Bailian ●] [Ollama]
│   └── 当前 Provider 面板
│       ├── 启用开关（toggle）
│       ├── API Key 输入框（明文；直接显示 ${VAR} 或字面值）
│       ├── Base URL 输入框
│       └── Models 表格
│           ├── 每行：ID · Name · Context · Caps · [Edit] [Delete]
│           └── [+ Add Model]
│
└── [Save Changes]（整体 PUT /v1/admin/llm）
```

```
┌─ Models ────────────────────────────────────────────────────┐
│ Active Provider [Anthropic ▾]   Default Model [Kimi K2.5 ▾] │
├──────────────────────────────────────────────────────────────┤
│ [Anthropic ●]  [OpenAI]  [Bailian ●]  [Ollama]              │
├──────────────────────────────────────────────────────────────┤
│ Enabled   ──●  ON                                           │
│ API Key   [${BAILIAN_CODINGPLAN_API_KEY}          ]         │
│ Base URL  [https://coding.dashscope.aliyuncs.com..]         │
├──────────────────────────────────────────────────────────────┤
│ Models                                      [+ Add Model]   │
│  Model ID           Name       Context  Caps                │
│  kimi-k2.5          Kimi K2.5  128K     👁 💭  [✎] [✕]    │
│  qwen3.5-plus       Qwen 3.5+  32K      👁     [✎] [✕]    │
├──────────────────────────────────────────────────────────────┤
│                                        [Save Changes]       │
└──────────────────────────────────────────────────────────────┘
```

### 前端文件变更

```
web/src/features/admin/
├── components/
│   ├── AdminModelsSection.tsx           # 重构
│   └── models/
│       ├── ProviderPanel.tsx            # 新增
│       ├── ModelTable.tsx               # 新增
│       └── ModelFormModal.tsx           # 新增
└── hooks/
    └── useAdminLlmConfig.ts            # 新增：GET/PUT hooks，直接用 LlmConfig 类型

ts-sdk/packages/core/src/client.ts
    adminLlm: {
      get(): Promise<LlmConfig>
      update(llm: LlmConfig): Promise<LlmConfig>
    }
```

---

## 10. 模块接口清单

| 层次 | trait / struct / 函数 | 所在文件 |
|------|----------------------|---------|
| Store | `trait LlmConfigStore { load_raw, save }` | `crates/infra/config/src/store.rs` |
| Store | `struct YamlLlmConfigStore` | 同上 |
| Service | `trait LlmConfigService { get, update }` | `crates/infra/config/src/service.rs` |
| Service | `struct LlmConfigManager` | 同上 |
| HTTP | `get_admin_llm_handler → Json<LlmConfig>` | `crates/gateway/http/src/server/admin_users.rs` |
| HTTP | `put_admin_llm_handler(Json<LlmConfig>) → Json<LlmConfig>` | 同上 |
| State | `HttpApiState::reload_llm(&LlmConfig)` | `crates/gateway/http/src/server/state.rs` |
| Bootstrap | `YamlLlmConfigStore → LlmConfigManager → HttpApiState` | `crates/app/talos-server/src/lib.rs` |

---

## 11. 实施阶段

### Stage 1：Store + Service
- `LlmConfigStore` trait + `YamlLlmConfigStore`（只替换 yaml 的 `llm:` 节点）
- `LlmConfigService` trait + `LlmConfigManager`
- 单测：`save → load_raw` 往返验证；其余 yaml 段不变验证

### Stage 2：HTTP + 热更新
- `admin_llm.rs` 两个 handler
- `HttpApiState` 新增 `llm_config_service` 字段和 `reload_llm()` 方法
- `talos-server` bootstrap 接线
- 集成测试：PUT 后 `GET /v1/models` 验证新 catalog 生效

### Stage 3：TypeScript SDK
- `adminLlm.get()` / `adminLlm.update()` 及对应 TS 类型（对齐 `LlmConfig` 结构）

### Stage 4：前端 UI
- `ProviderPanel` + `ModelTable` + `ModelFormModal`
- 重构 `AdminModelsSection`
- e2e：改 provider / 增删模型 / Save → 验证
