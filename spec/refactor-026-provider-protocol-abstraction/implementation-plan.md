# Provider Protocol Abstraction Implementation Plan

## 阶段 0: Spec Review Gate
**目标**: 完成 provider/protocol 抽象方案评审，冻结第一阶段实现边界。  
**成功标准**:
- `spec/refactor-026-provider-protocol-abstraction/design.md` 已完成 Review。
- 明确第一阶段不修改 `llm.provider` / `llm.model` 字段名。
- 明确第一阶段不切 OpenAI 默认协议到 `responses`。
- 明确 OpenRouter 首版作为一等 provider 使用 `protocol: "chat"`。
**测试**:
- 文档自检：确认无 `active_provider` / `active_model` 等新字段名。
- 文档自检：确认阶段计划与 spec 中默认协议策略一致。
**状态**: 已完成

## 阶段 1: Protocol Config Field
**目标**: 在现有固定 provider 配置结构上新增 `protocol` 字段，并完成后端/SDK/UI 配置链路。  
**成功标准**:
- `ProviderConfig` 支持 `protocol?: "chat" | "responses" | "messages"`。
- 未配置 `protocol` 时按 provider 默认值解析。
- `anthropic` 默认且仅允许 `messages`。
- `openai` 默认保持当前 `chat` 行为，可配置 `responses`。
- `bailian` / `ollama` 默认且仅允许 `chat`。
- Settings Models 页面能展示/保存 protocol，单协议 provider 隐藏或只读。
- HTTP、JSON-RPC、Desktop、TS SDK 使用同一配置结构。
**测试**:
- `cargo test -p talos-config`
- `cargo test -p talos-types`
- `cargo test -p talos-gateway-http`
- `pnpm --dir talos-app/ts-sdk test`
- `pnpm --dir talos-app/web test -- SettingsModelsEditor`
- `pnpm --dir talos-app/web typecheck`
**状态**: 已完成

## 阶段 2: OpenRouter First-Class Provider
**目标**: 在不重构协议实现的前提下，先让 OpenRouter 作为一等 provider 可配置、可选择、可运行。  
**成功标准**:
- `LlmConfig` 增加 `openrouter: ProviderConfig`。
- `YamlLlmConfigStore` 支持 `TALOS_LLM_OPENROUTER_API_KEY` 的保存、脱敏、读取状态。
- `model_readiness_for_llm` 支持 `provider: "openrouter"`。
- `resolve_model_catalog` 支持 OpenRouter models。
- `create_llm_provider` 对 OpenRouter 复用现有 OpenAI Chat Completions provider，默认 base URL 为 `https://openrouter.ai/api/v1`。
- Settings Models 页面新增 OpenRouter tab。
- 不引入 OpenRouter plugins/routing/preferences 等扩展能力。
**测试**:
- `cargo test -p talos-config`
- `cargo test -p talos-types`
- `cargo test -p talos-base`
- `pnpm --dir talos-app/ts-sdk test`
- `pnpm --dir talos-app/web test -- SettingsModelsEditor`
- 手工或 e2e smoke：使用 mock server 验证 OpenRouter provider 请求路径为 `/chat/completions`。
**状态**: 已完成

## 阶段 3: Chat Protocol Base Extraction
**目标**: 从现有 OpenAI Chat Completions provider 中抽出可复用 `chat` protocol base，让 OpenAI/Bailian/OpenRouter/Ollama 共享通用逻辑。  
**成功标准**:
- `chat` protocol base 负责 request shape、tools、tool calls、SSE decode、usage include、model block stream。
- OpenAI dialect 只保留 default base URL、headers、`reasoning_effort`、usage/error 差异。
- Bailian dialect 只保留 default base URL、`enable_thinking`、`thinking_budget`、reasoning delta 字段、cache control、cache usage 差异。
- OpenRouter 首版通过 OpenAI-compatible identity 复用 `chat` protocol base，保留独立 provider name、default model 与 stream item prefix。
- 旧 OpenAI/Bailian 行为测试保持通过。
- 不复制第二套 Chat Completions parser。
**测试**:
- `cargo test -p talos-llm-openai`
- `cargo test -p talos-llm-bailian`
- 新增 chat protocol serialization tests。
- 新增 chat SSE split-line tests。
- 新增 tool call roundtrip tests。
- 新增 Bailian reasoning/cache usage dialect tests。
**状态**: 已完成

## 阶段 4: Anthropic Messages Base Extraction
**目标**: 将 Anthropic Messages-compatible 通用协议逻辑与 Anthropic provider 差异分离。  
**成功标准**:
- `messages` protocol base 负责 `/messages` request、system、content block、tool_use/tool_result、thinking、cache_control、stream event decode。
- Anthropic dialect 只保留 headers、version、default base URL、usage/error 差异。
- 原厂 Anthropic 行为不变。
- 后续 Anthropic-compatible provider 可通过 dialect 接入。
**测试**:
- `cargo test -p talos-llm-anthropic`
- Messages request serialization tests。
- Content block/cache control tests。
- Tool use/tool result stream tests。
- Thinking stream tests。
**状态**: 已完成

## 阶段 5: OpenAI Responses Optional Protocol
**目标**: 新增 `responses` protocol adapter，并作为 OpenAI provider 可选协议接入。  
**成功标准**:
- OpenAI provider 可配置 `protocol: "responses"`。
- `responses` adapter 独立处理 `/responses` request/response/stream，不复用 Chat Completions delta parser。
- Responses output item / reasoning item / function call item 能映射到 Talos model block。
- 默认 OpenAI 行为仍不自动切到 `responses`。
**测试**:
- OpenAI Responses request serialization tests：已覆盖。
- Responses item lifecycle stream tests：已覆盖。
- Responses function call item tests：已覆盖。
- Responses reasoning item tests：已覆盖。
- `cargo test -p talos-llm-provider`：通过。
- `cargo test -p talos-llm-openai`：通过。
- `cargo test -p talos-base`：通过。
- Bailian Responses 兼容 smoke：通过。使用 `TALOS_LLM_OPENAI_API_KEY`、`base_url=https://dashscope.aliyuncs.com/compatible-mode/v1`、`protocol=responses`、`model=qwen3.7-plus`，真实返回 `content=OK`。
**状态**: 已完成

## 阶段 6: Runtime Factory Cleanup
**目标**: 清理 provider factory 和重复实现，使新增 provider 只需要 dialect + 配置默认值。  
**成功标准**:
- `create_llm_provider` 基于 provider + effective protocol 选择 protocol adapter 和 dialect。
- 不再为每个 OpenAI-compatible provider 手写完整 HTTP provider。
- Provider readiness、model catalog、Settings、SDK schema 对 provider/protocol 一致。
- 无未关联 TODO，无兼容残留。
**测试**:
- `cargo test --workspace`：通过。
- `pnpm --dir talos-app/ts-sdk test`：通过。
- `pnpm --dir talos-app/web test`：通过。
- `pnpm --dir talos-app/web typecheck`：通过。
- LLM 相关 e2e：通过。执行范围：
  - `talos-e2e/run_e2e.sh --case 02_session_lifecycle`
  - `talos-e2e/run_e2e.sh --case 03_messages_http_sse`
  - `talos-e2e/run_e2e.sh --group 05_cli`
  - `talos-e2e/run_e2e.sh --group 06_tools`
  - `talos-e2e/run_e2e.sh --group 07_memory`
  - `talos-e2e/run_e2e.sh --group 09_app_server`
- OpenAI provider + Bailian Responses runtime e2e：通过。临时配置 `llm.provider=openai`、`llm.openai.protocol=responses`、`llm.openai.base_url=https://dashscope.aliyuncs.com/compatible-mode/v1`、`llm.model=qwen3.7-plus`，执行 `talos-e2e/run_e2e.sh --case 03_messages_http_sse`。
- LLM e2e 报告：
  - `talos-e2e/reports/e2e_report_20260615_221412.md`
  - `talos-e2e/reports/e2e_report_20260615_221415.md`
  - `talos-e2e/reports/e2e_report_20260615_221429.md`
  - `talos-e2e/reports/e2e_report_20260615_221437.md`
  - `talos-e2e/reports/e2e_report_20260615_221657.md`
  - `talos-e2e/reports/e2e_report_20260615_221728.md`
  - `talos-e2e/reports/e2e_report_20260615_222454.md`
  - `talos-e2e/reports/e2e_report_20260615_223500.md`
**状态**: 已完成

## 阶段 7: Model Reasoning And Speed Defaults
**目标**: 将模型级 reasoning effort 与 speed 默认值纳入配置、SDK、Settings、kernel 请求链路和 provider 协议映射。  
**成功标准**:
- `ProviderModelConfig` 使用简单字段表达能力：`reasoning: bool`、`reasoning_effort: "low" | "medium" | "high" | "xhigh"`、`speed: "standard" | "fast"`。
- 未配置 `reasoning_effort` 时默认为 `medium`，未配置 `speed` 时默认为 `standard`。
- 不引入 `supported`、`modes`、`default_effort`、`default` 等能力矩阵字段；不支持的 provider/protocol 直接忽略。
- Kernel 生成 `ChatRequest` 时使用选中模型的默认 `reasoning_effort` 和 `speed`，不再写死 `Medium + 2048`。
- `ReasoningEffort::XHigh` 可贯穿到 `chat` / `responses` / `messages` 协议层。
- Anthropic-compatible Messages 与 Bailian Chat 通过 provider adapter 将 Talos effort 映射为 thinking budget；Responses 通过 `reasoning.effort` 表达。
- Settings Models 能编辑模型默认 reasoning effort 与 speed。
**测试**:
- `cargo test -p talos-config`
- `cargo test -p talos-types`
- `cargo test -p talos-llm-provider`
- `cargo test -p talos-llm-openai`
- `cargo test -p talos-llm-bailian`
- `cargo test -p talos-llm-anthropic`
- `cargo test -p talos-agent-kernel`
- `pnpm --dir talos-app/ts-sdk test`
- `pnpm --dir talos-app/web test -- SettingsModelsEditor`
- `pnpm --dir talos-app/web typecheck`
- `talos-e2e/run_e2e.sh --case 03_messages_http_sse`
**验证结果**:
- `cargo test -p talos-config -p talos-types -p talos-llm-provider -p talos-llm-openai -p talos-llm-bailian -p talos-llm-anthropic -p talos-agent-kernel -p talos-base`：通过。
- `pnpm --dir talos-app/ts-sdk test`：通过。
- `pnpm --dir talos-app/web test -- SettingsModelsEditor`：通过。
- `pnpm --dir talos-app/web typecheck`：通过。
- LLM HTTP SSE e2e：通过，报告 `talos-e2e/reports/e2e_report_20260615_235340.md`。
**状态**: 已完成

## 阶段 8: Desktop Packaged Smoke
**目标**: 若变更影响 WebView、desktop main/preload、SDK、Rust sidecar，则基于最终 DMG 做 Desktop 验证。  
**成功标准**:
- 清理旧 Talos 进程和旧 DMG 挂载卷。
- 重新打包 DMG。
- `hdiutil verify` 通过。
- packaged `Talos.app` 可打开。
- Settings Models 中 provider/protocol/model 配置可见并可保存。
- 核心 Chat 流程可用。
**测试**:
- Desktop build/package 命令。
- `hdiutil verify <dmg>`
- Packaged app smoke。
**状态**: 未开始
