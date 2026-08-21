# Talos App Protocol

[English](README.md)

`talos-app-protocol` 定义 Talos app-server 协议的 JSON-RPC method registry、
notification registry、protocol schema 生成和 TypeScript protocol binding。

## 生成产物

Rust 协议类型是事实来源。不能手工修改生成的 schema 或 TypeScript 文件。

App protocol Request、Response、Notification 或共享 wire type 变化后，从仓库根目录运行：

```bash
just gen-app-protocol
```

Generator 刷新：

- `talos-rs/crates/gateway/app-protocol/schemas/app-protocol.schema.json`
- `talos-app/ts-sdk/packages/core/src/generated/app-protocol.ts`

生成的 TypeScript 文件由 `@talos/sdk-core` 消费，并通过面向 Browser 和 Desktop 的
SDK package 重新导出。产品代码应从 SDK package 导入协议 Request/Response 类型，
不能复制 app protocol DTO。

## Review 清单

运行 generator 后：

- 提交前检查两个生成 diff。
- 确认 Rust serde rename 与生成的 TypeScript 字段名一致。
- 运行 app protocol schema 测试。
- 生成 TypeScript 变化时运行 TypeScript SDK typecheck。

```bash
just test -p talos-app-protocol schema
pnpm --dir talos-app --filter @talos/sdk-core typecheck
```
