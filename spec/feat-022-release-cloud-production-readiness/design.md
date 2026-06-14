# Release Cloud Production Readiness

Status: draft

## 背景与问题

Talos Cloud 已经发布到线上：

- Cloud Web: `https://cloud.talos.love`
- Cloud Dashboard: `https://cloud.talos.love/dashboard`
- Cloud API: `https://api.talos.love`

Talos 当前仍保留多处本地开发期语义：

- 默认配置偏本地 open mode：`gateway.auth=false`，`cloud.api_base_url=http://127.0.0.1:8787`。
- Desktop packaged 资源配置仍指向本地 cloud API。
- App-server JSON-RPC 仍支持 `local_launch_token` / `TALOS_LOCAL_LAUNCH_TOKEN`，并把它作为 `ActorCredential` 参与业务授权。
- Desktop main 仍通过 `client.auth.refresh` 向 app-server 回传 launch token，再由 app-server 映射成业务 actor。
- 部分 release 验证仍默认本地 cloud-server，而非线上 cloud auth contract。

第一个 release 版本必须把 Talos 从“本地开发默认可跑”收口为“生产默认使用线上 cloud auth”。同时，`local_launch_token` 不应继续作为业务认证凭证存在：它可以表达“本地进程之间是同一 packaged app 启动的连接”，但不能代表用户身份，不能映射成 `Actor`，也不能和 Cloud token 并列成为业务授权路径。

## 目标

- Release build 默认使用线上 Cloud API：`https://api.talos.love`。
- Talos Web / Desktop 打开的 Cloud 前端地址统一为 `https://cloud.talos.love`。
- `gateway.auth=true` 的生产业务路径只接受 Cloud access token，不接受 local launch token。
- 从业务认证面移除 `local_launch_token`：删除 `ActorCredential::LocalLaunchToken`、用 launch token 创建 actor 的 validator、Desktop refresh 业务 token request 和相关测试。
- 如 Desktop 仍需要本地进程握手，握手只能保护 transport/channel 初始化，不能进入 `ActorCredentials`，不能授予任何业务权限。
- Desktop packaged app 使用已保存的 Cloud session token 或引导用户去 Cloud Web 登录。
- WebUI 与 Desktop 共享同一登录、登出、current user、workspace、memory、chat 语义。
- Release 前完成线上 Cloud smoke、HTTP/App-server e2e、Desktop packaged DMG smoke。

## 非目标

- 不在本 spec 中实现新的账号系统。
- 不恢复 Talos 本地注册/登录表单作为生产入口。
- 不把 Cloud API 当 Talos gateway；`https://api.talos.love` 只用于 cloud auth/config/introspection 等 Cloud 服务。
- 不新增绕过 Cloud auth 的本地 admin/debug token。
- 不把测试账号密码写入仓库、脚本、日志或 release 配置。

## 当前事实

| 领域 | 当前事实 | Release 风险 |
| --- | --- | --- |
| Cloud config | `cloud.enabled=false` 与 `cloud.api_base_url` 并存 | 和 `gateway.auth` 重复，容易产生非法组合 |
| Gateway auth | `gateway.auth=false` 是默认开发配置 | release 若沿用会进入 open mode |
| Gateway token | YAML 中仍有 `gateway.token=talos-local-test-token`，但配置结构不读取 | 死字段像认证凭据，误导 release 配置 |
| App-server credentials | `ActorCredential::LocalLaunchToken` 仍存在 | 形成 Cloud token 之外的业务授权入口 |
| Desktop server request | `client.auth.refresh` 返回 launch token | Desktop 可把本地握手凭证升级成业务 actor |
| Auth cache | `CloudSessionCache` 已存在 | 应成为 Desktop 离线前的唯一 session source，但仍需 Cloud introspection |
| Cloud Web | `https://cloud.talos.love/dashboard` 可访问 | UI 跳转需要使用 Web 地址，不是 API 地址 |
| Cloud API | `/health` 返回 `{"status":"ok"}`，`/v1/auth/config` 可访问 | release smoke 可先用 `/health` 判断 Cloud API 存活，再验证 auth config |

## 设计决策

### 1. 从业务认证中删除 local launch token

删除以下业务认证与协议面：

- `EnvLaunchTokenValidator`
- `AuthServiceCredentialValidator.local_launch_token`
- `LocalLaunchAccess`
- `ActorCredential::LocalLaunchToken`
- app-server `client.auth.refresh`
- Desktop main 向 app-server 提供 launch token 以换取业务 actor 的逻辑

App-server 在 `auth_enabled=true` 时只接受：

```text
ActorCredential::ExistingSessionToken { token }
```

该 token 必须由 Cloud auth 签发，并通过 Talos auth service / Cloud introspection 验证。验证失败时拒绝请求，并清理失效的 cloud session cache。

`auth_enabled=false` 仅作为开发 open mode，actor 固定为 `anonymous`，不得和 release profile 混用。

如果实现仍需要保护 Desktop main 与 app-server sidecar 的本地连接，可以保留或新增 internal-only handshake，但必须满足：

- 不出现在 `ActorCredential` / `ActorCredentials`。
- 不构造业务 `Actor`。
- 不允许访问业务方法。
- 不参与 session、workspace、memory、models、chat 的权限判断。
- 不作为 Cloud token refresh 或 fallback。

### 2. Release profile 使用线上 Cloud

Release profile 必须显式设置：

```yaml
gateway:
  auth: true

cloud:
  api_base_url: "https://api.talos.love"
  web_base_url: "https://cloud.talos.love"
```

Cloud Web 地址用于 UI 跳转：

```text
https://cloud.talos.love
```

建议配置模型明确区分：

- `cloud.api_base_url`: Cloud API base，用于 `/v1/auth/config`、token introspection、current user 等后端请求。
- `cloud.web_base_url`: Cloud Web base，用于打开 dashboard、account、admin 页面。

如果暂时不新增 `cloud.web_base_url`，则必须由 `/v1/auth/config` 或 Talos health/config 响应提供 Cloud Web 地址，前端不得从 API base 机械推导。

`cloud.enabled` 不再保留。auth mode 只由 `gateway.auth` 表达：

- `gateway.auth=false`: open mode，不调用 Cloud auth，不需要 token。
- `gateway.auth=true`: Cloud auth 必启，使用 `cloud.api_base_url` / `cloud.web_base_url`。

以下组合不再允许：

```yaml
gateway:
  auth: true
cloud:
  enabled: false
```

```yaml
gateway:
  auth: false
cloud:
  enabled: true
```

旧 `cloud.enabled` 字段应从 YAML 和配置结构中移除；如果配置文件显式出现，应作为 legacy/unknown field 拒绝。

### 3. Desktop 不再用本地握手 token 授权业务请求

Desktop packaged app 启动 sidecar 后，业务请求必须携带 Cloud session token。Desktop main 的职责是：

- 启动/停止 sidecar。
- 保存、读取、清理 Cloud session cache。
- 在未登录时引导用户打开 Cloud Web 登录。
- 将 renderer 的业务请求转给 app-server，但不替用户生成业务 actor。

App-server 初始化可以继续是进程内可信连接，但业务方法的 actor 必须来自 Cloud token 或 open mode。不存在“本地启动 token = 用户”的中间态。

### 4. Cloud session cache 只做缓存，不做授权源

`CloudSessionCache` 可以保存：

- access token
- Cloud API base URL
- user snapshot
- last verified / last used timestamp

但缓存命中不能直接授权业务请求。每次进程启动后，首次业务请求或 current user bootstrap 必须验证 Cloud token。Cloud API 不可达时，应进入 unauthenticated/error 状态，而不是使用缓存继续授权。

### 5. Release 配置与开发配置分离

开发配置可以继续默认 open mode，避免影响本地迭代：

```yaml
gateway:
  auth: false
cloud:
  api_base_url: "http://127.0.0.1:8787"
  web_base_url: "http://127.0.0.1:3000"
```

Release 配置必须独立生成或覆盖，不能依赖开发默认值。Desktop packaged resource config 应使用 release profile，不应直接复用开发 `talos.yaml`。

需要清理和明确归属的 YAML 文件：

| 文件 | 目标语义 | 要求 |
| --- | --- | --- |
| `conf/talos.yaml` | 本地开发默认配置 | 可保留 open mode 与本地 Cloud API，但必须删除 `gateway.token` / `cloud.enabled`，并标注为 dev-only |
| `talos-app/desktop/resources/conf/talos.yaml` | Desktop packaged 默认配置 | release 包内不得指向 `127.0.0.1:8787`，不得包含 `gateway.token` / `cloud.enabled`，必须使用线上 Cloud API 或由 release build 注入 |
| release profile YAML | 生产发布配置 | 必须 `gateway.auth=true`、`cloud.api_base_url=https://api.talos.love`、`cloud.web_base_url=https://cloud.talos.love` |
| test generated YAML | 集成测试临时配置 | 继续通过 env 覆盖 Cloud API，但不能污染 release profile |

YAML 清理规则：

- 所有 YAML 中删除 `gateway.token`，配置校验遇到 `gateway.token` 应拒绝为 legacy auth field。
- 所有 YAML 中删除 `cloud.enabled`，配置校验遇到 `cloud.enabled` 应拒绝为 legacy auth switch。
- release 配置中禁止 `gateway.auth=false`。
- release 配置中禁止 `cloud.api_base_url` 指向 localhost / `127.0.0.1`。
- release 配置中禁止任何 `local_launch_token`、`TALOS_LOCAL_LAUNCH_TOKEN` 或等价字段。
- dev-only YAML 必须在文件注释或文档中明确“not for release”。
- Desktop packaged build 必须显式选择 release YAML，不能隐式复制 `conf/talos.yaml`。
- CI/release smoke 需要检查最终打包进 `.app` 的 YAML 内容，而不是只检查源码模板。

## 运行流程

### WebUI / HTTP

```text
Browser
  |
  v
Talos Web
  |
  |  unauthenticated
  v
Redirect to https://cloud.talos.love/auth/login
  |
  |  redirect_uri = Talos /auth/login callback
  v
Cloud login succeeds
  |
  |  access_token callback
  v
Talos Web restores requested route
  |
  |  business request with Cloud token
  v
Talos HTTP Gateway
  |
  v
Auth middleware verifies token via Cloud API
  |
  v
RequestIdentity { user_id, username, role, scopes }
  |
  v
Application service / Agent / Workspace
```

### Desktop packaged

```text
Talos.app
  |
  v
Desktop main starts app-server sidecar
  |
  v
Renderer loads shared WebUI
  |
  |  no cloud session
  v
Redirect to https://cloud.talos.love/auth/login
  |
  |  access_token callback to Talos WebUI
  v
Renderer stores cloud session
  |
  |  business request
  v
Renderer sends JSON-RPC with ExistingSessionToken
  |
  v
App-server validates token via Cloud API
  |
  v
Application service / Agent / Workspace
```

`LocalLaunchToken` 不出现在上述业务认证流程中。若存在内部握手，它在流程图外，只用于建立本地 transport，不产生 `Actor`。

## 核心 API 伪代码

```rust
enum ActorCredential {
    ExistingSessionToken { token: String },
}

impl ActorCredentialValidator for CloudActorCredentialValidator {
    async fn validate(&self, credentials: &ActorCredentials) -> ProtocolResult<Actor> {
        let ActorCredential::ExistingSessionToken { token } = &credentials.credential;
        let identity = self.auth_service.verify_access_token(token).await?;
        self.cloud_session_cache.record_verified_token_use(token, now_ms())?;
        Ok(actor_from_identity(&identity))
    }
}

async fn resolve_request_actor(
    config: &AppServerConfig,
    method: ClientMethod,
    credentials: Option<ActorCredentials>,
) -> ProtocolResult<Actor> {
    if !config.auth_enabled {
        return Ok(Actor::regular("anonymous"));
    }

    if is_session_cache_method(method) && credentials.is_none() {
        return Ok(app_server_internal_actor_without_business_access());
    }

    let credentials = credentials.ok_or_else(unauthorized)?;
    cloud_validator.validate(&credentials).await
}
```

Cloud Web / API 配置：

```rust
pub struct CloudConfig {
    pub api_base_url: String,
    pub web_base_url: String,
    pub auth: CloudAuthConfig,
}

impl CloudConfig {
    pub fn production() -> Self {
        Self {
            api_base_url: "https://api.talos.love".to_string(),
            web_base_url: "https://cloud.talos.love".to_string(),
            auth: CloudAuthConfig::default(),
        }
    }
}
```

## 模块接口清单

| 模块 | 变更 | 说明 |
| --- | --- | --- |
| `talos_config::CloudConfig` | 增加或明确 `web_base_url` | API 与 Web 地址分离 |
| `talos_app_protocol::ActorCredential` | 删除 `LocalLaunchToken` variant | JSON-RPC 业务凭证只支持 Cloud session token |
| `talos-app-server::auth` | 删除 launch token 创建 actor 的 validator 与 access mode | 只保留 Cloud token validator |
| `talos-app-server::runtime` | 删除 `client.auth.refresh` 业务授权依赖 | 未登录时返回 unauthorized，不请求 Desktop 生成业务 token |
| `talos-app/desktop` | 删除 launch token 换取业务 actor 的生成、传递、refresh handler | Desktop 不再创建 actor |
| `talos-app/web` | 使用 Cloud Web base 打开登录/account/admin | 不从 API base 推导前端地址 |
| `CloudSessionCache` | 继续保存 token，但不直接授权 | 每次进程启动后需要 Cloud 验证 |
| `talos_config::CloudConfig` | 删除 `enabled` 字段 | `gateway.auth` 是唯一 auth mode 开关 |
| `talos_config::GatewayConfig` | 拒绝 `gateway.token` 旧字段 | 避免死字段伪装成认证配置 |
| `conf/talos.yaml` | 标注 dev-only 或拆为 dev profile，删除 `gateway.token` / `cloud.enabled` | 防止被 release 误用 |
| `talos-app/desktop/resources/conf/talos.yaml` | 改为 release profile 或 build-time 注入，删除 `gateway.token` / `cloud.enabled` | packaged app 默认线上 Cloud |
| release config | 新增 production YAML profile | `gateway.auth=true` + Cloud API/Web 线上地址 |

## 实施阶段

## 阶段 1: 移除 local launch token 业务授权
**目标**: 删除本地启动 token 作为业务凭证的协议、validator、Desktop refresh 和测试依赖。  
**成功标准**: 代码中不存在 `ActorCredential::LocalLaunchToken`、launch token 创建 actor、`client.auth.refresh` 业务授权路径；如仍有内部握手，不能授予业务权限。  
**测试**: app-protocol schema/type generation、app-server auth tests、desktop IPC tests。  
**状态**: 未开始

## 阶段 2: 生产 Cloud 配置
**目标**: release profile 明确使用线上 Cloud API/Web 地址。  
**成功标准**: packaged Desktop 与 release Web 配置均指向 `https://api.talos.love` 和 `https://cloud.talos.love`；最终打包 YAML 不包含 localhost Cloud API、`gateway.auth=false`、`gateway.token`、`cloud.enabled` 或 launch token；开发配置仍可本地 open mode。  
**测试**: config tests、desktop packaged config smoke。  
**状态**: 未开始

## 阶段 3: WebUI/Desktop 登录链路收口
**目标**: WebUI 与 Desktop 都通过 Cloud session token 访问业务 API。  
**成功标准**: 未登录状态不能访问业务方法；登录后 current user、sessions、workspace docs、memory 均按 Cloud user 工作。  
**测试**: web auth bootstrap tests、app-server protocol e2e、线上 Cloud smoke。  
**状态**: 未开始

## 阶段 4: Release 验证与产物
**目标**: 完成第一个 release 的必需验证。  
**成功标准**: DMG 可验证、packaged app 可登录线上 Cloud 并完成核心 chat/workspace/memory 流程。  
**测试**: `hdiutil verify`、packaged Desktop smoke、`talos-e2e/run_e2e.sh`。
**状态**: 未开始

## 验证策略

### 本地自动化

```bash
cargo test -p talos-config
cargo check -p talos-gateway-http -p talos-app-server -p talos-application-service -p talos-agent -p talos-coordinator
pnpm --filter @talos/sdk-core build
pnpm --filter talos-web typecheck
pnpm --filter talos-web test
talos-e2e/run_e2e.sh
```

### 线上 Cloud smoke

使用线上测试账号手工验证，但禁止把账号密码写入脚本、日志、CI 或仓库：

- `https://api.talos.love/health` 返回 `{"status":"ok"}`。
- `https://api.talos.love/v1/auth/config` 返回 Cloud auth 配置。
- 从 Talos Web / Desktop 未登录业务入口开始。
- Talos 自动跳转到 `https://cloud.talos.love/auth/login`，并携带 Talos callback `redirect_uri`。
- Cloud 登录成功后回到 Talos 自己的界面，而不是停留在 Cloud dashboard。
- Talos Web / Desktop 获取 current user。
- Settings -> Account / Tokens / Admin Users 主动打开 `https://cloud.talos.love` 对应管理页面。
- 创建 chat/session。
- 发送一条消息。
- Settings -> Agent Workspace 读写当前用户 docs。
- Memory list/search 只返回当前用户内容。
- 登出后业务 API 返回 unauthorized。

### Desktop packaged smoke

- 清理旧 Talos 进程。
- 卸载旧 DMG 挂载卷。
- 重新打包 DMG。
- `hdiutil verify`。
- 检查 packaged `Talos.app` 内置 YAML：
  - `gateway.auth=true`
  - `cloud.api_base_url=https://api.talos.love`
  - `cloud.web_base_url=https://cloud.talos.love`
  - 不包含 localhost Cloud API
  - 不包含 `gateway.token`
  - 不包含 `cloud.enabled`
  - 不包含 local launch token 配置
- 打开 packaged `Talos.app`。
- 使用线上 Cloud 登录。
- 验证 chat、settings、workspace docs、memory。
- 验证 packaged app 正确加载 `~/.talos/.env`。

## 成功标准

- Release profile 默认进入 Cloud auth mode。
- `local_launch_token` 不再出现在业务认证协议中，也不能创建 actor。
- Cloud API 和 Cloud Web 地址语义明确，不混用。
- `gateway.auth` 是唯一 auth mode 开关，`cloud.enabled` 已移除。
- `gateway.token` 已从 YAML 和配置模型中移除或被显式拒绝。
- WebUI 与 Desktop 未登录时不能访问业务 API。
- 登录后所有 workspace、memory、session 数据按 Cloud `user_id` 隔离。
- Cloud API 不可达时，不使用 cached token 继续授权。
- Desktop packaged DMG 通过真实线上 Cloud smoke。
- 发布文档说明 release 配置、登录入口、已知限制和回滚方式。

## 风险与待评审问题

- 如果删除 `client.auth.refresh` 后 app-server 初始化流程仍需要本地连接握手，需要把“transport handshake”与“business actor”分开建模，避免重新引入业务 token。
- 如果 Cloud Web base 暂时不在配置里，需要明确由哪个后端接口返回，避免前端硬编码或从 API 地址推导。
- Cloud `/health` 只能证明 Cloud API 存活，不能替代 auth config、token introspection 和真实登录 smoke。
- Desktop packaged 登录若依赖外部浏览器回调，需要确认 callback/deep link 或手动 token handoff 的最终产品路径。
- 第一个 release 允许 dev open mode 继续存在，但 release profile 不能默默落回 open mode。
