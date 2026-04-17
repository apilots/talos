# Web UI Project Page Design

**版本**: v0.4  
**状态**: 草案  
**最后更新**: 2026-04-17

---

## 1. 目标

在不改动现有 Web shell 的前提下，引入 **Project-first 浏览路径**：

1. Sidebar 新增 `Projects` 一级导航入口
2. 新增 ProjectsPage（Project 列表）
3. 新增 ProjectSessionsPage（Project 下的 Session 列表）
4. ChatSessionPage 顶部增加 `project / session` 层级标题

需补充两项共享协议能力，并同步更新 RS SDK 和 TS SDK：

1. `SessionQuery.project_id`
2. `ProjectRecord.session_count`

除此之外不新增全局 store，复用现有 query/type。

---

## 2. 现状速查

| 项 | 位置 |
|----|------|
| 主题 token | `src/app/styles/index.css` |
| Sidebar | `src/widgets/sidebar/Sidebar.tsx` |
| ProjectRecord | `src/shared/types/ui.ts` |
| useProjectsQuery | `src/features/chat/hooks/useChatQueries.ts` |
| useSessionsQuery | `src/features/chat/hooks/useChatQueries.ts` |
| 共享 UI | `src/shared/ui/` (Panel, Badge, Button, Input) |

**主题**：`ui-slice.ts` 默认值为 `"light"`，启动时由 `providers.tsx` 写入 `document.documentElement.dataset.theme`，因此实际默认是 **light theme**（`background: rgb(250,248,244)` 暖米白，`brand: rgb(194,109,39)` 暖棕橙）。

**样式模式**：纯 Tailwind + `cn()`，opacity 后缀（`border-border/80`、`bg-card/90`）。

---

## 3. 美学取向

延续现有 **Quiet Editorial** 风格（light theme 暖米白底色），不引入新视觉语言：

- 背景 `bg-background`，卡片 `bg-card/90`，边框 `border-border/80`
- 圆角沿用 `--radius-panel (1.18rem)` / `--radius-control (0.84rem)`
- `backdrop-blur` 用于浮层，正文区不用
- brand 橙色只用于：CTA 按钮、active 状态高亮
- status badge 复用现有 `statusTone()`：running → `success`，waiting_approval → `warning`，error → `danger`，done → `default`
- 信息克制：每行只露出必要字段，不堆砌 metadata

**极简原则**：

> 能用一行表达的不用卡片；能省略的 label 不写；能用颜色区分的不加 icon。

---

## 4. 路由

```
/projects                       ProjectsPage
/projects/:projectId            ProjectSessionsPage
/sessions/:sessionId            ChatSessionPage（已有，加 ChatContextBar）
```

`ProjectsPage` 只展示 projects 表里的真实 Project，不引入 `__unbound_project__` 虚拟路由。

未绑定 Project 的 session 仍通过现有 ChatHomePage、Sidebar thread list、直接 session URL 访问；进入聊天页后顶部标题继续按“只显示 session title”处理。

---

## 5. Sidebar 改动

当前 Sidebar 没有 `Chats` 这个独立 nav item，品牌卡片（logo 区）点击即回首页，thread 列表直接跟在其后。

本次**只新增 `Projects` 一个 NavItem**，插入在品牌卡片与 thread 列表之间。`Chats` 不新增，logo/home link 继续承担"返回聊天主页"的语义。

```
┌─────────────────────┐
│ Talos  [logo card]  │  ← 点击 → /（已有）
├─────────────────────┤
│ Projects  ← 新增    │
├─────────────────────┤
│ thread list...      │
├─────────────────────┤
│ Memory / Files /... │
└─────────────────────┘
```

样式完全复用现有 `NavItem` 组件（`Sidebar.tsx`）：active 状态为 `bg-card/92 text-ink` + inset border，不单独给 Projects 加 `text-brand`，保持与底部导航项一致。不改动其他部分。

---

## 6. ProjectsPage

**极简列表布局**，不做卡片网格——与 session list 保持一致的阅读节奏。

```
┌──────────────────────────────────────────────────────┐
│  Projects                           [+ New project]  │
│                                                      │
│  [ Search...                                     ]   │
│                                                      │
│  talos-rs                            12 sessions  >  │
│  investor-lab                         4 sessions  >  │
└──────────────────────────────────────────────────────┘
```

**行规格**：

```
高度:      52px
布局:      flex justify-between align-center
左侧:      project name，body font，text-ink
右侧:      "N sessions"，caption，text-muted
           count 来源：`GET /v1/projects` 返回的 `ProjectRecord.session_count`
           ProjectsPage 不从 sessions 列表派生 project 列表或 project count。
           若 projects 未加载，右侧只显示 ">" chevron，不显示 count
           ">" chevron icon，16px，text-muted/60

border-bottom: 1px border-border/40（行分隔）
hover: bg-surface/60，transition 150ms
```

**空态**：

```
"No projects yet."
[Create first project]  ← brand 色按钮
```

---

## 7. ProjectSessionsPage

**面包屑 + 单列 session 行**，布局与现有 ChatHomePage session 选择区高度一致。

```
┌──────────────────────────────────────────────────────┐
│  Projects / talos-rs                                 │
│                                                      │
│  [ Search...                                     ]   │
│                                                      │
│  Refactor router pipeline                            │
│  qwen-max  ·  12m ago                    ● running   │
│  ─────────────────────────────────────────────────── │
│  Fix memory flush race                               │
│  gpt-5.4  ·  2h ago                      ○ done     │
└──────────────────────────────────────────────────────┘
```

**面包屑**：

```
"Projects"：caption，text-muted，hover text-ink，cursor pointer
"/"：caption，text-muted/40，padding-x 6px
"talos-rs"：caption，text-ink（当前页，不可点击）
```

**Session 行规格**：

```
高度:      64px（两行内容）
左侧上:    session title，body，text-ink，单行截断
左侧下:    "model  ·  time ago"，caption，text-muted
右侧:      status badge（复用现有 Badge 组件 + 现有状态色）

hover: bg-surface/60，整行可点击
```

**Status badge** 直接复用现有逻辑（running/done/error/waiting_approval），不新增变体。

---

## 8. ChatContextBar

插入 ChatSessionPage 顶部，消息区上方，高度 **48px**。

```
bound:
┌──────────────────────────────────────────────────────┐
│  talos-rs  /  Refactor router pipeline           ˅   │
└──────────────────────────────────────────────────────┘

unbound:
┌──────────────────────────────────────────────────────┐
│  Refactor router pipeline                        ˅   │
└──────────────────────────────────────────────────────┘
```

**规格**：

```
background:    bg-card/80
border-bottom: 1px border-border/60
padding-x:     24px（与消息流对齐）

project name:  caption，text-muted，hover text-brand，cursor pointer
"/"：          caption，text-muted/40，padding-x 8px
session title: body，font-medium，text-ink，单行截断
"˅" caret:     text-muted/60，margin-left auto
```

点击 `project name` 跳转回 `ProjectSessionsPage`。首版 caret 只做视觉 affordance。

---

## 9. Loading & 空态

仓库当前无现成 skeleton 组件，需新建一个极简占位块：`animate-pulse rounded bg-surface/80`，高度与实际行高一致。

| 页面 | skeleton |
|------|---------|
| ProjectsPage | 5 行同等高度占位块 |
| ProjectSessionsPage | 6 行 session 行占位块 |
| ChatContextBar | 单行占位，宽 50% |

错误状态：内联文字 + `[Retry]` ghost 按钮，不跳转，不弹窗。

---

## 10. 新增模块清单


| 文件 | 职责 |
|------|------|
| `pages/projects/ProjectsPage.tsx` | 列表页路由挂载，只做组合 |
| `pages/projects/ProjectSessionsPage.tsx` | Session 列表页，只做组合 |
| `features/projects/hooks/useProjectsPageState.ts` | 搜索过滤 + 真实 project 列表排序 |
| `features/projects/hooks/useProjectSessionsPageState.ts` | project-scoped session 过滤 |
| `features/projects/components/ProjectRow.tsx` | Project 列表行 |
| `features/projects/components/ProjectSessionRow.tsx` | Session 行 |
| `widgets/chat-context-bar/ChatContextBar.tsx` | 聊天顶部层级标题 |

Sidebar.tsx 轻改：加一个 `Projects` 导航入口，不重构其他部分。

---

## 11. 后端与 SDK 扩展要求

### 11.1 问题

当前有两个缺口：

1. `SessionQuery`（`talos_types`）只支持 `user_id / channel_id / provider`，无 `project_id` 字段，因此 `ProjectSessionsPage` 无法按 project 从服务端拉取 session 列表。
2. `ProjectRecord` 只返回 `project_id / name / directory / created_at / updated_at`，无 `session_count`，因此 `ProjectsPage` 无法直接从 `/v1/projects` 获取每个 Project 的 session 数。

这里要明确：

- Project 列表来自 **projects 表**
- session 列表来自 **sessions 表**
- `ProjectsPage` 不能从 SessionQuery 反推出“有哪些 Project”
- 如果 `/v1/projects` 返回空数组，语义就是“当前没有任何 Project”

### 11.2 变更范围

**① 后端：复用并扩展 `talos_types::SessionQuery`**

```rust
// crates/common/types/src/session.rs
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SessionQuery {
    pub user_id: Option<String>,
    pub channel_id: Option<String>,
    pub provider: Option<String>,
    pub project_id: Option<String>,   // 新增
}
```

存储层 `list_session_records()` 的 SQL 同步增加 `WHERE project_id = ?` 条件（仅当字段有值时）。

`ProjectSessionsPage` 只查询真实 project id，不支持 `__unbound_project__` 这种虚拟值。

**② 后端：扩展共享 `ProjectRecord`，由 `/v1/projects` 直接返回 count**

```rust
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ProjectRecord {
    pub project_id: String,
    pub name: String,
    pub directory: PathBuf,
    pub created_at: i64,
    pub updated_at: i64,
    pub session_count: i64,   // 新增
}
```

`GET /v1/projects` 在列出 projects 表时，同时带回该 project 绑定的 session 数。

实现建议：

- 使用单条 SQL 直接返回 count
- 例如 `LEFT JOIN sessions` + `GROUP BY`
- 或相关子查询

目标是让 `ProjectsPage` 只依赖 `/v1/projects` 就能完成渲染。

**③ RS SDK：复用 `talos_types::SessionQuery`，不新增平行查询类型**

```rust
pub use talos_types::SessionQuery;

impl TalosClient {
    pub async fn list_sessions(&self) -> TalosResult<Vec<TalosSession>> {
        self.list_sessions_with_query(&SessionQuery::default()).await
    }

    pub async fn list_sessions_by_project(
        &self,
        project_id: &str,
    ) -> TalosResult<Vec<TalosSession>> {
        self.list_sessions_with_query(&SessionQuery {
            user_id: None,
            channel_id: None,
            provider: None,
            project_id: Some(project_id.to_string()),
        })
        .await
    }

    pub async fn list_sessions_with_query(
        &self,
        query: &SessionQuery,
    ) -> TalosResult<Vec<TalosSession>> {
        let url = format!("{}/v1/sessions", self.config.gateway_url);
        let payload: ListSessionsResponse = Self::decode_json(
            self.authorized(self.http_client.get(&url).query(query))
                .send()
                .await
                .map_err(|e| Self::request_error("GET", &url, e))?,
            "GET",
            &url,
        )
        .await?;
        Ok(payload.data)
    }
}
```

原 `list_sessions()` 保持兼容，等价于空查询。

**④ TS SDK：扩展共享类型，复用现有 client 查询能力**

```ts
export interface SessionQuery {
  user_id?: string;
  channel_id?: string;
  provider?: string;
  project_id?: string;   // 新增
}

export interface ProjectRecord {
  project_id: string;
  name: string;
  directory: string;
  created_at: number;
  updated_at: number;
  session_count: number; // 新增
}
```

`client.ts` 的 `sessions.list(query?)` 无需改动，`urlWithQuery` 已自动处理可选字段。

前端封装一个语义化方法（可在 `api.ts` 或 `useChatQueries` 旁边）：

```ts
// 复用 getTalosClient().sessions.list({ project_id })
export async function listSessionsByProject(projectId: string) {
  return getTalosClient().sessions.list({ project_id: projectId });
}
```

### 11.3 前端使用方式

`ProjectSessionsPage` 直接调用 `listSessionsByProject(projectId)`，不经过 `useSessionsQuery()`（后者绑定了全局 `threadFilter`）。

`ProjectsPage` 直接调用 `projects.list()`：

- 列表项来自真实 `ProjectRecord[]`
- `session_count` 直接渲染
- 不做前端 project 拼装
- 不做逐项 N+1 session 查询

---

## 12. 已知约束与明确决策

| # | 约束 | 决策 |
|---|------|------|
| 1 | Project 是独立表，不能从 sessions 反推出 project 列表 | `ProjectsPage` 只使用 `/v1/projects` 的真实结果；返回空数组就显示空态 |
| 2 | `ProjectSessionsPage` 不能被 Sidebar 的 `threadFilter` 污染 | session 列表统一走 `listSessionsByProject(projectId)`，见 §11 |
| 3 | `statusTone()` 中 running = `success`（青绿），不是橙色 | 直接复用，不另起颜色规则 |
| 4 | `NavItem` active = `bg-card/92 + inset border`，无 `text-brand` | Projects 入口直接复用，不特判 |
| 5 | 无现成 skeleton 组件 | 新建最小 `<SkeletonLine>` 组件，放 `src/shared/ui/` |
| 6 | ProjectSessionsPage 是否放 `+ New session` 入口 | **首版不放**；用户从 ChatHomePage 或 Sidebar 新建 session |

---

## 13. 实施顺序

**前置（阻塞后续前端）：**

0. 后端共享类型扩展：
   - `SessionQuery.project_id`
   - `ProjectRecord.session_count`
   存储层与 HTTP handler 同步支持

1. SDK 同步：
   - RS SDK 复用 `SessionQuery`，新增 `list_sessions_with_query()` / `list_sessions_by_project()`
   - TS SDK 扩展 `SessionQuery` / `ProjectRecord`
   - 前端封装 `listSessionsByProject()`

**前端（可并行或串行）：**

2. Sidebar 加 `Projects` 入口与路由注册
3. ProjectsPage（含 loading / empty / error）
4. ProjectSessionsPage
5. ChatContextBar 插入 ChatSessionPage

步骤 3-5 依赖步骤 0-1 完成；步骤 2 可提前独立完成。
