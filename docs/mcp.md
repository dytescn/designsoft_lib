# designsoft_lib MCP 服务文档

> 状态：接口文档（对应 `designsoft_lib` 1.0.2 的 FFI）
> 更新日期：2026-10-28
> 适用平台：Windows 10/11 x64
> 打包产物：`target/release/designsoft.dll`
> 底层接口：`post_info` / `post_start` / `post_status`

本文档描述如何把 `designsoft_lib` 封装成 MCP（Model Context Protocol）服务：
工具名称、输入输出结构、错误语义、资源与实现注意事项。MCP server 本身由宿主
实现（Rust / C# / Node 均可），底层通过 FFI 调用 `designsoft.dll`。

`designsoft_lib` 只负责“发现、启动、查询本机设计软件”这三件事，不包含任何
具体应用的功能实现。本文档不绑定具体软件清单：实际能启动什么，由本机安装情况
和部署方的包索引决定。

---

## 1. 概述

`designsoft_lib` 提供三个能力：

1. **发现**本机已安装的设计软件（注册表 Uninstall 键）；
2. **打开**软件，按以下顺序选择打开方式：
   - COM 接口（当前为 CorelDRAW，基于 `cdrsdk`）；
   - 托管包（按部署方发布的包索引下载到本地后启动）；
   - 本地 exe（按注册表安装信息启动，默认方式）；
3. **查询状态**（已启动 / 已安装但未启动）。

MCP 服务把它们映射为三个工具：

| 工具 | 对应 FFI | 读/写 | 幂等 | 主要副作用 |
| --- | --- | --- | --- | --- |
| `design_software_list` | `post_info` | 只读 | 是 | 无 |
| `design_software_start` | `post_start` | 写 | 是（已运行直接成功） | 启动进程；首次可能下载托管包 |
| `design_software_status` | `post_status` | 只读 | 是 | 无（不启动软件） |

推荐 MCP 能力声明：

```json
{
  "capabilities": {
    "tools": { "listChanged": false },
    "resources": { "subscribe": false, "listChanged": false }
  },
  "serverInfo": { "name": "designsoft_lib", "version": "1.0.2" }
}
```

---

## 2. 打包产物与运行环境

`designsoft_lib` 以单个动态库发布，宿主加载后即可获得三个 C ABI 接口，
不需要额外的运行时文件。

| 项目 | 值 |
| --- | --- |
| 发布名 | `designsoft_lib` |
| 版本 | `1.0.2` |
| 包类型 | `design_soft` |
| 打包产物 | `target/release/designsoft.dll` |
| 导出符号 | `post_info`、`post_start`、`post_status` |
| crate 名 | `designsoft`（`cdylib` + `rlib`） |
| 平台 | 仅 Windows；非 Windows 编译为空 rlib |

运行环境要求：

| 项目 | 要求 |
| --- | --- |
| 操作系统 | Windows 10/11 x64；本库以 `#![cfg(windows)]` 编译 |
| 进程权限 | 读取 HKLM/HKCU 注册表、枚举进程、启动子进程 |
| 网络 | 打开托管包时需要访问包索引所在的 S3 兼容存储；本地已下载时不需要 |
| 磁盘 | 托管包下载到**宿主进程当前工作目录**下的 `designsoft/`，该目录必须可写 |
| 线程 | 调用线程会做 COM STA 初始化（CorelDRAW），不要把相关会话跨线程传递 |
| 超时 | `design_software_start` 建议 ≥ 60 秒（下载大包或等待应用窗口） |

> `designsoft.dll` 每个接口的输出缓冲区至少 **64 KiB**（`OUTPUT_BUFFER_CAPACITY`，
> 含结尾 NUL）。缓冲区不足时 FFI 返回 `-2`，MCP server 应扩容后重试或报内部错误。

---

## 3. 通用调用约定

### 3.1 FFI 返回值

| 返回值 | 含义 | MCP 处理 |
| --- | --- | --- |
| `0` | 已写入合法 JSON，业务结果看 `code` 字段 | 正常解析 |
| `-1` | `json_out` 为空指针 | server 内部错误 |
| `-2` | 响应超过缓冲区 | server 内部错误（或扩容重试） |

### 3.2 响应结构

所有接口统一返回：

```json
{ "code": 200, "data": {}, "msg": "success" }
```

- `code = 200` 表示业务成功；其余为业务错误。
- `msg` 是简短英文标识（如 `success`、`started`、`not_found`）；
  错误时 `msg` 是可直接展示给用户的中文描述，例如
  `"未找到匹配的软件"`、`"存在多个版本，请指定版本号: 29.0.0, 29.1.0"`。

### 3.3 错误码

| code | 含义 | 典型触发 |
| --- | --- | --- |
| `200` | 成功 | — |
| `400100` | `json_in` 为空指针 | server 传了 null |
| `400101` | 入参不是合法 UTF-8 | 编码错误 |
| `400102` | 入参不是合法 JSON、字段不匹配或 `entry` 非法 | 缺少 `keyword`、类型错误、`entry` 不是 `gui`/`cli` |
| `400103` | `keyword` 为空 | 传了空字符串 |
| `500200` | 查询、启动或状态检测失败 | 软件未安装、版本不存在、下载失败 |

MCP 映射建议：

- `400100`/`400101`/`400102`/`400103` → JSON-RPC `-32602 Invalid params`，
  或工具结果 `isError: true`（二选一，建议统一用工具结果，便于模型看到原因）；
- `500200` → 工具结果 `isError: true`；
- FFI 返回 `-1`/`-2` → JSON-RPC `-32603 Internal error`。

---

## 4. 工具定义

### 4.1 `design_software_list`

**描述（可直接作为工具 description）**

> 列出本机已安装的设计软件，按关键字做不区分大小写的名称包含匹配。返回名称、
> 版本与安装路径，包含同一软件的多个版本。只读操作，不会启动任何程序。

**输入 Schema**

```json
{
  "type": "object",
  "properties": {
    "keyword": {
      "type": "string",
      "description": "软件名称关键字，例如 illustrator、photoshop、coreldraw、blender",
      "minLength": 1
    }
  },
  "required": ["keyword"],
  "additionalProperties": false
}
```

**输出 Schema**

```json
{
  "type": "object",
  "properties": {
    "code": { "type": "integer" },
    "msg": { "type": "string" },
    "data": {
      "type": "object",
      "properties": {
        "list": {
          "type": "array",
          "items": {
            "type": "object",
            "properties": {
              "name": { "type": "string" },
              "version": { "type": "string" },
              "major_version": { "type": "string" },
              "minor_version": { "type": "string" },
              "install_path": { "type": "string" }
            },
            "required": ["name", "version", "major_version", "minor_version", "install_path"]
          }
        }
      },
      "required": ["list"]
    }
  },
  "required": ["code", "msg", "data"]
}
```

**示例**

请求：

```json
{ "keyword": "illustrator" }
```

响应：

```json
{
  "code": 200,
  "msg": "success",
  "data": {
    "list": [
      {
        "name": "Adobe Illustrator 2025",
        "version": "29.1.0.128",
        "major_version": "29",
        "minor_version": "1.0.128",
        "install_path": "C:\\Program Files\\Adobe\\Adobe Illustrator 2025"
      }
    ]
  }
}
```

无匹配时返回 `code = 200`、`msg = "not_found"`、`data.list = []`。这不是错误，
工具结果应标记 `isError: false`，让模型自行决定下一步。

**注意**

- `install_path` 优先取注册表 `InstallLocation`，缺失时由 `DisplayIcon` 推断，
  仍无法确定时为空字符串；
- 版本号优先取 MSI 的 `ProductVersion`，其次取 `DisplayVersion`；
- 托管包没有注册表条目，`design_software_list` 不会列出（无论是否已下载）；
  查看托管包请用 `design_software_status` 或资源
  `designsoft://software/catalog`。

**MCP 工具结果示例**

```json
{
  "content": [
    { "type": "text", "text": "找到 1 个匹配的软件：Adobe Illustrator 2025 29.1.0.128" }
  ],
  "structuredContent": {
    "code": 200,
    "msg": "success",
    "data": {
      "list": [
        {
          "name": "Adobe Illustrator 2025",
          "version": "29.1.0.128",
          "major_version": "29",
          "minor_version": "1.0.128",
          "install_path": "C:\\Program Files\\Adobe\\Adobe Illustrator 2025"
        }
      ]
    }
  },
  "isError": false
}
```

---

### 4.2 `design_software_start`

**描述**

> 打开指定设计软件。已在运行时直接返回成功（幂等）。打开方式由本库的路由规则
> 决定：COM 接口、托管包下载或本地 exe。托管包首次打开时会下载到本地再启动，
> 可用 `entry` 选择 GUI 或 CLI。

**输入 Schema**

```json
{
  "type": "object",
  "properties": {
    "keyword": {
      "type": "string",
      "description": "软件名称关键字，例如 coreldraw、illustrator、photoshop",
      "minLength": 1
    },
    "version": {
      "type": "string",
      "description": "可选。完整版本号或主版本号；省略时要求关键字唯一匹配一个版本",
      "minLength": 1
    },
    "entry": {
      "type": "string",
      "enum": ["gui", "cli"],
      "description": "可选。托管包的启动入口：gui（默认）或 cli；包只有 CLI 时会自动使用 cli"
    }
  },
  "required": ["keyword"],
  "additionalProperties": false
}
```

**输出 Schema**

```json
{
  "type": "object",
  "properties": {
    "code": { "type": "integer" },
    "msg": { "type": "string" },
    "data": {
      "type": "object",
      "properties": {
        "message": { "type": "string" },
        "entry": { "type": "string", "enum": ["gui", "cli"] }
      },
      "required": ["message"]
    }
  },
  "required": ["code", "msg", "data"]
}
```

**示例**

请求：

```json
{ "keyword": "illustrator", "version": "29" }
```

响应：

```json
{
  "code": 200,
  "msg": "started",
  "data": { "message": "软件已启动或已在运行" }
}
```

托管包带入口时，响应会附带实际使用的入口：

```json
{
  "code": 200,
  "msg": "started",
  "data": { "message": "软件已启动或已在运行", "entry": "cli" }
}
```

**打开方式与行为**

| 打开方式 | 触发条件 | 行为 |
| --- | --- | --- |
| COM | 关键字命中内置的 COM 路由（CorelDRAW） | 通过 COM 打开并显示主窗口；未运行时由 COM 启动；已有实例会被置前 |
| 托管包 | 关键字命中包路由，且包已发布到包索引 | 读取包索引 → 下载（首次或版本变化）→ 按 `entry` 启动 GUI 或 CLI |
| 本地 exe | 其余所有关键字（默认） | 解析注册表安装路径 → 校验 exe → 启动 |

补充语义：

- **幂等**：本地安装类软件如果目标 exe 已在运行，直接返回成功，不会重复启动；
- **版本**：`version` 支持完整版本（`29.1.0.128`）或主版本（`29`）。关键字匹配到
  多个版本且未指定 `version` 时返回 `500200`，`msg` 会列出可选版本；
- **托管包忽略 `version`**：包版本由部署方的包索引决定，传入的 `version`
  不参与选择；
- **入口选择**：`entry` 只对托管包生效。省略时优先 GUI，包没有 GUI 时回退 CLI；
  显式请求 `entry: "cli"` 而包没有 CLI 时返回 `500200`（`msg` 为
  `"软件包没有 cli 入口"`）。COM / 本地 exe 会忽略 `entry`；
- **下载位置**：宿主进程当前工作目录下的 `designsoft/`，版本记录在
  `designsoft/versions.json`；
- **下载失败**：本次已写入的文件会被清理，`versions.json` 不更新；
- **COM 等待**：CorelDRAW 启动后会把主窗口置前，最多等待约 5 秒；COM 绑定本身
  最多重试 10 次（间隔 300 ms）。

**错误示例**

未安装：

```json
{
  "code": 500200,
  "msg": "未找到匹配的软件",
  "data": {}
}
```

多版本未指定：

```json
{
  "code": 500200,
  "msg": "存在多个版本，请指定版本号: 29.0.0, 29.1.0",
  "data": {}
}
```

包索引中没有对应包：

```json
{
  "code": 500200,
  "msg": "线上没有找到设计软件包 '<name>'，无法打开",
  "data": {}
}
```

**MCP 工具结果建议**

- 成功时 `isError: false`，`content` 返回 `data.message`；
- 失败时 `isError: true`，`content` 返回 `msg`，同时保留 `structuredContent`。

---

### 4.3 `design_software_status`

**描述**

> 查询设计软件是否已启动。只读操作：不会启动软件，也不会访问网络。对本机安装
> 软件按注册表安装信息检测进程；对托管包检查本地 `designsoft/versions.json`
> 与进程。返回 `"已启动"` 或 `"已安装但未启动"`。

**输入 Schema**

与 `design_software_start` 相同（不含 `entry`）：

```json
{
  "type": "object",
  "properties": {
    "keyword": { "type": "string", "minLength": 1 },
    "version": { "type": "string", "minLength": 1 }
  },
  "required": ["keyword"],
  "additionalProperties": false
}
```

**输出 Schema**

```json
{
  "type": "object",
  "properties": {
    "code": { "type": "integer" },
    "msg": { "type": "string" },
    "data": {
      "type": "object",
      "properties": {
        "status": { "type": "string", "enum": ["已启动", "已安装但未启动"] },
        "entries": {
          "type": "array",
          "description": "托管包的各入口明细（GUI / CLI）",
          "items": {
            "type": "object",
            "properties": {
              "entry": { "type": "string", "enum": ["gui", "cli"] },
              "exe": { "type": "string" },
              "running": { "type": "boolean" }
            },
            "required": ["entry", "exe", "running"]
          }
        }
      },
      "required": ["status"]
    }
  },
  "required": ["code", "msg", "data"]
}
```

**示例**

本机安装软件：

```json
{
  "code": 200,
  "msg": "success",
  "data": { "status": "已启动" }
}
```

托管包：

```json
{
  "code": 200,
  "msg": "success",
  "data": {
    "status": "已安装但未启动",
    "entries": [
      { "entry": "gui", "exe": "example.exe", "running": false },
      { "entry": "cli", "exe": "example-cli.exe", "running": false }
    ]
  }
}
```

**托管包的状态语义**

- 本地未下载：`500200`，`msg` 为 `"软件包 '<name>' 尚未下载到 <目录>"`；
- 已下载但文件被删除：`500200`，`msg` 为 `"软件包 '<name>' 的文件缺失: <路径>"`；
- 已下载：`status` 表示默认入口（GUI 优先）的运行状态，`entries` 给出每个入口的
  进程检测结果；
- 包只有 CLI 时，默认入口就是 CLI。

> 若托管包中的 CLI 是一次性命令（启动后立即退出），CLI 入口通常为
> `running: false`，这是预期行为。

---

## 5. 软件目录与关键字

本库不内置固定软件清单：任何已安装软件都可以用其名称关键字查询和启动。
只有两类关键字有特殊处理：

| 类别 | 说明 | 关键字示例 |
| --- | --- | --- |
| COM | 通过 COM 接口打开（当前为 CorelDRAW） | `coreldraw`、`corel draw` |
| 本地 exe | 按注册表安装信息启动，默认方式 | 任意已安装软件名 |

此外，部署方可以把便携包登记到包索引中，并让本库按包名路由；包名与内容由部署方
决定，本文档不假设任何具体包。

常见设计软件的关键字示例（均走默认的本地 exe 方式）：

| 软件 | keyword 别名 |
| --- | --- |
| Adobe Illustrator | `illustrator` |
| Adobe Photoshop | `photoshop` |
| Adobe InDesign | `indesign` |
| AutoCAD | `autocad` |
| 3ds Max | `3ds max`、`3dsmax` |
| Blender | `blender` |
| SketchUp | `sketchup` |
| Affinity Designer | `affinity designer` |
| Affinity Photo | `affinity photo` |
| Affinity Publisher | `affinity publisher` |
| Cinema 4D | `cinema 4d`、`cinema4d` |
| SolidWorks | `solidworks` |

关键字匹配规则：去掉首尾空白后转小写，做**包含匹配**；同一关键字命中多个路由时
按库内顺序取第一个。未命中的关键字仍按本地 exe 处理，因此不需要预先登记。

---

## 6. MCP 资源（可选）

建议暴露两个只读资源，便于模型在不调用工具的情况下了解环境。

### 6.1 `designsoft://software/catalog`

返回软件目录（静态说明 + 托管包状态 + 本机安装状态）：

```json
{
  "uri": "designsoft://software/catalog",
  "mimeType": "application/json",
  "name": "设计软件目录"
}
```

内容示例：

```json
{
  "packages": [
    { "name": "<package-name>", "keyword": "<keyword>", "online_version": "1.0.0" }
  ],
  "installed": [
    { "keyword": "illustrator", "name": "Adobe Illustrator 2025", "version": "29.1.0.128" }
  ]
}
```

> `installed` 需要调用 `post_info`（或对每个 keyword 调用），建议缓存 30–60 秒。

### 6.2 `designsoft://software/{keyword}/status`

按 keyword 返回状态，等价于 `design_software_status`：

```json
{
  "uri": "designsoft://software/illustrator/status",
  "mimeType": "application/json",
  "name": "软件状态"
}
```

内容：

```json
{ "keyword": "illustrator", "status": "已启动" }
```

---

## 7. MCP 提示词（可选）

```json
{
  "name": "open-design-software",
  "description": "查找并打开一个设计软件",
  "arguments": [
    { "name": "software", "description": "软件名称，例如 illustrator 或 coreldraw", "required": true }
  ]
}
```

提示词模板建议：

1. 调用 `design_software_list` 确认软件是否已安装；
2. 若未安装且部署方提供了托管包，可直接调用 `design_software_start`（会自动下载）；
3. 若返回多个版本，向用户确认后用 `version` 重试；
4. 用 `design_software_status` 确认启动结果。

---

## 8. 实现指南

### 8.1 通过 FFI 调用 DLL

C 签名：

```c
int post_info(const char *json_in, char *json_out);
int post_start(const char *json_in, char *json_out);
int post_status(const char *json_in, char *json_out);
```

Rust 示例（`libloading`）：

```rust
use libloading::{Library, Symbol};
use std::ffi::{c_char, CString};

let lib = unsafe { Library::new("designsoft.dll")? };
let post_status: Symbol<unsafe extern "C" fn(*const c_char, *mut c_char) -> i32> =
    unsafe { lib.get(b"post_status\0")? };

let input = CString::new(r#"{"keyword":"illustrator"}"#)?;
let mut out = vec![0 as c_char; 64 * 1024];
let ret = unsafe { post_status(input.as_ptr(), out.as_mut_ptr()) };
assert_eq!(ret, 0);

let response = unsafe { std::ffi::CStr::from_ptr(out.as_ptr()) }.to_str()?;
println!("{response}");
```

也可以让 MCP server 直接依赖本 crate（`rlib`），调用
`post_info` / `post_start` / `post_status`，省去动态加载。

### 8.2 缓冲区

- 固定分配 **64 KiB**（含结尾 NUL）；
- 返回 `-2` 时不要截断 JSON：扩容到 128 KiB 重试一次，仍失败则报内部错误；
- 返回 `-1` 表示 `json_out` 为空指针，属于 server 编码错误。

### 8.3 线程与 COM

- `post_start` 对 CorelDRAW 会在**调用线程**初始化 STA 并释放；
- 不要在异步任务之间传递 COM 对象；每次工具调用在固定线程内完成即可；
- 若 MCP server 使用 tokio，建议用 `spawn_blocking` 包住 FFI 调用，避免阻塞
  reactor；不要在 `spawn_blocking` 之外复用 CorelDRAW 相关状态。

### 8.4 超时与取消

| 工具 | 建议超时 | 说明 |
| --- | --- | --- |
| `design_software_list` | 10 s | 注册表遍历，通常 < 1 s |
| `design_software_status` | 10 s | 进程枚举，通常 < 1 s |
| `design_software_start` | 60–180 s | 首次下载托管包可能较大；COM 启动等待窗口 |

MCP 本身没有取消机制时，超时后不要重复调用 `start`，以免并发下载；先调用
`status` 确认。

### 8.5 并发

- 同一进程内对 `design_software_start` 做**串行化**（全局互斥锁）；
- 多进程同时打开同一托管包会竞争 `designsoft/versions.json`，当前没有文件锁，
  建议宿主保证单实例；
- `list` / `status` 可以并发，但都涉及注册表/进程枚举，建议限制并发数。

### 8.6 工作目录

托管包下载到宿主进程的**当前工作目录**下的 `designsoft/`。MCP server 启动时应：

- 固定 `current_dir` 到一个可写目录（如 `%LOCALAPPDATA%\designsoft`）；
- 不要在 `C:\Program Files` 等只读目录下启动；
- 同一宿主不要在多处使用不同 CWD 调用，否则会下载到不同位置。

---

## 9. 安全注意事项

- **不在工具描述或资源中暴露 S3 凭证**；凭证固化在库内，MCP 层不应读取或转发；
- `design_software_start` 可能下载并执行远端 exe，属于供应链风险：
  - 建议在受信任环境中运行；
  - 可对 `keyword` 做白名单（只允许需要的软件）；
  - 高风险场景可要求用户确认后再调用；
- `keyword` 只做名称匹配，不拼接到 shell 命令；不要把它当作命令行参数传给其它进程；
- 托管包当前没有签名/哈希校验，下载完整性依赖存储通道。

---

## 10. 版本与兼容性

| 项目 | 值 |
| --- | --- |
| 发布名 | `designsoft_lib` |
| crate / DLL 版本 | `1.0.2` |
| 打包产物 | `target/release/designsoft.dll` |
| FFI 接口 | `post_info`、`post_start`、`post_status` |
| 输出缓冲区契约 | 64 KiB，含结尾 NUL |
| MCP 协议 | 2025-06-18（stdio），工具/资源结构向下兼容 |
| 平台 | 仅 Windows；非 Windows 编译为空 rlib |

版本升级时：

- 新增字段：保持向后兼容，旧字段不删除；
- 错误码：只新增，不复用；
- 打包产物路径与发布名保持一致，避免宿主加载失败。

---

## 11. 故障排查

| 现象 | 可能原因 | 处理 |
| --- | --- | --- |
| `post_info` 返回空列表 | 关键字不匹配；软件便携版未写注册表 | 换关键字；或确认该软件是否需要托管包方式 |
| `post_start` 返回 `500200` “未找到匹配的软件” | 本机未安装且没有对应托管包 | 确认安装情况与包索引 |
| `post_start` 返回 `500200` “存在多个版本” | 未指定 `version` | 传主版本号或完整版本号 |
| `post_start` 返回 `500200` “线上没有找到…” | 包名/索引未发布 | 核对包索引 |
| `post_status` 返回 `500200` “尚未下载” | 托管包未打开过 | 先调用 `start` |
| `post_status` 始终“已安装但未启动” | CLI 程序启动后立即退出 | 预期行为 |
| FFI 返回 `-2` | 响应超过 64 KiB | 扩容缓冲区；检查返回列表是否异常大 |
| `design_software_start` 卡住 | CorelDRAW 弹窗/未响应，或下载慢 | 增大超时；检查网络与目标进程 |
| 下载目录不在预期位置 | 宿主 CWD 不同 | 固定 MCP server 的 `current_dir` |

---

## 12. 附录：完整工具列表示例（`tools/list`）

```json
{
  "tools": [
    {
      "name": "design_software_list",
      "description": "列出本机已安装的设计软件，返回名称、版本与安装路径。只读，不会启动软件。",
      "inputSchema": {
        "type": "object",
        "properties": {
          "keyword": { "type": "string", "description": "软件名称关键字，例如 illustrator" }
        },
        "required": ["keyword"],
        "additionalProperties": false
      }
    },
    {
      "name": "design_software_start",
      "description": "打开指定设计软件。已在运行时直接返回成功；托管包首次打开会自动下载，可用 entry 选择 GUI 或 CLI。",
      "inputSchema": {
        "type": "object",
        "properties": {
          "keyword": { "type": "string", "description": "软件名称关键字，例如 coreldraw" },
          "version": { "type": "string", "description": "可选版本号或主版本号" },
          "entry": {
            "type": "string",
            "enum": ["gui", "cli"],
            "description": "托管包入口，默认 gui；包只有 CLI 时自动使用 cli"
          }
        },
        "required": ["keyword"],
        "additionalProperties": false
      }
    },
    {
      "name": "design_software_status",
      "description": "查询软件状态，返回“已启动”或“已安装但未启动”。只读，不会启动软件。",
      "inputSchema": {
        "type": "object",
        "properties": {
          "keyword": { "type": "string", "description": "软件名称关键字，例如 illustrator" },
          "version": { "type": "string", "description": "可选版本号或主版本号" }
        },
        "required": ["keyword"],
        "additionalProperties": false
      }
    }
  ]
}
```
