# Phase 0 决策冻结清单

**状态**: 已完成  
**来源 spec**: `spec/desktop-appserver-shared-application-service-design.md`  
**记录时间**: 2026-04-29

本文只记录 Phase 1+ 需要依赖的基线决策，不承载新的架构决策。

## 已冻结决策

- [x] `talos-application-service` 是 HTTP 与 app-server 的唯一共享应用入口。
- [x] `coordinator` 收缩为下层 agent 协调组件，不再作为 application policy 边界。
- [x] `coordinator` 向上输出未编号的 `RawAgentEvent`；客户端最终事件由 application-service 事件处理层生成。
- [x] event id、turn id、session id、持久化、fan-out、replay 由 application-service 事件处理层负责。
- [x] 事件必须先持久化，再 fan-out 到 SSE 或 JSON-RPC notification。
- [x] 首批并发模型为每个 session 只允许一个 active turn。
- [x] `event.subscribe` 是普通 RPC ack，之后通过 notification 推送事件。
- [x] 每个 subscription 使用独立 bounded channel。
- [x] 慢订阅超过容量上限时，以 `subscription.terminated` 语义终止该订阅。
- [x] app-server actor 来自 `initialize.actor_credentials`。
- [x] app-server 初始化前收到非 `initialize` 请求必须拒绝。
- [x] 新 gateway crate 路径为 `crates/gateway/app-server` 与 `crates/gateway/app-protocol`。
- [x] 新共享 application crate 路径为 `crates/orchestration/application-service`。
- [x] JSON-RPC 使用 JSON-RPC 2.0 over stdio JSONL。
- [x] stdout 只输出协议消息，日志写 stderr。
- [x] 协议解码边界可以使用 `serde_json::Value`，进入协议语义 DTO 或 application service 后必须转换为强类型对象。

## Phase 1 移交约束

- HTTP handler 应逐步变成围绕 `TalosApplicationService` 的协议适配层。
- `gateway/http::server::access::ResourceAccessService` 是共享 access policy 的首个迁移候选。
- 迁移期间可临时保留 `CoordinatorImpl` 的旧 trait，但新增协议入口不得把这些旧 trait 当作 application API。
- `ApplicationEvent` 必须是强类型 enum，不允许使用灵活 metadata JSON 承载核心语义。
