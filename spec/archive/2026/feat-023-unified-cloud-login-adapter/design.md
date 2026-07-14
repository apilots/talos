# Unified Cloud Login Adapter

Status: archived
Archived: 2026-07-14

## 背景与问题

Talos 有 Web、Desktop、TUI 三种使用形态。当前 release readiness 中的 Web 登录曾验证过 `redirect_uri + access_token` callback，但这不是三端统一方案，也不进入最终 release 主线：

- access token 会出现在浏览器 URL 中。
- Desktop/TUI 不适合依赖浏览器 URL callback。
- Talos server 已经有 Cloud session cache，应由 Talos runtime 持有 token，而不是由 Cloud Web 把 token 明文传给浏览器页面。

目标方案应类似 GitHub CLI、Cloud Code、Codex/Claude Code 类本地客户端登录：Talos 打开浏览器，用户在 Cloud 登录授权，Talos runtime 轮询 Cloud API 换取 token 并写入本地 session cache。

Cloud 侧主能力由 `talos-cloud/spec/feat-023-device-login-flow/design.md` 定义。本 spec 只定义 Talos 如何适配该能力。

## 目标

- Web / Desktop / TUI 共享一套 Cloud 登录协议。
- Talos runtime 负责 device login start/poll/cancel 和 session cache 写入。
- WebUI、Desktop renderer、TUI 不直接处理 Cloud access token。
- HTTP gateway 与 app-server protocol 暴露一致的登录能力。
- 保留现有 `auth.session.read/store/clear` 与 Cloud session cache 语义。
- 登录成功后各端都回到 Talos 自己的界面，而不是停留在 Cloud。
- TUI 未登录时自动触发 Cloud login，而不是要求用户先手动执行登录命令。

## 非目标

- 不在 Talos 中实现账号密码登录。
- 不在 Talos 中签发 Cloud token。
- 不绕过 Cloud introspection。
- 不把 `local_launch_token` 或任何本地 token 恢复成业务身份。
- 不在本 spec 中实现 Cloud API；Cloud API 属于 `talos-cloud` spec。
- 不改变 Cloud 自身 Web 控制台的账号密码登录、dashboard、admin、token 管理逻辑。

## 当前事实

| 领域 | 当前事实 | 需要调整 |
| --- | --- | --- |
| HTTP auth | Gateway 已能校验 Cloud token | 新增 device login start/poll endpoint |
| App-server auth | `ExistingSessionToken` 是业务凭证 | 新增 auth login protocol，不新增业务凭证类型 |
| Session cache | `CloudSessionCache` 已存在 | 登录成功后由 runtime 写入 |
| WebUI | 当前可用 `redirect_uri + access_token` | 移除 URL token callback，改为调用 Talos gateway device login |
| Desktop | app-server sidecar 已负责 session cache | main/renderer 只打开浏览器和展示状态 |
| TUI | 可通过 SDK/app-server 访问 runtime | 未登录自动打开浏览器或打印 URL/user code |

## 设计决策

### 1. Talos runtime 是 token 持有方

Cloud access token 只在以下边界出现：

- Cloud API `device.poll` approved 响应。
- Talos runtime 内存与 session cache。
- Talos runtime 发起 Cloud introspection/current user 请求。

WebUI/Renderer/TUI UI 层不从 URL、localStorage 或 query string 中读取 Cloud token。UI 层只观察：

```ts
type LoginState =
  | { status: "idle" }
  | { status: "pending"; login_url: string; user_code?: string; expires_at_ms: number }
  | { status: "authenticated"; user: UserDto }
  | { status: "expired" }
  | { status: "denied" }
  | { status: "error"; message: string };
```

### 2. HTTP 与 app-server 暴露一致能力

HTTP:

```http
POST /v1/auth/login/start
GET  /v1/auth/login/status?login_id=...
POST /v1/auth/login/cancel
GET  /v1/auth/session
DELETE /v1/auth/session
```

App-server:

```text
auth.login.start
auth.login.status
auth.login.cancel
auth.session.read
auth.session.clear
auth.logout
```

协议层只做 transport 映射，业务逻辑复用 application/service 层。

### 3. 登录状态机由 Talos application/service 层管理

新增 `CloudLoginService` 或同等 application service，不放在 WebUI。

职责：

- 调 Cloud SDK `device_start`。
- 保存本地 pending login state。
- 打开浏览器所需的 `verification_uri_complete` 返回给客户端。
- 按 Cloud 返回的 `interval_ms` 轮询。
- approved 后写入 `CloudSessionCache`。
- 调 `current user` / introspection 校验并返回用户快照。
- cancel/expired 时清理 pending state。

Pending state 可以先放进进程内内存；如果 Desktop sidecar 重启，用户重新发起登录即可。后续如需要跨进程恢复，再持久化 pending state。

### 4. 三端 UX 差异只在打开 URL、展示 pending 与成功后的本端路由

Cloud 只负责：

- 展示登录页。
- 让用户确认 approve device session。
- approve 后显示“登录成功，可以回到 Talos”。

Cloud 不负责：

- 跳回 WebUI、Desktop 或 TUI。
- 决定 Talos 端的页面路由。
- 把 access token 放进 redirect URL。

Talos 各端在 `auth.login.status` 返回 authenticated 后自行决定本端行为：

- WebUI 刷新 current user，并回到原 Talos route 或默认首页。
- Desktop renderer 刷新 current user，并关闭/替换 pending UI。
- TUI 刷新 session 状态，并进入已登录主界面。

Web:

- 未登录进入业务页。
- WebUI 调 `POST /v1/auth/login/start`。
- 当前页或新窗口打开 `login_url`；该选择只影响 WebUI 交互，不影响协议。
- WebUI 轮询 `/v1/auth/login/status`。
- success 后刷新 current user 并回到原 Talos route。

Desktop:

- Renderer 调 app-server `auth.login.start`。
- Desktop main 用系统浏览器打开 `login_url`。
- Renderer 展示 pending 状态。
- app-server 轮询成功后写 session cache。
- Renderer 刷新 current user。

TUI:

- 未登录启动 TUI 或访问需要登录的能力时，自动触发登录。
- TUI 调 SDK/app-server `auth.login.start`。
- 尝试用系统浏览器打开 `login_url`。
- 同时打印 `login_url` 和 `user_code`。
- TUI 轮询 status。
- 成功后显示当前用户并进入主界面。

### 5. 移除旧 redirect_uri token callback

`redirect_uri + access_token` 不作为 release 最终登录协议。项目尚未最终上线，不保留兼容分支。目标状态是在 release 前移除默认产品入口中的 URL token callback。

Device flow 落地后：

- Web 默认改用 device flow。
- URL token callback 从 Web 登录主链路删除。
- Desktop/TUI 从不接入 URL token callback。
- release smoke 不覆盖也不依赖 URL token callback。

完成判断：

- Talos Web / Desktop / TUI 均能通过 `auth.login.start/status` 登录。
- Talos runtime 成功写入现有 Cloud session cache。
- UI 层不再从 URL、localStorage、query string 读取 Cloud access token。
- Cloud Web `/device` approve 页面上线。
- Cloud approve 完成页只显示成功状态，不控制 Talos 端跳转。

### 6. 清理旧浏览器临时登录配置

Talos 侧需要删除所有为了 URL token callback 或浏览器临时登录形态存在的配置与代码路径，包括：

- WebUI 从 `redirect_uri` query 中读取 `access_token` 的逻辑。
- WebUI 把 Cloud access token 写入 localStorage/sessionStorage 的逻辑。
- Talos gateway / Web config 中用于拼接 Cloud `redirect_uri` callback 的配置项。
- Desktop/TUI 中任何依赖浏览器 callback、localhost callback 或 URL token 的分支。

保留项：

- Cloud API base URL、Cloud Web base URL。
- Talos Home 下现有 Cloud session cache。
- `auth.session.read/store/clear` 这类 runtime session 管理能力。

原则是：配置只描述 Cloud 地址和 device login 行为，不描述“Cloud 登录后把 token 重定向回 Talos”。

## 三端交互示例

### WebUI

用户打开 Talos Web：

```text
http://127.0.0.1:5173/
```

未登录时，Talos Web 显示登录 pending 页面并调用 Talos gateway：

```http
POST /v1/auth/login/start
```

返回：

```json
{
  "login_id": "dls_123",
  "login_url": "https://cloud.talos.love/device?session_id=dls_123&user_code=ABCD-EFGH",
  "user_code": "ABCD-EFGH",
  "expires_at_ms": 1760000000000,
  "interval_ms": 2000
}
```

WebUI 打开 `login_url`，同时轮询：

```http
GET /v1/auth/login/status?login_id=dls_123
```

成功后返回：

```json
{
  "status": "authenticated",
  "user": {
    "user_id": "user_1",
    "username": "xiliu",
    "role": "user"
  }
}
```

WebUI 回到 Talos 主界面。整个过程 access token 不进入 WebUI URL 或 localStorage。
WebUI 的跳转逻辑由 WebUI 自己决定；Cloud 完成页只提示用户可以返回 Talos。

### Desktop

Renderer 调 app-server：

```json
{
  "method": "auth.login.start",
  "params": {
    "client_name": "Talos Desktop",
    "client_kind": "desktop"
  }
}
```

app-server 返回 `login_url`，Desktop main 用系统浏览器打开。Renderer 展示：

```text
Waiting for Cloud login...
Code: ABCD-EFGH
```

Renderer 轮询：

```json
{
  "method": "auth.login.status",
  "params": {
    "login_id": "dls_123"
  }
}
```

成功后 app-server 已写入 Cloud session cache，Renderer 只刷新 current user。

### TUI / CLI

TUI 未登录时自动触发同一流程。用户也可以显式执行命令：

```bash
talos auth login
```

输出：

```text
Opening browser to complete Talos Cloud login...

If the browser did not open, visit:
https://cloud.talos.love/device

Code:
ABCD-EFGH

Waiting for approval...
Signed in as xiliu.
```

TUI/CLI 可以自动调用系统 `open`，也必须支持无浏览器环境手动复制 URL 和 code。登录成功后 token 写入 Talos Home session cache，后续 TUI/CLI 复用该缓存。

TUI 不等待 Cloud 回跳，也不依赖浏览器 callback。Cloud 页面显示登录成功后，TUI 轮询拿到 authenticated 状态，刷新本地 session，并进入带账号的 TUI 主界面。

## 核心 API 伪代码

### Application service

```rust
pub struct CloudLoginService {
    cloud_client: TalosCloudClient,
    session_cache: CloudSessionCacheManager,
    pending: LoginPendingStore,
}

impl CloudLoginService {
    pub async fn start(&self, actor_context: LoginClientContext) -> TalosResult<LoginStartResponse> {
        let cloud = self.cloud_client.device_start(DeviceLoginStartRequest {
            client_name: actor_context.client_name,
            client_kind: actor_context.client_kind,
            scopes: vec!["talos:runtime".to_string()],
        }).await?;

        self.pending.insert(LoginPendingState {
            login_id: cloud.session_id.clone(),
            device_code: cloud.device_code,
            interval_ms: cloud.interval_ms,
            expires_at_ms: cloud.expires_at_ms,
            next_poll_after_ms: now_ms(),
        })?;

        Ok(LoginStartResponse {
            login_id: cloud.session_id,
            login_url: cloud.verification_uri_complete,
            user_code: cloud.user_code,
            expires_at_ms: cloud.expires_at_ms,
            interval_ms: cloud.interval_ms,
        })
    }

    pub async fn status(&self, login_id: &str) -> TalosResult<LoginStatusResponse> {
        let state = self.pending.get(login_id)?;
        throttle_until(state.next_poll_after_ms)?;

        let result = self.cloud_client.device_poll(DeviceLoginPollRequest {
            session_id: state.login_id.clone(),
            device_code: state.device_code.clone(),
        }).await?;

        match result {
            DevicePollResponse::Pending { interval_ms } => {
                self.pending.update_interval(login_id, interval_ms)?;
                Ok(LoginStatusResponse::Pending { interval_ms })
            }
            DevicePollResponse::Approved { auth } => {
                self.session_cache.store(auth_session_from_cloud(auth.clone()))?;
                self.pending.remove(login_id)?;
                Ok(LoginStatusResponse::Authenticated { user: auth.user })
            }
            DevicePollResponse::Denied => {
                self.pending.remove(login_id)?;
                Ok(LoginStatusResponse::Denied)
            }
            DevicePollResponse::Expired => {
                self.pending.remove(login_id)?;
                Ok(LoginStatusResponse::Expired)
            }
        }
    }
}
```

### HTTP handler

```rust
async fn start_login(State(state): State<HttpApiState>) -> Json<LoginStartResponse> {
    state.cloud_login.start(LoginClientContext::web()).await
}

async fn login_status(
    State(state): State<HttpApiState>,
    Query(query): Query<LoginStatusQuery>,
) -> Json<LoginStatusResponse> {
    state.cloud_login.status(&query.login_id).await
}
```

### App-server protocol

```rust
enum ClientMethod {
    AuthLoginStart,
    AuthLoginStatus,
    AuthLoginCancel,
}
```

## ASCII 流程图

```text
Web/Desktop/TUI                 Talos runtime                   Talos Cloud
     |                                |                              |
     | auth.login.start               |                              |
     |------------------------------->|                              |
     |                                | device_start                 |
     |                                |----------------------------->|
     |                                | login_url + device_code      |
     | login_url                      |<-----------------------------|
     |<-------------------------------|                              |
     | open browser                   |                              |
     |-------------------------------------------------------------->|
     |                                |                              |
     | auth.login.status              |                              |
     |------------------------------->|                              |
     |                                | device_poll                  |
     |                                |----------------------------->|
     | pending                        | pending / approved           |
     |<-------------------------------|<-----------------------------|
     |                                |                              |
     | auth.login.status              |                              |
     |------------------------------->|                              |
     |                                | device_poll                  |
     |                                |----------------------------->|
     | authenticated                  | access_token + user          |
     |<-------------------------------|<-----------------------------|
     |                                | store CloudSessionCache      |
```

## 模块接口清单

| 模块 | 接口 | 责任 |
| --- | --- | --- |
| `talos-auth` | `CloudDeviceLoginClient` | 调用 Cloud device start/poll/cancel |
| `talos-application-service` 或 `talos-base` | `CloudLoginService` | 管理 pending 登录状态与 session cache |
| `talos-gateway-http` | `/v1/auth/login/start/status/cancel` | WebUI HTTP 接入 |
| `talos-app-protocol` | `auth.login.start/status/cancel` | Desktop/TUI app-server 协议 |
| `talos-app-server` | auth login dispatch | 复用 `CloudLoginService` |
| `talos-sdk` | login start/status helpers | TUI/CLI/Rust SDK 调用 |
| `@talos/sdk-core` | login start/status types | Web/Desktop TS 调用 |
| `talos-web` | AuthPage pending UI | 不接触 token，只观察 status |
| `talos-desktop` | open external browser | main process 打开 Cloud login URL |
| `talos-tui` | print/open login URL | 终端登录 UX |

## 验证策略

- Rust 单测：
  - `CloudLoginService.start()` 保存 pending state。
  - `status()` pending 遵守 poll interval。
  - approved 后写入 `CloudSessionCache`。
  - consumed/expired/denied 清理 pending state。
- HTTP tests：
  - 未登录 Web 可 start login。
  - status approved 后 `/v1/user` 能返回 Cloud user。
- App-server tests：
  - `auth.login.start/status/cancel` 和 HTTP 语义一致。
- Web tests：
  - 未登录业务页进入 login pending。
  - login approved 后回到原 Talos route。
  - URL、localStorage 不包含 Cloud access token。
- Desktop smoke：
  - app 打开系统浏览器。
  - 登录成功后 packaged WebView 显示 current user。
- TUI smoke：
  - 打印 login URL/user code。
  - 登录成功后 TUI 显示 current user。

## 风险与待定

- Web 是当前页打开 Cloud、popup、新 tab，还是只显示 URL。统一协议都支持；更推荐 pending 页面 + 新 tab，避免丢失 Talos 上下文。
- Pending login state 是否需要持久化。第一版可进程内，Desktop/TUI 重启后重新登录。
- `CloudSessionCache` 是否需要迁移到系统 credential store。release 后可作为安全增强，不阻塞 device flow。
- 是否需要同一账号多设备 revoke 入口。Cloud admin/token 管理可后续扩展。
- 旧 URL token callback 的移除时间点需要和 Cloud Web device flow 上线同步。
- TUI 未登录时自动触发登录；显式 `talos auth login/logout/status` 可以作为手动入口和诊断命令，不改变默认自动登录行为。
