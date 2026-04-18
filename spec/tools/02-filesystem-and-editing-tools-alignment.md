# Filesystem And Editing Tools Alignment

## 1. 范围

本篇覆盖以下 Claude Code 工具：

- `Read`
- `Edit`
- `Write`
- `Glob`
- `Grep`
- `NotebookEdit`

---

## 2. 对齐原则

### 2.1 Read / Edit / Write 必须形成闭环

Talos 文件工具不能再是三个彼此独立的函数式入口，必须构成：

```text
Read -> cache file state -> Edit/Write validate against cached state -> mutate -> refresh state
```

其中 `read_file_state` 明确采用 session-scoped 版本化状态，而不是 per-turn 临时缓存。

### 2.2 默认安全而非默认方便

- `Edit` 默认单次精确替换
- 多匹配替换必须显式 `replace_all=true`
- `Write` 覆盖现有文件前必须满足 read-before-write
- `NotebookEdit` 必须独立，不允许继续走普通文本 `Edit`

---

## 3. 当前源码基线

本篇基于本轮对以下源码的对照探索：

- Claude Code:
  - `src/tools/FileReadTool/FileReadTool.ts`
  - `src/tools/GrepTool/GrepTool.ts`
- Talos:
  - `crates/action/tools/src/tools/read_file.rs`
  - `crates/action/tools/src/tools/grep_files.rs`

### 3.1 Claude Code 当前现状

`Read` 当前已经不是“读文本文件”这么简单，而是一个完整的文件读取平台入口，实际覆盖：

- 文本读取 + 行号
- 图片读取与尺寸/压缩处理
- PDF 分页读取
- Notebook 结构化读取
- token/size 限制
- 设备文件拒读
- 路径与权限检查
- `preparePermissionMatcher`
- 文件读取后的状态记录

`Grep` 当前也不是 `rg --files-with-matches` 的薄封装，而是已经具备：

- `content/files_with_matches/count` 三种输出模式
- `-A/-B/-C/context/-n/-i/type/head_limit/offset/multiline`
- path existence validation
- permission checking
- permission matcher
- read-only / concurrency-safe 声明
- 默认结果上限与分页语义

### 3.2 Talos 当前现状

`read_file` 当前仍是典型最小工具：

- 公开参数只有 `file_path/offset/limit`
- 主要职责是把输入反序列化后调用 `fs_read::run`
- 工具层没有权限匹配、危险路径拒绝、token/size 预算、图片/PDF/notebook 分流

`grep_files` 当前也仍是最小实现：

- 参数只有 `pattern/include/path/limit`
- 工具层直接调用 `fs_search::run`
- 返回内容仍以 `response.content` 为主
- 工具层没有 `output_mode`、上下文行、计数模式、offset/head_limit、多行模式、permission matcher

### 3.3 本篇方案落点

因此本篇不是“从零设计文件工具”，而是：

- 保留 Talos 现有文件 runtime 可复用部分
- 把 Claude Code 已经沉淀到工具契约和平台层的能力补齐
- 明确哪些能力必须进入平台层，哪些能力属于文件工具特例

---

## 4. 工具能力矩阵

| 工具 | Claude Code 能力 | Talos 目标 |
|---|---|---|
| `Read` | 文本、图片、PDF、Notebook、分页读取、token/size 限制、权限检查 | 完全对齐 |
| `Edit` | old/new replace、replace_all、文件已读校验、stale-write 检测、diff 输出 | 完全对齐 |
| `Write` | 创建/覆盖、已读校验、diff 输出、mtime 校验 | 完全对齐 |
| `Glob` | 按 glob 查文件、权限检查、结果截断 | 完全对齐 |
| `Grep` | rg content/files/count、多参数模式、offset/head_limit | 完全对齐 |
| `NotebookEdit` | 按 cell 操作 notebook，replace/insert/delete | 完全对齐 |

---

## 5. `Read`

### 4.1 目标输入契约

```json
{
  "file_path": "/abs/path/file.rs",
  "offset": 1,
  "limit": 200,
  "pages": "1-5"
}
```

### 4.2 必须补齐的能力

- 路径归一化与权限检查
- 设备文件/危险路径拒读
- 大文件 size/token 限制
- 图片直接返回图片型结果
- PDF 支持 `pages`
- notebook 支持结构化读取
- 读取完成后写入 `read_file_state`

### 4.3 关键行为

- 文本文件：返回带行号结果
- 二进制文件：不直接按文本读
- 图片：走图像输出模型
- PDF：优先分页读取，不默认整体灌入
- Notebook：返回 cell 视图而不是原始 JSON

---

## 6. `Edit`

### 5.1 目标输入契约

```json
{
  "file_path": "/abs/path/app.rs",
  "old_string": "foo()",
  "new_string": "bar()",
  "replace_all": false
}
```

### 5.2 必须补齐的约束

- `old_string == new_string` 直接拒绝
- 文件不存在时，仅允许 `old_string == ""` 的创建语义
- 文件存在且 `old_string == ""` 时，只允许空文件场景
- 文件未读过则拒绝
- 文件在读取后已被修改则拒绝
- 匹配不到则拒绝
- 多匹配且 `replace_all=false` 则拒绝
- `.ipynb` 必须转走 `NotebookEdit`

### 5.3 输出要求

`Edit` 输出不能只是“成功替换 N 次”，必须携带：

- `filePath`
- `oldString`
- `newString`
- `originalFile`
- `structuredPatch`
- `replaceAll`
- 可选 `gitDiff`

---

## 7. `Write`

### 6.1 目标输入契约

```json
{
  "file_path": "/abs/path/new.txt",
  "content": "..."
}
```

### 6.2 关键行为

- 新文件：允许直接创建
- 已存在文件：必须满足 read-before-write
- 若文件在读取后修改，拒绝写入
- 保留编码与行尾风格
- 写入后回写 `read_file_state`

### 6.3 输出要求

输出必须包含：

- `type=create|update`
- `filePath`
- `content`
- `structuredPatch`
- `originalFile`
- 可选 `gitDiff`

---

## 8. `Glob`

### 7.1 目标能力

- `pattern`
- `path`
- 只读并发安全
- 结果默认截断
- 返回相对路径列表

### 7.2 Talos 决策

Talos 当前没有 `Glob`，应新增独立工具，不再要求模型使用 `list_dir` 或 shell `find` 替代。

---

## 9. `Grep`

### 8.1 当前 Talos 问题

当前 `grep_files` 仅基于 `rg --files-with-matches` 返回文件路径，无法承载 Claude Code 的 `Grep` 语义。

### 8.2 目标输入契约

至少覆盖：

- `pattern`
- `path`
- `glob`
- `output_mode`
- `-A/-B/-C/context`
- `-n`
- `-i`
- `type`
- `head_limit`
- `offset`
- `multiline`

### 8.3 目标输出模式

| 模式 | 说明 |
|---|---|
| `content` | 返回匹配内容与上下文 |
| `files_with_matches` | 返回匹配文件 |
| `count` | 返回匹配计数 |

---

## 10. `NotebookEdit`

### 9.1 为什么必须独立

Notebook 不是普通文本文件：

- 修改单位是 cell
- 需要 JSON 结构校验
- 需要读前校验与 stale-write 保护
- `replace/insert/delete` 语义和普通文本替换不同

### 9.2 目标输入契约

```json
{
  "notebook_path": "/abs/path/demo.ipynb",
  "cell_id": "abc123",
  "new_source": "print('hi')",
  "cell_type": "code",
  "edit_mode": "replace"
}
```

### 9.3 输出要求

- `new_source`
- `cell_id`
- `cell_type`
- `language`
- `edit_mode`
- `notebook_path`
- `original_file`
- `updated_file`

---

## 11. Talos 目标模块表

| 目标模块 | 职责 |
|---|---|
| `tools/file_read` | 外部 `Read` 工具定义 |
| `tools/file_edit` | 外部 `Edit` 工具定义 |
| `tools/file_write` | 外部 `Write` 工具定义 |
| `tools/glob` | 外部 `Glob` 工具定义 |
| `tools/grep` | 外部 `Grep` 工具定义 |
| `tools/notebook_edit` | 外部 `NotebookEdit` 工具定义 |
| `runtime/fs_read_state` | 已读文件状态缓存 |
| `runtime/fs_diff` | diff / structured patch |
| `runtime/fs_encoding` | 编码与行尾处理 |
| `runtime/notebook` | notebook parse/edit helpers |

---

## 12. 验收标准

- `Read/Edit/Write/Grep/Glob/NotebookEdit` 的模型可见契约与 Claude Code 对齐
- 文件写工具全部具备 read-before-write 和 stale-write 保护
- `Grep` 不再只是文件名搜索
- `NotebookEdit` 独立存在，不再由普通 `Edit` 勉强承担
