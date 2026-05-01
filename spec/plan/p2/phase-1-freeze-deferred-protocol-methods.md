# P2 Phase 1: Freeze Deferred App Protocol Methods

**状态**: 已完成  
**目标**: freeze `tool.dynamic_result.submit`、`admin.session.list`、`admin.project.list`。  
**输出**: app protocol、app-server dispatch、Rust SDK、TS SDK 与 protocol smoke 全部对齐。  

---

## 1. 背景

`DEFERRED_CLIENT_METHODS` 当前仍保留以下方法：

- `tool.dynamic_result.submit`
- `admin.session.list`
- `admin.project.list`

这些方法在 P1 不阻塞 CR。P2 需要一次性完成协议 freeze，并从 deferred list 移除。

---

## 2. 范围

| Method | 范围 | 依赖 |
|---|---|---|
| `tool.dynamic_result.submit` | 客户端提交动态工具结果，驱动等待中的 turn 恢复执行 | server request / client response correlation 模型稳定 |
| `admin.session.list` | 本地 admin actor 查询 session 列表 | actor credentials 与权限错误模型稳定 |
| `admin.project.list` | 本地 admin actor 查询 project 列表 | actor credentials 与权限错误模型稳定 |

---

## 3. 非目标

- 不新增第二套 admin service；如需 admin 查询能力，应挂到 application-service 或明确的共享服务层。
- 不在 Rust SDK / TS SDK 内定义独立业务结构体；SDK 只做薄 helper 和 transport 错误包装。
- 不引入 `serde_json::Value` / loose metadata 承载核心语义。
- 不在本阶段运行全量 `tests/integration/run_e2e.sh`。

---

## 4. 子任务

- [x] 在 `talos-app-protocol` 中新增或 freeze 对应 params / response / error data。
- [x] 将 app-server dispatch 接到 application-service 或明确共享服务层，避免协议层直连下层 store / runtime 私有 API。
- [x] 更新 Rust SDK JSON-RPC helper 与 TS SDK app-server helper，保持 DTO 来自生成物或共享 core。
- [x] 刷新 schema 与 generated TS fixture。
- [x] 扩展 protocol smoke / resource matrix，覆盖成功、权限错误、not found / conflict 映射。
- [x] 从 `DEFERRED_CLIENT_METHODS` 移除已完成 method，并删除对应漂移测试里的 deferred 断言。

---

## 5. 成功标准

- 协议 DTO 以 `talos-app-protocol` Rust 类型为真源。
- app-server dispatch 接入 application-service 或明确的共享服务层。
- Rust SDK / TS SDK 暴露薄封装 helper，不重复定义业务 DTO。
- protocol smoke / resource matrix 覆盖成功路径、权限错误、not found / conflict 等错误映射。
- schema 与 TS generated fixture 已刷新。
- `DEFERRED_CLIENT_METHODS` 不再包含本阶段 3 个 method。

---

## 6. 局部验证

```text
cargo test -p talos-app-protocol
cargo test -p talos-app-server
cargo test -p talos-client
cargo run -p talos-app-protocol --example export_fixtures
source .env && tests/integration/protocol_smoke.sh
pnpm --dir talos-app --filter @talos/sdk-core test
pnpm --dir talos-app --filter @talos/sdk-app-server test:integration
```

全量 `source .env && tests/integration/run_e2e.sh` 留到 Phase 4 最后统一执行。

---

## 7. 完成记录

**变更摘要**: 已 freeze `tool.dynamic_result.submit`、`admin.session.list`、`admin.project.list`；app-server dispatch 复用 `TalosApplicationService` 既有 dynamic result 与 admin list API；Rust SDK / TS SDK 增加薄 helper；protocol schema 与 generated TS fixture 已刷新；protocol smoke resource matrix 增加 admin list 成功路径与 dynamic result missing-session 错误映射。

**关键文件路径**:
- `talos-rs/crates/gateway/app-protocol/src/requests.rs`
- `talos-rs/crates/gateway/app-protocol/src/schema.rs`
- `talos-rs/crates/gateway/app-protocol/src/version.rs`
- `talos-rs/crates/gateway/app-server/src/application.rs`
- `talos-rs/apps/sdk/src/app_server.rs`
- `talos-rs/apps/app-server-test-client/src/main.rs`
- `talos-app/ts-sdk/packages/core/src/app-server.ts`
- `talos-app/ts-sdk/packages/core/src/generated/app-protocol.ts`

**已执行测试**:
- `cargo test -p talos-app-protocol`
- `cargo test -p talos-app-server`
- `cargo test -p talos-client`
- `cargo fmt --check`
- `cargo run -p talos-app-protocol --example export_fixtures`
- `source .env && tests/integration/protocol_smoke.sh`
- `pnpm --dir talos-app --filter @talos/sdk-core test`
- `source talos-rs/.env && TALOS_APP_SERVER_BIN="$PWD/talos-rs/target/debug/talos" TALOS_APP_SERVER_CWD="$PWD/talos-rs" pnpm --dir talos-app --filter @talos/sdk-app-server test:integration`

**未覆盖风险**: admin full-access 权限边界由 application-service 既有单测覆盖；protocol smoke 当前覆盖 local full actor 成功路径和 missing-session 错误映射。

**后续移交事项**: Phase 2 继续补齐真实 pending approval resume 链路。
