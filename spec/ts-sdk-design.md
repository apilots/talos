# Talos TypeScript SDK 设计文档

**版本**: v0.2
**状态**: 已实现并进入 Phase 6 收口
**最后更新**: 2026-04-30

> 更新说明：早期草案里的泛名 browser SDK 与独立 Node SDK 命名已废弃。
> 当前实现采用 `@talos/sdk-core`、`@talos/sdk-browser`、`@talos/sdk-app-server`：
> `core` 承载共享内核和类型，`browser` 承载 HTTP/SSE transport，`app-server` 承载
> JSON-RPC stdio 与 Electron IPC transport adapters。

> TS SDK 是 Talos 协议层的 TypeScript 封装，目标是让 Web UI、Node.js 脚本、第三方集成
> 都能以相同方式访问 Talos Server，且与 Rust SDK 保持**协议能力对等**（而非平台实现等价）。

---

## 1. 为什么要做 TS SDK

| 价值点 | 说明 |
|--------|------|
| **Web UI 复用** | `talos-app` 的 `shared/api` 和 `shared/sse` 层不重复造轮子，直接依赖 SDK |
| **Node.js 脚本** | 自动化测试、CI 集成、数据导出脚本可直接使用 |
| **第三方集成** | 外部开发者可用 `@talos/sdk-browser` 快速对接 Talos Server，无需手写 HTTP + SSE |
| **类型契约** | TypeScript 类型与 Rust DTO（`§12.7` of `web-ui-design.md`）同源同步，防止协议漂移 |
| **测试一致性** | SDK 层的集成测试可覆盖协议契约，不依赖 UI 层 |

---

## 2. 与 Rust SDK 的能力对齐原则

**对齐的是协议能力，不是平台实现**：

| Rust SDK 有 / TS SDK 无 | 原因 |
|------------------------|------|
| Agent Kernel（LLM 调度、规划器）| Server 侧能力，TS 端无需实现 |
| Tool Registry（工具注册、执行）| Server 侧能力，TS 端无需实现 |
| Memory Engine（向量存储）| Server 侧能力，TS 端通过 API 访问 |
| LLM Provider（模型调用）| Server 侧能力 |

**协议能力对齐表（现状 vs 目标态）**：

> "Rust SDK 现状"列以 `talos-rs/apps/sdk/src/lib.rs` 实际方法为准；
> "目标态"指后端接口上线后双侧 SDK 均须跟进的能力。

| 能力域 | Rust SDK 现状 | TS SDK 现状 | 目标态 |
|--------|-------------|------------|--------|
| Session CRUD（create/list/get/patch/delete）| ✅ | 必须 ✅ | — |
| Session 操作（cancel / compact / clear）| ✅ | 必须 ✅ | — |
| 消息提交（async ack）| ✅ `send_message` | 必须 ✅ | — |
| Session Events SSE（持久化，`after_event_id`）| ✅ `subscribe_session_events` / `send_message_events` | 必须 ✅ | — |
| 工具审批提交 | ✅ `submit_tool_approval` | 必须 ✅ | — |
| 模型列表 | ✅ `list_models` | 必须 ✅ | — |
| 动态工具结果提交 | ❌ 未实现 | ❌ 未实现 | 后端上线后同批跟进 |
| Transcript SSE 流（持久化，`after_event_id`）| ✅ 已由 session events 覆盖 | 必须 ✅ | — |
| 全局 SSE 流（`/v1/events`）| ❌ 未实现 | ❌ 未实现 | 后端上线后同批跟进 |
| EffectiveApprovalPolicy 查询 + 切换 | ❌ 未实现 | ❌ 未实现 | 后端上线后同批跟进 |
| Project CRUD | ✅ app-server client 已实现；HTTP client 保持既有能力边界 | ✅ app-server client 已实现 | 已纳入 protocol e2e resource matrix |
| Memory file / Workspace docs | ✅ HTTP + app-server client 已实现 | ✅ browser/app-server adapter 已实现 | 已纳入 SDK 与 e2e 回归 |

> **同步原则**：后端新增 endpoint 时，Rust SDK 和 TS SDK 必须同批次跟进，
> 不允许 TS SDK 长期落后于 Rust SDK 的协议版本。

---

## 3. 包位置与目录结构

### 3.1 仓库顶层结构

`talos-app/` 与 `talos-rs/` 平级，共同位于仓库根目录下：

```
talos/                         ← 仓库根
├── talos-rs/                  ← Rust 后端 monorepo
└── talos-app/                 ← TypeScript/JS 应用 monorepo
    ├── ts-sdk/                ← 共享 TS SDK（web + desktop 共用）
    ├── web/                   ← Web UI（React + Vite，Phase 1 MVP）
    └── desktop/               ← Electron 桌面端（Phase 2）
```

| 选项 | 原因 |
|------|------|
| ❌ `talos-rs/ts-sdk/` | Rust monorepo，职责混乱；构建工具链不一致 |
| ❌ `talos-app/web/src/sdk/` | SDK 是跨应用共享包，不应内嵌在单个应用源码里 |
| ✅ `talos-app/ts-sdk/` | Web UI + Electron Desktop 均可作为 workspace 依赖引用；CI 共享；类型同步方便 |

### 3.2 目录结构

```
talos-app/
├── pnpm-workspace.yaml        # 声明 packages: [ts-sdk/packages/*, web, desktop]
├── ts-sdk/                    # TypeScript SDK
│   ├── packages/
│   │   ├── core/              # @talos/sdk-core — 纯 TS，无运行时假设
│   │   ├── app-server/        # @talos/sdk-app-server — JSON-RPC stdio 与 Electron IPC 适配
│   │   └── browser/           # @talos/sdk-browser — HTTP/SSE 浏览器适配
│   ├── tsconfig.base.json
│   └── README.md
├── web/                       # Web UI 应用
│   └── package.json           # dependencies: { "@talos/sdk-browser": "workspace:*" }
└── desktop/                   # Electron 桌面端（Phase 2）
    └── package.json           # dependencies: { "@talos/sdk-app-server": "workspace:*" }
```

> `talos-app/pnpm-workspace.yaml` 统一管理 workspaces，`web/` 与 `desktop/` 均通过
> `workspace:*` 协议引用本地 SDK 包，构建时直接取源码，无需发布到 npm。
>
> **Electron 注意**：主进程依赖 `@talos/sdk-app-server/desktop-main` 管理 app-server
> lifecycle；渲染进程通过 preload 暴露的 typed bridge 使用
> `@talos/sdk-app-server/desktop`，不直接接触 Node/Electron API。

---

## 4. 分层架构：core / node / browser

```
┌──────────────────────────────────────────────────┐
│              @talos/sdk-browser                │
│  implements: BrowserHttpAdapter, BrowserStreamAdapter│
│  re-exports: everything from @talos/sdk-core      │
└──────────────────┬───────────────────────────────┘
                   │ extends
┌──────────────────▼───────────────────────────────┐
│              @talos/sdk-app-server                      │
│  implements: stdio transport + Electron IPC bridge │
│  re-exports: everything from @talos/sdk-core      │
└──────────────────┬───────────────────────────────┘
                   │ extends
┌──────────────────▼───────────────────────────────┐
│              @talos/sdk-core                      │
│  types:    全部协议类型（镜像 §12.6 / §12.7）     │
│  abstract: HttpAdapter, StreamAdapter 接口        │
│  class:    TalosClient（依赖注入 Adapters）        │
│  pure TS，无 fetch / EventSource / RequestInit    │
└──────────────────────────────────────────────────┘
```

### 4.0 SSE 认证方案（关键决策）

**问题**：浏览器原生 `EventSource` 不支持自定义请求头，无法注入 `Authorization: Bearer xxx`。
现有 gateway 对所有 `/v1/*` 路由统一执行 Bearer 鉴权中间件（`server/mod.rs:163`），
在 `auth_disabled=false` 的真实环境下，裸 `EventSource(url)` 必然收到 401。

**选定方案：fetch-based streaming（适配器内部实现，对 core 透明）**

| 方案 | 优点 | 缺点 | 决策 |
|------|------|------|------|
| query token `?token=xxx` | 简单 | token 暴露在 URL、日志、referer 头，有安全风险 | ❌ 不选 |
| Cookie session | 无需代码改动 | 需要额外的 cookie auth 流程，与现有 Bearer JWT 并存复杂 | ❌ 不选 |
| ticket 换票（一次性短 token）| token 不在主 URL 中 | 需后端新增换票接口，增加一次 RTT | ❌ 复杂度过高 |
| **fetch-based SSE** | 支持任意请求头；与 HTTP 层使用同一 token 机制 | 需要手动解析 SSE 帧（使用 `@microsoft/fetch-event-source` 库） | ✅ **选定** |

**实现要点**：
- `@talos/sdk-core` 的 `StreamAdapter` 接口与认证无关（核心层保持纯净）
- 认证 token 由适配器构造时注入（同 `HttpAdapter`），通过 `getToken()` 回调懒获取
- Browser 适配器使用 `@microsoft/fetch-event-source`（支持 headers；**禁用内部自动重连**，onerror 抛异常阻断）
- app-server 适配器使用 JSON-RPC over stdio JSONL；Electron 仅通过 typed IPC bridge 暴露能力。
- 两侧适配器对 `TalosClient` 完全透明；`subscribeTranscript` / `subscribeGlobalEvents` 签名不变

---

### 4.1 `@talos/sdk-core` — 协议核心

**职责**：类型定义 + `TalosClient` 主类 + 抽象适配器接口。

**关键约束**：core 不引入任何运行时类型（`RequestInit`、`Event`、`fetch`、`EventSource`），
使用自定义最小传输类型替代，确保在 Node-only 环境或无 DOM lib 配置下均可编译。

```typescript
// packages/core/src/adapters.ts

// 自定义最小 HTTP 选项，不依赖 DOM RequestInit
export interface HttpRequestOptions {
  method?: 'GET' | 'POST' | 'PATCH' | 'DELETE' | 'PUT';
  body?: string;
  headers?: Record<string, string>;
}

export interface HttpAdapter {
  // 接收完整 URL（baseUrl + path 由 TalosClient 组装后传入）
  request<T>(url: string, options?: HttpRequestOptions): Promise<T>;
}

// 自定义最小 SSE 错误信息，不依赖 DOM Event 类型
export interface StreamErrorInfo {
  message?: string;
}

export interface StreamAdapter {
  /**
   * 打开 SSE 连接，返回清理函数。
   * - url: 完整 URL（含 query params），由 TalosClient 组装后传入
   * - onMessage: 每条事件的原始 JSON 字符串
   * - onError: 可选，连接错误回调（不要求重连，由适配器内部处理）
   * 认证头由适配器构造时注入，此接口层不感知 token。
   */
  open(
    url: string,
    onMessage: (data: string) => void,
    onError?: (err: StreamErrorInfo) => void,
  ): () => void;  // 返回清理函数
}
```

```typescript
// packages/core/src/client.ts

export class TalosClient {
  constructor(
    private readonly http: HttpAdapter,
    private readonly stream: StreamAdapter,
    private readonly baseUrl: string,  // 必填，不默认空串，防止相对路径 bug
  ) {}

  // ── URL 组装收口（HTTP 和 SSE 共用同一逻辑）────────────────────
  private url(path: string): string {
    // 去掉 baseUrl 尾部斜杠，统一拼接
    return `${this.baseUrl.replace(/\/$/, '')}${path}`;
  }

  private urlWithQuery(path: string, query: Record<string, string | number | undefined>): string {
    const params = Object.entries(query)
      .filter(([, v]) => v !== undefined)
      .map(([k, v]) => `${k}=${encodeURIComponent(String(v))}`)
      .join('&');
    return params ? `${this.url(path)}?${params}` : this.url(path);
  }

  // ── Sessions ──────────────────────────────────────────
  sessions = {
    list: (query?: SessionQuery) =>
      this.http.request<SessionRecord[]>(
        this.urlWithQuery('/v1/sessions', query ?? {}),
      ),
    create: (body: CreateSessionBody) =>
      this.http.request<SessionRecord>(this.url('/v1/sessions'), {
        method: 'POST', body: JSON.stringify(body),
      }),
    get: (id: string) =>
      this.http.request<SessionRecord>(this.url(`/v1/sessions/${id}`)),
    patch: (id: string, body: PatchSessionBody) =>
      this.http.request<void>(this.url(`/v1/sessions/${id}`), {
        method: 'PATCH', body: JSON.stringify(body),
      }),
    delete: (id: string) =>
      this.http.request<void>(this.url(`/v1/sessions/${id}`), { method: 'DELETE' }),
    cancel: (id: string) =>
      this.http.request<void>(this.url(`/v1/sessions/${id}/cancel`), { method: 'POST' }),
    compact: (id: string) =>
      this.http.request<SessionRecord>(this.url(`/v1/sessions/${id}/compact`), { method: 'POST' }),
    clear: (id: string) =>
      this.http.request<SessionRecord>(this.url(`/v1/sessions/${id}/clear`), { method: 'POST' }),
    turns: (id: string) =>
      this.http.request<TurnRecord[]>(this.url(`/v1/sessions/${id}/turns`)),
    effectivePolicy: (id: string) =>
      this.http.request<EffectiveApprovalPolicy>(this.url(`/v1/sessions/${id}/effective-policy`)),
    setApprovalPolicy: (id: string, policy: ApprovalPolicyOption) =>
      this.http.request<void>(this.url(`/v1/sessions/${id}/approval-policy`), {
        method: 'PATCH', body: JSON.stringify({ policy }),
      }),
  };

  // ── Messages ──────────────────────────────────────────
  messages = {
    send: (sessionId: string, body: SendMessageBody) =>
      this.http.request<TalosMessageResponse>(
        this.url(`/v1/sessions/${sessionId}/messages`),
        { method: 'POST', body: JSON.stringify({ ...body, stream: false }) },
      ),
  };

  // ── Tool interactions ─────────────────────────────────
  toolApprovals = {
    submit: (sessionId: string, body: ToolApprovalBody) =>
      this.http.request<void>(
        this.url(`/v1/sessions/${sessionId}/tool-approvals`),
        { method: 'POST', body: JSON.stringify(body) },
      ),
  };

  dynamicToolResults = {
    submit: (sessionId: string, body: DynamicToolResultBody) =>
      this.http.request<void>(
        this.url(`/v1/sessions/${sessionId}/dynamic-tool-results`),
        { method: 'POST', body: JSON.stringify(body) },
      ),
  };

  // ── Models ────────────────────────────────────────────
  models = {
    list: () => this.http.request<ModelInfo[]>(this.url('/v1/models')),
  };

  // ── SSE streams ───────────────────────────────────────

  /**
   * 全局事件流：无 seq，幂等处理，用于侧边栏状态同步。
   * 认证由 StreamAdapter 内部注入，调用方无需传 token。
   * 返回清理函数。
   */
  subscribeGlobalEvents(
    handler: (event: GlobalEvent) => void,
    onError?: (err: StreamErrorInfo) => void,
  ): () => void {
    return this.sse.open(
      this.url('/v1/events'),
      (data) => handler(JSON.parse(data) as GlobalEvent),
      onError,
    );
  }

  /**
   * 单会话 Transcript 流。
   * - afterSeq: 从 seq > afterSeq 开始补流（断线重连专用）
   *   语义：返回 seq > afterSeq 的事件（开区间）
   * 认证由 StreamAdapter 内部注入。
   * 返回清理函数。
   */
  subscribeTranscript(
    sessionId: string,
    handler: (event: TranscriptEvent) => void,
    afterSeq?: number,
    onError?: (err: StreamErrorInfo) => void,
  ): () => void {
    const url = this.urlWithQuery(
      `/v1/sessions/${sessionId}/stream`,
      { after_seq: afterSeq },
    );
    return this.sse.open(url, (data) => handler(JSON.parse(data) as TranscriptEvent), onError);
  }
}
```

### 4.2 `@talos/sdk-app-server` — 本地 app-server 适配

```typescript
// packages/app-server/src/stdio.ts
import { AppServerClient } from "@talos/sdk-core";

export class StdioAppServerClient extends AppServerClient {
  // Starts `talos app-server` and speaks JSON-RPC over stdio JSONL.
}
```

当前实现中共享内核类位于 `@talos/sdk-core`，包名保持：

- `@talos/sdk-core`: shared domain types, generated protocol, `TalosClient`, `AppServerClient`
- `@talos/sdk-browser`: `createBrowserTalosClient({ baseUrl, getToken })`
- `@talos/sdk-app-server`: stdio transport, Desktop main controller, Desktop renderer bridge

### 4.3 `@talos/sdk-browser`（browser） — 浏览器适配

**注意**：浏览器原生 `EventSource` 不支持自定义请求头，因此 SSE 适配器使用
`@microsoft/fetch-event-source`，它基于 `fetch()` 实现 SSE 解析，支持 `Authorization` 头。
内部自动重连**已禁用**（`onerror` 抛异常阻断重试），重连责任统一由调用方（hook）持有，
调用方携带最新 `after_seq` 重建连接，确保不回放已处理的历史事件。

```typescript
// packages/browser/src/index.ts
import { fetchEventSource } from '@microsoft/fetch-event-source';
import { TalosClient, HttpAdapter, HttpRequestOptions, StreamAdapter, StreamErrorInfo, TalosHttpError } from '@talos/sdk-core';

class BrowserHttpAdapter implements HttpAdapter {
  constructor(private readonly getToken: () => string) {}

  async request<T>(url: string, options: HttpRequestOptions = {}): Promise<T> {
    const res = await fetch(url, {
      method: options.method ?? 'GET',
      body: options.body,
      headers: {
        'Content-Type': 'application/json',
        'Authorization': `Bearer ${this.getToken()}`,
        ...options.headers,
      },
    });
    if (!res.ok) throw new TalosHttpError({ status: res.status, statusText: res.statusText, body: await res.text(), url, method: options.method ?? 'GET' });
    if (res.status === 204) return undefined as T;
    return res.json() as Promise<T>;
  }
}

class BrowserStreamAdapter implements StreamAdapter {
  constructor(private readonly getToken: () => string) {}

  open(
    url: string,
    onMessage: (data: string) => void,
    onError?: (err: StreamErrorInfo) => void,
  ): () => void {
    // AbortController 用于清理连接（等价于 EventSource.close()）
    const controller = new AbortController();

    fetchEventSource(url, {
      headers: { Authorization: `Bearer ${this.getToken()}` },
      signal: controller.signal,
      onmessage(ev) { onMessage(ev.data); },
      onerror(err) {
        onError?.({ message: String(err) });
        // 必须 throw：阻止 fetchEventSource 内部自动重连。
        // 重连责任归调用方（hook），由调用方携带最新 after_seq 重建连接，
        // 确保每次重连都从正确的 seq 断点恢复，不回放不必要的历史事件。
        throw err;
      },
    });

    return () => controller.abort();
  }
}

export function createBrowserTalosClient(options: BrowserClientOptions): TalosClient {
  return new TalosClient(
    new BrowserHttpAdapter(options.getToken),
    new BrowserStreamAdapter(options.getToken),
    options.baseUrl,
  );
}

export * from '@talos/sdk-core';
```

---

## 5. Session-first 核心能力（已实现基线）

当前 SDK 基线覆盖 session-first 与 app-server resource parity：

```
Session CRUD       create / list / get / patch / delete
Turn 管理          cancel / compact / clear / turns
消息发送           send（同步）
流式订阅           subscribeTranscript / subscribeGlobalEvents
工具交互           toolApprovals.submit；dynamicToolResults.submit 暂缓 freeze
审批策略           effectivePolicy / setApprovalPolicy
模型列表           models.list
Project            create / list / read / patch / delete（app-server protocol）
Memory/Docs        memory file 与 workspace docs resource matrix
```

**仍未 freeze 的能力**：
- `tool.dynamic_result.submit`
- `admin.session.list`
- `admin.project.list`
- 全局 `/v1/events`

---

## 6. Web UI 集成方式

Web UI (`talos-app/web/`) 不自行维护 HTTP + SSE 底层，直接使用 `@talos/sdk-browser`：

```typescript
// talos-app/web/src/shared/api/client.ts
import { createBrowserTalosClient } from '@talos/sdk-browser';
import { getToken } from '@/shared/auth';

export const talosClient = createBrowserTalosClient({
  baseUrl: import.meta.env.VITE_API_BASE_URL,
  getToken: () => getToken(),
});
```

```typescript
// talos-app/web/src/shared/sse/session-stream.ts
// shared/sse 层变成 SDK 流接口的 React hook 包装，不再包含 EventSource 原语
import { useEffect, useRef } from 'react';
import type { TranscriptEvent } from '@talos/sdk-browser';
import { talosClient } from '@/shared/api/client';
import { useAppStore } from '@/shared/store';

// 重连责任归属：
// - 适配器（BrowserStreamAdapter）不自动重连（onerror 抛异常阻断 fetchEventSource 内部重试）
// - hook 在 onError 回调里持有最新 lastSeqRef，延迟后重建连接并传 after_seq
// - 每次重建时 after_seq = lastSeqRef.current，服务端只返回 seq > after_seq 的事件，
//   不会回放已处理的历史，无需 seq 去重兜底（但去重保留作防御）
export function useSessionStream(sessionId: string | null) {
  const dispatch = useAppStore((s) => s.dispatchTranscriptEvent);
  // seq 是 session 级单调递增；切换 sessionId 时必须归零
  const lastSeqRef = useRef<number>(-1);

  useEffect(() => {
    if (!sessionId) return;

    // 切换 session → 重置 seq（不同 session 的 seq 互相独立）
    lastSeqRef.current = -1;

    let cleanupRef: (() => void) | null = null;
    let retryTimer: ReturnType<typeof setTimeout> | null = null;
    let cancelled = false;

    function connect() {
      if (cancelled) return;
      // 重连时取最新 seq：首次 undefined（全量），断线后 = lastSeqRef.current
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
          // 适配器连接断开（onerror 已阻断内部重试）→ hook 负责重连
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

**关系说明**：

| 层 | 职责 | 依赖 |
|----|------|------|
| `@talos/sdk-core` | 类型定义 + 抽象接口 + `TalosClient` | 无运行时依赖；无 DOM 类型 |
| `@talos/sdk-browser`（browser）| 浏览器适配（fetch + @microsoft/fetch-event-source）| `@talos/sdk-core` |
| `web/shared/api/client.ts` | 初始化 `TalosClient`，注入 token | `@talos/sdk-browser` |
| `web/shared/sse/*.ts` | React hook 包装 | `@talos/sdk-browser` + client |
| Features / Widgets | 业务逻辑 | `shared/api` hooks / `shared/sse` hooks |

---

## 7. 类型与 Rust DTO 同步策略

**单一真源**：`spec/web-ui-design.md §12.6` 中的 TypeScript 类型和 `§12.7` 中的 Rust DTO 是
协议契约的规范文本，`@talos/sdk-core/src/types/` 是 TypeScript 侧的实现落地。

**同步规则**：

1. 后端新增/修改 `TalosEvent` 或 Response DTO → 先更新 spec `§12.6 / §12.7`
2. 再同步修改 `@talos/sdk-core/src/types/`（对应 TypeScript 类型）
3. 后更新 `talos-rs/crates/common/types/`（对应 Rust 类型）
4. Web UI 的 `shared/types/` 直接从 `@talos/sdk-browser` 重导出，不维护独立副本

**禁止**：在 `web/shared/types/` 中自行定义已在 `@talos/sdk-core` 中存在的协议类型。

---

## 8. 演进路线

```
Phase 1（MVP）
  ├── @talos/sdk-core：types + TalosClient + 抽象接口
  ├── @talos/sdk-browser：BrowserHttpAdapter + BrowserStreamAdapter
  └── web/shared/api + shared/sse 切换到 SDK

Phase 2（本地 app-server 支持，已完成）
  ├── @talos/sdk-app-server：StdioAppServerClient + Electron IPC bridge
  └── CLI / TUI / Desktop 本地模式统一走 JSON-RPC app-server

Phase 3（Project / Memory / Files，已完成 app-server resource parity）
  └── protocol/resource matrix 覆盖 project、memory file、workspace docs

Phase 4（可选：公开发布）
  └── npm publish @talos/sdk-browser / @talos/sdk-app-server
      维护 CHANGELOG，语义化版本
```

---

## 9. 质量要求

### 9.1 测试策略

```
@talos/sdk-core
  纯 TS，100% 可 Vitest 覆盖
  用 Mock HttpAdapter + StreamAdapter 测试 TalosClient 方法

@talos/sdk-app-server / @talos/sdk-browser
  适配器实现测试：JSDOM/Node + MSW 模拟 HTTP，fake-sse 模拟 SSE
  e2e：对接真实 talos-server（test env）跑 session 生命周期

Web UI 集成
  shared/sse 的 React hook 用 renderHook + fake SSE server 测试去重逻辑
```

### 9.2 构建产物要求

| 目标 | 格式 | 备注 |
|------|------|------|
| `@talos/sdk-core` | ESM only | 无 CJS（tree-shaking 友好）|
| `@talos/sdk-browser`（browser）| ESM only | 浏览器 target，不包含 Node 专有 API |
| `@talos/sdk-app-server` | ESM only | Node/Electron app-server transport adapter |

### 9.3 禁止项

- `@talos/sdk-core` 中**禁止**引用 `RequestInit`、`Event`、`fetch`、`EventSource` 等 DOM / 运行时类型；使用 `HttpRequestOptions`、`StreamErrorInfo` 代替
- `@talos/sdk-browser`（browser）中**禁止** `import` Node.js built-in（`fs`、`path`、`net` 等）
- `@talos/sdk-browser`（browser）中**禁止**使用原生 `new EventSource(url)`（不支持 Authorization 头）；统一使用 `fetchEventSource`
- **禁止**在 `web/` 中直接 `new EventSource`、裸 `fetch`，或绕过 `talosClient` 自行拼接请求 URL

---
