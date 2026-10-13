# Talos 管理节点同步持久化与流程

Status: Implemented；新开关专项已通过，统一8/8验收通过，见[本轮证据](verification.md)。Hub持久化只在[独立Sync存储](https://github.com/apilots/talos-sync/blob/7230f9fdf294e7ae218bdb6d3cf1c4ef60203380/spec/feat-044-talos-sync/database.md)维护。纯Worker不建管理DB。

## 1. 管理节点本地表

由现有 migration runner 在原业务 DB 中安装；纯 Worker 不创建这些表。
源业务写入、capture、导入和本地游标共用原 Storage 事务，库私有 records 不暴露产品 API。

| 表 | 关键字段 / 约束 | 必要性 |
| --- | --- | --- |
| `sync_identity` | singleton PK；peer_id UUID；loro_writer_id | DB重启保持管理身份；克隆为独立节点必须新writer，不充当Session owner。 |
| `sync_bindings` | PK hub/account/source_user；typed ComputerAlias[]；download_cursor | 已验证账号与本地用户/节点别名关联、下载水位；不另存登录Token。 |
| `sync_account_preferences` | account_id PK；enabled 0/1 | 当前管理宿主的已验证账号选择，独立于hub/binding；未保存默认沿登录启用；不作为同步事实上传。 |
| `sync_documents` | PK hub/account/kind/resource_id；computer_id?；generation；deleted；snapshot_blob；VV；ack_VV | 唯一Loro持久文档与本地/共享版本；查询业务目录仍复用原Store，不建第二套列表索引。 |
| `sync_outbox` | seq PK；binding/resource key；typed SyncChange[]；exported_version?；created_at | 原事务实际变更，进程崩溃也可导出；不记录RPC调用/回应。 |
| `sync_event_map` | PK hub/account/session/generation/stable_event_key；local_event_id；local_event_seq；UNIQUE local_event_id | 去重导入、源notice识别、History分页与memory/幂等水位转换。因DB整数不能跨节点复制，这张映射有真实用途。 |

event map 的 key 为版本化闭合 SyncEventKey 编码，不是临时字符串约定。
导入事实包括原SessionConf/必要environment与来源字段、原Project/File metadata、Timeline、无事件的最小取消展示事实及必要幂等完成态（独立idempotencyKey/requestId）/进度水位；活动锁/claim/任务只由本端实际执行流程创建。Session永久绑定原Computer/Worker，换控制面不迁移Worker或目录。
导入不再次产生 outbox，也不产生 accounting/反思副作用。源未同步的 outbox 和远端 import 按同一资源串行、同事务Loro因果cut处理，避免import覆盖尚未导出的本地修改。
clear 的 generation 在实际排他提交时递增，不能依据尚未导出的快照猜；tombstone 不外键级联到已删业务行。

## 3. 首次登录、绑定与全量同步

1. 管理runtime构建同一个Worker并启Worker RPC；本机通过agent-client注册Harness，不登录/不运行sync也能独立工作；云注册和同步在管理API ready之后独立启停。
2. 用户真实登录；可信宿主认领sourceUser，保存已注册Computer UUID/embedded别名。共享时同一Worker主动注册agent入口，不再构建一个Worker。
3. 账号内新数据正常启用capture/同步；旧匿名数据先明确认领，再用原Store keyset扫描Computer、所有Project（含空项目）、Session/子树、ready File及完整历史。固定导出cut，期间变更留outbox。
4. 管理节点连接独立sync服务，account/peer鉴权；catalog返回资源键、tombstone和feed cut；room按VV补齐。
5. 文档、原Store物化事实、event map和cursor同事务commit，然后发布既有业务通知。未到的依赖不能被空值代替。
6. 持续按committed batch导出，不等Turn结束；实时/离线重连都使用同一持久路径。

sourceUser -> account授权由可信宿主和现有Auth确认，sourceUser不从远端JSON决定。
多账号云DB各自scope隔离；切账号关闭旧连接/取消在途请求，旧数据不改归属。未启sync的DEV保留无登录路径。
FileScope::Project在账号内解析同一Project；File正文所在peer单独保留位置，不能推导Session永久控制权。

## 4. B协调A的Computer

完整双NAT、资源回调和结果反向通道见[rpc-routing时序](rpc-routing.md#2-ab都在nat后的完整链)。
B先确认自己的Store包含Worker所需历史游标的完整事实；缺失才catch_up。已有本地/缓存事实满足时不要求每次Start连接H。
B经agent传输调用Worker，自己提交admission/Timeline与capture，立即发布本端UI；Loro上传独立于执行RPC。
H持久确认后才能将这次新历史游标标记为Shared供另一协调者使用；本次结果交付和同一协调者继续不等待H。

A不需要为B的空闲Turn执行业务准入或写DB；A只需承载它的Worker RPC（在同一进程时自然需要该进程运行）。
云Gateway与自身Coordinator分离：即使Worker向云注册，B的结果也先回B，不能先写云业务DB再假装B协调。
本机情况B=A仍经loopback RPC完成完全同一条链，不以直接Harness替代自测。
云注册/Connect承载网络路由，Worker不可达就明确失败；不能把A Computer路径解释成B的本机路径。

上次Turn的Shared 历史游标是跨协调者边界；单一Worker的reservation决定执行排他，不由sync服务决定。
本地离线Turn可以连续由同一控制面执行，但必须先flush并补齐后，别的控制面才能续跑。
不把“本地active表为空”或“Worker重启active map为空”当可靠idle；未确认终态/清理的Turn沿原恢复路径处理。
Worker读取本机最小持久历史游标标记；若最新Local事实只在B，A旧DB不能覆盖标记。B按原状态RPC恢复确认；B不可达只阻塞该Session，不影响其他已确认本机Session。规则见[Worker恢复](worker-coordination.md#3-排他完整历史与切换)。

## 5. 原子提交、排序和恢复

1. 业务写入与typed capture同事务；先后快照diff只改实际变化字段，不能把旧完整snapshot当远端整行覆盖。
2. exporter按outbox次序形成Loro操作；源文档与导出水位同事务commit。暂存LoroDoc，不先改变活跃VV。
3. hub/client接收后暂存import，验证scope/immutable binding、事件一致性、前驱/generation；依赖缺失先补齐，不能发布不完整terminal快照。
4. hub的document/update/feed、client的document/业务事实/event map/cursor分别同事务持久化。
5. commit成功后替换可见文档/通知；hub提交后发上传成功Ack，client提交后只mark_committed本地恢复水位，不发送下载成功Ack。失败不推进VV/cursor，不开放跨端handoff。
6. ACK丢失按VV对账；source commit后crash从outbox恢复；import不得重新触发模型/工具/副作用。

只有H已提交cut确认为terminal Delete，导入事务才同时淘汰该资源被删除终态覆盖的离线pending patch；这是终态覆盖，不伪造上传ACK，不能因本端候选文档含未上传Delete就清空outbox。本地未ACK Delete遇H并发patch仍保留pending并上传Delete，不阻塞其他资源。flush固定调用时原事务的seq与VV，只有H durable cut包含该VV才成功；被远端Delete覆盖则返回Conflict，不能把outbox为空当ACK成功。
Loro VV的官方编码不保证map迭代顺序；持久snapshot校验、checkpoint相等/去重使用官方decode后的版本语义及准确generation，不比较二进制bytes或另造canonical排序编码。真正不一致或损坏仍明确失败。

Agent event稳定键去重；Notice带source peer/event ID。Turn按previousTurnId链、Turn内按sequence，迟到notice插入所属Turn。
本地event_seq仅为查询分页分配；接收旧Turn迟到notice不打乱现有游标，generation/cut变化要求既有recover重读。
memory watermark转换为“业务顺序覆盖的稳定事件前缀”，不能对不同DBseq做max；乱序未补齐时不推进。
clear/delete阻止旧代数据复活；删除parent tree捕获每个实际child。Project删除沿用现有绑定检查，不增隐式级联。

## 6. 场景边界

| 情况 | 行为 |
| --- | --- |
| H断线/停止 | 当前协调者继续执行和本地commit；已绑定变更留outbox，另一端读最后状态。缺Shared事实时不能换协调者。 |
| G断线/停止 | NAT远端RPC不可达；本机loopback仍可执行，H只提供历史同步，不代传命令。 |
| 创建节点A的Coordinator未提供该Session业务服务 | B仍能操作空闲Session，只要同一Worker RPC可达、历史完整；没有固定创建节点路由。 |
| 当前Turn协调者失联 | 不接管运行态；原连接取消/清理与事实恢复后开放新Turn。waiting claim不导入另一执行器。 |
| Worker不可达 / cleanup未确认 | 禁止执行，不改派Computer；按既有错误明确反馈。 |
| 两控制面同时Start / response丢失 | 同Worker原子reservation只接受一个；已有completed command跨端去重，未知结果查询，不自动重放。 |
| 旧外置附件正文源离线 | metadata/history可读，真实附件读取明确不可用；不声称正文全量复制或离线可用。 |
| queue | 当前owner process-local；其他端在线操作当前队列，不自动转移/恢复到另一控制面。 |
| 账号/云节点凭据过期 | 原Auth/Node lease撤销对应远程连接；不撤销本机凭据或无关loopback执行，不由sync身份授予执行权。 |

错误继续使用统一TalosError/ApiError；网络处理owner报告一次，不重复报告原执行失败。
同步管理节点与Computer在线分别展示，不能用历史running或已存heartbeat推断可执行。

## 7. 配置与部署

配置owner派生运行策略：sync.enabled未配置时，未登录false、已验证登录/binding后true；显式false优先。DEV模板显式false，PROD可省略沿登录默认；旧匿名历史仍需认领，不因登录自动上传。
`sync.hub_endpoint` 为完整`wss://sync.talos.love/v1/sync`，状态与enable/disable复用同一配置owner，不另造模式开关。目录/feed/全量增量复用同一连接，见[WSS协议](https://github.com/apilots/talos-sync/blob/7230f9fdf294e7ae218bdb6d3cf1c4ef60203380/spec/feat-044-talos-sync/wss-protocol.md)。
Worker listener配置独立于runner transport，管理API与Worker RPC两个endpoint可明确自测；Desktop的stdio保留同一管理业务入口。
共享的个人Worker默认注册 `agent.talos.love`，保留本机loopback listener；pure local不开公网注册/同步。
不提供sync任务pull或RPC relay配置；agent连接与sync连接不能共用一个生命周期故障开关。
独立sync服务部署自己的认证、TLS终止、持久目录和备份；两个域名不是同一talos-server进程的别名。
管理端lib沿用Talos workspace与生成/检查流程；server/SDK/types归同级独立Rust工程，自有workspace/lockfile/target/发布流程。新服务端口、路由与证书不硬编码进业务调用。

## 账号选择与版本化迁移

原API鉴权后先将选择写入sync_account_preferences。关闭随后只停止该账号H handle；没有binding不进行hub discovery。登录/status/重启读取同一选择，只有显式enable才能重新启用。YAML false是整个管理服务限制，不能被任何账号覆盖；当前端选择不改其他端。已有binding在部署/账号禁用期间保留capture/outbox，Worker执行、G注册/RPC/源FileService独立可用。

202610130001_sync_account_preferences.sql按原migration runner事务创建账号选择表、保留旧binding.enabled选择并移除旧列；没有运行时双写、旧结构兼容分支或伪造hub ID。
