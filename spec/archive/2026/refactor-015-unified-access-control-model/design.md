# Talos 统一权限模型重构方案

## 1. 目标

本文只定义一件事：

- Talos 如何重构为一套统一权限模型

本文暂不展开代码实现细节，只明确：

- 权限模型
- 框架落点
- 分层职责
- 重构方向

## 2. 当前问题

当前权限判断分散在多个位置：

- `auth middleware`
- handler 内局部 `require_admin`
- service 内局部 owner/admin 判断
- 前端根据接口报错再推断权限边界

结果是：

- `auth=false` / `auth=true` 语义容易漂移
- 同一权限规则重复实现
- admin 路由和普通资源路由边界不清
- handler 承担了过多权限逻辑

## 3. 设计原则

### 3.1 不自造权限框架

Talos 不应再自建一套“拦截器框架”。

权限重构优先使用 `axum/tower` 原生能力：

- `middleware`
- `route_layer`
- `from_fn_with_state`
- 自定义 `extractor`
- 路由分组 `Router::nest`

也就是说，路由级权限治理应直接落在框架提供的过滤链上，而不是继续散落在 handler 内部。

### 3.2 统一成两种模式

服务端只保留两种运行模式：

- `AuthMode::Open`
- `AuthMode::Enforced`

语义如下：

- `Open`
  - 不登录
  - 服务端注入固定匿名身份
  - admin 路由默认放行
  - 普通资源仍按匿名 owner 视角访问
- `Enforced`
  - 必须登录
  - 普通资源按 owner 访问
  - admin 路由仅 admin 放行

### 3.3 路由级授权与资源级授权分离

必须明确分层：

1. 路由级授权  
2. 资源级授权

其中：

- 路由级授权交给 `axum/tower` filter/middleware
- 资源级授权保留在统一 authorizer/service

不允许把两类问题继续混写。

## 4. 统一权限模型

### 4.1 统一身份对象

所有请求进入服务端后，必须先归一成统一身份：

```rust
pub struct RequestActor {
    pub mode: AuthMode,
    pub user_id: String,
    pub role: UserRole,
}
```

其中：

- `Open` 模式固定：
  - `user_id = "anonymous"`
  - `role = User`
- `Enforced` 模式：
  - 来自 JWT / PAT / 当前登录用户

### 4.2 统一路由访问级别

所有路由必须显式标注访问级别：

```rust
pub enum RouteAccessLevel {
    Public,
    UserScoped,
    AdminOnly,
}
```

语义如下：

- `Public`
  - 不需要 actor
  - 例如 `/health`
- `UserScoped`
  - 需要统一 actor
  - 资源仍按 owner 过滤
- `AdminOnly`
  - `Open` 模式放行
  - `Enforced` 模式仅 admin 放行

## 5. 框架落点

### 5.1 全局认证注入 middleware

保留一层全局 middleware，只做两件事：

1. 判断当前 `AuthMode`
2. 注入统一 `RequestActor`

它不负责：

- owner 资源判断
- admin 资源查询
- 业务资源加载

### 5.2 路由组 filter / route_layer

路由级授权必须通过 `route_layer` 或分组后的 middleware 统一处理。

建议按三类路由分组：

- `public routes`
- `user-scoped routes`
- `admin routes`

然后分别挂统一 filter。

示意：

```text
/health                  -> Public
/v1/auth/*               -> Public
/v1/models               -> UserScoped
/v1/sessions/*           -> UserScoped
/v1/projects/*           -> UserScoped
/v1/admin/*              -> AdminOnly
```

### 5.3 自定义 extractor

可选增加轻量 extractor，避免 handler 手写权限判断。

例如：

- `Actor`
- `AdminActor`
- `UserScopedActor`

这些 extractor 的职责只有：

- 从 request extensions 中取 actor
- 校验当前路由是否允许该 actor 进入
- 失败时直接返回 `401/403`

它们不负责数据库级 owner 检查。

## 6. 资源级授权

路由通过后，资源权限统一进入单一出口：

```rust
pub trait ResourceAuthorizer {
    fn can_read_session(&self, actor: &RequestActor, session: &TalosSession) -> bool;
    fn can_write_session(&self, actor: &RequestActor, session: &TalosSession) -> bool;
    fn can_read_project(&self, actor: &RequestActor, project: &ProjectRecord) -> bool;
    fn can_write_workspace_doc(&self, actor: &RequestActor) -> bool;
}
```

当前约束很简单：

- 普通 session/project 路由只按 owner 判断
- admin 路由不在这里做 route admin 判断
- workspace docs / memory files 这类写接口，只看当前模式下的管理权限语义

## 7. 明确的边界

### 7.1 middleware / route filter 负责什么

- 注入 actor
- 拦截未登录
- 拦截非 admin 访问 admin routes
- 在 `Open` 模式下直接放行 admin routes

### 7.2 middleware / route filter 不负责什么

- 判断某个 session 是否归属于当前用户
- 判断某个 project 是否归属于当前用户
- 读取数据库后做 owner 匹配

这些仍属于资源级授权问题。

### 7.3 handler 不再做什么

重构后，handler 不应再自行写：

- `require_admin()`
- `if actor.role.is_admin()`
- `if actor.has_full_access()`

handler 只做：

- 调业务 service
- 调资源 authorizer
- 返回结果

## 8. 目标行为

### 8.1 `auth=false`

- 不登录
- actor 固定为 `anonymous`
- `/v1/sessions` 只返回 `anonymous` 的 session
- `/v1/admin/*` 可直接访问
- `/admin` 页面直接可访问

### 8.2 `auth=true`

- 未登录：
  - `UserScoped` 路由拒绝
  - `AdminOnly` 路由拒绝
- 普通用户：
  - 可访问自己的 session/project
  - 不可访问 `/v1/admin/*`
- admin：
  - 可访问 admin routes

## 9. 不再允许的旧模式

重构后不再允许继续保留这些写法：

- 在多个 handler 中重复 `require_admin`
- 同时混用 `role.is_admin()` 与 `has_full_access()`
- 用 `auth_disabled` 在各层做特殊分支
- 前端依赖接口报错自行猜测权限边界

## 10. 建议落地顺序

### 阶段 1

- 定义 `AuthMode`
- 定义 `RequestActor`
- 定义 `RouteAccessLevel`

### 阶段 2

- 路由分组
- 用 `route_layer` / middleware 统一 admin 与 user-scoped 路由放行逻辑

### 阶段 3

- 引入轻量 extractor
- 删除 handler 级重复权限判断

### 阶段 4

- 收敛资源级授权到单一 authorizer/service
- 清理旧权限辅助函数和分散判断

## 11. 一句话结论

Talos 权限重构应优先借助 `axum/tower` 原生 filter/middleware/extractor 能力，实现：

- 一个统一模式入口：`AuthMode`
- 一套路由级过滤：`route_layer + middleware`
- 一套资源级授权：`ResourceAuthorizer`

而不是继续在 handler 和 service 中散落权限判断。
