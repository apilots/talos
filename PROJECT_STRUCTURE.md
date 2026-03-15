# Talos-RS 工程目录划分

## 整体结构

```
talos/
├── README.md                       # 项目总览（多语言说明）
├── PROJECT_STRUCTURE.md            # 本文档
├── specs/                          # 语言无关的接口规范
│   ├── agent.md                    # Agent 接口定义
│   ├── channel.md                  # Channel 接口定义
│   ├── tool.md                     # Tool 接口定义
│   ├── memory.md                   # 记忆系统接口定义
│   └── gateway-protocol.md         # Gateway 通信协议
│
└── talos-rs/                       # Rust 实现
    ├── README.md                   # Rust 实现说明
    ├── Cargo.toml                  # Workspace 根配置
    ├── Cargo.lock
    ├── .gitignore
    ├── architecture.jpg            # 架构图
    │
    ├── crates/                     # 所有子 crate
    │   │
    │   ├── infra/                  # 基础设施层
    │   │   ├── runtime/            # 异步运行时封装
    │   │   │   ├── Cargo.toml
    │   │   │   └── src/
    │   │   │       └── lib.rs      # Tokio 运行时封装
    │   │   │
    │   │   ├── config/             # 配置管理
    │   │   │   ├── Cargo.toml
    │   │   │   └── src/
    │   │   │       ├── lib.rs      # 配置加载/热更新
    │   │   │       ├── loader.rs   # 配置文件加载器
    │   │   │       └── types.rs    # 配置类型定义
    │   │   │
    │   │   ├── error/              # 统一错误处理
    │   │   │   ├── Cargo.toml
    │   │   │   └── src/
    │   │   │       ├── lib.rs      # TalosError 定义
    │   │   │       └── result.rs   # TalosResult 别名
    │   │   │
    │   │   ├── logging/            # 日志/追踪
    │   │   │   ├── Cargo.toml
    │   │   │   └── src/
    │   │   │       ├── lib.rs      # tracing 集成
    │   │   │       └── layer.rs    # 自定义 tracing layer
    │   │   │
    │   │   ├── storage/            # 存储抽象
    │   │   │   ├── Cargo.toml
    │   │   │   └── src/
    │   │   │       ├── lib.rs      # 存储 trait 定义
    │   │   │       ├── kv.rs       # KV 存储接口
    │   │   │       ├── sql.rs      # SQL 存储接口
    │   │   │       └── vector.rs   # 向量存储接口
    │   │   │
    │   │   └── auth/               # 认证/配对
    │   │       ├── Cargo.toml
    │   │       └── src/
    │   │           ├── lib.rs      # 认证 trait
    │   │           ├── device.rs   # 设备配对
    │   │           └── token.rs    # Token 管理
    │   │
    │   ├── perception/             # 感知层
    │   │   ├── input/              # 多模态输入
    │   │   │   ├── Cargo.toml
    │   │   │   └── src/
    │   │   │       ├── lib.rs
    │   │   │       ├── types.rs    # Input 类型定义
    │   │   │       └── parser.rs   # 输入解析
    │   │   │
    │   │   ├── channel/            # Channel trait
    │   │   │   ├── Cargo.toml
    │   │   │   └── src/
    │   │   │       ├── lib.rs      # Channel trait 定义
    │   │   │       ├── message.rs  # 消息类型
    │   │   │       └── adapter.rs  # 渠道适配器基类
    │   │   │
    │   │   ├── listener/           # 事件监听
    │   │   │   ├── Cargo.toml
    │   │   │   └── src/
    │   │   │       ├── lib.rs
    │   │   │       ├── calendar.rs # 日历事件
    │   │   │       ├── email.rs    # 邮件监听
    │   │   │       └── webhook.rs  # Webhook 处理
    │   │   │
    │   │   └── transport/          # 传输适配
    │   │       ├── Cargo.toml
    │   │       └── src/
    │   │           ├── lib.rs
    │   │           ├── http.rs     # HTTP 适配
    │   │           ├── ws.rs       # WebSocket 适配
    │   │           └── cli.rs      # CLI 适配
    │   │
    │   ├── cognition/              # 认知层
    │   │   ├── memory/             # 记忆系统
    │   │   │   ├── Cargo.toml
    │   │   │   └── src/
    │   │   │       ├── lib.rs
    │   │   │       ├── short_term.rs   # 短期记忆
    │   │   │       ├── long_term.rs    # 长期记忆
    │   │   │       └── procedural.rs   # 程序性记忆
    │   │   │
    │   │   ├── memory-store/       # 记忆存储
    │   │   │   ├── Cargo.toml
    │   │   │   └── src/
    │   │   │       ├── lib.rs
    │   │   │       ├── store.rs    # 持久化存储
    │   │   │       └── cache.rs    # 缓存层
    │   │   │
    │   │   ├── knowledge/          # 知识图谱/RAG
    │   │   │   ├── Cargo.toml
    │   │   │   └── src/
    │   │   │       ├── lib.rs
    │   │   │       ├── graph.rs    # 知识图谱
    │   │   │       ├── rag.rs      # RAG 检索
    │   │   │       └── embedding.rs# 嵌入生成
    │   │   │
    │   │   ├── reasoning/          # 推理引擎
    │   │   │   ├── Cargo.toml
    │   │   │   └── src/
    │   │   │       ├── lib.rs
    │   │   │       ├── planner.rs  # 任务规划
    │   │   │       └── inference.rs# 因果推理
    │   │   │
    │   │   └── decision/           # 决策系统
    │   │       ├── Cargo.toml
    │   │       └── src/
    │   │           ├── lib.rs
    │   │           ├── priority.rs # 优先级判断
    │   │           └── risk.rs     # 风险评估
    │   │
    │   ├── action/                 # 执行层
    │   │   ├── tools/              # 工具系统
    │   │   │   ├── Cargo.toml
    │   │   │   └── src/
    │   │   │       ├── lib.rs      # Tool trait 定义
    │   │   │       ├── registry.rs # 工具注册表
    │   │   │       ├── builtin/    # 内置工具
    │   │   │       │   ├── mod.rs
    │   │   │       │   ├── read.rs
    │   │   │       │   ├── exec.rs
    │   │   │       │   ├── edit.rs
    │   │   │       │   └── write.rs
    │   │   │       └── external/   # 外部工具
    │   │   │           └── mod.rs
    │   │   │
    │   │   ├── tools-policy/       # 工具策略
    │   │   │   ├── Cargo.toml
    │   │   │   └── src/
    │   │   │       ├── lib.rs
    │   │   │       ├── allowlist.rs
    │   │   │       ├── denylist.rs
    │   │   │       └── elevated.rs
    │   │   │
    │   │   ├── tools-sandbox/      # 沙箱执行
    │   │   │   ├── Cargo.toml
    │   │   │   └── src/
    │   │   │       ├── lib.rs
    │   │   │       ├── docker.rs   # Docker 沙箱
    │   │   │       └── local.rs    # 本地沙箱
    │   │   │
    │   │   ├── output/             # 输出通道
    │   │   │   ├── Cargo.toml
    │   │   │   └── src/
    │   │   │       ├── lib.rs
    │   │   │       ├── text.rs
    │   │   │       ├── voice.rs
    │   │   │       └── device.rs
    │   │   │
    │   │   └── skill/              # 技能系统
    │   │       ├── Cargo.toml
    │   │       └── src/
    │   │           ├── lib.rs
    │   │           ├── loader.rs   # 技能加载
    │   │           ├── registry.rs # 技能注册
    │   │           └── skill.rs    # Skill trait
    │   │
    │   ├── orchestration/          # 编排层
    │   │   ├── agent/              # Agent 核心
    │   │   │   ├── Cargo.toml
    │   │   │   └── src/
    │   │   │       ├── lib.rs
    │   │   │       ├── agent.rs    # Agent trait
    │   │   │       ├── lifecycle.rs# 生命周期管理
    │   │   │       └── context.rs  # Agent 上下文
    │   │   │
    │   │   ├── agent-workspace/    # 工作空间管理
    │   │   │   ├── Cargo.toml
    │   │   │   └── src/
    │   │   │       ├── lib.rs
    │   │   │       ├── loader.rs   # 文件注入
    │   │   │       └── files.rs    # AGENTS.md/SOUL.md 等
    │   │   │
    │   │   ├── session/            # 会话管理
    │   │   │   ├── Cargo.toml
    │   │   │   └── src/
    │   │   │       ├── lib.rs
    │   │   │       ├── key.rs      # SessionKey 生成
    │   │   │       ├── context.rs  # 上下文管理
    │   │   │       └── scope.rs    # dmScope 策略
    │   │   │
    │   │   ├── session-store/      # 会话存储
    │   │   │   ├── Cargo.toml
    │   │   │   └── src/
    │   │   │       ├── lib.rs
    │   │   │       ├── store.rs    # sessions.json 管理
    │   │   │       ├── transcript.rs# JSONL 转录
    │   │   │       └── maintenance.rs# 维护任务
    │   │   │
    │   │   ├── routing/            # 路由规则
    │   │   │   ├── Cargo.toml
    │   │   │   └── src/
    │   │   │       ├── lib.rs
    │   │   │       ├── binding.rs  # Binding 定义
    │   │   │       ├── router.rs   # 路由匹配
    │   │   │       └── policy.rs   # 路由策略
    │   │   │
    │   │   ├── scheduler/          # 任务调度
    │   │   │   ├── Cargo.toml
    │   │   │   └── src/
    │   │   │       ├── lib.rs
    │   │   │       ├── cron.rs     # Cron 调度
    │   │   │       └── heartbeat.rs# Heartbeat
    │   │   │
    │   │   ├── planner/            # 任务规划
    │   │   │   ├── Cargo.toml
    │   │   │   └── src/
    │   │   │       ├── lib.rs
    │   │   │       ├── workflow.rs # 工作流引擎
    │   │   │       └── decompose.rs# 任务分解
    │   │   │
    │   │   ├── coordinator/        # 多 Agent 协调
    │   │   │   ├── Cargo.toml
    │   │   │   └── src/
    │   │   │       ├── lib.rs
    │   │   │       ├── distribute.rs# 任务分配
    │   │   │       └── aggregate.rs# 结果聚合
    │   │   │
    │   │   └── queue/              # 消息队列
    │   │       ├── Cargo.toml
    │   │       └── src/
    │   │           ├── lib.rs
    │   │           ├── mode.rs     # steer/followup/collect
    │   │           └── buffer.rs   # 消息缓冲
    │   │
    │   ├── gateway/                # 网关层
    │   │   ├── core/               # Gateway 核心
    │   │   │   ├── Cargo.toml
    │   │   │   └── src/
    │   │   │       ├── lib.rs
    │   │   │       ├── gateway.rs  # Gateway trait
    │   │   │       └── builder.rs  # Gateway 构建器
    │   │   │
    │   │   ├── ws/                 # WebSocket 服务器
    │   │   │   ├── Cargo.toml
    │   │   │   └── src/
    │   │   │       ├── lib.rs
    │   │   │       ├── server.rs   # WS 服务器
    │   │   │       ├── protocol.rs # 协议解析
    │   │   │       └── frame.rs    # 帧处理
    │   │   │
    │   │   ├── events/             # 事件系统
    │   │   │   ├── Cargo.toml
    │   │   │   └── src/
    │   │   │       ├── lib.rs
    │   │   │       ├── bus.rs      # 事件总线
    │   │   │       ├── types.rs    # 事件类型
    │   │   │       └── pubsub.rs   # 发布订阅
    │   │   │
    │   │   └── health/             # 健康检查
    │   │       ├── Cargo.toml
    │   │       └── src/
    │   │           ├── lib.rs
    │   │           ├── metrics.rs  # 指标收集
    │   │           └── status.rs   # 状态报告
    │   │
    │   └── meta/                   # 元认知层
    │       ├── monitor/            # 系统监控
    │       │   ├── Cargo.toml
    │       │   └── src/
    │       │       ├── lib.rs
    │       │       ├── performance.rs# 性能监控
    │       │       └── health.rs   # 健康检查
    │       │
    │       ├── learner/            # 持续学习
    │       │   ├── Cargo.toml
    │       │   └── src/
    │       │       ├── lib.rs
    │       │       ├── feedback.rs # 反馈收集
    │       │       └── optimize.rs # 策略优化
    │       │
    │       ├── personality/        # 人格管理
    │       │   ├── Cargo.toml
    │       │   └── src/
    │       │       ├── lib.rs
    │       │       ├── identity.rs # IDENTITY.md
    │       │       └── style.rs    # 响应风格
    │       │
    │       └── maintenance/        # 维护任务
    │           ├── Cargo.toml
    │           └── src/
    │               ├── lib.rs
    │               ├── prune.rs    # 剪枝策略
    │               ├── archive.rs  # 归档处理
    │               └── rotate.rs   # 轮转处理
    │
    └── talos/                      # 主 crate（整合所有模块）
        ├── Cargo.toml              # 依赖所有子 crate
        └── src/
            ├── lib.rs              # 统一 API 出口
            ├── prelude.rs          # 常用类型重导出
            └── integration/        # 集成测试
                └── mod.rs
```

---

## 开发顺序（Phased）

### Phase 1 - 基础设施（Week 1-2）
```bash
# 优先开发
crates/infra/error/
crates/infra/config/
crates/infra/logging/
crates/infra/runtime/
```

### Phase 2 - Agent 核心（Week 3-4）
```bash
crates/orchestration/agent/
crates/orchestration/agent-workspace/
crates/action/tools/
crates/action/skill/
```

### Phase 3 - 会话与记忆（Week 5-6）
```bash
crates/cognition/memory/
crates/cognition/memory-store/
crates/orchestration/session/
crates/orchestration/session-store/
```

### Phase 4 - 渠道与网关（Week 7-9）
```bash
crates/perception/channel/
crates/perception/transport/
crates/gateway/core/
crates/gateway/ws/
crates/gateway/events/
```

### Phase 5 - 编排与路由（Week 10-11）
```bash
crates/orchestration/routing/
crates/orchestration/queue/
crates/orchestration/scheduler/
crates/orchestration/coordinator/
```

### Phase 6 - 元认知（Week 12-13）
```bash
crates/meta/monitor/
crates/meta/maintenance/
crates/meta/personality/
crates/meta/learner/
```

### Phase 7 - 整合与优化（Week 14-16）
```bash
crates/talos/  # 主 crate 整合
# 性能优化、文档、示例
```

