# 阶段 8：执行归属与可靠性收敛

状态：Complete。用户已授权修复全局 Review 问题；不执行 Git 提交。

## 8.1 目录归属与结算恢复

状态：Complete。

- 默认 Workspace 使用 Worker 的 Config/TalosPaths/root_prefix/user_id；删除控制面隐式默认工作目录转换，保留显式项目/目录。
- 原 settlement 的停止证据用于终态提交失败恢复；网络未知仍保守，不新增账本或完成记录表。
- 补既有目录与恢复测试，确认内嵌/远程一致。

成功标准：默认文件不落入控制面旧 Workspace；可靠停止后的数据库失败不留下活动 Turn。验证：`just test -p talos-service -p talos-agent`。

## 8.2 RPC、取消与错误边界

状态：Complete。依赖：8.1。

- 给 unary/admission/accepted 设置阶段等待预算，保持长流持续反馈。
- 取消背压流时释放消费者并处理真实 Harness 清理结果；关闭 Worker 先撤本地租约再注销。
- 复用 talos-error 严格 wire 校验和标准 gRPC details，CLI 使用公开错误；删除无消费者 diagnostic_id。
- 合并心跳有效判定。

成功标准：不响应 RPC 有界返回，真实 Harness 清理不被满队列阻塞，错误结构跨传输一致。验证：相关 `just test`/`just clippy`/fmt/repo check。

## 8.3 Worker Dream 维护与消融

状态：Complete。依赖：8.2。

- 形成日期、Provider 配置、文件读写使用绑定 Worker；控制面继续调度与业务持久化/用量。
- 复用原形成算法和 typed 对象，移除控制面按 embedded Provider 查找远程 Agent 的耦合；只增加真实消费者所需端口。
- 合并重复公开包装和旧 E2E 脚手架，保留实际业务断言。

成功标准：异构时区/远端独有 Agent 可以形成 Dream Memory，cursor 和用量正确；内嵌同样可用。

## 8.4 真实验收与交付

状态：Complete。依赖：8.1–8.3。

- 扩展 `17_multi_node`：旧控制面 Workspace、显式目录、异构时区、运行中 shutdown 与不响应调用。
- Docker 构建补 protoc，验证可用工具/环境；无法完成的检查明确记录。
- 运行 scoped Rust 回归、clippy、fmt、repo check、实际 E2E，核对 Spec 与代码。

成功标准：新增故障场景及既有场景通过；没有新状态表/平行业务模型；消融不丢失覆盖。

## 验收中发现的故障与修正

- 暂停 Worker 的两个请求受原全局关闭锁串行化影响，第二个请求未进入 RPC；改为分别验证 unary 和 admission，不放宽错误预期。
- 完整 group 暴露 tonic 本地 TimeoutExpired 被映射成 Cancelled；按 typed source 还原 Timeout，远端主动 Cancelled 保留原义。失败报告：`talos-e2e/reports/e2e_report_20261009_144756.md`。
- 控制面暂停时 HTTP/2 graceful drain 阻塞 Worker 退出；改为先关闭本地 Harness/Provider/MCP，再有界排空传输。保留 25 秒退出预算与真实 Bash 延迟副作用断言。失败报告：`talos-e2e/reports/e2e_report_20261009_150231.md`。

## 最终验收（2026-10-09）

- 九包 `just test`（`--test-threads=4`）：329/329，0 skipped；覆盖已有本地行为和本轮回归。
- 九包 scoped `just clippy`、`just fmt-check`、`git diff --check`、`pnpm --dir talos-app check:repo` 均通过。
- `just gen-app-protocol` 通过，生成文件无变化；Spec/根计划共 82 个本地链接通过，ShellCheck/Python 语法通过。
- **完整 `just e2e-group 17_multi_node`：8/8，0 失败**。
  报告：`talos-e2e/reports/e2e_report_20261009_151309.md`（本地生成报告，不入库）。
  实际启动 Cloud Auth、Server 和三个 Worker，覆盖 UUID/注册/重启/认证、归属/路由、实时流/续租、
  子 Agent、旧控制面目录、异构时区 Memory/Dream、附件/压缩/问答重启续接/内嵌、租约到期、
  暂停 Worker 的 unary/admission Timeout、暂停控制面时 Worker 真实 Bash 取消与有界退出。
- 最终代码 Linux Docker `--target rust-builder` 构建通过；期间一次 Registry TLS 握手超时，重试成功。
  容器内 protoc 3.21.12、Server/Worker 0.1.9-SNAPSHOT `--version` 和 sandbox seccomp probe 均通过。
  这是 Rust builder 验证，不宣称运行了完整 Web runtime 镜像。
- 消融没有增加执行表/调用账本或业务模型；保留未知执行清理的原边界，未增加自动迁移/跨进程恢复。
- 根计划与 Spec 已同步；所有阶段 Complete，未提交 Git。保留历史失败报告，不以静态检查替代真实验收。
