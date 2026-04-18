# Network And Web Tools Alignment

## 1. 范围

本篇覆盖：

- `WebFetch`
- `WebSearch`
- 与之相关的真实网络执行层

---

## 2. 现状判断

Talos 当前网络工具属于直接重写对象：

- `web_search` 返回 mock 文本
- `web_fetch` 返回 mock HTML
- `http_request` 返回 mock response

这些实现不具备生产价值，因此不建议做兼容式渐进改造，建议直接按目标契约重写。

---

## 3. 当前源码基线

本篇基于本轮对以下源码的对照探索：

- Claude Code:
  - `src/tools/WebFetchTool/WebFetchTool.ts`
  - `src/tools/WebSearchTool/WebSearchTool.ts`
- Talos:
  - `crates/action/tools/src/tools/network.rs`

### 3.1 Claude Code 当前现状

`WebFetch` 当前已经具备完整工具契约能力：

- `url + prompt` 输入模型
- deferred tool
- read-only / concurrency-safe 声明
- URL validation
- domain-based permission decision
- preapproved host
- redirect 跨 host 提示
- 页面抓取后 markdown 化
- 结果结构化输出与大结果持久化阈值

`WebSearch` 当前也不是普通 HTTP 搜索封装，而是：

- deferred tool
- provider/model capability gating
- `query/allowed_domains/blocked_domains`
- validate input
- search provider 结果结构化输出
- result 与 citation/source url 语义配套

### 3.2 Talos 当前现状

Talos 当前 `web_search/web_fetch/http_request` 都仍是 mock 或近似 mock：

- `web_search` 只拼接 mock 文本
- `web_fetch` 只返回 mock HTML
- `http_request` 只返回 mock response 摘要
- 工具层没有 provider、permission、redirect、markdown 化、citation、deferred、result policy

### 3.3 本篇方案落点

因此网络域不是在现有工具上做小修小补，而是：

- 保留 Talos 工具注册与治理骨架
- 直接重写网络 runtime 与工具契约
- 对齐 Claude Code 的 `WebFetch/WebSearch`
- 将 `http_request` 从对齐目标集中移除

---

## 4. `WebFetch`

### 3.1 目标定位

`WebFetch` 不是“发 HTTP 请求的通用工具”，而是：

- 获取页面内容
- 转成 markdown / 可读文本
- 按 prompt 做抽取或总结
- 受 host/domain 权限规则约束

### 3.2 目标输入契约

```json
{
  "url": "https://example.com/page",
  "prompt": "Summarize the API authentication requirements"
}
```

### 3.3 关键能力

- URL 校验
- domain-based permission
- preapproved host 机制
- 重定向处理
- 页面转 markdown
- 对认证/private URL 给出工具选择提醒

---

## 5. `WebSearch`

### 4.1 目标定位

`WebSearch` 是时效性信息搜索工具，不是普通搜索结果模拟器。

### 4.2 目标输入契约

```json
{
  "query": "latest Rust edition stabilization status",
  "allowed_domains": ["blog.rust-lang.org"],
  "blocked_domains": ["spam.example"]
}
```

### 4.3 关键能力

- 真正调用 web search provider
- 结果包含 hits 与文字摘要
- 权限与提示独立于 `WebFetch`
- 结果要支持 citations / source urls

---

## 6. `http_request` 处理决策

该工具当前明确不保留。

已确认方向：

- Claude Code 对齐范围只包含 `WebFetch` + `WebSearch`
- 当前 mock `http_request` 直接从模型公开工具集中移除
- 后续若真的需要 API 调试能力，再单独设计 Talos 扩展工具，但不占用本轮对齐语义位

---

## 7. Talos 模块建议

| 模块 | 职责 |
|---|---|
| `tools/web_fetch` | 对外 `WebFetch` 契约 |
| `tools/web_search` | 对外 `WebSearch` 契约 |
| `runtime/web_permissions` | host/domain 规则 |
| `runtime/web_fetcher` | 页面抓取与 markdown 化 |
| `runtime/web_search_provider` | search provider 抽象 |

---

## 8. 验收标准

- Talos 不再存在生产态 mock 网络工具
- `WebFetch` 与 `WebSearch` 的模型可见契约对齐 Claude Code
- `http_request` 不再作为模型公开工具存在
