# Talos Web UI 前端工程规范

**版本**: v1.0
**状态**: 已按 Web/Desktop 共享 runtime 更新
**最后更新**: 2026-04-30

> Phase 6 更新：Web 继续通过 `@talos/sdk-browser` 使用 HTTP/SSE；Desktop renderer
> 复用同一 Web UI 领域模型和主要组件，通过 runtime factory 注入
> `@talos/sdk-app-server/desktop` typed bridge，不直接访问 transport 或 Node/Electron API。

> 本文档约定 Talos Web UI 的前端工程结构、分层规则、复用标准和质量门禁。
> 所有实现人员在动手编码前必须阅读，Code Review 以此为基准。
> UI 界面设计参见 [web-ui-design.md](./web-ui-design.md)。

---

## 1. 设计哲学与参考项目

### 1.1 核心原则

| 原则 | 含义 |
|------|------|
| **单向数据流** | UI 是状态的函数，不在组件里维护"派生数据"的副本 |
| **关注点分离** | UI 渲染 / 业务逻辑 / 服务端通信 严格分层，不混写 |
| **面向接口复用** | 共享的是接口（hook / type），不是具体实现组件 |
| **最小权力原则** | 组件只访问它需要的状态，不传递整个 store |

### 1.2 参考项目

以下项目是本规范的主要参考来源，遇到架构问题优先查阅：

| 项目 | 借鉴点 | 链接 |
|------|--------|------|
| **Bulletproof React** | 目录结构、feature 分层、组件设计规范 | https://github.com/alan2207/bulletproof-react |
| **Feature-Sliced Design** | 模块边界划分、层级依赖约束 | https://feature-sliced.design |
| **Vercel AI Chatbot** | SSE 流式消息处理、对话状态管理模式 | https://github.com/vercel/ai-chatbot |
| **shadcn/ui** | 组件封装方式、Radix 无障碍集成 | https://ui.shadcn.com |
| **TanStack Query** | 服务端状态缓存、loading/error 状态规范 | https://tanstack.com/query |

---

## 2. 目录结构

采用**改版 Feature-Sliced Design**，按职责分层，层间依赖单向（下层不能 import 上层）。

```
src/
├── app/                    # 应用入口与全局配置
│   ├── providers.tsx       # React 上下文组装（QueryClient, Router, Store）
│   ├── router.tsx          # 路由定义（route-based code splitting）
│   └── styles/             # 全局样式、Tailwind base
│
├── pages/                  # 路由页面（薄层，只做特性组合，不含业务逻辑）
│   ├── chat/
│   ├── memory/
│   ├── files/
│   ├── settings/
│   └── admin/
│
├── features/               # 功能模块（核心业务，按 domain 组织）
│   ├── chat/               # 对话功能
│   ├── memory/             # 记忆管理
│   ├── files/              # 文件管理
│   ├── settings/           # 设置
│   └── admin/              # 管理后台
│
├── widgets/                # 复杂可复用 UI 块（跨 feature 复用，不含 API 调用）
│   ├── sidebar/
│   ├── message-list/
│   ├── tool-call-card/
│   └── command-palette/
│
└── shared/                 # 基础设施（无业务，任意层均可引用）
    ├── api/                # HTTP 客户端封装
    ├── sse/                # SSE 客户端封装
    ├── hooks/              # 通用 hooks（useDebounce, useClickOutside 等）
    ├── ui/                 # shadcn 组件二次封装（Button, Input, Dialog 等）
    ├── lib/                # 纯工具函数（formatDate, cn 等）
    └── types/              # 全局共享 TypeScript 类型
```

### 2.1 层级依赖规则

```
pages  →  features  →  widgets  →  shared
  ↓           ↓
  └───────────┴──→  shared

禁止：
  shared  →  features      （shared 不能依赖业务逻辑）
  features/chat → features/admin  （feature 之间不能横向依赖）
  widgets  →  features      （widget 不知道具体业务）
```

跨 feature 通信通过 `shared/types` 中的事件类型或 store 中的 selector 完成，不直接 import 另一个 feature 的内部模块。

### 2.2 Feature 模块内部结构

每个 feature 遵循统一的内部结构：

```
features/chat/
├── index.ts            # 公开 API（只 export 外部需要的，其余不暴露）
├── components/         # 该 feature 的 UI 组件
│   ├── ChatInput.tsx
│   ├── MessageBubble.tsx
│   └── ToolCallCard.tsx
├── hooks/              # 该 feature 的业务 hooks
│   ├── useChat.ts
│   └── useStreamingMessage.ts
├── store.ts            # Zustand slice（仅 chat 状态）
├── api.ts              # 该 feature 的 API 调用（包装 shared/api）
└── types.ts            # feature 内部类型（不对外暴露）
```

**外部只能 import `features/chat/index.ts` 导出的内容，不能 import 内部路径。**

---

## 3. 组件设计规范

### 3.1 组件分类

| 类型 | 位置 | 职责 | 规则 |
|------|------|------|------|
| **Page** | `pages/` | 路由挂载点，组合 Feature 组件 | 不含 JSX 逻辑，只做布局和组合 |
| **Feature Component** | `features/*/components/` | 业务 UI，可访问 store 和 API | 单一 feature 内部使用 |
| **Widget** | `widgets/` | 复杂可复用块，如 Sidebar | 不调用 API，接收 props 或读公共 store |
| **UI Primitive** | `shared/ui/` | 原子组件，shadcn 二次封装 | 无状态或只有 UI 状态，无业务逻辑 |

### 3.2 组件编写规则

**Props 设计**：
```typescript
// 好：明确的接口，不传递整个对象
interface MessageBubbleProps {
  content: string;
  role: 'user' | 'assistant';
  timestamp: number;
  isStreaming?: boolean;
}

// 坏：传递整个 Message 对象（耦合数据结构）
interface MessageBubbleProps {
  message: Message;  // 避免
}
```

**组件大小**：单个组件文件不超过 200 行。超出说明职责不单一，需要拆分。

**副作用隔离**：数据获取、SSE 订阅等副作用放在 hooks 里，组件只负责渲染。

```typescript
// 好
function ChatPage() {
  const { messages, isLoading } = useChat(sessionId);
  return <MessageList messages={messages} isLoading={isLoading} />;
}

// 坏：在组件里直接 fetch
function ChatPage() {
  useEffect(() => {
    fetch('/v1/sessions').then(...);  // 避免
  }, []);
}
```

### 3.3 Compound Components 模式

复杂组件（如 ToolCallCard）使用 Compound Components 模式，避免 props 爆炸：

```typescript
// shared/ui/tool-call-card/index.tsx
export const ToolCallCard = {
  Root: ToolCallCardRoot,
  Header: ToolCallCardHeader,
  Input: ToolCallCardInput,
  Output: ToolCallCardOutput,
  ApprovalActions: ToolCallCardApprovalActions,
};

// 使用方
<ToolCallCard.Root status="waiting">
  <ToolCallCard.Header toolName="exec_command" duration={null} />
  <ToolCallCard.Input args={{ command: 'cargo build' }} />
  <ToolCallCard.ApprovalActions onApprove={handleApprove} onReject={handleReject} />
</ToolCallCard.Root>
```

参考：[Radix UI Compound Components](https://www.radix-ui.com/primitives/docs/guides/composition)

---

## 4. 状态管理规范

### 4.1 状态分类

| 状态类型 | 管理方式 | 说明 |
|---------|---------|------|
| **服务端状态**（会话列表、记忆、文件） | TanStack Query | 缓存、失效、重新获取由 Query 管理 |
| **流式消息状态**（正在生成的 token） | Zustand（chat slice） | 高频更新，不适合 React Query |
| **UI 状态**（弹窗开关、选中项） | `useState` / `useReducer` | 就近管理，不放全局 store |
| **全局 UI 状态**（侧边栏折叠、主题） | Zustand（ui slice） | 跨组件共享的 UI 状态 |
| **SSE 连接状态** | Zustand（connection slice） | Server 连接、会话运行状态 |

### 4.2 Zustand Store 结构

按 domain 切片，不用单一大 store：

```typescript
// shared/store/index.ts
import { create } from 'zustand';
import { createChatSlice, ChatSlice } from '@/features/chat/store';
import { createConnectionSlice, ConnectionSlice } from '@/features/connection/store';
import { createUISlice, UISlice } from './ui-slice';

type AppStore = ChatSlice & ConnectionSlice & UISlice;

export const useAppStore = create<AppStore>()((...a) => ({
  ...createChatSlice(...a),
  ...createConnectionSlice(...a),
  ...createUISlice(...a),
}));
```

**组件只 select 需要的字段**，避免无关状态变化引起重渲染：

```typescript
// 好
const isConnected = useAppStore(s => s.isConnected);

// 坏：select 整个 store
const store = useAppStore();  // 避免
```

### 4.3 TanStack Query 使用规范

```typescript
// features/chat/api.ts
export const sessionKeys = {
  all: ['sessions'] as const,
  list: () => [...sessionKeys.all, 'list'] as const,
  detail: (id: string) => [...sessionKeys.all, id] as const,
};

// features/chat/hooks/useSessions.ts
export function useSessions() {
  return useQuery({
    queryKey: sessionKeys.list(),
    queryFn: () => api.sessions.list(),
    staleTime: 30_000,  // 30s 内不重新请求
  });
}

// 写操作配合 optimistic update
export function useDeleteSession() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: (id: string) => api.sessions.delete(id),
    onMutate: async (id) => {
      await queryClient.cancelQueries({ queryKey: sessionKeys.list() });
      const previous = queryClient.getQueryData(sessionKeys.list());
      queryClient.setQueryData(sessionKeys.list(), (old: Session[]) =>
        old.filter(s => s.id !== id)
      );
      return { previous };
    },
    onError: (_, __, context) => {
      queryClient.setQueryData(sessionKeys.list(), context?.previous);
    },
  });
}
```

---

## 5. API 与 SSE 客户端封装

### 5.1 HTTP 客户端

所有 API 调用必须通过 `shared/api` 中的类型化客户端，禁止在 feature 里直接 `fetch`。

```typescript
// shared/api/client.ts
export function createApiClient(baseUrl: string) {
  async function request<T>(path: string, init?: RequestInit): Promise<T> {
    const res = await fetch(`${baseUrl}${path}`, {
      ...init,
      headers: {
        'Content-Type': 'application/json',
        Authorization: `Bearer ${getToken()}`,
        ...init?.headers,
      },
    });
    if (!res.ok) {
      const error = await res.json().catch(() => ({}));
      throw new ApiError(res.status, error.message ?? res.statusText);
    }
    return res.json();
  }

  return {
    sessions: {
      list: () => request<Session[]>('/v1/sessions'),
      create: (body: CreateSessionBody) =>
        request<Session>('/v1/sessions', { method: 'POST', body: JSON.stringify(body) }),
      delete: (id: string) =>
        request<void>(`/v1/sessions/${id}`, { method: 'DELETE' }),
    },
    toolApprovals: {
      submit: (sessionId: string, body: ToolApprovalBody) =>
        request<void>(`/v1/sessions/${sessionId}/tool-approvals`, {
          method: 'POST',
          body: JSON.stringify(body),
        }),
    },
    memory: {
      list: () => request<MemoryEntry[]>('/v1/memory'),
      delete: (id: string) => request<void>(`/v1/memory/${id}`, { method: 'DELETE' }),
    },
    // ... 其余接口
  };
}

export class ApiError extends Error {
  constructor(public status: number, message: string) {
    super(message);
  }
}
```

### 5.2 SSE 客户端封装

> Phase 6 约束：本节早期 `EventSource` 示例仅保留为历史设计说明。当前实现禁止
> Web UI 直接 `new EventSource`；HTTP/SSE 统一走 `@talos/sdk-browser` 的
> fetch-based stream adapter，Desktop 则通过 `@talos/sdk-app-server/desktop`
> 的 typed bridge 注入同一 client 能力。实际接入方式见 §13。

两种 SSE 连接（全局事件流 + 单会话 token 流）统一封装，不允许在组件里直接 `new EventSource`。

```typescript
// shared/sse/global-events.ts

type GlobalEventMap = {
  'session.status_changed': { id: string; status: SessionStatus };
  'session.title_updated': { id: string; title: string };
  'approval.requested': ApprovalRequestedEvent;
  'approval.resolved': { session_id: string; call_id: string; approved: boolean };
  'agent.task_started': { session_id: string; trigger: 'autonomous' | 'user' };
};

class GlobalEventBus {
  private es: EventSource | null = null;
  private handlers = new Map<string, Set<Function>>();

  connect(baseUrl: string) {
    this.es = new EventSource(`${baseUrl}/v1/events`);
    this.es.onmessage = (e) => {
      // 全局流无 seq，事件之间无顺序依赖，幂等处理即可
      // session.status_changed 属于此流，不受 transcript seq 规则约束
      const { type, data } = JSON.parse(e.data);
      this.handlers.get(type)?.forEach(fn => fn(data));
    };
    this.es.onerror = () => {
      // 断线自动重连由 EventSource 原生处理
    };
  }

  on<K extends keyof GlobalEventMap>(
    event: K,
    handler: (data: GlobalEventMap[K]) => void
  ) {
    if (!this.handlers.has(event)) this.handlers.set(event, new Set());
    this.handlers.get(event)!.add(handler);
    return () => this.handlers.get(event)!.delete(handler);
  }
}

export const globalEvents = new GlobalEventBus();

// Hook 封装，供组件使用
export function useGlobalEvent<K extends keyof GlobalEventMap>(
  event: K,
  handler: (data: GlobalEventMap[K]) => void
) {
  useEffect(() => globalEvents.on(event, handler), [event, handler]);
}
```

```typescript
// shared/sse/session-stream.ts

// 单会话 TranscriptEvent 流
// - 所有事件携带 seq（单调递增），重连时用于补流去重
// - message_committed 触发历史 Query 失效；done 停止 spinner（职责不得混用）
export function useSessionStream(sessionId: string | null) {
  const dispatch = useAppStore(s => s.dispatchTranscriptEvent);
  const lastSeqRef = useRef<number>(-1);

  useEffect(() => {
    if (!sessionId) return;

    // 重连补流：after_seq=N 返回 seq > N 的事件（开区间，N 本身不重发）
    const url = lastSeqRef.current >= 0
      ? `${API_BASE}/v1/sessions/${sessionId}/stream?after_seq=${lastSeqRef.current}`
      : `${API_BASE}/v1/sessions/${sessionId}/stream`;

    const es = new EventSource(url);

    es.onmessage = (e) => {
      const event: TranscriptEvent = JSON.parse(e.data);
      // 去重：跳过已处理的 seq（补流可能重叠）
      if (event.seq <= lastSeqRef.current) return;
      lastSeqRef.current = event.seq;
      dispatch(event);
    };

    es.onerror = () => {
      // EventSource 自动重连，onerror 后会重新触发 useEffect
    };

    return () => es.close();
  }, [sessionId]);
}
```

### 5.3 错误处理规范

- `ApiError` 统一在 TanStack Query 的 `onError` 或 mutation `onError` 里处理
- SSE 错误写入 store，由对应组件展示（不在 SSE 层弹 toast）
- 全局 Server 断连在 `ConnectionSlice` 里管理，Banner 组件订阅此状态

---

## 6. 路由与代码分割

### 6.1 路由定义

```typescript
// app/router.tsx
import { lazy, Suspense } from 'react';
import { createBrowserRouter } from 'react-router-dom';

const ChatPage    = lazy(() => import('@/pages/chat'));
const MemoryPage  = lazy(() => import('@/pages/memory'));
const FilesPage   = lazy(() => import('@/pages/files'));
const SettingsPage = lazy(() => import('@/pages/settings'));
const AdminPage   = lazy(() => import('@/pages/admin'));

export const router = createBrowserRouter([
  {
    path: '/',
    element: <AppLayout />,  // 包含 Sidebar + 主内容区
    children: [
      { index: true, element: <Suspense><ChatPage /></Suspense> },
      { path: 'memory',   element: <Suspense><MemoryPage /></Suspense> },
      { path: 'files',    element: <Suspense><FilesPage /></Suspense> },
      { path: 'settings', element: <Suspense><SettingsPage /></Suspense> },
      {
        path: 'admin',
        element: <AdminGuard><Suspense><AdminPage /></Suspense></AdminGuard>,
      },
    ],
  },
]);
```

每个 page 是独立的 chunk，首屏只加载 Chat 模块。

### 6.2 布局组件

`AppLayout` 只负责侧边栏 + 主内容区的框架，不含业务逻辑：

```typescript
// widgets/app-layout/AppLayout.tsx
export function AppLayout() {
  return (
    <div className="flex h-screen bg-[#0D0D0D]">
      <Sidebar />
      <main className="flex-1 overflow-hidden">
        <Outlet />
      </main>
    </div>
  );
}
```

---

## 7. 国际化（i18n）规范

### 7.1 方案选型

使用 **react-i18next**（基于 i18next），支持中文（简体）和英文双语切换。

```
src/shared/i18n/
├── index.ts          # i18next 初始化
├── locales/
│   ├── zh-CN/
│   │   ├── common.json      # 通用词（按钮、状态、错误）
│   │   ├── chat.json
│   │   ├── settings.json
│   │   └── admin.json
│   └── en/
│       ├── common.json
│       ├── chat.json
│       ├── settings.json
│       └── admin.json
```

### 7.2 强制规则

**禁止硬编码中文或英文字符串到 JSX**，所有展示文本必须走 `t()` 函数：

```typescript
// 好
const { t } = useTranslation('chat');
<button>{t('send')}</button>

// 坏：硬编码
<button>发送</button>
<button>Send</button>
```

**key 命名规则**：`namespace.scope.element`，全部小写下划线：

```json
// locales/zh-CN/chat.json
{
  "input.placeholder": "输入你的问题，或粘贴代码...",
  "input.send": "发送",
  "tool_call.approve": "批准执行",
  "tool_call.reject": "拒绝",
  "tool_call.no_permission": "你没有审批「{{toolName}}」的权限",
  "session.status.running": "运行中",
  "session.status.waiting_approval": "等待审批",
  "session.status.done": "已完成",
  "session.status.error": "执行出错"
}
```

### 7.3 语言持久化

```typescript
// shared/i18n/index.ts
import i18n from 'i18next';
import { initReactI18next } from 'react-i18next';

const savedLang = localStorage.getItem('talos_lang') ?? 'zh-CN';

i18n.use(initReactI18next).init({
  lng: savedLang,
  fallbackLng: 'en',
  resources: { ... },
  interpolation: { escapeValue: false },
});

// 切换语言时同步写 localStorage
export function switchLanguage(lang: 'zh-CN' | 'en') {
  i18n.changeLanguage(lang);
  localStorage.setItem('talos_lang', lang);
}
```

### 7.4 语言切换组件

语言切换器放在侧边栏底部，复用 `shared/ui` 中的 Button：

```typescript
// widgets/sidebar/LanguageToggle.tsx
export function LanguageToggle() {
  const { i18n } = useTranslation();
  const isZh = i18n.language === 'zh-CN';

  return (
    <button
      onClick={() => switchLanguage(isZh ? 'en' : 'zh-CN')}
      className="text-text-secondary hover:text-text-primary text-xs"
      title={isZh ? 'Switch to English' : '切换中文'}
    >
      🌐 {isZh ? '中文' : 'EN'}
    </button>
  );
}
```

---

## 9. 样式规范

### 7.1 Tailwind 使用规则

- **Design token 优先**：颜色、间距使用 `tailwind.config.ts` 中定义的 token，不硬编码 hex
- **禁止内联 style**：动态值通过 CSS 变量或 `cn()` 工具函数处理
- **`cn()` 工具函数**：合并条件 class，替代字符串拼接

```typescript
// shared/lib/cn.ts
import { clsx, type ClassValue } from 'clsx';
import { twMerge } from 'tailwind-merge';

export function cn(...inputs: ClassValue[]) {
  return twMerge(clsx(inputs));
}

// 使用
<div className={cn(
  'rounded-lg border border-border p-4',
  isActive && 'bg-accent',
  isError && 'border-destructive'
)} />
```

### 7.2 Design Token 配置

```typescript
// tailwind.config.ts
export default {
  theme: {
    extend: {
      colors: {
        background: '#0D0D0D',
        sidebar:    '#111111',
        card:       '#1A1A1A',
        border:     '#2A2A2A',
        'text-primary':   '#F0F0F0',
        'text-secondary': '#888888',
        'text-muted':     '#555555',
        brand:      '#6366F1',
        success:    '#22C55E',
        warning:    '#F59E0B',
        destructive: '#EF4444',
      },
    },
  },
};
```

**禁止**在组件里直接写 `text-[#888888]`，必须用 `text-text-secondary`。

---

## 10. TypeScript 规范

### 8.1 类型定义位置

| 类型范围 | 位置 |
|---------|------|
| 跨 feature 共享的领域类型 | `shared/types/domain.ts` |
| API 请求/响应类型 | `shared/api/types.ts` |
| SSE 事件类型 | `shared/sse/types.ts` |
| Feature 内部类型 | `features/*/types.ts`（不对外暴露） |
| 组件 Props 类型 | 与组件同文件，export 出去 |

### 8.2 核心领域类型

```typescript
// shared/types/domain.ts

export interface Project {
  id: string;
  name: string;
  path: string;                 // 本地或远端文件系统路径
  created_at: number;
  last_used_at: number;
}

// 与 web-ui-design.md §12.1 统一，禁止使用 'waiting'/'complete' 等别名
export type SessionStatus = 'running' | 'waiting_approval' | 'done' | 'error';

export interface Session {
  id: string;
  project_id: string;           // 必须归属于某个 Project
  title: string | null;         // null 表示标题生成中
  status: SessionStatus;
  model: string;
  created_at: number;
  updated_at: number;
}

// ProjectRoot 三种强类型，禁止泛用 string 路径（见 web-ui-design.md §2.1）
export type ProjectRoot =
  | { type: 'managed_workspace'; name: string }
  | { type: 'server_visible_path'; path: string }
  | { type: 'desktop_local_path'; path: string };  // 仅 Electron 本地模式

export interface Project {
  id: string;
  name: string;
  root: ProjectRoot;
  created_at: number;
  last_used_at: number;
}

// TranscriptEvent — 完整定义见 web-ui-design.md §12.4.2
// 所有事件必须携带 session_id / turn_id / seq（单调递增），用于重连去重
export interface TranscriptEventBase {
  session_id: string;
  turn_id:    string;
  seq:        number;  // session 级单调序号，从 0 开始严格 +1
}

// message_committed：turn 已落盘，进入历史真源 → 触发 Query 失效
// done：本轮流结束，停止 spinner → 两者职责不得混用
export type TranscriptEventType =
  | 'output_text_delta' | 'reasoning_delta'
  | 'tool_call_started' | 'tool_call_completed' | 'tool_call_error'
  | 'approval_requested' | 'approval_resolved'
  | 'message_committed' | 'done' | 'error';

// EffectiveApprovalPolicy（server-evaluated，见 web-ui-design.md §3.1）
export interface EffectiveApprovalPolicy {
  current: 'needs_approval' | 'skip';
  available: ('needs_approval' | 'skip')[];
  locked_tools: string[];
}

export type MessageRole = 'user' | 'assistant';

export interface Message {
  id: string;
  session_id: string;
  role: MessageRole;
  content: MessageContent[];
  trigger?: 'autonomous' | 'user';  // assistant 消息的触发来源
  created_at: number;
}

export type MessageContent =
  | { type: 'text'; text: string }
  | { type: 'tool_call'; tool_call: ToolCall }
  | { type: 'thinking'; text: string };

export type ToolCallStatus = 'running' | 'done' | 'error' | 'waiting_approval' | 'no_permission';

export interface ToolCall {
  id: string;
  tool_name: string;
  args: Record<string, unknown>;
  status: ToolCallStatus;
  result?: string;
  duration_ms?: number;
}
```

### 8.3 禁止事项

- 禁止使用 `any`，用 `unknown` + 类型收窄
- 禁止 `as` 类型断言（除非处理外部 API 响应的边界层）
- 枚举用 `as const` 对象或联合类型，不用 TypeScript `enum`

```typescript
// 好
export type SessionStatus = 'running' | 'waiting_approval' | 'done' | 'error';

// 坏
enum SessionStatus { Running, WaitingApproval, Done, Error }
```

---

## 11. 测试规范

### 9.1 测试分层

| 层次 | 工具 | 覆盖目标 |
|------|------|---------|
| 单元测试 | Vitest | shared/lib 工具函数、store slice 逻辑、hook 逻辑 |
| 组件测试 | Vitest + Testing Library | UI 组件行为（交互、条件渲染） |
| 集成测试 | Playwright | 关键用户路径（发消息、工具审批、Settings 配置） |

### 9.2 组件测试规范

- 测试**行为**，不测实现（不测内部状态，测用户能看到/操作的结果）
- Mock 网络请求用 `msw`（Mock Service Worker），不 mock 组件内部函数

```typescript
// features/chat/components/__tests__/ToolCallCard.test.tsx
import { render, screen, fireEvent } from '@testing-library/react';
import { ToolCallCard } from '../ToolCallCard';

test('waiting_approval 状态展示批准和拒绝按钮', () => {
  const onApprove = vi.fn();
  render(
    <ToolCallCard
      toolName="exec_command"
      status="waiting_approval"
      args={{ command: 'rm -rf /tmp' }}
      onApprove={onApprove}
      onReject={vi.fn()}
    />
  );
  expect(screen.getByText('批准执行')).toBeInTheDocument();
  fireEvent.click(screen.getByText('批准执行'));
  expect(onApprove).toHaveBeenCalledOnce();
});

test('no_permission 状态展示权限提示和 Settings 跳转按钮', () => {
  render(
    <ToolCallCard
      toolName="exec_command"
      status="no_permission"
      args={{ command: 'rm -rf /tmp' }}
    />
  );
  expect(screen.getByText(/没有审批/)).toBeInTheDocument();
  expect(screen.getByText('前往 Settings')).toBeInTheDocument();
});
```

### 9.3 覆盖率要求

| 模块 | 最低覆盖率 |
|------|----------|
| `shared/lib` | 90% |
| `shared/api` | 80% |
| `shared/sse` | 80% |
| Feature hooks | 70% |
| UI 组件 | 关键交互路径必测，不追求行覆盖率 |

---

## 12. 质量门禁

### 10.1 提交前检查清单

- [ ] `pnpm typecheck` 无错误（`tsc --noEmit`）
- [ ] `pnpm lint` 无 ESLint 错误（不允许 `eslint-disable` 注释，除非有明确注释说明理由）
- [ ] `pnpm test` 全部通过
- [ ] 单文件不超过 200 行（组件），300 行（hook/store）
- [ ] 没有直接 `fetch`，必须走 `shared/api`
- [ ] 没有直接 `new EventSource`，必须走 `shared/sse`
- [ ] 没有跨 feature 直接 import 内部路径

### 10.2 Code Review 检查点

1. **分层是否正确**：组件有没有混入 API 调用？
2. **状态归属是否正确**：服务端状态用 Query，本地 UI 状态用 `useState`？
3. **类型是否严格**：有没有用 `any`？
4. **复用是否合理**：新写的东西是否已在 `shared` 或 `widgets` 有实现？
5. **测试是否覆盖**：新增的 hook 和关键组件有没有测试？

### 10.3 ESLint 关键规则

```json
{
  "rules": {
    "no-restricted-imports": [
      "error",
      {
        "patterns": [
          {
            "group": ["@/features/*/components/*", "@/features/*/hooks/*", "@/features/*/store*"],
            "message": "只能从 feature 的 index.ts 导入，不能访问内部路径"
          }
        ]
      }
    ],
    "@typescript-eslint/no-explicit-any": "error",
    "@typescript-eslint/no-unnecessary-type-assertion": "error"
  }
}
```

---

## 13. TypeScript SDK 依赖关系

Web UI 的 `shared/api` 和 `shared/sse` 层**不自行实现** HTTP 客户端和 SSE 原语，
而是依赖 `@talos/sdk-browser`（位于 `talos-app/ts-sdk/packages/browser/`）。

SDK 的完整设计见 [`spec/ts-sdk-design.md`](./ts-sdk-design.md)。

### 13.1 为什么 Web UI 不自己实现

| 原因 | 说明 |
|------|------|
| **避免重复** | Electron Desktop（Phase 2）复用同一套 UI API 注入点，本地 transport 由 `@talos/sdk-app-server` 提供 |
| **类型单源** | 协议类型只在 `@talos/sdk-core` 定义一次，Web UI 通过 re-export 获取 |
| **测试隔离** | SDK 层用 Mock adapter 独立测试；Web UI 的 hook 测试只 mock SDK 接口，不 mock fetch |

### 13.2 Web UI 中的使用方式

```typescript
// shared/api/client.ts
import { createBrowserTalosClient, TalosClient } from '@talos/sdk-browser';
import { getToken } from '@/shared/auth';

export const talosClient: TalosClient = createBrowserTalosClient(
  import.meta.env.VITE_API_BASE_URL,
  () => getToken(),
);
```

```typescript
// shared/sse/session-stream.ts — 只做 React hook 封装，不含 EventSource 原语
// 重连责任归属：适配器不自动重连；本 hook 在 onError 时携带最新 after_seq 显式重建连接
import { useEffect, useRef } from 'react';
import type { TranscriptEvent } from '@talos/sdk-browser';
import { talosClient } from '@/shared/api/client';
import { useAppStore } from '@/shared/store';

export function useSessionStream(sessionId: string | null) {
  const dispatch = useAppStore((s) => s.dispatchTranscriptEvent);
  // seq 是 session 级单调递增；切换 sessionId 时必须归零
  const lastSeqRef = useRef<number>(-1);

  useEffect(() => {
    if (!sessionId) return;

    lastSeqRef.current = -1;   // 切换 session → 重置 seq
    let cleanupRef: (() => void) | null = null;
    let retryTimer: ReturnType<typeof setTimeout> | null = null;
    let cancelled = false;

    function connect() {
      if (cancelled) return;
      const afterSeq = lastSeqRef.current >= 0 ? lastSeqRef.current : undefined;
      cleanupRef = talosClient.subscribeTranscript(
        sessionId!,
        (event: TranscriptEvent) => {
          if (event.seq <= lastSeqRef.current) return;   // 防御性去重
          lastSeqRef.current = event.seq;
          dispatch(event);
        },
        afterSeq,
        () => {
          // 适配器连接断开 → hook 持有最新 seq，延迟 2s 重建
          retryTimer = setTimeout(connect, 2_000);
        },
      );
    }

    connect();

    return () => {
      cancelled = true;
      if (retryTimer) clearTimeout(retryTimer);
      cleanupRef?.();
    };
  }, [sessionId]);
}
```

### 13.3 类型来源规则

| 类型来源 | 正确做法 |
|----------|---------|
| 协议类型（Session, TranscriptEvent 等）| 从 `@talos/sdk-browser` 或 `@talos/sdk-core` 导入，**禁止**在 `shared/types/` 重复定义 |
| UI 专用类型（组件 Props 等）| 在对应 feature 或 widget 内部定义 |
| 跨 feature 共享 UI 类型 | 放 `shared/types/ui.ts`，与协议类型文件分离 |

---

*最后更新：2026-04-16*
