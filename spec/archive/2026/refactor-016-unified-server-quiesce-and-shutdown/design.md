# TUI Embedded / Gateway Server Unified Shutdown Design

## 1. 背景与问题

当前 Talos 存在两类 server 运行形态：

1. 独立 gateway/server 进程
2. TUI 内嵌 embedded server

二者都承载同一套 session、memory flush、异步后台任务执行能力，因此理论上应共享同一套停止协议：

1. 先停止接受新请求
2. 等待已接收请求完成或超时
3. 等待挂起的异步后台任务完成或超时
4. 最终退出进程/返回控制权

但当前 embedded 模式并未复用完整 runtime 生命周期：

- `ui-tui` 启动 embedded 时，创建了 `TalosServerRuntime`
- 随后立即调用 `into_http_server()` 取出 `HttpServer`
- 再单独 `spawn http_server.run()`
- TUI 退出后，进程直接结束，绕过了 `TalosServerRuntime::run()` 尾部的 pending memory flush drain

结果是：

- `/quit -> close_session` 只保证“已提交 flush”
- 不保证“flush 已完成持久化”
- 独立 server 模式通常没问题，因为 server 进程仍然存活
- embedded 模式容易在进程退出时中断后台 flush，导致 memory 未落盘

## 2. 目标

本方案目标不是只修 embedded memory flush，而是抽象一套统一的 server quiesce / shutdown 机制，供以下场景复用：

1. TUI embedded 模式退出
2. gateway/server 独立进程收到退出信号
3. 后续其他宿主形态需要优雅停机

## 3. 非目标

本方案不包含以下内容：

1. 不修改 `close_session` 为“同步等待 memory flush 完成再返回”
2. 不在 TUI 层直接调用 coordinator/agent 的 drain 能力
3. 不引入 sleep-based 等待策略
4. 不增加兼容性 fallback 分支

## 4. 根因分析

### 4.1 当前关闭语义割裂

当前“优雅退出”被拆成了两段互不一致的逻辑：

1. `HttpServer::run()` 只负责监听 HTTP 请求，并响应 OS signal 做 graceful shutdown
2. `TalosServerRuntime::run()` 在 `http_server.run()` 返回后，额外执行 coordinator pending memory flush drain

这意味着真正完整的 shutdown 语义属于 `TalosServerRuntime`，而不是 `HttpServer`。

### 4.2 Embedded 模式丢失 runtime 所有权

embedded 启动过程只保留了 `HttpServer`，没有保留 `TalosServerRuntime` 的 shutdown 所有权，因此：

1. 无法复用 runtime 尾部 drain 逻辑
2. 无法统一控制“停止接收请求”与“等待后台任务完成”的顺序
3. 导致 TUI 和 gateway server 实际上走了两套退出路径

## 5. 设计原则

1. 统一关闭协议归属于 server runtime，不归属于 TUI
2. “停止接受请求”与“等待后台任务 drain”必须是同一条主路径
3. embedded 与 standalone 只允许宿主不同，不允许 shutdown 语义不同
4. timeout 必须可配置并可观测
5. 不新增松散 metadata 结构，关闭控制应使用强类型对象

## 6. 总体方案

引入统一的两阶段 server 生命周期模型：

1. `quiesce`
2. `shutdown`

语义定义如下：

- `quiesce`: server 停止接受新请求，但继续处理已进入系统的请求，并允许现有后台任务收尾
- `shutdown`: 在 `quiesce` 基础上，等待 runtime 级异步任务 drain 完成，然后结束运行

其中：

1. `HttpServer` 负责 network ingress quiesce
2. `TalosServerRuntime` 负责 orchestrator/runtime task drain
3. TUI embedded 与独立 gateway 都只调用 `TalosServerRuntime` 的统一 shutdown API

## 7. 生命周期流程

### 7.1 正常运行

```text
Host Process
    |
    v
TalosServerRuntime
    |
    +--> HttpServer accepts requests
    |
    +--> Coordinator / Agent handle sessions
    |
    +--> Background tasks
          - memory flush
          - future async cleanup tasks
```

### 7.2 统一关闭流程

```text
Shutdown Request
    |
    v
TalosServerRuntime::begin_shutdown()
    |
    +--> mark runtime state = Quiescing
    |
    +--> signal HttpServer to stop accepting new requests
    |
    +--> wait HttpServer serve loop exits
    |
    +--> coordinator.drain_pending_tasks(timeout)
            |
            +--> agent.drain_memory_flushes(timeout)
            +--> future task families
    |
    +--> mark runtime state = Stopped
    |
    v
return shutdown result
```

### 7.3 Embedded TUI 退出流程

```text
User /quit
    |
    v
TUI closes current session
    |
    v
TUI exits main loop
    |
    v
Embedded host calls runtime.shutdown()
    |
    v
runtime quiesces server + drains tasks
    |
    v
process exits
```

## 8. 关键设计

### 8.1 引入统一 runtime 控制柄

需要为 `TalosServerRuntime` 增加显式控制能力，而不是把 shutdown 隐藏在 `run()` 尾部。

推荐拆分为：

1. `TalosServerRuntime`
2. `TalosServerHandle`

职责：

- `TalosServerRuntime`: 拥有 server 组件与协调逻辑
- `TalosServerHandle`: 宿主持有的控制接口，可发起 quiesce/shutdown 并等待完成

这样 embedded TUI 和 standalone main 都能拿到同一种句柄。

#### 8.1.1 `handle.shutdown()` 与 `run()` 的协调机制

`TalosServerHandle::shutdown()` 不能直接“自己执行 shutdown 全流程”，否则会与 `runtime.run()` 形成双 owner 竞态。  
本方案要求：

1. `run()` 是唯一执行 shutdown 状态流转与资源回收的 owner
2. `handle.shutdown()` 只负责发送 shutdown 请求并等待 `run()` 完成

推荐机制：

1. 使用 `tokio_util::sync::CancellationToken` 作为 shutdown 请求广播信号
2. 使用 `tokio::sync::watch` 作为 lifecycle 状态广播通道
3. `run()` 内部 `select!` 监听：
   - OS signal
   - `handle.shutdown()` 触发的 cancellation
4. `handle.shutdown()`：
   - 触发 cancellation token
   - 等待 `watch` 状态进入 `Stopped`
   - 若进入错误态或超时，则返回错误

这样可以保证：

1. standalone 与 embedded 复用同一条 `run()` 主路径
2. 宿主不会绕过 runtime 自己执行 drain
3. `server_task.await` 与 `handle.shutdown().await` 语义一致且无重复清理

### 8.2 HttpServer 需要支持主动 quiesce

当前 `HttpServer::run()` 只支持基于 OS signal 的 graceful shutdown，这不足以支持 embedded 宿主主动关闭。

因此需要把 HTTP 层改为支持：

1. 外部 shutdown signal
2. 在 quiescing 状态下拒绝新请求

建议行为：

1. listener 停止接受新连接/新请求
2. 已进入 axum 服务链的请求继续完成
3. 新请求返回明确错误

推荐返回：

- HTTP `503 Service Unavailable`
- 文案：`server is shutting down`

#### 8.2.1 lifecycle gate 的放置位置

本方案要求同时使用两层控制，而不是二选一：

1. `HttpServer` listener shutdown
2. axum router 顶层 middleware gate

原因：

1. 仅关闭 listener，无法覆盖 keep-alive / 连接复用下已建立连接继续发送的新请求
2. 仅依赖 middleware，虽然能拒绝业务请求，但不能尽快停止网络入口与 serve loop

因此推荐顺序：

1. runtime 先把 lifecycle 状态切到 `Quiescing`
2. router 顶层 middleware 开始返回 `503`
3. 同时触发 listener graceful shutdown，停止接受新的网络入口
4. 已经进入 tower service 的请求继续自然完成

其中 middleware 应放在 axum `Router` 顶层公共 layer，覆盖所有业务路由；`/health` 明确绕过该 gate。

### 8.3 Runtime 统一 drain 后台任务

当前 runtime 只 drain pending memory flush。方案要求抽象成更泛化的 runtime task drain 概念。

建议保留当前 coordinator 接口风格，但语义改为统一任务族：

1. 当前必须覆盖 `memory_flush`
2. 后续可扩展其他后台任务，无需再改 TUI 或 HTTP shutdown 流程

### 8.4 生命周期状态的并发表示

`ServerLifecycleState` 需要同时满足两类访问特征：

1. middleware 高频只读检查
2. runtime 低频状态推进与等待

因此不建议仅用 `RwLock<ServerLifecycleState>` 作为唯一实现。

推荐组合：

1. `AtomicU8` 保存当前 lifecycle 状态枚举值
2. `watch::Sender<ServerLifecycleState>` 广播状态流转事件

职责分工：

1. middleware 热路径通过 `AtomicU8` 做无锁判断 `is_quiescing_or_later()`
2. `handle.shutdown()` 和测试代码通过 `watch::Receiver` 等待 `Stopped` 或错误终态

这样兼顾：

1. 热路径性能
2. 状态可观测性
3. 宿主等待 shutdown 完成的同步需求

### 8.5 Session close 语义保持不变

本方案不改 `close_session` 语义，仍然保持：

1. session close 触发异步 flush
2. 真正的“等待它完成”由 runtime shutdown 负责

这样做的原因：

1. `close_session` 是业务动作
2. `shutdown` 是宿主生命周期动作
3. 两者职责不应耦合

## 9. API 伪代码

### 9.1 Runtime 状态

```rust
pub enum ServerLifecycleState {
    Running,
    Quiescing,
    Draining,
    Stopped,
}

pub struct ServerShutdownConfig {
    pub request_drain_timeout: Duration,
    pub background_task_drain_timeout: Duration,
}

pub struct ServerLifecycle {
    state: AtomicU8,
    updates_tx: watch::Sender<ServerLifecycleState>,
}
```

### 9.2 Runtime 对外接口

```rust
pub struct TalosServerRuntime {
    http_server: HttpServer,
    coordinator: Arc<CoordinatorImpl>,
    shutdown: ServerShutdownController,
    lifecycle: Arc<ServerLifecycle>,
}

impl TalosServerRuntime {
    pub fn handle(&self) -> TalosServerHandle;

    pub async fn run(self) -> TalosResult<()> {
        self.run_until_shutdown(None).await
    }

    pub async fn run_until_shutdown(
        self,
        external_signal: Option<ShutdownSignal>,
    ) -> TalosResult<()>;

    async fn perform_shutdown(self) -> TalosResult<()> {
        self.lifecycle.store(ServerLifecycleState::Quiescing);
        self.http_server.begin_quiesce().await?;
        self.http_server.wait_stopped().await?;
        self.lifecycle.store(ServerLifecycleState::Draining);
        self.coordinator
            .drain_background_tasks(self.shutdown.background_task_drain_timeout)
            .await?;
        self.lifecycle.store(ServerLifecycleState::Stopped);
        Ok(())
    }
}
```

备注：

1. `perform_shutdown()` 应为 runtime 内部私有方法，不对宿主暴露
2. 宿主只允许通过 `TalosServerHandle` 控制 shutdown

### 9.3 Host 侧用法

独立 server：

```rust
let runtime = start_server(config, options).await?;
runtime.run().await?;
```

embedded TUI：

```rust
let runtime = start_server(config, options).await?;
let handle = runtime.handle();
let server_task = tokio::spawn(async move { runtime.run().await });

run_tui_app_with_client(client, TalosConnectionMode::Embedded).await?;

handle.shutdown().await?;
server_task.await??;
```

说明：

1. `handle.shutdown()` 内部只触发 shutdown token 并等待 lifecycle `Stopped`
2. `runtime.run()` 内部 `select!` 监听 OS signal 与 shutdown token
3. 真正执行 `perform_shutdown()` 的只有 `run()`

### 9.4 HTTP 拒绝新请求伪代码

```rust
async fn reject_when_quiescing(
    State(state): State<Arc<HttpApiState>>,
    req: Request,
    next: Next,
) -> Result<Response, StatusCode> {
    if state.lifecycle.is_quiescing_or_later() {
        return Ok(service_unavailable("server is shutting down"));
    }
    Ok(next.run(req).await)
}
```

## 10. 模块接口清单

| 模块 | 类型/接口 | 变更 | 说明 |
| --- | --- | --- | --- |
| `crates/app/talos-server` | `TalosServerRuntime` | 扩展 | 成为统一 quiesce/shutdown 生命周期 owner |
| `crates/app/talos-server` | `TalosServerHandle` | 新增 | 宿主发起 shutdown 并等待完成 |
| `crates/app/talos-server` | `ServerLifecycleState` | 新增 | 强类型 runtime 生命周期状态 |
| `crates/app/talos-server` | `ServerLifecycle` | 新增 | `AtomicU8 + watch` 的状态并发包装 |
| `crates/app/talos-server` | `ServerShutdownConfig` | 新增 | shutdown 超时配置 |
| `crates/gateway/http` | `HttpServer` | 扩展 | 支持外部 quiesce signal、等待 serve loop 退出 |
| `crates/gateway/http` | `HttpApiState.lifecycle` | 新增 | 用于拒绝新请求 |
| `crates/gateway/http` | `LifecycleGateLayer` | 新增 | Router 顶层 middleware，`/health` 明确绕过 |
| `crates/orchestration/coordinator` | `drain_background_tasks(timeout)` | 重命名或扩展 | 统一 drain 后台任务族，当前至少包含 memory flush |
| `apps/ui-tui` | embedded 启动流程 | 调整 | 保留 runtime handle，不再拆出裸 `HttpServer` |

## 11. 行为约束

### 11.1 Quiesce 期间请求行为

建议规则：

1. health endpoint 可继续访问
2. 已进入系统的请求允许自然完成
3. 新业务请求统一拒绝
4. SSE/流式请求若已建立，允许流结束
5. `health` 路由不经过 lifecycle gate middleware

### 11.2 Shutdown 超时行为

必须显式可观测：

1. quiesce 超时
2. background drain 超时
3. 任一后台任务失败

建议：

1. 记录 warning/error
2. 对 embedded 宿主返回错误，让上层决定是否强制退出
3. 不允许静默吞掉失败

## 12. 与现有逻辑的兼容关系

### 12.1 TUI `/quit`

`/quit` 行为保持两段式：

1. 先执行 `close_session`
2. TUI 主循环退出后，由 embedded host 统一执行 runtime shutdown

这样 TUI 不需要知道 memory flush 细节，只负责“结束用户交互”。

### 12.2 Standalone Server

收到 OS signal 时，不再只依赖 `HttpServer` 自身的 signal handler，而是统一经由 `TalosServerRuntime` 的 shutdown 主路径。

这样可以确保：

1. 停止收请求
2. 等待 HTTP serve loop 退出
3. drain pending background tasks

三步永远一起发生。

## 13. 实施步骤

### 阶段 1: Runtime Shutdown 能力抽象

目标：

- 从 `TalosServerRuntime::run()` 尾部内联逻辑中抽离统一 shutdown 主路径

成功标准：

- runtime 支持显式 quiesce/shutdown
- standalone 运行行为不变

测试：

- `talos-server` 单测：`Running -> Quiescing -> Draining -> Stopped` 状态流转正确
- `talos-server` 单测：shutdown 调用 coordinator drain
- `talos-server` 单测：background drain timeout 返回正确错误
- `talos-server` 单测：重复调用 `handle.shutdown()` 不会重复执行 drain

### 阶段 2: HTTP Server 可控 Quiesce

目标：

- `HttpServer` 支持外部 shutdown signal 与 quiescing 拒绝新请求

成功标准：

- 外部可以主动让 HTTP server 停止接受新请求
- in-flight 请求可自然完成

测试：

- HTTP 单测：quiescing 后新请求返回 `503`
- 已建立流请求可正常结束

### 阶段 3: Embedded TUI 接入统一 Runtime

目标：

- embedded 模式持有 runtime/handle，退出时走统一 shutdown

成功标准：

- `/quit` 后 embedded 退出前会等待 runtime drain

测试：

- TUI 集成测试：embedded 模式 `/quit` 后 server shutdown 被调用

### 阶段 4: End-to-End 回归

目标：

- 覆盖 memory flush 与 shutdown 语义回归

成功标准：

- embedded 和 standalone 都能在退出时持久化 memory

测试：

- 新增 embedded e2e：消息 -> `/quit` -> 退出 -> memory 文件包含 session 记录
- 保留现有 `07_session_close_flushes_memory.sh`

## 14. 风险与决策

### 风险 1: 只停 listener 不足以拒绝复用连接请求

需要在服务中间件层增加 lifecycle gate，不能只依赖 listener 关闭。

缓解措施：

1. Router 顶层 middleware 负责请求级拒绝
2. listener graceful shutdown 负责入口级停止
3. 两者同时存在，避免连接复用穿透 quiesce 期

### 风险 2: 后台 drain 范围持续扩张

因此不应把接口命名永久绑定到 `memory_flush`，而应抽象为后台任务 drain。

### 风险 3: TUI 与 server 双重等待导致竞态

shutdown 的唯一 owner 必须是 `TalosServerRuntime`。TUI 只调用 handle，不直接 drain 子系统。

## 15. 验收标准

1. embedded 与 standalone 共用一套 runtime shutdown 主路径
2. shutdown 先停止接受请求，再等待后台任务完成
3. `/quit` 后 embedded 模式 memory 能稳定落盘
4. standalone 进程退出时仍保留原有 drain 能力
5. 新请求在 quiescing 阶段被明确拒绝，而不是隐式连接失败
