# 阶段3：共享事实与任意控制面续跑

Status: Complete（基线实现）；本轮开关/公开SDK接入及一次联合验收以根PLAN为准。

| 子阶段 | 动作 | 成功标准 | 状态 |
| --- | --- | --- | --- |
| 3.1 capture/物化 | 原事务接Node/Project/Session/全部Timeline/File写入；adapter导入原Store，不触发执行/副作用 | 空Project、child、notice、失败、usage、reset/delete完整；本地未导出修改不被import覆盖 | Complete |
| 3.2 稳定事实/原投影 | 原SessionConf/必要环境与来源投影，None/显式覆盖保真；event-map/前驱/memory/幂等原requestId；无事件取消最小展示事实 | 同Worker自定义目录/权限续跑一致；不同event_seq正确，取消实时可见、重试完整原响应；无锁/源游标/重复计费 | Complete |
| 3.3 准入边界 | Worker最小历史游标恢复文件及原协调者恢复，原子期望历史游标/排他；缺Shared事实才catch_up；terminal/Shared确认 | 双CP不双执行；B最后Local后重启不能被A旧DB覆盖，pure Worker同样安全；已确认本机不等待H | Complete（2026-10-13补验） |
| 3.4 当前Turn交互 | 原结果/资源通道接共享事实；Input/Interrupt/queue仅到active协调者；FileStore远端位置与源FileService读取/更新/delete接线 | 双NAT在线源旧附件可读，源侧删除同步且import不删B文件；结果先落B不等H ACK；同Computer/Worker不迁移 | Complete（2026-10-13补验） |
| 3.5 自审/验证 | 两真实CP/DB+独立hub+同Worker RPC，覆盖crash/并发/换端/clear/delete | 完整闭环通过，再进入UI；不能用历史列表可见代替真实续跑 | Complete（2026-10-13补验） |

SessionExecutionBinding/SessionTreeIdentity/原完成态enum使用唯一domain定义，Store仍私有records。
物化与document/event-map/cursor同事务；导入active状态仅展示，不安装fake execution或触发恢复。
阶段2接入自审的实施门槛：先在同一原读事务为旧资源capture完整baseline，再开启增量hook；大历史按固定cut分批处理并验证完整依赖，不能以metadata默认值补齐。Session catalog的UUID顺序不保证父先于child，原adapter导入必须按SessionTreeIdentity补父/root依赖；apply内不能等待同一runner的catch_up，避免自我死锁。相应真实child/大历史场景纳入3.1与3.5。
来源NodeMode::Embedded及environment.connectionMode只描述来源环境；物化端根据当前Host的ComputerAlias与既有Gateway路由重建本机/远端状态，不把A内嵌Worker注册成B的本机Worker，不安装来源lease或listener。3.1/3.5验证A本机在B显示并执行为远端、B本机仍走自身loopback；不增加协议字段。
Worker重启先核对持久最小标记及最后协调者的原Store/事实；A旧DB或旧Shared 历史游标不能覆盖B未Shared标记。已确认本机恢复不等待G/H；未知明确阻塞该Session，不接管运行态。Session不可跨Worker执行。

验证（实施时）：`just test -p talos-sync -p talos-store -p talos-service -p talos-coordinator -p agent-client`，新增必要PB/DTO的原生成器与scoped检查。