# Talos 服务端 Auth 与 User Access 极简设计

## 1. 文档范围

本文定义 Talos 服务端极简版 auth 与 user access 方案。

本方案只解决 3 个问题：

- 用户如何注册与登录
- 服务端如何签发与校验 Bearer Token
- `Session` 与 `Project` 如何基于用户 ownership 做访问控制

本文不覆盖：

- `AuthSession`
- `refresh token`
- `PAT`
- `device code`
- memory/workspace 物理隔离
- CLI/TUI/Web 交互细节

## 2. 目标

- 先以最小复杂度跑通用户注册、登录、鉴权、资源访问
- 所有后续资源访问统一基于服务端签发的 Bearer Token
- 为 `Session` 与 `Project` 接入 `owner_user_id`
- 统一请求身份注入与权限判断
- 严格执行 `store -> service -> api` 三层分离
- 保持后续可平滑扩展更复杂的 token/session 体系

## 3. 非目标

- 不实现多设备登录态管理
- 不实现单设备粒度 logout
- 不实现 refresh token 续签
- 不实现 PAT
- 不实现 device pairing
- 不保留 `/v1/me/*` 命名

## 4. 核心设计

### 4.1 单一 Bearer Token 模型

当前阶段只保留一种访问凭证：

- 服务端签发的 JWT access token

客户端访问其他资源时，必须携带：

```text
Authorization: Bearer <token>
```

客户端不能自行拼装权限结构体回传给服务端。

### 4.2 Token 默认有效期 30 天

access token 默认有效期为 `30d`。

这样可以先满足“减少频繁登录”的产品目标，同时避免引入 refresh token 与登录态表。

该 TTL 必须进入 `config` 模块统一管理。

### 4.3 logout 采用 `token_version` 失效策略

由于当前阶段不引入独立登录态表，`logout` 采用用户级别的 token 失效策略：

- `users.token_version` 初始值为 `1`
- 签发 token 时把 `token_version` 写入 claims
- `POST /v1/auth/logout` 时，将当前用户的 `token_version` 加 `1`
- middleware 校验 token 时，若 claims 中的 `token_version` 与数据库不一致，则鉴权失败

该方案语义简单，但意味着 logout 会让该用户所有已签发 token 同时失效。

### 4.4 资源访问只做 owner/admin 两级权限

当前阶段只保留两级资源访问规则：

- owner：只能访问自己的资源
- admin：可访问全部资源

本阶段不引入更细粒度 ACL / RBAC 子系统。

### 4.5 `Session` 与 `Project` 保持独立

业务 `Session` 和 `Project` 继续保持独立资源，不与 auth 领域混合建模。

本次只做 ownership 接入：

- `Session.owner_user_id`
- `Project.owner_user_id`

### 4.6 service 不直接碰数据库

只有 store 实现可以访问数据库。

service 只负责：

- 调用 store
- 实现密码校验与 token 签发逻辑
- 实现 owner/admin 权限判断
- 组装 DTO

## 5. 领域模型

### 5.1 `User`

核心字段建议：

- `user_id`
- `username`
- `password_hash`
- `role`
- `token_version`
- `disabled`
- `created_at`
- `updated_at`

建议持久化表：

- `users`

### 5.2 `UserRole`

角色必须是强类型枚举，不使用字符串裸传。

```rust
pub enum UserRole {
    User,
    Admin,
}
```

### 5.3 `AccessTokenClaims`

JWT claims 建议包含：

- `sub`: `user_id`
- `role`
- `scopes`
- `token_version`
- `exp`
- `iat`
- `iss`

说明：

- `role` 用于 admin 判定
- `scopes` 用于后续扩展，不在本阶段过度设计
- `token_version` 用于 logout 失效控制

### 5.4 `RequestIdentity`

middleware 验证 bearer token 成功后，注入统一身份对象：

```rust
pub struct RequestIdentity {
    pub user_id: String,
    pub role: UserRole,
    pub scopes: Vec<Scope>,
}
```

规则：

- handler 将 `&RequestIdentity` 传给 service
- service 根据 `RequestIdentity` 做 owner/admin 判定
- handler 不自行拼装领域权限逻辑

### 5.5 资源 ownership

资源侧统一使用：

- `Session.owner_user_id`
- `Project.owner_user_id`

注意：

- `owner_user_id` 是资源权限字段
- `TalosContext.user_id` 是业务上下文字段
- 权限判断一律基于 `owner_user_id`，不能混用 `TalosContext.user_id`

## 6. 数据存储与迁移

### 6.1 `users` 表

建议字段：

```text
users
  user_id TEXT PRIMARY KEY
  username TEXT UNIQUE NOT NULL
  password_hash TEXT NOT NULL
  role TEXT NOT NULL
  token_version INTEGER NOT NULL
  disabled BOOLEAN NOT NULL
  created_at INTEGER NOT NULL
  updated_at INTEGER NOT NULL
```

### 6.2 `sessions` 与 `projects`

需要增加并统一使用：

- `sessions.owner_user_id`
- `projects.owner_user_id`

迁移要求：

1. 为 `sessions` 增加 `owner_user_id`
2. 将历史 `sessions.user_id` 回填到 `owner_user_id`
3. 为 `projects` 增加 `owner_user_id`
4. 所有资源权限查询改为基于 `owner_user_id`

本阶段不保留新的权限字段与旧字段长期并存的双语义状态。

## 7. 分层规范

| 层 | 职责 | 禁止事项 |
| --- | --- | --- |
| store | 数据库 CRUD、强类型查询参数、强类型记录 | 权限判断、HTTP 错误语义 |
| service | 注册、登录、token 校验、ownership、DTO 组装 | 直接写 SQL、直接操作 storage |
| api | 请求解析、middleware 接入、service 调用、错误映射 | 直接写 SQL、直接调用 store 做业务拼装 |

补充规则：

- 每个业务实体拥有自己的 store API
- handler 不做 owner 过滤拼装
- 越权返回 `not found` 还是 `forbidden` 由 API 层做错误映射

## 8. 核心流程

### 8.1 注册流程

```text
Client
  -> POST /v1/auth/register
  -> Auth handler
  -> AuthService.register()
  -> UserStore.create_user()
  -> JWT signer issue 30d token
  -> return access_token + user + capabilities
```

### 8.2 登录流程

```text
Client
  -> POST /v1/auth/login
  -> Auth handler
  -> AuthService.login()
  -> UserStore.get_user_by_identifier()
  -> verify password hash
  -> JWT signer issue 30d token
  -> return access_token + user + capabilities
```

### 8.3 鉴权流程

```text
Client
  -> Authorization: Bearer <token>
  -> auth middleware
  -> verify JWT signature
  -> verify exp
  -> load user by sub
  -> check disabled == false
  -> check claims.token_version == users.token_version
  -> build RequestIdentity
  -> handler
  -> service
```

### 8.4 资源访问流程

```text
HTTP handler
  -> auth middleware
  -> RequestIdentity
  -> SessionService / ProjectService
  -> if actor.role == Admin:
       query all or target owner
     else:
       force owner_user_id = actor.user_id
  -> store query by owner-aware filter
  -> DTO
  -> handler maps error
```

## 9. 权限模型

### 9.1 普通用户

- 只能访问自己的 `Session`
- 只能访问自己的 `Project`
- 列表查询自动附加 `owner_user_id = actor.user_id`
- 详情查询若 owner 不匹配，service 返回资源不存在语义，由 API 层映射为 `404`

### 9.2 管理员

- 可访问全部 `Session`
- 可访问全部 `Project`
- 可执行 `/v1/admin/*` 用户管理接口

### 9.3 logout 语义

当前阶段 `logout` 的语义是：

- 让当前用户所有旧 token 一次性失效

不是：

- 仅让当前设备失效

该限制是本阶段为保持极简而接受的约束。

## 10. HTTP API 形态

### 10.1 Public 路由

- `POST /v1/auth/register`
- `POST /v1/auth/login`
- `GET /health`

### 10.2 Authenticated 路由

- `POST /v1/auth/logout`
- `GET /v1/user`
- `PATCH /v1/user`
- `GET /v1/sessions`
- `POST /v1/sessions`
- `GET /v1/projects`
- `POST /v1/projects`
- `GET /v1/projects/:project_id`
- `PATCH /v1/projects/:project_id`
- `DELETE /v1/projects/:project_id`

### 10.3 Admin 路由

- `GET /v1/admin/users`
- `POST /v1/admin/users`
- `PATCH /v1/admin/users/:user_id`
- `DELETE /v1/admin/users/:user_id`
- `GET /v1/admin/sessions`
- `GET /v1/admin/projects`

### 10.4 登录响应结构

登录成功响应建议：

```json
{
  "access_token": "<jwt>",
  "token_type": "Bearer",
  "expires_at": 1760000000,
  "user": {
    "user_id": "user-123",
    "username": "alice",
    "role": "user"
  },
  "capabilities": {
    "is_admin": false,
    "role": "user",
    "scopes": [
      "sessions:read",
      "sessions:write",
      "projects:read",
      "projects:write"
    ]
  }
}
```

说明：

- `capabilities` 用于客户端展示与本地逻辑判断
- 服务端后续鉴权只相信 Bearer Token，不相信客户端回传的 `capabilities`

## 11. Store 与 Service 设计

### 11.1 `UserStore`

核心操作：

- `create_user`
- `get_user_by_id`
- `get_user_by_identifier`
- `update_user`
- `update_password_hash`
- `increment_token_version`
- `list_users`
- `delete_user`

### 11.2 `AuthService`

职责：

- register
- login
- logout
- verify bearer token

依赖：

- `UserStore`
- JWT 编码组件
- 密码哈希组件

### 11.3 `UserService`

职责：

- 获取当前用户
- 修改当前用户
- admin list users
- admin create user
- admin patch user
- admin delete user

### 11.4 `SessionService`

职责：

- session CRUD
- owner-aware session 查询
- admin session 查询

### 11.5 `ProjectService`

职责：

- project CRUD
- owner-aware project 查询
- admin project 查询

## 12. 接口伪代码

### 12.1 Store Trait 示例

```rust
#[async_trait]
pub trait UserStore: Send + Sync {
    async fn create_user(&self, input: CreateUser) -> TalosResult<User>;
    async fn get_user_by_id(&self, user_id: &str) -> TalosResult<Option<User>>;
    async fn get_user_by_identifier(&self, identifier: &str) -> TalosResult<Option<User>>;
    async fn update_user(&self, user_id: &str, patch: UserPatch) -> TalosResult<User>;
    async fn update_password_hash(&self, user_id: &str, password_hash: &str) -> TalosResult<()>;
    async fn increment_token_version(&self, user_id: &str) -> TalosResult<u64>;
    async fn list_users(&self, query: UserListQuery) -> TalosResult<Page<User>>;
    async fn delete_user(&self, user_id: &str) -> TalosResult<()>;
}
```

### 12.2 Auth Service 示例

```rust
#[async_trait]
pub trait AuthService: Send + Sync {
    async fn register(&self, input: RegisterRequest) -> TalosResult<AuthResponse>;
    async fn login(&self, input: LoginRequest) -> TalosResult<AuthResponse>;
    async fn logout(&self, actor: &RequestIdentity) -> TalosResult<()>;
    async fn verify_access_token(&self, token: &str) -> TalosResult<RequestIdentity>;
}
```

### 12.3 资源 Service 示例

```rust
#[async_trait]
pub trait SessionService: Send + Sync {
    async fn list_sessions(
        &self,
        actor: &RequestIdentity,
        query: SessionListQuery,
    ) -> TalosResult<Page<SessionDto>>;
}
```

## 13. 模块布局建议

```text
crates/infra/auth/
  src/
    model/
      user.rs
      user_role.rs
      access_token_claims.rs
      request_identity.rs
    store/
      user_store.rs
    service/
      auth_service.rs
      user_service.rs
    jwt.rs
    password.rs
```

HTTP 模块只保留：

- auth handlers
- user handlers
- admin handlers
- middleware
- HTTP 错误映射

## 14. 配置治理

所有默认行为与开关统一进入 `config` 模块。

建议新增配置：

- `gateway.auth.enabled`
- `gateway.jwt_secret`
- `gateway.access_token_ttl_days`
- `gateway.allow_register`

默认建议：

- `access_token_ttl_days = 30`

## 15. 落地顺序

1. 先在 `users` 表与 `UserStore` 中落地用户模型
2. 增加密码哈希、JWT 签发与 JWT 校验组件
3. 实现 `POST /v1/auth/register`、`POST /v1/auth/login`、`POST /v1/auth/logout`
4. 在 middleware 中把 Bearer Token 转换为 `RequestIdentity`
5. 为 `Session` 与 `Project` 增加 `owner_user_id`
6. 让 `SessionService` 与 `ProjectService` 接收 `actor` 并做 owner/admin 判定
7. 最后补齐 `/v1/user/*` 与 `/v1/admin/*` 接口

## 16. 测试要求

至少覆盖：

- 用户注册成功
- 用户登录成功
- 错误密码登录失败
- token 过期后访问失败
- `token_version` 不匹配后访问失败
- 普通用户只能看到自己的 `Session`
- 普通用户不能访问其他人的 `Project`
- admin 可访问所有资源

至少执行：

- 相关 crate 单元测试
- `talos-e2e/run_e2e.sh`

## 17. 明确不做的事情

本阶段明确不做：

- `AuthSession`
- refresh token
- PAT
- device code
- 细粒度 RBAC
- 复杂审计子系统

先把“用户注册/登录 -> 返回 30 天 token -> 带 token 访问资源 -> 服务端按 owner/admin 判权”这条主链路做稳定，再考虑扩展。
