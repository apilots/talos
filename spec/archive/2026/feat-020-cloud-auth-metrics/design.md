# Talos Cloud Auth 迁移设计与 Product Metrics 后续计划

Status: archived
Archived: 2026-07-14

## 1. 背景与结论

Talos 当前已经有一套本地权限体系：`talos-auth` 提供本地用户、密码、JWT/PAT、`AuthService`、`UserService`、`RequestIdentity`、`UserRole`、`AuthScope` 和 `AuthResponse`；HTTP middleware 与 Desktop app-server 通过 `AuthService.verify_access_token()` 消费身份；WebUI 也已有本地注册/登录页面。

上线前的目标不是在 Talos 主仓继续增强这套本地权限系统，而是把权限系统迁到 `../talos-cloud`：

- `talos-cloud` 承载账号、邮箱验证、Google OAuth、设备、entitlement、Auth Web 页面、Rust/TypeScript SDK。
- `talos` 继续作为本地数据面，保存 session、message timeline、memory、workspace docs、tool 参数和 LLM 配置。
- `talos` 多端统一接入 `talos-cloud` 的轻量 SDK/API，不再保留本地账号权限业务。

Product Metrics 是第二阶段后续工作，不和权限迁移混在同一个执行计划里。

## 2. 两个独立计划

### Plan A: Auth 完整迁移

当前优先执行。

目标：

- 将 Talos 中现有权限后端能力迁移到 `talos-cloud` 对应模块。
- 将 Talos 中现有权限前端页面能力迁移到 `talos-cloud` Auth Web。
- `talos-cloud` 对现有功能保持 100% 行为兼容，并在此基础上增加邮箱验证与 Google 登录。
- `talos-cloud` 提供 Rust SDK 与 TypeScript SDK。
- SDK 只做轻量封装，能力来自服务端，不在 SDK 里复制业务逻辑。
- Talos 多端通过 SDK 接入 cloud auth。
- 完整迁移后，Talos 移除本地权限业务能力。

### Plan B: Product Metrics

后续再做。

目标：

- 新增多端 Product Metrics SDK/API。
- 上报 `app_daily_opened`、设备、地域、版本、注册和登录等低敏产品指标。
- 严格禁止上传 prompt、response、session、memory、workspace docs、文件名、workspace path、tool 参数和本地执行指标。

Plan B 不阻塞 Plan A，也不进入第一轮实现范围。

## 3. 当前实现事实

| 接入点 | 当前事实 | 迁移目标 |
| --- | --- | --- |
| `talos-auth` | 本地用户、密码、JWT/PAT、role/scope、`AuthService` | 权限业务迁入 `talos-cloud`；Talos 只保留 cloud auth 消费边界 |
| `AuthManager` | 本地 register/login/logout/verify/token 管理 | 迁移完成后从 Talos 产品路径移除 |
| `UserService` | current user、admin users、本地用户更新 | 迁入 `talos-cloud` user/admin API |
| HTTP middleware | 读取 Bearer token 并调用 `verify_access_token` | 改为调用 cloud auth SDK/introspection |
| app-server | `ExistingSessionToken` 映射本地 Actor | 改为消费 cloud token；`LocalLaunchToken` 仍只做本地握手 |
| Web Auth 页面 | 本地注册/登录 UI | 迁入 `talos-cloud` Auth Web |
| SQLite `users` / `user_tokens` | 本地账号、密码哈希和 PAT | 迁移后 Talos 不再作为账号源 |
| Admin 用户界面 | 本地用户管理 | 迁入 `talos-cloud` Admin |

## 4. Plan A 目标状态

### 4.1 `talos-cloud` 拥有

- Auth API contract。
- Auth entity / application services。
- SQL repositories 和 migrations。
- SQLite local/test adapter。
- Postgres production adapter。
- 用户、密码哈希、邮箱验证状态。
- OAuth 账号绑定。
- opaque access token / session。
- `/v1/auth/introspect`。
- current user / account API。
- Admin user API。
- device activation。
- entitlement / signed receipt。
- Auth Web 页面：
  - register
  - login
  - email verify
  - Google OAuth start/callback
  - set password
  - account
- Rust SDK。
- TypeScript SDK。

### 4.2 `talos` 最终只保留

- 本地 runtime 与本地数据面。
- cloud auth config。
- cloud auth SDK 调用。
- token / receipt 本地安全保存。
- login gate / open cloud auth / callback token exchange。
- cloud identity 到本地 `Actor` 的映射。
- open mode 配置下的匿名本地访问。

### 4.3 `talos` 最终移除

- 本地账号注册。
- 本地账号登录。
- 本地密码哈希。
- 本地 JWT/PAT 签发与验证。
- 本地用户表作为生产账号源。
- 本地 user token 管理。
- 本地 admin user 管理。
- 本地完整 auth form 页面。
- 权限业务逻辑和账号状态判断。

保留 `RequestIdentity` 或等价本地 actor 映射结构是允许的，但它只能表达已由 `talos-cloud` 验证后的身份，不再承载本地认证业务。

## 5. 兼容性要求

迁移必须做到功能兼容优先，避免多端一次性大面积断裂。

### 5.1 API 兼容

`talos-cloud` 第一版需要覆盖现有 Talos 权限能力：

- register。
- login。
- logout。
- current user。
- update current user。
- user token / session token 替代能力。
- admin list/create/patch/delete users。
- token introspection。

旧 API 可通过兼容 route 或 SDK adapter 映射到新 API，但业务源必须在 `talos-cloud`。

### 5.2 行为兼容

- 已有 username/password 体验需要迁移到 cloud。
- email 从 optional 提升为注册必填，并增加验证流程。
- Google 登录是新增能力。
- role/admin 判定与现有 WebUI/Desktop 可见性保持一致。
- logout 后 token 必须失效。
- disabled/suspended 用户必须无法通过 introspection。

### 5.3 多端兼容

- WebUI、Desktop WebView、CLI、TUI 都使用同一套 cloud auth 语义。
- Desktop 的 `LocalLaunchToken` 只负责本地进程握手，不表示用户身份。
- Desktop packaged app 必须基于最终 DMG 验证。
- CLI first：CLI 要能完成登录、读取当前用户、验证 entitlement、跑最小 e2e。

## 6. 数据结构分层原则

`talos-cloud` 采用三种主结构，不再额外铺开 `DomainModel`、`Command`、`ViewModel` 等平行对象体系：

| 结构 | 所属边界 | 作用 | 典型命名 |
| --- | --- | --- | --- |
| DTO | API / SDK / Web contract | 对外输入输出契约，供 HTTP、Rust SDK、TypeScript SDK 和 Auth Web 复用 | `RegisterRequestDto`、`LoginResponseDto`、`IntrospectionResponseDto` |
| Entity | Application business boundary | 业务对象，承接从 Record 转换后的安全字段、枚举、状态和必要业务语义 | `UserEntity`、`AuthSessionEntity`、`DeviceEntity`、`EntitlementEntity` |
| Record | Repository / SQL boundary | 数据库存储对象，对应表结构或查询结果 | `UserRecord`、`AuthSessionRecord`、`EmailVerificationCodeRecord` |

规则：

- DTO 放在 `cloud-types`，是公开 contract 和 SDK 类型来源。
- Entity 放在业务层边界，表达已校验、已转换的业务对象；Entity 可以包含必要的业务方法，但不要发展成另一套复杂 DomainModel 层。
- Record 放在 `cloud-repository`，只服务 SQLite/Postgres repository，不直接返回给 handler、SDK 或 Web。
- `Application Service` 负责显式完成 `Dto -> Entity`、`Record -> Entity`、`Entity -> Dto`、`Entity -> Record` 的转换。
- Auth 核心对象需要 Entity：`UserEntity`、`AuthSessionEntity`、`OAuthAccountEntity`、`DeviceEntity`、`EntitlementEntity`。
- 简单 lookup、配置、短生命周期查询结果如果只是搬字段，可以只保留 DTO/Record，不强制创建 Entity。
- 禁止为了字段搬运新增 `Command`、`ViewModel`、`Model`、`Payload` 等同构结构。
- 少量强类型 value object / enum 允许存在，例如 `UserId`、`EmailAddress`、`UserRole`、`UserStatus`、`AuthProvider`、`PlanCode`，它们用于替代裸 `String`，不算第四套主结构。

推荐流向：

```text
HTTP / SDK / Web
  -> DTO
  -> Application Service
  -> Entity
  -> Repository
  -> Record
  -> SQL
```

反向读取：

```text
SQL
  -> Record
  -> Repository
  -> Entity
  -> Application Service
  -> DTO
  -> HTTP / SDK / Web
```

## 7. SDK 原则

`talos-cloud` 需要输出两个 SDK：

- Rust SDK：供 Talos Rust runtime、CLI、TUI、sidecar 使用。
- TypeScript SDK：供 WebUI、Desktop renderer、前端工具链使用。

SDK 规则：

- SDK 是薄封装，不复制服务端业务逻辑。
- SDK 只负责 transport、typed DTO、错误映射、少量 token/callback helper。
- 权限判断、账号状态、entitlement 判断、设备激活规则都来自服务端响应。
- SDK 不本地计算 role、plan、scope。
- SDK 不接受任意 metadata JSON。
- SDK 类型由 `talos-cloud` contract 生成或统一导出，避免 Rust/TS/服务端 DTO 漂移。

最小 SDK 能力：

```text
auth.register
auth.verifyEmail
auth.login
auth.logout
auth.introspect
auth.googleStart
auth.exchangeCallbackCode
me.get
me.update
me.setPassword
devices.activate
devices.heartbeat
entitlements.current
admin.users.list/create/patch/delete
```

## 8. Auth Web 迁移

`talos-cloud` 拥有权限相关页面实现：

- `/auth/register`
- `/auth/verify-email`
- `/auth/login`
- `/auth/google`
- `/auth/google/callback`
- `/auth/set-password`
- `/account`

Talos 本地 WebUI/Desktop 只保留：

- 登录 gate。
- 打开 cloud auth 页面。
- 接收 callback code 或 token exchange 结果。
- 保存 token / receipt。
- 展示当前用户、plan、登录状态。

本地不再维护完整注册/登录表单。

## 9. Cloud Auth 集成

第一个跨仓契约是 `/v1/auth/introspect`。

最小响应：

```json
{
  "active": true,
  "user": {
    "user_id": "user_123",
    "username": "alice",
    "email": "alice@example.com",
    "role": "user",
    "status": "active"
  },
  "capabilities": {
    "is_admin": false,
    "scopes": ["sessions:read", "sessions:write"]
  },
  "entitlement": {
    "can_use": true,
    "plan": "free",
    "reason": null
  },
  "expires_at": 1790000000000
}
```

Talos mapper 必须拒绝：

- `active=false`。
- unknown role。
- unknown scope。
- user status 非 active。
- entitlement `can_use=false`。
- introspection response 缺必填字段。

本地配置需要区分：

```yaml
cloud:
  enabled: true
  api_base_url: "https://api.talos.love"
  auth:
    provider: "cloud"
    introspection_cache_ttl_seconds: 300

gateway:
  auth: true
```

规则：

- `gateway.auth=false`：open mode。
- `gateway.auth=true + cloud.auth.provider=cloud`：只使用 cloud auth。
- 迁移完成后不再有生产 local auth provider。

## 10. 数据与存储

`talos-cloud` 使用 SQL repository：

- local/test：SQLite。
- production：Postgres。
- in-memory：只用于窄单元测试 fake。

基础表：

- `users`
- `auth_sessions`
- `email_verification_codes`
- `oauth_accounts`
- `devices`
- `entitlements`

SQL 规则：

- unique constraint 放在数据库中。
- 账号创建、邮箱验证、OAuth 绑定、token 签发必须走事务。
- migration 避免 SQLite-only 语法。
- 关键查询做 SQLite/Postgres 兼容测试。

## 11. Plan A 阶段

### A1: Cloud Auth Scaffold

**目标**: 在 `talos-cloud` 建立 Rust workspace、SQL migrations、SQLite repository、typed contract、Auth API skeleton。

**成功标准**: `talos-cloud` 本地可启动，SQLite migration 和基础 route tests 通过。

**测试**: route tests、repository tests、migration tests、contract tests。

**状态**: 未开始

### A2: 迁移现有 Auth 后端能力

**目标**: 将 Talos 现有 register/login/logout/current user/admin users/token verify 行为迁到 `talos-cloud`。

**成功标准**: `talos-cloud` 覆盖现有 Talos auth 行为，并增加 email verification。

**测试**: password hash、email verification TTL、login/logout、disabled user、admin user、introspection。

**状态**: 未开始

### A3: 迁移 Auth Web 页面

**目标**: 将注册、登录、邮箱验证、Google、set password、account 页面迁到 `talos-cloud`。

**成功标准**: Talos 本地不再维护完整 auth form，WebUI/Desktop 通过 cloud auth 页面完成登录。

**测试**: web tests、OAuth callback tests、Desktop packaged auth smoke。

**状态**: 未开始

### A4: SDK 输出与 Talos 多端接入

**目标**: `talos-cloud` 输出 Rust SDK 和 Node/TS SDK；Talos Web/Desktop/CLI/TUI 接入。

**成功标准**: 多端通过 SDK 完成登录、me、introspection、entitlement 检查。

**测试**: SDK tests、Talos Rust tests、Web typecheck/tests、CLI e2e、Desktop DMG smoke。

**状态**: 未开始

### A5: Talos 本地权限能力移除

**目标**: 移除 Talos 本地权限业务、用户表生产用途、本地 auth pages、admin user 业务。

**成功标准**: Talos 权限统一走 `talos-cloud`；本地不再能创建生产账号或签发生产 token。

**测试**: `cargo test`、Web tests、SDK tests、`talos-e2e/run_e2e.sh`、Desktop packaged smoke。

**状态**: 未开始

## 12. Plan B: Product Metrics 后续计划

Plan B 在 Plan A 完整迁移并验证后启动。

核心方向：

- `talos-cloud` 提供 Product Metrics API 与 SDK。
- 多端上报低敏指标。
- 每天打开上报一次：`app_daily_opened`。
- 服务端从请求上下文推导粗粒度地域。
- 指标只记录产品使用状态，不记录用户内容。

核心事件候选：

- `app_daily_opened`
- `app_started`
- `device_registered`
- `device_seen`
- `user_registered`
- `email_verified`
- `user_signed_in`
- `google_account_linked`
- `entitlement_checked`
- `feature_used`
- `app_version_seen`

明确不采集：

- prompt / response。
- session title/content。
- memory / workspace docs。
- file name / workspace path。
- tool arguments。
- exact location。
- raw IP as event property。
- `TurnExecutionMetrics`。

## 13. 验证策略

Plan A 必须验证：

```text
# talos-cloud
cargo test
cargo test -p cloud-repository
cargo test -p cloud-http

# talos
cargo test -p talos-auth
cargo test -p talos-gateway-http
cargo test -p talos-gateway-app-server
talos-e2e/run_e2e.sh

# web/sdk
pnpm --filter @talos/sdk-core test
pnpm --filter @talos/sdk-browser test
pnpm --filter talos-web test
pnpm --filter talos-web typecheck

# desktop
pnpm --filter talos-desktop test
pnpm --filter talos-desktop typecheck
pnpm --filter talos-desktop build:dmg
hdiutil verify <Talos.dmg>
```

不要把间接验证说成手工验证。Desktop 影响 WebView、SDK、main/preload 或 sidecar 时必须基于最终 DMG 验证。

## 14. 风险与约束

| 风险 | 影响 | 处理 |
| --- | --- | --- |
| 追求 100% 兼容但 contract 不冻结 | 多端改动反复 | A1 先冻结 auth contract 和 SDK DTO |
| SDK 复制业务逻辑 | 权限规则漂移 | SDK 只做薄封装，规则来自服务端 |
| Talos 本地残留 auth 能力 | 双权限源导致线上不一致 | A5 明确删除本地账号/权限业务 |
| SQLite/Postgres 方言漂移 | 本地通过但生产失败 | migration 和关键查询做双库兼容测试 |
| Auth Web 与 Talos WebUI 行为分叉 | Desktop/Web 登录体验不一致 | Auth 页面归属 `talos-cloud`，Talos 只消费回调 |
| Product Metrics 提前混入 | 延误 auth 迁移 | Plan B 后置，不进入 Plan A 实现范围 |
