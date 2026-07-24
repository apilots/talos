# 文件存储与模型文件输入设计

Status: draft
Owner: agent
Created: 2026-07-22
Updated: 2026-07-23
Related: `refactor-038-llm-model-selection`, `archive/2026/refactor-026-provider-protocol-abstraction`, `archive/2026/feat-021-user-scoped-agent-workspace`, `archive/2026/refactor-027-core-api-object-boundary`

## 背景与问题

Talos Web 已有 Files 页面，但当前只有前端占位数据。服务端虽然已有 `TalosInputItem::Image`、`TalosInputItem::File` 和 `ModelInputKind`，文件仍会在 Agent 输入归一化时丢失，三种 LLM 协议也只能编码文本。

本能力只补齐一条现有链路：文件先持久化为用户资源，Turn 只引用 `file_id`，Agent 执行时按当前用户和项目解析内容，最后由现有 Chat、Responses、Messages 编码器生成协议原生的图片或文件块。

## 设计结论

1. 文件是用户级持久资源，不存在 chat-only 临时文件；Chat、TUI、Files 页面共用一个上传 API。
2. `UserInput` 使用显式 `file_ids` 字段，不把文件引用塞进自由 metadata，也不允许 URL、本地路径或二进制进入 `StartTurnRequest`。
3. 数据库逻辑隔离是权限边界；OpenDAL 只负责对象读写。数据库保存 `storage_id + object_key`，不保存或暴露 backend URL。
4. 复用现有 `talos-store`、`TalosService`、Gateway 和 `LlmProvider` SPI，在原有消息链路内扩展。
5. `FileService` 是文件生命周期的唯一 owner；Agent 只依赖消费方定义的只读接口，Provider 只接收已解析的强类型二进制输入。

## 目标

1. Files 页面支持上传、分页列表、读取 metadata、更新 tags/metadata 和删除。
2. 文件 metadata 按用户隔离存入数据库，内容通过 OpenDAL 存入 Local FS 或 S3-compatible backend。
3. 配置可切换新上传文件的默认 backend；已有文件始终按记录中的 `storage_id` 读取。
4. Chat Composer 支持粘贴图片和选择附件，TUI Composer 支持粘贴图片，发送时都只提交有序 `file_ids`。
5. Agent 在模型请求前完成 owner/project 授权、内容读取、类型分类和模型/Provider 能力校验。
6. Chat Completions、Responses 和 Messages 使用同一个 Provider-neutral 附件模型并分别编码。

## 非目标

1. 不实现 OCR、文档切片、向量索引或文件问答检索。
2. 不实现跨用户共享、独立 ACL/RBAC、预签名 URL 或公开内容下载 API。
3. 不实现 backend 间迁移、运行时热切换或后台迁移任务；配置变更在重启后生效。
4. 不自动重新注入历史 Turn 的附件。
5. 首版 TUI 不提供普通文件选择器。

## 总体架构与职责

```text
Chat Composer / TUI Composer / Files page
  -> SDK files.upload
  -> Gateway transport adapter
  -> TalosService / FileService
       -> talos-store::file (metadata transaction)
       -> opendal::Operator (object bytes)
  <- FileInfo { file_id, ... }

StartTurnRequest { input: [Text, Files { file_ids }, ...] }
  -> existing Turn / Agent pipeline
  -> AttachmentReader (implemented by FileService)
  -> ChatMessage { content, attachments }
  -> Chat / Responses / Messages encoder
  -> concrete Provider
```

| Module | Sole responsibility |
| --- | --- |
| `talos-config` | `storage.files` 的强类型配置、默认值和启动校验 |
| `talos-error` | Files 领域需要跨层识别的最小错误种类 |
| `talos-store::file` | `FileRecord`、`FileStore` 和现有 migration 系统中的文件表 |
| `talos-service::file` | 授权、校验、OpenDAL 读写、状态机、恢复和 DTO 映射 |
| `talos-types` | 唯一的公开 Request/Response/Domain Object 和 `TalosInputItem::Files` |
| `talos-agent-kernel` | Agent 消费方的 `AttachmentReader` port 和已解析附件对象 |
| `talos-llm-provider` | Provider-neutral 附件对象、三种协议编码和附件能力校验 |
| `talos-base` | 构造 Store、Operators、FileService，注入 Agent，并在启动时执行恢复 |
| Gateway / SDK / UI | 仅处理 transport 或交互，不复制授权、MIME 和对象 key 规则 |

`FileService` 直接持有经校验的 `BTreeMap<String, Operator>`；OpenDAL `Operator` 是 Local/S3 的统一读写边界。

## Storage 配置

现有数据库配置下新增 `files`：

```yaml
storage:
  database_type: "sqlite"
  max_connections: 5

  files:
    default: "local"
    backends:
      local:
        type: "fs"
        root: "files"

      s3:
        type: "s3"
        root: "/talos-files"
        bucket: "talos-files"
        region: "us-east-1"
        endpoint: "https://s3.example.com"
        access_key_id: "${TALOS_FILE_S3_ACCESS_KEY_ID}"
        secret_access_key: "${TALOS_FILE_S3_SECRET_ACCESS_KEY}"

    limits:
      max_upload_bytes: 52428800
      max_files_per_turn: 5
      max_turn_file_bytes: 20971520
      max_filename_bytes: 255
      max_tags: 20
      max_tag_bytes: 64
      max_metadata_entries: 32
      max_metadata_key_bytes: 64
      max_metadata_value_bytes: 2048
      max_metadata_bytes: 8192

    allowed_mime_types:
      - "application/msword"
      - "application/pdf"
      - "application/vnd.ms-excel"
      - "image/png"
      - "image/jpeg"
      - "image/webp"
      - "text/csv"
      - "application/vnd.openxmlformats-officedocument.wordprocessingml.document"
      - "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet"
      - "application/vnd.openxmlformats-officedocument.presentationml.presentation"
```

配置对象只有一套定义：

```rust
pub struct FileStorageConfig {
    pub default: String,
    pub backends: BTreeMap<String, FileStorageBackendConfig>,
    pub limits: FileStorageLimits,
    pub allowed_mime_types: BTreeSet<String>,
}

#[serde(tag = "type", rename_all = "snake_case")]
pub enum FileStorageBackendConfig {
    Fs(FsFileStorageConfig),
    S3(S3FileStorageConfig),
}

pub struct FsFileStorageConfig {
    pub root: PathBuf,
}

pub struct S3FileStorageConfig {
    pub root: Option<String>,
    pub bucket: String,
    pub region: Option<String>,
    pub endpoint: Option<String>,
    pub access_key_id: Option<String>,
    pub secret_access_key: Option<String>,
}

pub struct FileStorageLimits {
    pub max_upload_bytes: u64,
    pub max_files_per_turn: usize,
    pub max_turn_file_bytes: u64,
    pub max_filename_bytes: usize,
    pub max_tags: usize,
    pub max_tag_bytes: usize,
    pub max_metadata_entries: usize,
    pub max_metadata_key_bytes: usize,
    pub max_metadata_value_bytes: usize,
    pub max_metadata_bytes: usize,
}
```

确定性规则：

- `backends` 的 map key 就是持久化的 `storage_id`，backend 内不重复配置 ID。
- `storage.files` 省略时使用产品默认值：`default = local`，Local root 为 `TalosPaths.data_dir/files`；`conf/talos*.yaml` 同步写出该默认值。
- FS `root` 是宿主文件系统目录：相对路径统一基于 `TalosPaths.data_dir`，绝对路径和 `~` 使用现有路径解析器处理。
- S3 `root` 是 bucket 内 prefix，不参与本地路径解析；`talos-files` 与 `/talos-files` 均由 OpenDAL 规范化为 `/talos-files/`，省略时为 `/`，并拒绝 `.`、`..` 路径段。
- S3 `bucket` 必填；`access_key_id` 与 `secret_access_key` 必须同时存在或同时省略；同时省略时明确使用 OpenDAL S3 credential chain。
- 所有 limits 必须大于零；`max_metadata_value_bytes <= max_metadata_bytes`。
- `default` 不存在、环境变量未解析、凭证只配置一半或配置了 detector 不认识的 MIME 时启动失败。
- 数据库引用的任一 `storage_id` 未配置时启动失败；凭证、endpoint 和 object key 不进入配置响应或日志。
- OpenDAL 负责把 backend 的 bucket/root 与业务 `object_key` 组合，业务代码不手工拼 URL。

## 数据库模型与隔离

文件持久化加入现有 `talos-store` 的 `file` 模块和 migration 目录，不新建 crate。

### `files`

```text
files
  file_id TEXT PRIMARY KEY
  owner_user_id TEXT NOT NULL
  project_id TEXT NULL
  storage_id TEXT NOT NULL
  object_key TEXT NOT NULL
  original_filename TEXT NOT NULL
  mime_type TEXT NULL
  size_bytes INTEGER NULL
  status TEXT NOT NULL
  metadata_json TEXT NOT NULL
  created_at INTEGER NOT NULL
  updated_at INTEGER NOT NULL

UNIQUE(storage_id, object_key)
INDEX(owner_user_id, created_at, file_id)
INDEX(owner_user_id, project_id, created_at, file_id)
INDEX(status, updated_at)

CHECK(status IN ('uploading', 'ready', 'deleting'))
CHECK(
  (status = 'uploading' AND mime_type IS NULL AND size_bytes IS NULL)
  OR
  (status IN ('ready', 'deleting') AND mime_type IS NOT NULL AND size_bytes IS NOT NULL)
)
```

对应 `FileRecord.mime_type` 和 `FileRecord.size_bytes` 使用 `Option`；Store 只有在 `Ready` / `Deleting` 状态下才能转换为必填的 `FileInfo` 字段，违反 CHECK 对应的是数据损坏错误。

### `file_tags`

```text
file_tags
  file_id TEXT NOT NULL
  tag TEXT NOT NULL

PRIMARY KEY(file_id, tag)
FOREIGN KEY(file_id) REFERENCES files(file_id) ON DELETE CASCADE
INDEX(tag, file_id)
```

`metadata_json` 直接序列化 `BTreeMap<String, String>`，不增加 schema wrapper。owner、project、storage、MIME、size 和 status 都是明确列，不能混入 metadata。

数据库不保存 `url`：Local 没有可用 URL，S3 URL 也不是稳定授权边界。唯一对象定位信息是内部的 `storage_id + object_key`。

用户隔离规则：

- `owner_user_id` 只来自认证后的 `Actor.user_id`，任何 request 都不能提交 owner ID。
- 用户可见的单文件查询和 mutation 始终包含 `file_id + owner_user_id`；越权与不存在统一返回 `TalosError::ResourceNotFound`。
- `project_id = NULL` 是个人文件；项目文件还必须通过现有 Project access service 校验。
- 首版文件始终归上传者私有；`project_id` 只限制文件可在哪个项目中使用，不授予项目其他成员读取权限。
- 个人文件可用于该用户任意 Turn；项目文件只能用于同一项目的 Session/Turn。
- `FileStore::count_by_project(project_id)` 是 Project delete 检查关联文件的唯一入口；存在关联文件时沿用现有 conflict 语义阻止删除，用户必须先删除这些文件。
- tag 查询必须 join `files` 后应用 owner scope，tag 表不重复保存 owner。
- `file_id` 的不可猜测性不是权限边界；`storage_id`、`object_key` 和 Operator 永不暴露给客户端。

对象 key 由服务端生成：

```text
{file_id}/content{normalized_suffix}
```

`object_key` 始终是相对 backend root 的路径且不以 `/` 开头；backend root 已表达 `files` 目录或 S3 prefix，因此 key 不再重复业务目录名。`normalized_suffix` 取原始文件名最后一个后缀并转为小写；只有匹配 `[a-z0-9]{1,16}` 时才保存为 `.{suffix}`，否则为空。它只方便对象运维，不参与 MIME 判断或授权。

## 文件状态机与恢复

`FileStatus` 仅有三个内部状态：

```rust
pub enum FileStatus {
    Uploading,
    Ready,
    Deleting,
}
```

上传：

1. `FileService` 校验 request、项目权限和默认 backend，创建 `Uploading` record。
2. Gateway 提供受限字节流；Service 使用 OpenDAL `Writer` 流式写入，同时计算大小并保留前 64 KiB 作为 MIME sniff buffer。
3. MIME 由 `infer` 的内容签名结果确定；请求 Content-Type 和文件后缀只作为诊断信息，不能覆盖检测结果。
4. 类型和大小通过后调用 `Writer.close()`，再把 record 原子更新为 `Ready`。
5. 任一步失败都调用 `Writer.abort()` 并幂等删除对象；清理完成后删除 record，清理未完成则保留 `Uploading` 供恢复。

删除：

1. owner-scoped compare-and-set 把 `Ready` 更新为 `Deleting`；重复删除 `Deleting` 文件时继续同一流程。
2. 使用 record 对应 Operator 幂等删除 object。
3. 删除 `files` record，tags 由外键级联删除。

`FileService::reconcile_incomplete()` 是唯一恢复入口：按 `(updated_at, file_id)` 稳定排序，每批最多读取并处理 100 条，循环删除遗留 `Uploading` 的对象与记录并完成 `Deleting`。`talos-base` 在开放 Gateway 前调用它；恢复失败则启动失败并保留上下文错误。

切换 `storage.files.default` 只影响重启后的新上传；旧文件始终按 record 中的 `storage_id` 读取。

## 公开对象与 API

公开对象统一定义在 `talos-types`，Store record、OpenDAL 类型和 transport body 不得外泄。

```rust
#[serde(tag = "type", rename_all = "camelCase")]
pub enum FileScope {
    Personal,
    Project {
        #[serde(rename = "projectId")]
        project_id: String,
    },
}

pub struct FileInfo {
    pub file_id: String,
    pub scope: FileScope,
    pub original_filename: String,
    pub mime_type: String,
    pub size_bytes: u64,
    pub tags: Vec<String>,
    pub metadata: BTreeMap<String, String>,
    pub created_at: i64,
    pub updated_at: i64,
}

pub struct FileUploadRequest {
    pub filename: String,
    pub scope: FileScope,
    pub tags: Vec<String>,
    pub metadata: BTreeMap<String, String>,
}

pub struct FileUploadResponse {
    pub file: FileInfo,
}

pub struct FileListRequest {
    pub cursor: Option<String>,
    pub limit: Option<u32>,
    pub scope: Option<FileScope>,
    pub tags: Option<Vec<String>>,
}

pub struct FileListResponse {
    pub data: Vec<FileInfo>,
    pub next_cursor: Option<String>,
}

pub struct FileReadRequest {
    pub file_id: String,
}

pub struct FileReadResponse {
    pub file: FileInfo,
}

pub struct FileUpdateRequest {
    pub file_id: String,
    pub tags: Option<Vec<String>>,
    pub metadata: Option<BTreeMap<String, String>>,
}

pub struct FileUpdateResponse {
    pub file: FileInfo,
}

pub struct FileDeleteRequest {
    pub file_id: String,
}

pub struct FileDeleteResponse {
    pub file_id: String,
}
```

API 语义固定如下：

- Wire 字段使用 `camelCase`，ID 为普通 `String`，时间为 Unix seconds。
- Upload 的 `scope`、`tags`、`metadata` 都必填；空集合明确表示无值。scope 首版创建后不可修改。
- List 的 `scope` 省略表示全部，`Personal` 只查个人文件，`Project` 只查指定项目。
- List 默认 `limit = 50`，最大 100；稳定排序为 `(created_at DESC, file_id DESC)`，cursor 不透明。
- `tags` 省略表示不筛选；非空列表使用 AND 语义。空 tag filter 拒绝，避免与省略产生双重语义。
- Update 字段省略表示不变，空 tags/metadata 表示清空；两者都省略时返回 validation error。
- filename trim 后必须非空，按 UTF-8 bytes 执行 `max_filename_bytes`，且不能包含路径分隔符、NUL 或控制字符；持久化 trim 后的值。
- tags 是大小写敏感的集合：每项 trim 后必须非空并满足 `max_tag_bytes`，trim 后不能重复，返回值按字典序排序。
- metadata key 必须非空、不得带首尾空白并满足 `max_metadata_key_bytes`；value 原样保存并满足 `max_metadata_value_bytes`。整个 map 同时执行 entry 数量和 JSON 序列化字节上限。
- 普通 list/read 只返回 `Ready` 文件，内部状态不暴露。

`FileService` 的上传 body 使用唯一内部类型，避免 Gateway 各自缓冲或定义业务对象：

```rust
pub type FileByteStream =
    Pin<Box<dyn Stream<Item = std::io::Result<Bytes>> + Send + 'static>>;
```

Gateway 只把 transport 转为 `FileUploadRequest + FileByteStream`，然后调用同一个 `FileService::upload`。其他操作直接使用上述 Request/Response。

文件格式校验集中在 `FileService` 的现有 validation 模块：

- DOC/DOCX 只按规范化后缀映射 MIME，不解析文档内容。
- CSV 使用 `csv` crate 读取完整上传内容；XLS/XLSX 使用 `calamine` 打开完整工作簿。
- 只有 CSV/XLS/XLSX 在流式写入 OpenDAL 的同时保留校验内容，内存上限复用 `max_upload_bytes`；其他格式最多保留前 64 KiB 给 `infer`。
- parser 校验失败时沿用 uploading object 的现有 abort/delete/record cleanup，不产生第二条上传状态机。

### HTTP

```text
POST   /v1/files
GET    /v1/files
GET    /v1/files/:file_id
PATCH  /v1/files/:file_id
DELETE /v1/files/:file_id
```

- Upload 使用 `multipart/form-data`，且只接受一个必填 `metadata` JSON part（`FileUploadRequest`）和一个必填 `file` binary part。
- Part 顺序固定为 `metadata` 在前、`file` 在后，使 Gateway 能先校验业务元数据，再将文件 part 直接流式交给 `FileService`，无需完整缓冲或临时落盘。
- Gateway 对 `metadata` part 使用 64 KiB transport cap，对 `file` part 流式执行 `max_upload_bytes`，不得先完整缓冲 multipart body。
- `metadata.filename` 是业务文件名的唯一来源；multipart file part 的 transport filename 不参与持久化或校验。
- List query 使用 `cursor`、`limit`、重复的 `tag`，以及 `scope=all|personal|project`；`scope=project` 时 `projectId` 必填，其他情况禁止 `projectId`。
- Read/Delete 的 path ID 映射到对应 Request；Patch body 只包含 `tags` 和 `metadata`。
- Browser 不手工设置 multipart `Content-Type`，boundary 由运行时生成。

### App Server

方法名：

```text
file/upload
file/list
file/read
file/update
file/delete
```

只有上传存在 transport envelope：

```rust
pub struct FileUploadParams {
    pub request: FileUploadRequest,
    pub data_base64: String,
}
```

`dataBase64` 只接受 RFC 4648 standard padded Base64，不接受 data URL prefix 或空白。Gateway 在解码前先校验 `dataBase64.len() <= ((max_upload_bytes + 2) / 3) * 4`，解码后再次执行 `max_upload_bytes`。其余方法直接使用共享 Request/Response。生成 schema 和 TS 类型必须包含全部对象。

### SDK

Browser、Desktop 和 TUI client 对调用方统一暴露：

```text
TypeScript:
files.upload(request, data: Blob | Uint8Array, options?: FileUploadOptions) -> Promise<FileUploadResponse>
files.list(request)                            -> Promise<FileListResponse>
files.read(request)                            -> Promise<FileReadResponse>
files.update(request)                          -> Promise<FileUpdateResponse>
files.delete(request)                          -> Promise<FileDeleteResponse>

Rust:
files.upload(request, data: Bytes) -> TalosResult<FileUploadResponse>
files.list(request)                -> TalosResult<FileListResponse>
files.read(request)                -> TalosResult<FileReadResponse>
files.update(request)              -> TalosResult<FileUpdateResponse>
files.delete(request)              -> TalosResult<FileDeleteResponse>
```

SDK 只隐藏 multipart 与 Base64 差异，不复制 tags、scope、分页或权限规则。

`FileUploadOptions` 只有可选 `signal` 和 `onProgress({ loadedBytes, totalBytes })`。Browser 仅在 multipart 上传需要进度时使用原生 XHR，其他请求保持现有 fetch；Desktop/App Server 在 Base64 transport 前后报告确定性起止进度。该状态不进入文件 DTO、数据库或服务端协议。

## Turn 输入与 Session 持久化

图片和普通文件只有一个引用结构：

```rust
pub enum TalosInputItem {
    Text { text: String },
    Files {
        #[serde(rename = "fileIds")]
        file_ids: Vec<String>,
    },
    SkillSelection { name: String, path: String },
    Voice { /* existing fields */ },
    Video { /* existing fields */ },
}
```

同一个 Turn 最多一个 `Files` item；`file_ids` 不能为空、不能重复、保持用户选择顺序，并受 `max_files_per_turn` 限制。请求不接受 URL、对象 key、本地路径、Base64 或 marker。

Agent 输入使用显式字段，不使用自由 metadata：

```rust
pub struct UserInput {
    pub text: String,
    pub file_ids: Vec<String>,
}
```

输入只要文本或文件任一非空即可。Skill selection 继续走现有独立提取逻辑。

Session artifact 同样只保存引用：

```rust
RunItem::UserInput {
    text: String,
    #[serde(default, rename = "fileIds")]
    file_ids: Vec<String>,
}
```

旧 artifact 缺少 `fileIds` 时读取为空数组；不双写旧结构。后续 Turn 只有再次提交 `file_id` 才会重新读取内容。

## Agent 文件解析边界

消费方接口定义在 `talos-agent-kernel`；`FileService` 实现它，Agent 不依赖 Store、Service 或 OpenDAL：

```rust
/// Authorization scope used while resolving one turn's file references.
pub struct TurnFileScope {
    pub owner_user_id: String,
    pub project_id: Option<String>,
}

/// Ordered, authorized file content resolved for one model turn.
pub struct ResolvedAttachment {
    pub file_id: String,
    pub filename: String,
    pub media_type: String,
    pub size_bytes: u64,
    pub bytes: Bytes,
}

/// Resolves ordered, authorized file references for one model turn.
pub trait AttachmentReader: Send + Sync {
    fn read<'a>(
        &'a self,
        scope: &'a TurnFileScope,
        file_ids: &'a [String],
    ) -> Pin<
        Box<
            dyn Future<Output = TalosResult<Vec<ResolvedAttachment>>>
                + Send
                + 'a,
        >,
    >;
}
```

Agent 通过 `Arc<dyn AttachmentReader>` 注入该依赖；boxed future 保持 trait object-safe，并沿用现有 `MemoryReader` port 的实现模式，不引入新的异步 trait 机制。

一次 `read` 必须：

1. 使用批量 metadata 查询校验全部文件为当前 owner 的 `Ready` 文件。
2. 允许个人文件，项目文件必须与当前 Session project 完全一致。
3. 保持输入顺序；任一缺失或越权时整体失败。
4. 读取时再次执行单文件、总字节和数量上限。
5. 只返回上述对象，不返回 Store record、storage ID 或 object key。

Turn Service 只做 `TalosInputItem` 结构校验；执行期的 `AttachmentReader` 是授权和读取的唯一事实来源，避免两套校验漂移。

标题、memory、skill suggestion、compaction 文本投影只消费 `UserInput.text`，不得把 Base64、文件名或 metadata 拼成 prompt。

## Provider-neutral 附件模型

`ChatMessage` 是附件的正确 owner：附件属于一条具体 user message，而不是整个 request；当前 `UserInput` 也不支持文本与附件任意交错。因此保留现有 `content: String`，只增加一个强类型可选字段 `attachments`，可以避免波及 tool、thinking、compaction、token estimate、cache 和大量字符串逻辑。

类型已经位于 `talos-llm-provider` 命名空间，不再重复使用 `Model` 前缀：

```rust
pub struct ChatMessage {
    pub role: Role,
    pub content: String,
    #[serde(skip)]
    pub attachments: Option<Vec<Attachment>>,
    // Existing tool, reasoning, cache and identity fields remain unchanged.
}

/// Binary input attached to one user message.
pub struct Attachment {
    pub kind: AttachmentKind,
    pub filename: String,
    pub media_type: String,
    pub bytes: Bytes,
}

/// Closed set of attachment representations supported by protocol encoders.
/// Add a variant only when every shared encoder and validator supports it.
pub enum AttachmentKind {
    Image,
    File,
}
```

Talos `file_id` 只保留在 Agent 的 `ResolvedAttachment` 中用于授权和错误上下文，不进入 Provider 类型或 wire payload。`image/*` 归类为 `AttachmentKind::Image`，其余受支持文档归类为 `AttachmentKind::File`。不直接复用范围更宽的 `ModelInputKind`，避免构造出 Text、Audio 或 Video 这类首版 encoder 无法处理的附件。

`None` 是无附件的唯一表示；`Some` 必须包含至少一个附件。现有 `system`、`user`、`assistant`、`tool` helper 和所有文本读取逻辑保持不变并设置 `attachments: None`；只新增 `ChatMessage::user_with_attachments(content, attachments) -> TalosResult<Self>`，其非空 `Vec<Attachment>` 参数构造 `Some(attachments)`。`Some(Vec::new())`、非 user role 携带 `Some` 都返回 validation error。

`UserInput` 不支持文本与附件任意交错，因此协议编码顺序固定为：非空文本在前，随后按 `file_ids` 顺序编码附件。tool loop 保留当前 user message 的附件；title、memory、compaction 文本投影继续只读取 `content`。通用 `ChatMessage` serde 跳过附件，只有三种协议 encoder 可以序列化二进制；旧消息反序列化时 `attachments = None`。

## 三种协议映射

共享编码器定义强类型 wire block，不使用自由 `serde_json::Value`：

| Protocol | Image | File |
| --- | --- | --- |
| Chat Completions | `image_url`，使用 Base64 data URL | `file`，包含 `filename` 与 Base64 `file_data` |
| Responses | `input_image`，使用 Base64 data URL | `input_file`，包含 `filename` 与 Base64 `file_data` |
| Messages | `image`，Base64 source 含 `media_type` 与 data | `document`，Base64 source 含 `media_type` 与 data |

首版只发送 inline Base64，不上传到 Provider Files API，也不生成远程 URL。编码结果仅存活于单次请求构造，日志和错误不得记录正文。

协议事实来源：

- OpenAI File Inputs: <https://developers.openai.com/api/docs/guides/file-inputs>
- OpenAI Image Inputs: <https://developers.openai.com/api/docs/guides/images-vision>
- Anthropic Files and document blocks: <https://platform.claude.com/docs/en/build-with-claude/files>
- Anthropic Vision: <https://platform.claude.com/docs/en/build-with-claude/vision>
- OpenRouter PDF Inputs: <https://openrouter.ai/docs/guides/overview/multimodal/pdfs>

## 模型与 Provider 能力

模型目录中的 `ModelInputKind::Image` / `File` 仍是产品能力事实。Provider SPI 新增一个确定性方法，默认实现只接受空附件：

```rust
/// Conservative attachment token estimates returned in input order.
/// The item count must exactly match the validated attachments.
pub struct AttachmentUsage {
    /// Conservative token upper bound for each attachment in input order.
    pub item_tokens: Vec<usize>,
}

pub trait LlmProvider: Send + Sync {
    /// Validates attachment kinds, MIME types and hard limits for one model.
    /// Supporting providers return one conservative token bound per item.
    fn validate_attachments(
        &self,
        model: &str,
        attachments: &[Attachment],
    ) -> TalosResult<AttachmentUsage> {
        if attachments.is_empty() {
            return Ok(AttachmentUsage {
                item_tokens: Vec::new(),
            });
        }
        Err(TalosError::FileError {
            kind: TalosFileErrorKind::UnsupportedAttachment,
            message: "model attachments are unsupported".to_string(),
        })
    }

    // Existing methods remain unchanged.
}
```

支持附件的具体 Provider 在该方法内校验 kind、MIME、单项字节、数量和总字节，并按附件顺序返回保守 token 上界；公共校验 helper 供 Provider 实现复用。只有同时满足以下条件才编码请求：

```text
model declares the classified input kind
AND provider validate_attachments succeeds
AND each item token bound is <= 10K
AND text plus attachment estimates fit the existing working-context budget
```

Agent 在选定 Provider 后、网络请求前调用该方法，并把 `item_tokens` 汇入现有 working-context budget。返回数量与附件数量不一致属于 Provider implementation error。协议 adapter 在直接调用场景复用同一方法，不能维护第二套判断。

标准协议差异由三个共享 encoder 处理；已确认存在的厂商细微差异只留在对应 Provider adapter。不能通过改模型、丢附件、改 MIME 或把文件降级成文本来 fallback。每个支持附件的实现必须在英文 doc comment 中记录 token 估算公式与来源；无法给出确定性上界的类型保持默认不支持。存储允许的 MIME 与当前模型可处理的 MIME 是两套独立约束。

## 产品入口

仅有三个上传入口，全部调用同一个 SDK API，上传成功后都能在 Files 页面管理：

1. Web/Desktop Chat Composer：粘贴图片或选择附件。
2. TUI Composer：粘贴图片。
3. Web/Desktop Files 页面：手动上传。

### Files 页面与 Chat Composer

- Files 页面删除本地 placeholder store，使用 SDK 完成 upload/list/read/update/delete。
- Files 页面与 Chat Composer 通过 `features/files` 的同一个 `uploadFile` 函数组装上传请求。
- Chat Composer 本地文件选择支持多选；私有 draft item 使用封闭状态 `uploading | ready | failed`。
- 上传中卡片显示每个文件自己的圆形进度；移除会取消该请求。成功后同一卡片转为 ready，移除只解除本消息引用，不删除 Files 记录；失败保留内联错误。
- 上传中或存在失败项时禁止发送；所有 ready 项按原选择顺序生成 `file_ids`。也可从现有 Files 中选择文件。
- 发送时生成一个 `TalosInputItem::Files { file_ids }`。
- 前端可按模型目录禁用不支持的附件控件，但服务端仍执行全部校验。
- 已删除引用在历史 Timeline 中显示不可用，不阻断纯文本历史读取。

### TUI Composer

TUI 使用 `arboard` 读取系统剪贴板图片，并用 `image` 编码为 PNG；不定义跨层 clipboard trait。`TerminalRuntimeEvent::Paste(String)` 继续只表示文本粘贴。

当前 draft 使用私有强类型状态：

```rust
pub(crate) struct PastedImageAttachment {
    pub marker: String,
    pub filename: String,
    pub state: PastedImageState,
}

pub(crate) enum PastedImageState {
    Pending { png_bytes: Bytes },
    Uploaded { file_id: String },
}
```

- 粘贴成功后插入稳定 marker，例如 `[Image #1]`；上传文件名为 `pasted-image-1.png`，MIME 为 `image/png`。
- marker 只用于 UI，不进入 `UserInput.text`、queue、artifact 或模型 prompt。
- Send 时按 marker 顺序上传所有 `Pending` 图片；Session 有项目时使用对应 Project scope，否则使用 Personal scope。
- 全部成功后才启动 Turn，并生成一个有序 `Files { file_ids }`。
- 失败时不启动 Turn，保留文本、marker 和状态；已成功项保持 `Uploaded`，重试不重复上传。
- `QueuedMessage` 和 `TerminalTurnRequest` 只保存 `file_ids`，不保存图片字节。
- 删除 marker 同时删除 draft attachment；附件-only draft 是有效输入。

## 错误语义

不新增错误 crate 或第二套 envelope。所有 Store、Service、Agent 和 Provider 路径继续返回 `TalosResult<T>`；仅在现有 `talos-error` 中增加调用方确实需要稳定识别的 Files 领域错误：

```rust
/// Stable machine-readable kind for file input failures.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TalosFileErrorKind {
    UploadTooLarge,
    UnsupportedMediaType,
    AttachmentLimitExceeded,
    UnsupportedAttachment,
}

pub enum TalosError {
    // Existing variants remain unchanged.
    #[error("File error ({kind:?}): {message}")]
    FileError {
        kind: TalosFileErrorKind,
        message: String,
    },
}
```

该枚举只表达服务端必须在不解析 message 的情况下区分处理的四类失败；缺失、普通参数校验、冲突、配置和基础设施失败继续复用现有通用错误，不复制 `FileNotFound`、`InvalidMetadata`、`FileConflict` 或 `FileStorageError`：

| Condition | Unified error |
| --- | --- |
| 文件不存在、非 owner、项目不匹配或 read/model resolve 时非 `Ready` | `TalosError::ResourceNotFound` |
| filename/scope/tags/metadata 非法或文件引用重复 | `TalosError::ValidationError` |
| 原始上传流超过 `max_upload_bytes` | `TalosError::FileError { kind: UploadTooLarge, .. }` |
| Storage allowlist 或当前 Provider 不接受 MIME | `TalosError::FileError { kind: UnsupportedMediaType, .. }` |
| Turn 或 Provider 的附件数量、单项字节或总字节超限 | `TalosError::FileError { kind: AttachmentLimitExceeded, .. }` |
| 当前模型或 Provider 不支持该附件 kind，或 Provider 使用默认拒绝实现 | `TalosError::FileError { kind: UnsupportedAttachment, .. }` |
| 项目仍有关联文件 | `TalosError::Conflict` |
| backend 配置非法或数据库引用未配置 backend | `TalosError::InvalidConfiguration` |
| OpenDAL write/close/abort/read/delete 失败 | `TalosError::StorageError` |
| 附件单项超过 10K tokens 或合并后超过当前 context budget | `TalosError::ContextWindowExceeded` |

multipart/JSON/Base64 在构造共享 Request 前失败时，沿用 HTTP `invalid_request` 或 App Server `INVALID_PARAMS` transport error；进入 Service 后统一使用上表中的 `TalosError`。HTTP 的 `ApiErrorCode` 增加 `File(TalosFileErrorKind)`：`UploadTooLarge` 和 `AttachmentLimitExceeded` 映射 413，其余两类映射 400。App Server 沿用现有 `VALIDATION_FAILED` 数字类别，SDK 透传既有 envelope。所有映射都穷尽匹配 `TalosFileErrorKind`，Gateway 不解析 message 推导业务语义。

文件校验在已启动 Turn 内失败时，`TalosError::to_run_error_info` 将 `FileError` 映射到新增的 `TalosRunErrorCode::FileInputRejected`；该错误属于用户可修正输入，不自动重试。公开 message 不得包含凭证、endpoint、Local root、storage ID 或 object key，完整内部上下文只写 tracing。

## 开源库复用

| Need | Library / existing facility | Boundary |
| --- | --- | --- |
| Local/S3 object IO | `opendal` with FS/S3 services | 直接使用 `Operator`、`Writer`、`close`、`abort`、`delete` |
| Multipart parsing | Axum `Multipart` extractor | Gateway 将解析结果转换为共享 Request 和字节流 |
| MIME detection | `infer` | 对非 Office/CSV 格式最多读取前 64 KiB 内容签名 |
| CSV validation | `csv` | 读取完整 CSV，拒绝非法 UTF-8 或结构错误 |
| Excel validation | `calamine` | 按 XLS/XLSX 后缀打开完整工作簿 |
| Word storage | Existing suffix normalizer | DOC/DOCX 只做闭合后缀到 MIME 的映射 |
| Base64 | Workspace existing `base64` crate | 三协议和 App Server 共用同一 codec |
| TUI clipboard | `arboard` | 只留在 TUI 私有模块，不进入 Service API |
| PNG encoding | `image` | 把 clipboard RGBA 规范化为 PNG |
| Metadata DB | Existing `talos-store` / `talos-storage` migrations | 沿用现有 Record、Store 和 migration 模式 |

新增依赖只启用实际需要的 features，并同步更新 `Cargo.lock` 和构建声明。

## Breaking Change 与影响范围

- 删除未形成有效行为的 `TalosInputItem::Image { url, mime_type }` 和 `TalosInputItem::File { url, file_id, mime_type }`。
- 新增唯一附件引用 `TalosInputItem::Files { fileIds }`，不保留 alias、URL 入口或双写路径。
- `RunItem::UserInput` additive 增加 `fileIds`，旧 artifact 通过 `serde(default)` 读取。
- `ChatMessage.content: String` 保持不变，增加由通用 serde 跳过的 `Option<Vec<Attachment>>`；该变化对持久化/wire 数据兼容，但会使下游 Rust struct literal 发生源码不兼容。公开且稳定的构造入口是既有 role constructors 和新增的 `ChatMessage::user_with_attachments`；仓库内调用方统一使用这些入口，下游 literal 调用方需迁移，不保留第二套消息结构或兼容字段。
- `Gateway` 与 `GatewayClient` 的 Files 方法提供显式 `GatewayError` / `ClientError` 默认实现，既有第三方实现无需立即增加方法即可继续编译；支持 Files 的正式 transport 必须覆盖这些方法，调用未覆盖实现时确定性失败。
- `talos-error` additive 增加 `TalosFileErrorKind`、`TalosError::FileError` 和 `TalosRunErrorCode::FileInputRejected`，并更新既有 transport 映射。
- 配置 schema、App Server schema、TS generated types、Browser/Desktop SDK、Web 和 TUI 必须在同一能力变更中更新。

| Area | Change |
| --- | --- |
| `talos-config` / `conf` | `storage.files`、默认 Local 配置、schema 与校验 |
| `talos-error` | Files 领域错误、HTTP/App Server 映射和 Turn failure code |
| `talos-store::file` | tables、record、store API、owner-scoped queries |
| `talos-types` | 文件 DTO、`FileScope`、`TalosInputItem::Files`、artifact 字段 |
| `talos-service` | `FileService`、OpenDAL 状态机、恢复和 reader implementation |
| `talos-base` | Operators 构造、依赖注入和启动恢复 |
| Gateway / App Server | multipart routes、RPC methods、schema generation |
| SDK / Web / Desktop | 统一 Files client、Files 页面和 Chat attachments |
| TUI | clipboard PNG、marker 状态、上传和 `file_ids` |
| Agent / Kernel | input normalization、reader port、附件解析与 artifact |
| LLM Provider / adapters | typed attachments、三协议编码、统一校验和 token 上界 |

## 验证策略

### Store、Service 与配置

- 完整对象比较覆盖 FS/S3/default/limits 配置；非法 backend、凭证、路径和 MIME 启动失败。
- 使用临时目录的真实 OpenDAL FS Operator 验证 upload/read/delete。
- 解析附件前先以 OpenDAL `stat` 校验对象真实大小，再读取字节；数据库大小与对象大小不一致或任一对象超限时，整批在读取前失败。
- 数据库集成测试覆盖状态列的 nullable/CHECK 不变量、owner/project 隔离、tags AND filter、稳定 cursor、update 清空语义和完整 DTO 映射。
- 项目删除测试证明有关联文件时返回 conflict，其他项目成员不能读取上传者的项目文件。
- 使用 fixture 验证普通 MIME 的 64 KiB sniff、DOC/DOCX 后缀映射、完整 CSV 读取、有效 XLSX 和损坏 Excel 拒绝；transport Content-Type 不参与持久化判断。
- 故障注入覆盖 write、close、Ready update、object delete 和 record delete；创建超过 100 条遗留记录验证恢复按批收敛且不无界加载。
- MinIO 只用于独立 S3 E2E，不进入普通 deterministic test。

### Gateway、SDK 与 UI

- HTTP multipart 和 App Server Base64 对同一输入返回相同 `FileInfo`。
- 验证超限中止、multipart boundary、query scope 映射和统一错误码。
- Files 页面覆盖 upload/list/filter/read/update/delete；Chat 覆盖 paste、多选、进度、取消、失败、existing file、发送阻塞和有序 IDs。
- TUI snapshot/integration 覆盖 `paste image -> marker -> upload -> Files { file_ids }`，并证明 marker 不进入文本；失败保留 draft 且不启动 Turn。
- `12_files` E2E 使用完整有效图片和可检查请求体的 mock Provider，确定性验证 Local 文件管理与模型附件 wire 编码；独立 MinIO 场景验证 S3 生命周期。真实模型只在配置了明确支持对应附件类型的模型时做人工验收，不能用 mock 结果冒充。

### Agent 与 Provider

- 集成测试证明文件不再被文本提取丢弃，owner/project 越权在读取时整体失败。
- artifact 只出现 `text + file_ids`，不出现 Base64、object key 或 storage 配置。
- 对同一 PNG/PDF 分别比较 Chat、Responses、Messages 的完整 outbound request。
- 模型不支持、Provider 默认未启用、MIME 不接受、字节上限和 token 上限失败都发生在网络请求前。
- 验证四个 `TalosFileErrorKind` 的穷尽映射，以及 Turn 内文件错误产生 `FileInputRejected` 且不自动重试。
- 校验 `AttachmentUsage.item_tokens` 数量、单项 10K cap 和现有 working-context budget 合并行为。
- `ChatMessage.content` 的现有字符串读取、纯文本、tool call、Thinking/Reasoning、cache control、streaming 和历史恢复保持现有行为；旧消息的 attachments 为 `None`，并拒绝 `Some(Vec::new())`。

## 最终约束

文件内容是高风险模型输入。实现必须同时执行 storage 上传上限、Turn 数量/总字节上限和 Provider 单项/总量/MIME 上限；任何不能证明满足项目 context cap 的 adapter 不得启用。

本设计只新增数据库侧 `FileStore`、Agent 消费侧 `AttachmentReader`，并在现有 `LlmProvider` SPI 上增加 `validate_attachments`。Service、Gateway 与 Provider 均使用本文定义的唯一对象和职责边界。
