# 工程结构

[English](PROJECT_STRUCTURE.md)

> Historical translation. The [English document](PROJECT_STRUCTURE.md) owns the current
> 2026-09-07 contracts, diagrams, deployment rules and verification guidance.

本文描述当前仓库的顶层布局。详细 crate 清单参见
[Rust workspace 模块](RUST_MODULES_zh.md)。

```text
talos/
|-- .githooks/             Repository-managed Git hooks
|-- build/                 Packaging, release, branding, and product resources
|-- conf/                  Shared configuration and database migrations
|-- deploy/                Server deployment and TUI installation assets
|-- docs/
|   |-- architecture/      Current cross-component architecture
|   |-- development/       Development workflow
|   `-- reference/         Current repository and workspace facts
|-- scripts/               Repository automation and quality checks
|-- spec/                  Active and archived change specifications
|-- talos-app/
|   |-- desktop/           Electron Desktop shell
|   |-- ts-sdk/            Core, browser, and app-server TypeScript SDKs
|   `-- web/               React Web UI
|-- talos-e2e/
|   |-- docker/            E2E container resources
|   |-- scenarios/         Executable scenarios grouped by product flow
|   |-- scripts/           Shared E2E helpers
|   `-- tests/             Runner and helper tests
|-- talos-rs/
|   |-- apps/              Rust SDKs, TUI, and protocol test client
|   |-- bin/               Product and protocol-adapter binaries
|   `-- crates/            Layered Rust libraries
|-- AGENTS.md              Repository engineering rules
|-- CLAUDE.md              Claude-specific repository guidance
|-- README.md              English product and repository entry point
|-- README_zh.md           Chinese product and repository entry point
`-- justfile               Canonical local development commands
```

图中技术路径和职责使用英文，确保可以直接对应源码。

## 事实来源

| 事实 | 来源 |
| --- | --- |
| Rust workspace member | `talos-rs/Cargo.toml` |
| JavaScript workspace member | `talos-app/pnpm-workspace.yaml` |
| 常用开发命令 | 根目录 `justfile` |
| 活跃和归档的变更设计 | `spec/REGISTRY.md` |
| E2E 场景发现 | `talos-e2e/run_e2e.sh` 和 `talos-e2e/scenarios/` |
| 部署拓扑 | `deploy/` 资源和 README |

`target/`、`dist/`、report、log 和 test result 等生成输出不属于维护的源码结构。
