# 客户端与 SDK 架构

[English](CLIENT_ARCHITECTURE.md)

> Historical translation. The [English document](CLIENT_ARCHITECTURE.md) owns the current
> 2026-09-07 contracts, diagrams, deployment rules and verification guidance.

## SDK-first 目标

每个 Talos 产品客户端都使用受支持的 SDK。TypeScript SDK、Rust SDK 和 embedded Rust SDK
必须暴露相同的产品能力族和共享领域语义。客户端只选择 transport 实现，不能选择一套
更小或不同的产品 API。这是强制目标契约，不能替代 parity test。

## 客户端链路

```text
+------------------+  +------------------+  +------------------+  +------------------+
| Web UI           |  | Desktop         |  | TUI / Command    |  | Rust SDK client  |
+--------+---------+  +--------+---------+  +--------+---------+  +--------+---------+
         |                     |                     |                     |
         v                     v                     v                     v
+------------------+  +------------------+  +------------------+  +------------------+
| TS TalosClient   |  | TS product SDK   |  | Rust SDK        |  | Rust SDK        |
| shared types     |  | Client + types   |  | GatewayClient   |  | GatewayClient   |
+--------+---------+  +--------+---------+  +--------+---------+  +--------+---------+
         |                     |                     |                     |
         v                     v                     v                     v
+------------------+  +------------------+  +------------------+  +------------------+
| Browser adapter  |  | HTTP/SSE or IPC  |  | EmbeddedServer  |  | Rust AppServer   |
| HTTP + SSE       |  | personal Worker  |  | Client          |  | Client           |
+--------+---------+  +--------+---------+  +--------+---------+  +--------+---------+
         |                     |                     |                     |
         +---------------------+----------+----------+---------------------+
                                          |
                                          v
+------------------------------------------------------------------------------------+
| HTTP Gateway | main-process JSON-RPC stdio | in-process Gateway | app-server       |
+-----------------------------------------+------------------------------------------+
                                          |
                                          v
+------------------------------------------------------------------------------------+
| Shared TalosService domain results, ErrorCode, and Timeline behavior               |
+------------------------------------------------------------------------------------+
```

## 统一能力面

各 SDK 必须在产品能力、强类型 Request/Response、错误和 Stream 行为上保持对齐：

```text
Authentication and profile
Models and configuration
Sessions, Turns, history, compaction, retry, and interruption
Timeline subscription, replay, recovery, and terminal state
Message queue, steering, dynamic tool results, and user actions
Projects, files, filesystem directories, memory, and workspace documents
Skills, plugins, plugin sources, and lifecycle operations
```

不同语言可以使用符合自身习惯的命名，但能力含义、生命周期、ErrorCode、顺序、分页、
optional 语义和终态必须一致。只有 TypeScript SDK facade、Rust SDK、embedded SDK
以及对应产品 transport 都可达时，能力才算完整。

## TypeScript SDK

| Package | 职责 |
| --- | --- |
| `@talos/sdk-core` | 共享能力 Client、领域类型、错误和生成协议 binding |
| `@talos/sdk-browser` | HTTP、multipart upload、认证 header 和 SSE transport 实现 |
| `@talos/sdk-app-server` | JSON-RPC transport、Electron main process 和 typed preload IPC adapter |

Web 和云模式 Desktop 通过 Browser transport 使用 `TalosClient`，访问同一控制面的
HTTP/SSE 产品 API。Desktop 的 typed preload IPC 和 Electron main-process stdio
通过 `AppServerClient` 管理已打包的 Worker 宿主、登录和节点身份；纯本地模式也使用这条
IPC/stdio 路径访问产品 API。两个 facade 共享生成领域与协议类型，但当前没有实现同一个
TypeScript client interface。因此能力对齐依赖 API review 和 parity test，而不能只依赖
TypeScript 类型系统。

## Rust SDK 与 embedded SDK

`talos-sdk` 负责 Rust `GatewayClient` 能力 contract、共享公共类型和 Rust
`AppServerClient`。`AppServerClient` 通过 JSON-RPC stdio 实现该 contract。

`talos-sdk-embedded` 提供 `EmbeddedServerClient`，通过进程内 Gateway 实现同一个
`GatewayClient` contract。因此 TUI 和 command mode 与协议型 Rust client 使用相同且
由编译器检查的 SDK operation、Response、错误和 Timeline 行为，同时不需要子进程或本地端口。

## 共享类型流水线

```text
Rust domain and current protocol definitions
                  |
                  v
       App Protocol generator
             /          \
            v            v
 JSON Schema        generated/app-protocol.ts
                         |
                         v
             @talos/sdk-core aliases and exports
                         |
                         v
          Browser SDK | App Server SDK | Web | Desktop
```

协议生成产物来源于 Rust 定义。客户端代码不能手改生成文件、复制 Request/Response DTO，
也不能把 transport envelope 暴露为领域对象。

## 客户端职责

Web/Desktop 进入 Session 时先读取全部 history 分页，已完成轮次由 history 展示，
未完成轮次再接入增量事件。`lastStableEventId` 是当前历史页最后一个已完成轮次的
事件游标；读取所有分页后使用最后一页的值。它不是可能包含未完成轮次的最新事件位置，
也不能与 history 的分页游标混用。该响应字段要求后端与生成客户端同步部署，HTTP 和
JSON-RPC 使用相同契约；Rust SDK/TUI 的历史读取方可忽略该字段。

持续查看时保留已消费的事件游标，轮次结束后与 history 衔接，不重播已完成内容。
空闲 Session 通过查询发现运行中轮次和队列变化，不保持执行事件订阅；显式维护操作
仍可观察 Session 事件。active-turn 响应的 `latestEventId` 表示 Session 最新已落库事件，
包含失败轮次和产品通知；空闲时快照变化才刷新 history，可发现轮询间完成的远端轮次。
该字段是变化信号，不能替代订阅所用的 `lastStableEventId`，后端和生成客户端须同步升级。
底层 Session 订阅继续支持跨轮次推送，由 UI 决定何时关闭。

- SDK 负责完整产品 API、强类型领域对象、结构化错误、Stream contract 和
  transport-neutral 生命周期语义。
- Transport adapter 负责序列化、认证传输、连接生命周期、上传编码和事件送达机制。
- Client 负责展示、交互、乐观状态和重连体验。
- Server 负责搜索、匹配、解析、权限和持久化生命周期规则。

## 对齐要求

- API 变更同时审查 Rust SDK、TypeScript SDK、embedded SDK、HTTP、JSON-RPC、Web、
  Desktop 和 TUI 影响。
- 当前协议变化通过 `just gen-app-protocol` 重新生成 schema 和 TypeScript binding。
- SDK parity test 比较可观察行为，不比较 transport 实现。
- Embedded execution 不能引入公共 SDK contract 不可达的私有能力。
- HTTP 和 JSON-RPC envelope 可以不同，但领域 Result 和 `ErrorCode` 语义必须一致。
- 用户可见 Timeline 变化必须覆盖每个受影响客户端的渲染。

Rust trait 当前强制 app-server 与 embedded 实现对齐。TypeScript Browser 和 App Server
facade 是独立 API，因此需要显式 parity review。相关路径和测试未对齐前，文档不能把
新能力描述为完成。

## Computer 选择与执行归属

联网 Desktop 与 Web 共用 HTTP/SSE 产品 SDK，连接同一控制面。Desktop main 仍通过
现有 IPC/stdio 管理已打包 app-server 内的一个 TalosWorker，保存 UUID/注册凭据并主动
Connect；renderer 只读取控制面配置和安全身份，不持有节点密钥。明确选择纯本地模式时，
产品 API 使用原 IPC/stdio 路径，离线不会自动切回本地。

features/computers 提供独立栏、完整搜索/分页、选择与管理页；本机匹配控制面地址和 UUID，
Web 多台时明确选择。环境和列表携带目标 nodeId，已有 Session 保持原归属。缓存按账号、
控制面、资源隔离，环境/列表再按 Computer 隔离；草稿和最近选择由客户端保存，切换不取消
运行中的 Turn。控制面保存业务元数据与 Timeline，Worker 保存配置、MCP 凭据和 Workspace
文件；Harness/WorkerEnvironment 屏蔽传输，远端 OAuth 回到原 Worker attempt。


Native Browser 后端的当前标签和元素引用同时绑定 managed/connected 类型。切换类型时先清除旧观察，不能向用户 Chrome 发送自带浏览器的标签 ID，反向切换亦然。同类型的新 Turn 可保留当前标签，元素引用仍需重新观察。账号或控制 epoch 改变时也清除当前标签，包括插件关闭并重新启用后自有网页视图重建的情况。

## 应用外壳与右工作区

Web 与 Desktop 具有相同的三类资源 Tab；Computer 内容按执行电脑的 canonical
BrowserSnapshot.host/display 能力分支，不按查看客户端 isDesktop 整体禁用。本机原生
电脑没有远程桌面；Desktop 连接 dockerDesktop Worker 时仍可查看并经原鉴权接管。

AppLayout 组装三个全高同级宏区：全局 Computer rail｜中间业务（Session/Project
侧栏 + 主页面）｜右工作区。导航、rail、侧栏、主页面和工作区仍各自独立组件化；
AppBusinessRegion 只组合自己的 header 和 sidebar/main 行。各区 header 位于本区
贯穿内容的垂直边界内，右分界线由自身 header 贯穿 Tab/内容；不能用一条跨中右的
header 或两张内缩卡片替代分区。复用既有 token 和普通区域结构，不靠测量或新布局框架。
rail 与顶导复用连续的轻量底色，侧栏、主内容、右内容在顶导下以小圆角和低对比细边界
衔接，不增加外部留白或厚重色块。

分栏时中间 header 拥有 PanelLeft（仅中区业务侧栏）、前后导航、新聊天及页面/
Session/Project 上下文，不提供查看电脑入口。右区顶部只有一行 Tabs、＋、放大/还原
和显示/收起控件，PanelRight 与左 PanelLeft 对称；不另加“右工作区”整行 label，
下方直接是所选内容（含网页地址栏/body）。收起或为空时控件移至中间 header 末尾。
窄屏/展开 workspace modal 同样只含资源/工作区控件，中区保留原 owner 并 inert/
aria-hidden，不向右区投影导航或 Session 上下文。Settings 可与已打开工作区并存，
MCP 行仍仅启停。

SidebarComputerEntry 经原 SidebarContainer/SidebarPrimaryNav 插槽位于“新会话”下。
“你的电脑”仅在目标 snapshot 的 host=docker 且 display.kind=dockerDesktop 时显示，
点击原 computer/nodeId Tab。Session 路由使用其绑定 nodeId，未加载时不借用 rail 选择；
其他页面使用当前 Computer。Desktop 查看 Docker Worker 仍可显示，本机原生无远程
入口；ComputerInspector 保留 bundled/My Chrome 配置及本地动作入口。

顶导不提供单独的 globe 网页快捷入口。最右工作区主开关在 0 Tab 时显示可点击 Plus，调用原
createWorkspaceWebpage 创建空网页 Tab 并展开 New Tab；已有 Tab 时显示 PanelRight
和真实数量，展开/收起均显示，点击仅 toggle，不再创建 Tab。

中间 header 自身按 sidebar/main 再分左右：AppNavigation 与侧栏同宽/同边界，
AppSessionContext 与主内容对齐。AppBusinessRegion 唯一 sidebar 宽度同时控制
header/body 两列，以 200ms ease-out CSS 在原宽度与零之间做抽屉过渡：侧栏向左
退场并由所属容器裁切，main/composer 同步铺开；侧栏内部固定宽度，避免文字被挤压
换行。reduced-motion 偏好禁用过渡。宽屏业务侧栏是普通 aside，只有窄屏覆盖场景
使用 native modal dialog，不用宽屏非模态 dialog 保持抽屉。关闭立即让侧栏内容 inert/aria-hidden 并按原规则返回可见开关焦点，
不等动画；唯一 AppNavigation DOM 保持可达，主内容 header 连续让位，不能把开关
一并 inert；关闭后不留固定空列或重复入口。几何动画不重挂载保留的
网页 DOM，也不申请或恢复 Agent/远端输入权限。右抽屉复用同一 200ms/reduced-motion/
resize 策略，以原生 display/overlay 过渡呈现；隐藏立即 inert/aria-hidden 并清理
Computer 媒体，不用计时器或延迟权限回收。

初载、刷新和被动视口变化不因 body 获焦而聚焦导航。宽屏 sidebarOpen 是持久布局偏好，
AppLayout 的窄屏抽屉是初始关闭、仅显式 toggle 打开的瞬时状态；进入工作区或离开窄屏
时关闭，组件接受所属关闭/切换回调。工作区呈现变化保留仍存活地址框的焦点。
跨域 iframe 仅用浏览器允许的 Window.focus 恢复浏览上下文，不读取子 DOM 或添加 bridge：
Chromium 的原生 modal 进入会清除子页面内层输入焦点，用户需重新点选，但 DOM/草稿保留；
退出 modal 保留其当前输入焦点。只有真实关闭/移除才恢复焦点，不抢占已移到其他区域的
焦点，保留键盘焦点提示。

只有 sidebar/main 和 middle/workspace 两条竖线支持拖宽，56px rail 固定。原 UiSlice
中的 sidebarWidth/workspaceWidth/resizingPane 统一拥有临时几何，按账号/连接 scope
重置且不持久化。有效宽度共享给 header/body 和侧栏内部，按视口及主内容空间执行
min/max。两处复用 AppResizeHandle 键盘 separator（左右方向键、Home/End 和可见焦点）；
pointer capture 拖动实时改宽并禁抽屉过渡，松手、取消、失去 capture 或卸载时释放
capture 并清理临时 resize 状态，恢复原 200ms/reduced-motion 策略。不重挂载资源、
不恢复 Agent 权限，不新增配置服务或布局框架。

| 区域组件 | 责任 |
| --- | --- |
| `AppTopBar` / `AppNavigation` / `AppSessionContext` | 分区 header 基元、侧栏对齐的导航和主内容对齐的业务上下文；不拥有工作区几何 |
| `ComputerRail` | 原 Computer 选择与全局 rail |
| `AppSidebar` | 宽屏普通 aside / 窄屏 native modal 与焦点边界；复用 SidebarContainer 内容 |
| `AppMainContent` | 主路由内容表面，保留各页面业务 owner |
| `RightWorkspace` / `WorkspaceControls` | 右区 header/几何操作、typed Tab 与内容组装 |

AppLayout 将独立组件组装为三宏区；ComputerDesktop、MarkdownFilePreview、WebPagePreview 分别负责
具体内容。窗口外壳与几何不能成为第二个文件、浏览器或媒体授权 owner。

现有 AppLayout 组装应用外壳。右工作区是呈现区域，Computer 只是具体窗口类型，
不拥有工作区。`shared/store/ui-slice.ts` 统一保存不持久化的
闭合类型的真实资源描述、当前项、可见和展开状态。Computer 内容绑定明确 nodeId；
Markdown 及网页/本地服务预览复用其实际内容 owner 的授权。
单一 `shared/store/workspace-window.ts` 闭合定义 computer/nodeId、markdown/fileId、
webpage/id/url 使用 UI owner 生成的稳定独立 id；Computer/Markdown 的 key 来自
类型与真实资源身份。每次新建网页 Tab 都有独立身份，同 URL 可多 Tab 共存，地址
改变不更换身份或按 URL 合并。原 UI slice 的 createWorkspaceWebpage 生成 id 和空 url，
导航只更新该描述，不新建身份 owner。导航仍只接受规范化且无内嵌凭据的 HTTP/HTTPS 地址。重开同一 Computer
只选中已有项，关闭移除描述并按需选相邻项。窗口数来自保留的描述，包含已收起项，
不等于媒体流数或原生浏览器标签数。Computer/Markdown 按类型与真实资源身份去重，
网页保持独立 Tab 身份；不虚构未实现的占位提供者或任意窗口类型，也不新增后端管理器、协议或持久注册表。

三类真实描述以浏览器式 Tab 共存，每描述一项，与 Agent 浏览器内部标签分离，
不保持全部内容后台活跃。Tab 条末尾“＋”立即创建并选中真实空网页 Tab，计入数量，
在页内地址栏输入；可操作的 New Tab 空态不先弹 URL dialog，也不是地址错误。空地址不创建 iframe 或声称已访问，刷新/外部打开不可用，“输入
地址”仅聚焦页内地址栏。收起或为空时在中间 header 末尾提供该入口，Tab 条可见时去重。使用 tablist/tab/
tabpanel 关联与当前 Tab 的单一键盘入口，左右箭头、Home/End 切换；关闭后焦点回到
相邻/当前项，最后一项关闭后回到可见顶栏入口。只有选中且可见的 panel 可交互；
网页 DOM 跨 Tab/收起保持挂载，非活动时 hidden/inert，保留表单与滚动。Computer
仅 selected 且 visible 时挂载媒体，Markdown 只挂载当前项并复用查询缓存。原
Computer 媒体 cleanup 和 Human 暂停规则不变；关闭网页 Tab 或清空 scope 才真正
销毁其 iframe，不另建保存 iframe 的 registry。

资源内容与外壳几何、能力授权分别归属原 owner。Markdown 从真实 Files 项打开，复用
原 File API 和 MarkdownRenderer，不新建文件存储或读取服务。原 FileService 对完整
`.md`/`.markdown` 上传验证有效 UTF-8 文本（除换行、回车、制表外拒绝控制字符），
识别为 text/markdown，再执行配置的 MIME 白名单、大小和所有权规则；
扩展名或客户端 MIME header 不绕过准入，配置排除 Markdown 时仍拒绝。网页/本地服务预览为
独立 iframe，与 Agent 浏览器的 profile、标签和控制权分离；不附着 Browser MCP 目标，
不调用 Desktop 浏览器宿主。遵守目标页面 CSP/X-Frame-Options 等嵌入限制，不绕过，
不代理远端 Worker localhost。预览可达性来自查看客户端，其 localhost 不代表所选
Worker。资源组件各自负责加载/渲染/清理，原能力 owner 继续负责文件、浏览器和媒体鉴权。
Files 详情按 fileId 打开 Markdown；Tab 条“＋”或收起/为空时中间 header 末尾入口立即创建新
网页 Tab，页内输入地址；不复制未实现的 Tools、recents 或 Terminal 占位功能。Markdown 元数据/
内容查询保留原 account/connection/file scope、加载错误和显式重试，复用既有渲染器，
不执行原始 HTML；有效绝对 HTTP(S) 链接经原外部打开路径处理，不导航产品主窗。
WebPagePreview 拥有 New Tab 空态、地址、刷新及外部打开动作，URL 变更交回 UI owner
更新同一稳定 Tab 的描述，不按 URL 合并。地址代表本 UI 显式提交的目标，不能观察或
宣称同步跨域 iframe 内部跳转和浏览历史。
其 iframe 仅给 sandbox allow-scripts/allow-forms，使用 no-referrer，不授予 same-origin、
popup、top-navigation，不注入 Talos token、preload 或宿主 bridge。嵌入限制提示常驻，
iframe load/error 事件不能可靠区分成功、网络失败或拒绝嵌入，产品不据事件宣告结果；
浏览器展示实际结果，保留刷新、改址和外部打开。

账号或管理端身份 scope 变化时，原 app-store 订阅同步清空窗口和布局；选择呈现窗口
不改变 rail 的 Computer 选择，不迁移 Session 的执行归属。这里的工作区与文件系统
Workspace 状态是不同概念。

只有选中且可见的 Computer 内容挂载 viewer。收起保留描述，但卸载 Computer 媒体
内容；切窗和关闭同样沿原 noVNC/SDK cleanup 结束连接并使待完成准入失效。保留的
独立网页 DOM 不保留或获得 Computer 媒体授权。重新显示先读权威 snapshot，
再申请新的媒体授权。描述不持有 ticket、controller epoch 或 socket。只读 viewer 结束
不暂停 Agent；Human 控制流结束保持 Worker 的暂停意图，外壳操作不隐式 resume。
浏览器配置、暂停、接管和交还留在 Computer 内容内；MCP 设置只保留内置插件启停。
