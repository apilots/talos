# 阶段 9：五项 Review 消融

状态：Complete。范围：用户授权将五项审查结果写入计划并推动实现；不提交 Git。
基线：阶段 8 已完成的 329 项 Rust 回归和真实多节点 E2E 8/8 是历史证据，不计本轮验收。

## 9.1 接口与重复数据

状态：Complete。

- 问题 2：删除没有生产调用方的 Coordinator.unregister_agent / Registry.unregister；删除其专用句柄比较测试，将原路由缺席断言改为生产使用的整节点撤下。
- 问题 3：Runner 只保留一份 AgentNodeLeaseUpdate；Worker 的 deployments 从实际 Harness 列表派生，不保存第二份 Agent ID 列表。
- 问题 4：删除 compact Context 的纯转发编解码包装，复用唯一 Context 投影。

成功标准：上述重复接口/字段消失；原路由、注册描述、续租 deadline 和 Harness 方法保持语义。
验证：`just test -p talos-coordinator -p agent-client -p agent-protocol -p talos-agent`。

## 9.2 完整 RPC 服务组合

状态：Complete。依赖：9.1。

- 问题 1：NodeRpcServer 构造时要求 publisher、Session data、subagent 和 tool image 服务；移除可选安装方法和无 publisher 的测试构造入口。
- 生产入口和现有 TLS/管理测试共用完整组合；保留用户归属、token 校验、注册发布、注销和管理撤销的行为断言。
- 只保留是否启用整个节点 listener 的既有配置，不新增运行模式或依赖框架。

成功标准：无法构造半成品节点 RPC server；管理与注册均执行真实发布/撤下；原资源 RPC 与错误契约不变。
验证：`just test -p talos-base -p talos-worker -p talos-server`。

## 9.3 Spec 与计划收敛

状态：Complete。依赖：9.1–9.2。

- 问题 5：Store 契约补 upsert_embedded，移除 embedded 经 create/update 管理的过时表述。
- 节点接口清单改为实际 register/replace/unregister_node；说明 Register 发布 Client，健康心跳决定可路由性。
- 主计划保留阶段入口和证据引用；删除重复过程流水账。子计划保留必要失败原因与最终验收，不把历史等待状态写成当前待办。

成功标准：Spec 对象、方法和注册顺序与代码一致；根计划可定位所有阶段和历史证据。
验证：本地链接、源码接口对应和 `git diff --check`。

## 9.4 消融回归与交付

状态：Complete。依赖：9.1–9.3。

- `just fmt`、scoped clippy、`pnpm --dir talos-app check:repo`。
- `just e2e-group 17_multi_node`：真实 Server + 三 Worker 验证注册/重启、归属/路由、实时流/续租、子 Agent、Worker 文件、附件/压缩/续接/内嵌、租约到期和 RPC 关闭边界。
- 复核本轮 diff 没有新增业务对象、PB wire 变更、数据库字段或测试专用生产分支。

成功标准：局部回归与真实八场景通过，原鉴权、执行清理及 Workspace 归属不退化。

## 最终验收（2026-10-09）

- 9.1：四包 `just test` 59/59，0 skipped；未使用注销接口/专用测试和 compact 包装已删除，租约与部署列表去重。
- 9.2：三包 `just test` 32/32，0 skipped；生产与管理/TLS 测试均要求完整 publisher/资源组合，发布/撤下行为断言通过。
- TLS 测试等待 Runner 已安装 auth 的通知再调用资源 RPC，避免以数据库写入误判 Client 更新；定向复核 1/1（过滤跳过其他 30 项）。
- 七包 scoped clippy、修改测试所属两包 clippy-tests、fmt-check、repo/error-contract 检查及 diff whitespace 检查通过。
- 85 个本地链接有效；Spec 对应实际 Store/Registry API 与注册发布顺序；根计划由 170 行收敛为 53 行的阶段入口/当前状态/证据引用，阶段 8 保留必要故障原因与最终验收。
- **完整 `just e2e-group 17_multi_node`：8/8，0 失败**，实际启动 Cloud Auth、Server 和三个 Worker。
  报告：`talos-e2e/reports/e2e_report_20261009_155343.md`（本地生成报告，不入库）。
- 原用户归属/鉴权、实时流/续租、固定 Session 路由/重启续接、子 Agent、Worker Memory/Dream、附件/压缩/内嵌、租约到期和 RPC/关闭边界均通过。
- 本轮没有新增业务对象、PB wire、数据库字段、执行账本或测试专用生产分支。未提交 Git。

五项问题已落地，各阶段实现与最终契约一致。无待办或阻塞；本会话未提供内置 plan/TODO 工具，状态维护在根计划和本文件。
