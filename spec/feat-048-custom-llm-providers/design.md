# Custom provider connections and configuration-driven model selection

Status: implemented

## Background and problem

Built-in Provider slots configure branded public services. Independently hosted endpoints may share upstream model IDs while requiring different Base URLs, wire protocols and explicit request extensions. Custom connections must select the supported wire protocol rather than a branded adapter. Public Bailian and DeepSeek integrations retain their existing service-specific behavior. The compact model picker continues consuming canonical configuration instead of model-name guesses.

## Goals

- Let each named custom connection select Chat, Messages or Responses with existing model metadata and optional credentials.
- Reuse LlmProvider and the existing protocol encoders, stream parsers, attachment representations and tool-history owners.
- Bind explicit KV thinking switches to the current session selection and provide a separate typed Chat reasoning-replay policy.
- Preserve qualified Provider/model identity, canonical reasoning/history and structured tool call/result order.
- Keep the existing localized Settings/model forms and composer picker, and remove obsolete custom adapter configuration, routing and tests.
- Verify compatibility contracts locally without requiring private model availability.

## Non-goals

- Change public Bailian/DeepSeek reasoning budgets, headers, model-specific public API behavior or existing adapter defaults.
- Add model-name, Provider-name or host heuristics, automatic capability probes, shotgun parameters or retry a completed model turn with another parameter dialect.
- Add thinking-budget UI, accelerated Fast transport, a second configuration registry or a second model form.
- Replace the independent context circle, composer input, attachment, permission or submission owners.
- Claim a mock service proves a private deployment supports an extension; real model acceptance remains separate.
- Add arbitrary authentication/header configuration or a general-purpose compatibility-profile engine in this slice.

## Current implementation facts

- LlmProvider already exchanges canonical messages, tool calls/results and ReasoningContinuation; it does not need model-family branches.
- Shared protocols/chat, protocols/messages and protocols/responses own encoding, streaming and reasoning/tool continuation projections.
- ConfiguredProvider already binds per-model request_parameters and the session thinking selection before the inner provider executes.
- The unreleased custom adapter enum and clone-and-rewrite-to-built-in factory are superseded by protocol-only connections. Built-in service adapters remain separate.
- Generic/dev/prod YAML examples are disabled comments using reserved example domains. Real deployment profiles, such as a local conf/talos-pre.yaml, remain outside the public source change.

## Design decisions

### Configuration and identity

Keep llm.custom_providers as a map keyed by stable lowercase connection IDs (letters/digits/hyphens, starting with a letter, at most 64 characters). Built-in names are reserved. CustomProviderConfig contains name and flattened existing ProviderConfig fields: required protocol, HTTP(S) base_url, optional api_key and models. Remove the unreleased adapter field/type instead of retaining a hidden brand selection. Reuse the existing LlmProtocol enum: chat, messages, responses.

ModelRef remains {provider: connection ID, id: upstream ID}; the same upstream ID at multiple endpoints remains distinguishable. Catalog ordering, configured default/inner-model resolution and removed-model validation retain existing ownership. Missing custom collections and request maps default to empty.

ProviderModelConfig gains reasoning_replay with the typed values protocol_default (default), omit, reasoning and reasoning_content. Nondefault values are accepted only for models inside a custom Chat connection. Other protocols and built-in connections reject a nondefault policy before persistence. Configuration APIs intentionally mirror snake_case YAML fields; Agent protobuf and generated SDK/schema carry the same enum losslessly. No new ChatRequest or ModelSelection persistence field is required.

```yaml
llm:
  custom_providers:
    example-connection:
      name: Example Connection
      protocol: chat
      base_url: https://llm.example.invalid/v1
      models:
        - id: example-model
          name: Example Model
          context_window: 131072
          reasoning: true
          reasoning_effort: medium
          speed: standard
          input: [text]
          reasoning_replay: omit
          request_parameters:
            chat_template_kwargs: {enable_thinking: true}
```

### Protocol transport and ownership

The Worker keeps its built-in adapter factory and creates a small ProtocolProvider for custom connections. The shared client implements LlmProvider and composes existing protocol functions rather than selecting a branded provider or copying an agent execution loop. It owns HTTP request/response handling, the selected protocol endpoint and standard headers. ConfiguredProvider continues owning connection identity, per-model KV and selection binding.

Chat calls chat/completions and Responses calls responses, using Bearer authentication when a key is configured. Messages calls messages and uses x-api-key plus anthropic-version for its protocol default. Empty keys add no authentication header. No URL substring decides credentials, thinking style or replay policy. A compatible gateway with different authentication requirements needs a separately scoped explicit connection feature rather than another branded adapter option.

Protocol request defaults are distinct from static endpoint overrides. Chat effort serialization is independent of configured enable_thinking or chat_template_kwargs.enable_thinking switches. Responses uses its native reasoning representation. Messages can use adaptive thinking plus output_config effort by default, and disabled thinking for Off, without inventing a budget. Endpoints requiring manual Messages thinking must configure their supported thinking object (including a required budget if applicable) explicitly. Protocol-specific effort values are mapped at the protocol boundary, not by model name.

The presence of a thinking KV key alone is not a binary control. Messages owns adaptive thinking semantics: a configured thinking object remains intact while session effort is generated independently in output_config. Chat and Responses do not interpret adaptive as a native mode or generate Messages controls; opaque user extensions remain explicit KV and do not suppress their native effort fields. A thinking switch does not suppress a selected effort; accepted effort values are declared by optional reasoning_efforts metadata. Explicit KV effort fields still take precedence over generated defaults.

Models declared without reasoning support do not receive generated thinking/effort fields. Off is encoded explicitly only for a configured reasoning-capable model. For custom Messages, an outgoing disabled thinking object omits the enabled-only budget_tokens field while leaving canonical model KV unchanged. Other configured siblings survive normal switch updates.

Explicit KV values take precedence over generated request defaults. Retain protected runtime fields: models, messages/input/system, tools/tool choice, stream settings and credentials cannot be replaced by request KV. Keys are nonempty and unique. The existing secret store and redacted read/save/clear lifecycle remain authoritative.

### Thinking switches and KV examples

The current session selection controls only explicitly recognized binary extension paths: enable_thinking, chat_template_kwargs.enable_thinking, and thinking.type=enabled/disabled. Unrelated sibling KV values survive the update. Multiple explicit binary defaults must agree. Nonbinary modes such as a configured adaptive value remain explicit static wire configuration; do not silently reinterpret their semantics as a generic boolean binding.

Do not auto-send every known thinking field. Optional examples are manually applied and distinguish service contracts rather than old/new model names:

| Example service contract | Request KV position | Switch behavior |
| --- | --- | --- |
| vLLM/SGLang-style Chat endpoint | chat_template_kwargs: {enable_thinking: true} | Session On/Off updates only the configured nested bool |
| Top-level enable_thinking endpoint, including Bailian-compatible APIs | enable_thinking: true | Session On/Off updates the configured root bool |
| Endpoint with binary thinking object | thinking: {type: enabled} | Session On/Off updates only type, preserving sibling fields |

A user may explicitly configure multiple supported controls with matching defaults; the runtime does not add them automatically. A successful HTTP response alone does not establish that a real endpoint honored a parameter. Keep public Bailian's existing mapping inside its native adapter.

### Reasoning replay and complete tool history

Reading a response and replaying history are separate operations. The Chat parser may accept supported reasoning aliases without assuming all aliases should be sent back. For custom Chat, protocol_default retains the existing OpenAI-compatible reasoning-field projection. The explicit reasoning/reasoning_content policies select exactly one outbound assistant field. The explicit omit policy sends no historical reasoning fields; it is not the existing unconfigured None case, which continues to fail if reasoning continuation reaches Chat without a replay format.

The policy affects only outgoing projection. Stored canonical reasoning, final assistant text, tool call IDs/names/arguments, tool outputs and ordering are never removed or rewritten. Unsupported structured reasoning must still fail explicitly rather than flattening encrypted/signature material into text. Streaming custom Chat must not silently skip incoming structured details that nonstream parsing preserves, and text replay must reject signed Text blocks instead of discarding the signature. Explicit omit remains an intentional outbound omission policy, not permission for the response parser to lose canonical state. Messages preserves thinking/signature blocks and tool_use/tool_result; Responses preserves its reasoning items and function_call/function_call_output. Neither accepts the Chat-only override.

At the streaming boundary, custom Chat accepts both text reasoning aliases and structured details independently of the selected outbound replay policy. Nonempty details take precedence over aliases in the same delta; an empty details array permits alias fallback, and text before or after structured details remains part of canonical continuation. Native clients retain their previous explicit incoming formats. Shared text-only replay rejects nonempty signed Text blocks as well as encrypted/summary blocks; native structured-details replay still preserves their signatures. Messages content-block indexes and Responses output indexes identify wire blocks, not Agent tool ordinals: emit consecutive tool ordinals while retaining original block indexes for deltas, provider identity, signatures and encrypted continuation. Reasoning or text before a tool must not prevent tool execution.

Serving-side parser/template problems remain serving-side problems. An explicit omit setting can provide an endpoint-specific compatibility choice where validated; it does not justify model-name branches, tag stripping, default global reasoning removal or claims that one model family requires the same policy everywhere.

### Web and TUI

Keep Settings -> Models and built-in tabs. The existing Custom menu manages named connections. Its connection modal exposes only the three protocol choices, name, Base URL and optional key. Remove obsolete adapter labels, combined options, fallback helpers and dependent tests. Provider/model deletion keeps the existing in-app draft confirmation and page-level Save lifecycle.

Reuse SettingsModelModal and its collapsed Advanced section. Add a localized reasoning-replay selector only for custom Chat models beside the existing KV editor; do not put replay into the top-level request body. Switching to another protocol while a model has a nondefault replay policy requires the user to reset that policy first; do not silently discard it. Editing models preserves hidden existing fields such as cost. The optional KV examples explain nested versus root controls and never auto-populate new models or broadcast both forms. An example whose key already exists must not replace the existing value or silently remove sibling JSON fields; operators edit or remove that row explicitly before inserting another example. Reuse mature Radix select/menu/dialog primitives and the existing Input/Textarea for the configuration forms. Shared wrappers own visual consistency only; the library owns popup layering, keyboard navigation, dismissal and focus trapping. Advanced values support multiline JSON and narrow layouts without truncating editable content.

The composer picker and TUI continue using the existing qualified model identity and typed selection_capabilities projection. Thinking switches do not imply binary-only capabilities. Only an explicit empty reasoning_efforts list exposes Off/On for a configured switch; other effort models use declared or standard effort values/defaults. No new TUI configuration editor or Fast step is introduced. Fast remains model metadata and temporary Web visual state only; no Fast session/API/Worker/Sync feature is added.

## Impacted areas

- Configuration: existing LlmConfig/ProviderConfig/ProviderModelConfig, custom validation, secret persistence and upgrade normalization.
- Runtime: custom connection composition, shared protocol transport and Chat reasoning projection; native service construction and incoming modes remain. Shared text replay now explicitly rejects lossy signed-block projection instead of silently removing signatures.
- Interfaces: config/catalog DTOs, Agent protobuf and generated App schema/TypeScript SDK.
- Clients: existing connection modal and advanced model configuration, localized examples and owning tests.
- Examples: disabled generic/dev/prod YAML; private deployment profiles are maintained locally using the same schema.
- QA/CI: secretless 01_models configuration and protocol compatibility scenarios plus controlled transport integration tests.

## Verification strategy

Use controlled loopback services with opaque model IDs; no test should select compatibility by model name, display name, host or Provider ID. Verify actual request payloads and tool continuation, not just static definitions or status codes. The reusable matrix is:

| Boundary | Required cases | Observable assertions |
| --- | --- | --- |
| Configuration/persistence | Three protocols; identical upstream IDs; all replay values; invalid override boundaries | Save/read/reload/disk and protobuf/SDK parity; invalid config fails before publish |
| Protocol transport | Chat/Messages/Responses, streaming and nonstreaming | Correct endpoint/header/body, structured response, existing error contract |
| Thinking KV | Nested bool, root bool, thinking.type, On/Off and explicit override | Exact configured position/value, preserved siblings, no automatic duplicate field or budget |
| Adaptive effort | Messages adaptive KV with medium/high in one session | Messages thinking/siblings preserved with changing output_config.effort and signed tool history |
| Chat replay | Four outgoing policies; text aliases, empty/nonempty details, signed/encrypted and mixed incoming streams | Correct outgoing field; stream/nonstream canonical parity; typed rejection of lossy signed projection; explicit omit preserves stored history |
| Tool continuation | Two model rounds with a real local tool execution | Call/result IDs, arguments, content and order survive; expected final response |
| Structured protocols | Messages signature/tool blocks; Responses reasoning/function items; reasoning/text before sparse-indexed tools | Consecutive Agent tool ordinals, original wire/block identity and structured continuation; Chat override is rejected |
| Public regression | Existing Bailian and DeepSeek tests | Existing public defaults, encoding and replay unchanged |
| Web configuration | Protocol-only creation/edit; Advanced policy; optional examples | Bilingual labels, preserved hidden fields, explicit invalid-protocol handling, normal save/delete behavior |

The Agent path normally streams. ProtocolProvider integration tests own nonstream tool-history coverage; do not call a collected SSE response a nonstream upstream request or add production endpoints only for testing. Run just e2e-group 01_models against a worktree-local binary and isolated homes/ports, plus owning Rust/Web checks and generators. Audit old custom adapter symbols/options/tests and verify maintained config defaults remain unchanged.

Real private-model, image and serving-parser acceptance requires reachable endpoints and is separate from controlled QA. This iteration does not access pre. Existing image metadata/file/attachment owners remain unchanged. Actual commands, results, logs, screenshots and limitations belong in ignored output and the root execution plan, not this design document.

## Open risks and follow-ups

- Compatible endpoint authentication beyond the protocol default is outside this slice; it must be explicit if introduced.
- Remote extension support and deployment versions cannot be inferred from protocol selection or HTTP 200. Operators must configure and verify the real endpoint contract.
- History-sensitive serving parser incompatibilities require serving-boundary correction or an explicitly validated replay policy; do not infer compatibility from a model name.
- Changing a model/protocol while an asynchronous session save is pending uses the existing session owner; this change introduces no new synchronization policy.
- Fast transport, persistent per-session speed overrides and new thinking-budget controls remain outside scope.
