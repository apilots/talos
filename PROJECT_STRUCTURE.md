# Talos 工程目录结构

**最后更新**: 2026-06-14
**说明**: 本文档描述当前仓库的真实目录结构；目标架构请看 `talos-rs/ARCHITECTURE.md`，crate 清单请看 `talos-rs/MODULES.md`。

## 顶层结构

```text
talos/
├── AGENTS.md
├── IMPLEMENTATION_PLAN.md
├── PROJECT_STRUCTURE.md
├── README.md
├── docs/
├── spec/
├── talos-app/
├── talos-e2e/
└── talos-rs/
```

## Rust Workspace 结构

```text
talos-rs/
├── ARCHITECTURE.md
├── Cargo.lock
├── Cargo.toml
├── README.md
├── MODULES.md
├── apps/
│   ├── cli/
│   ├── sdk/
│   └── ui-tui/
├── bin/
│   └── talos/
├── conf/
├── crates/
│   ├── action/
│   │   ├── tools/
│   │   ├── tools-policy/
│   │   └── tools-sandbox/
│   ├── app/
│   │   └── talos-server/
│   ├── cognition/
│   │   └── memory/
│   ├── common/
│   │   ├── llm/
│   │   │   ├── anthropic/
│   │   │   ├── bailian/
│   │   │   ├── openai/
│   │   │   └── provider/
│   │   ├── prompt/
│   │   └── types/
│   ├── gateway/
│   │   ├── core/
│   │   └── http/
│   ├── infra/
│   │   ├── auth/
│   │   ├── config/
│   │   ├── error/
│   │   ├── logging/
│   │   └── storage/
│   ├── orchestration/
│   │   ├── agent/
│   │   ├── agent-kernel/
│   │   ├── coordinator/
│   │   ├── router/
│   │   └── session-store/
│   └── perception/
│       └── channels/
└── tests/
    └── integration_test.rs
```

## E2E 结构

```text
talos-e2e/
├── README.md
├── protocol_smoke.sh
├── run_e2e.sh
├── scenarios/
├── scripts/
├── reports/
└── .tmp/
```

## 文档职责划分

```text
AGENTS.md                -> Agent 协作规则与工程约束
PROJECT_STRUCTURE.md     -> 当前目录树与文档边界
talos-rs/ARCHITECTURE.md -> 目标架构与设计意图
talos-rs/MODULES.md      -> 当前 workspace crate 清单
talos-rs/README.md       -> 对外能力概览与运行方式
talos-e2e/README.md      -> 端到端测试入口、场景分组与产物路径
spec/                    -> 接口与方案规格
```

## 本次清理后保留的结构原则

- 只保留已经接入 workspace 且存在真实职责的 crate。
- 设计草案与实时清单分离，避免一个文档同时承担两种角色。
- 目录图只描述当前存在的路径，不再保留历史占位模块。
