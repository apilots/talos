# 阶段 6：落实独立审查问题

状态：Complete。生产修复、必要消融与真实七场景验收完成。

## 6.1 执行可靠性

状态：Complete。

- 同一 token 续租只延长 deadline，不撤销在途 RPC。
- Server 关闭只释放自己拥有的内嵌执行运行时；保持独立 Worker 的生命周期，原 Harness shutdown 语义不变。
- 完整 drain progress 后再传 settlement；远程 wait 使用业务等待时间。
- 用真实 TLS 场景覆盖续租和事件完整性，并复跑多节点路由/长任务/协同场景。

成功标准：续租不取消长任务；控制面重启后原 Worker 和 Session 可执行；事件连续，子任务长等待不被提前截断。

## 6.2 Worker Workspace 归属

状态：Complete。依赖：6.1 的基础可靠性已验证。

- 原 Memory 反思和 Dream 维护保持业务策略、用户用量记录和 Session 互斥。
- 所有远程 Workspace 文件读写在绑定 Worker，使用现有 typed 业务对象及标准 RPC 投影。
- 内嵌与远程通过相同执行端口处理，不增加文件副本、执行数据库或平行 Registry。
- 补节点 Memory、维护及其他遗漏的实际用户链路验收。

成功标准：Worker 产生并读取 Memory/产物，控制面不产生远程 Workspace 文件；原业务回归通过。

## 6.3 消融与验收

状态：Complete。依赖：6.2。

- 删除只供测试使用的 current_registration 以及多余 token 明文副本；依据真实消费者决定注册能力字段。
- 保留事务顺序、节点亲和、鉴权和故障语义测试，删除被替代的测试观察接口。
- 执行 scoped `just test`、`just clippy`、fmt、仓库检查和真实多节点 E2E，修复本次变更导致的问题。
- 核对 Spec、代码和实际结果，完成后再更新总状态。

成功标准：目标场景通过，文档准确；失败和未验证项不写成完成。

## 实际进展（2026-09-21）

- 生产修复：续租原位更新；控制面 shutdown 仅关闭所属内嵌 Harness；progress drain 后结算；
  wait 使用业务等待时间加传输余量，其他资源 RPC 保留 10 秒 deadline。
- 补查到期边界：控制 RPC 正在等待时租约到期进入重新注册，不通过 `?` 意外退出 Worker。
- 文件归属：WorkerHarness 组合原执行与 Memory owner；反思、Dream 维护经绑定 Harness
  读写原文件，保持 InnerModel 用量、Session 互斥及 cursor 顺序。接口见 [Memory API](../contracts/memory-api.md)。
- 已消融：current_registration 测试专用入口、Service 明文 token/部署副本、无消费者的注册 capabilities。
- 验证：316 个 scoped Rust 测试通过；续租及连续事件增强断言后 43 个 Agent/Client 测试通过。
  scoped clippy、repo check、ShellCheck、Python 编译、diff 检查通过。
- 第一轮修复复验：注册、路由/重启、长任务、Memory/Dream 完整场景通过；协同业务断言通过，
  外层脚本因运行期间编辑引起读取偏移报错，整组不记为通过，须稳定脚本后复跑。
- 补充场景已单独通过：真实 PNG 输入、显式 compact 与上下文保留、问答等待后重启 Worker
  续接同一 Turn、控制面失联直至租约到期并重新注册。视觉场景使用可访问的
  deepseek-v4-flash-vision-exp；原不支持纯文本上传、文本模型不接受图片的校验保持不变。
- 最终七场景 group：**7 PASS / 0 FAIL**，全部使用真实 Server/Worker/Cloud 和公开 API。
  `talos-e2e/reports/e2e_report_20260921_172621.md`（本地生成报告，不入库）；测试进程均已清理。

## 最终验收

| 场景 | 结果 | 时间 |
| --- | --- | --- |
| 注册、用户隔离、续租、凭证轮换 | PASS | 81.96s |
| 路由、崩溃、Worker/Server 重启后原 Session | PASS | 97.80s |
| 实时增量、长任务跨续租和结算 | PASS | 63.55s |
| 真实子 Agent spawn/wait、结果及节点亲和 | PASS | 41.57s |
| Worker Memory/Dream、控制面用量与 cursor | PASS | 49.70s |
| 附件、显式压缩、重启续接、内嵌执行回归 | PASS | 47.25s |
| 控制面暂停至租约过期、Worker 保活并重新注册 | PASS | 111.93s |

验证命令：

```sh
just test -p agent-protocol -p talos-worker-bin -p talos-agent -p talos-coordinator -p talos-service -p talos-base -p talos-memory -p talos-agent-provider-kernel
# 316 passed; after the last RPC assertion/lease change:
just test -p talos-agent -p agent-client
# 43 passed (overlaps the previous run).
just clippy -p talos-worker -p talos-base -p agent-client -p agent-protocol -p talos-coordinator -p talos-agent -p talos-agent-provider-kernel -p talos-service -p talos-memory
just fmt-check
pnpm --dir talos-app check:repo
just e2e-group 17_multi_node
```

上述检查通过；ShellCheck、Python 编译、diff 与 Spec 本地链接检查通过。既有 ts-rs serde
属性解析警告仍存在，没有关闭 lint/test。没有新增生产测试开关，没有更改既有附件支持范围。
本轮只移除测试专用观察接口及无消费者注册元数据，保留原 admission、鉴权、亲和与保守清理语义。

设计核对：原七个执行 API 未变；新增文件端口只投影既有 Memory 类型。控制面没有新增执行
账本，只有原 metadata、Session/Turn/cursor/用量；远程 Workspace 文件始终在绑定 Worker。
未知执行结果的跨进程自动恢复、无 Session 的多节点文件管理、其他 Provider 的模型兼容性
不在本阶段验收范围。本阶段无剩余阻塞，未执行 Git 提交。
