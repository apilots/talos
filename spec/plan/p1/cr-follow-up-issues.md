# CR Follow-up Issues

**状态**: 待 CR 合并后处理  
**记录时间**: 2026-04-30  
**范围**: Phase 1-7 已完成后的非阻塞后续事项。  

本文只记录不阻塞当前 CR 的后续 issue。新增能力实现时仍需另建 phase / subplan，并按 `AGENTS.md` 要求补齐测试与完成记录。

---

## Issue 1: Freeze Deferred App Protocol Methods

**优先级**: P1  
**Owner**: `gateway/app-protocol`, `gateway/app-server`, SDK  
**状态**: 未开始  

### 背景

`DEFERRED_CLIENT_METHODS` 当前仍保留以下方法：

- `tool.dynamic_result.submit`
- `admin.session.list`
- `admin.project.list`

这些方法暂不阻塞当前 CR。原因是 dynamic tool result 依赖 server request / client response 闭环，admin 方法依赖 local actor credentials 与权限边界稳定。

### 完成标准

- 协议 DTO 以 `talos-app-protocol` Rust 类型为真源。
- app-server dispatch 接入 application-service 或明确的共享服务层。
- Rust SDK / TS SDK 暴露薄封装 helper，不重复定义业务 DTO。
- protocol e2e resource matrix 覆盖成功路径、权限错误、not found / conflict 等错误映射。
- 刷新 schema 与 TS generated fixture。
- 从 `DEFERRED_CLIENT_METHODS` 移除对应 method。

### 建议拆分

- 先处理 `tool.dynamic_result.submit`。
- 再处理 `admin.session.list` 与 `admin.project.list`。

---

## Issue 2: Add Real Pending Approval Resume Regression

**优先级**: P0  
**Owner**: `orchestration/runtime`, `gateway/app-server`, CLI  
**状态**: 未开始  

### 背景

当前已覆盖 tool approval 的 transport 入口与错误映射，但仍需要真实回归覆盖：

```text
pending approval event
  -> submit approval
  -> resumed turn continues
  -> tool completes
  -> final message completes
```

这是执行状态机正确性的核心风险，应在 CR 合并后优先处理。

### 完成标准

- CLI first 集成 case 覆盖 pending approval 到 resumed turn 的完整链路。
- app-server protocol case 覆盖同一链路的 JSON-RPC event sequence。
- 校验 `session_id`、`turn_id`、`call_id`、event sequence 一致。
- 校验 approval rejected / approved 两类路径至少覆盖 approved 主链路。
- 新增回归接入 `tests/integration/run_e2e.sh`。

### 建议测试位置

- `tests/integration/scenarios/06_tools/cli_tool_approval_resume.sh`
- `tests/integration/scenarios/09_app_server/tool_approval_resume.sh`

---

## Issue 3: Add Desktop App-server Crash Recovery Product Smoke

**优先级**: P2  
**Owner**: `desktop`, `ts-sdk/app-server`  
**状态**: 未开始  

### 背景

Desktop controller 已有 restart + subscription replay 单测，但仍缺产品级 smoke，覆盖 Electron runtime 下 app-server 子进程异常退出后的重连体验。

该事项不阻塞当前 CR，因为 server / SDK / controller 的基础能力已被单测和协议回归覆盖。

### 完成标准

- 启动 desktop main/controller。
- 创建 session 并订阅事件。
- 模拟或终止 app-server child process。
- controller 自动重启 app-server。
- 使用 last seen event id replay subscription。
- renderer 继续收到后续事件。
- 测试记录 app-server lifecycle 状态变化：starting / ready / restarting / ready。

### 约束

- 不和 dynamic tool result protocol freeze 合并实现。
- Desktop main process 只负责 lifecycle 与 typed IPC，不承载业务逻辑。
- Renderer 不直接访问 Node / filesystem 能力。
