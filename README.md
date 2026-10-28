# designsoft

查询、检测并启动本机安装的 Windows 设计软件（Illustrator、Photoshop、CorelDRAW、AutoCAD 等）。

本库编译为动态库（`cdylib`），通过 C ABI 提供三个 JSON 接口，供宿主系统加载调用；另提供基于 `cdrsdk` 的 CorelDRAW COM 封装和线上 `design_soft` 包管理（Rust API，当前未导出为 FFI）。

## 功能

| FFI 接口 | 说明 |
| --- | --- |
| `post_info` | 按关键字列出已安装软件的名称、版本与安装路径 |
| `post_start` | 启动指定软件（可指定版本），已在运行时直接返回成功；按路由表选择 COM、线上包下载或本地 exe 启动 |
| `post_status` | 查询软件状态：`已启动` / `已安装但未启动` |

数据来源为注册表 `SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall` 的 64 位、32 位与当前用户三个视图；版本号优先取 MSI 的 `ProductVersion`，其次取 `DisplayVersion`。

> 仅支持 Windows。非 Windows 平台下 crate 编译为空的 rlib，不提供任何接口。

## 软件路由表

支持的软件及打开方式集中声明在 `utils::apps::APPS` 数组中，`post_start` 只按表路由：

| 软件 | 关键字（小写包含匹配） | 打开方式 |
| --- | --- | --- |
| CorelDRAW | `coreldraw`、`corel draw` | `OpenMethod::Com` → `utils::cdr` |
| CadCraft / DesignCraft / EffectCraft / FilmCraft / LightCraft / PDFCraft / VectorCraft | 软件名（如 `cadcraft`、`vectorcraft`） | `OpenMethod::Package("<包名>")` → `utils::pkg` |
| 其余常见设计软件 | `illustrator`、`photoshop`、`indesign`、`autocad`、`3ds max`、`blender`、`sketchup`、`affinity designer/photo/publisher`、`cinema 4d`、`solidworks` | `OpenMethod::Installed` → 注册表 + 本地 exe |

新增软件时只改数组，不需要改 `post_start`：

```rust
AppEntry {
    name: "Affinity Designer",
    aliases: &["affinity designer"],
    open: OpenMethod::Installed,
}
```

- `Installed` 是默认方式：按注册表安装信息解析可执行文件并启动；
- `Package` 的版本由线上 `design_soft/packages.json` 决定，请求里的 `version` 会被忽略；
  包内同时提供 GUI 与 CLI 时，默认启动 GUI，可用 `entry = "cli"` 启动命令行入口；
- 未命中路由表的关键字仍按 `Installed` 处理，任意已安装软件都可以启动。

## CorelDRAW COM 接口（Rust API）

`utils::cdr` 基于 [`cdrsdk`](https://docs.rs/cdrsdk) 的 COM late-binding 接口，用于打开 CorelDRAW 应用对象、查询状态和关闭，**当前不通过 FFI 导出**。

`post_start` 命中路由表中的 CorelDRAW 项时，会直接调用 `cdr::open_software` 通过 COM 打开并显示 CorelDRAW，不再走注册表 + exe 启动流程。

```rust
use designsoft::utils::cdr::{self, CdrSession};

// 打开并显示 CorelDRAW（未运行则自动启动）
let info = cdr::open_software(None)?;
println!("{info:?}");

// 打开 COM 接口：已运行则复用实例，未运行且已安装时自动启动
let session = CdrSession::open(None)?;
let info = session.info();          // version / program_path / document_count ...
let app = session.application();    // 继续使用 cdrsdk 的文档、图层、图形接口
drop(session);

// 状态：只查询，不会启动 CorelDRAW
let status = cdr::status(None)?;
assert!(status.running);

// 关闭：Quit 优雅退出，默认等待 10 秒后强制结束进程
cdr::close_with_default_timeout(None)?;
```

注意事项：

- COM 在调用线程以 STA 初始化；`CdrSession` 持有初始化守卫，**必须在创建它的线程上使用和释放**（`cdrsdk` 的类型均为 `!Send`）。
- `close` 发送 `Quit` 后轮询进程退出，超时后对 `CorelDRW.exe` 执行 `TerminateProcess`。存在未保存文档时，`Quit` 可能弹出保存对话框并一直等到超时。
- `CdrSession::open` 启动的实例默认不可见；`open_software` 会设置 `Visible = true`、把主窗口置前，并在无文档时短暂创建/关闭一个文档，确保 COM 客户端释放后窗口仍然显示。
- 只对 `CdrSession` 调用 `set_visible(true)` 时，最后一个 COM 客户端断开后 CorelDRAW 会重新隐藏窗口。
- CorelDRAW 的 `version` 属性可能带本地化前缀（如 `版本 26.1.0.143`），判断版本请优先用 `version_major`。

## 线上设计软件包（Rust API）

`utils::pkg` 用于按名称打开托管在 `design_soft/` 发布目录中的便携软件包
（发布规则见 [`docs/soft.md`](docs/soft.md)）。它通过 `post_start` / `post_status`
的关键字路由间接使用，未单独导出 FFI。

```rust
use designsoft::utils::pkg::{self, Entry};

// 查询线上包版本；包不存在时返回 Ok(None)
let version = pkg::lookup("vectorcraft")?;

// 打开：线上没有该包直接报错；本地缺失或版本落后时先下载再启动
let opened = pkg::open("vectorcraft")?;
println!("{} {} {} -> {}", opened.name, opened.version, opened.entry.as_str(), opened.exe_path.display());

// 指定入口：包同时提供 GUI 与 CLI 时按需选择
let cli = pkg::open_with("vectorcraft", Entry::Cli)?;

// 状态：只读本地记录与进程，不访问网络、不启动软件
let status = pkg::status("vectorcraft")?;
for entry in &status.entries {
    println!("{} {} running={}", entry.entry.as_str(), entry.exe, entry.running);
}
```

行为：

- 先读取线上 `design_soft/packages.json`，并校验 `design_soft/<name>/dytes.json` 的
  `version` 与索引一致；
- 本地目录默认是当前工作目录下的 `designsoft/`（`pkg::open_in` / `pkg::status_in`
  可指定其它目录），版本记录保存在 `designsoft/versions.json`；
- 本地版本一致且入口文件存在时直接启动；否则按 `exports` / `platform-exports`
  下载导出文件后启动，下载中途失败会清理本次已写入的文件；
- 入口按文件名区分：`<name>-cli.exe` 为 CLI，其余 `.exe` 为 GUI；省略入口时
  GUI 优先，包只有 CLI 时回退 CLI；
- `status` 未下载时返回错误，文件缺失时返回错误，否则返回默认入口状态与
  `entries` 明细；
- 仅支持便携包：导出目标（`exports` 右侧的文件名）必须是单层文件名，且至少一个
  `.exe`；目录需先打包。

## 构建与测试

宿主若通过 MCP 暴露这些接口，协议、工具 Schema 与错误语义见
[`docs/mcp.md`](docs/mcp.md)。

```bash
cargo build --release   # 产物：target/release/designsoft.dll
cargo test              # 部分用例依赖本机已安装的设计软件，未安装时自动跳过
cargo clippy --all-targets
cargo fmt --check
```

## FFI 契约

### 通用约定

- 入参 `json_in`：以 NUL 结尾的 UTF-8 JSON 字符串。
- 出参 `json_out`：调用方分配的缓冲区，容量至少 **64 KiB**（`OUTPUT_BUFFER_CAPACITY`，含结尾 NUL）。
- 返回值是「写入结果」，不是业务结果：

  | 返回值 | 含义 |
  | --- | --- |
  | `0` | 已写入合法 JSON（成功或业务错误都在 JSON 的 `code` 字段中） |
  | `-1` | `json_out` 为空指针 |
  | `-2` | 响应超过缓冲区容量 |

- 响应统一结构：

  ```json
  { "code": 200, "data": {}, "msg": "success" }
  ```

### 错误码

| code | 含义 |
| --- | --- |
| `200` | 成功 |
| `400100` | `json_in` 为空指针 |
| `400101` | 入参不是合法的 UTF-8 |
| `400102` | 入参不是合法 JSON 或字段不匹配 |
| `400103` | `keyword` 为空 |
| `500200` | 查询或启动软件失败 |

### `post_info`

请求：

```json
{ "keyword": "illustrator" }
```

响应 `data.list`（无匹配时为空数组，`msg` 为 `not_found`）：

```json
{
  "name": "Adobe Illustrator 2025",
  "version": "29.1.0.128",
  "major_version": "29",
  "minor_version": "1.0.128",
  "install_path": "C:\\Program Files\\Adobe\\Adobe Illustrator 2025"
}
```

`install_path` 优先取注册表 `InstallLocation`，缺失时由 `DisplayIcon` 推断，仍无法确定时为空字符串。

### `post_start`

请求（`version` 可省略；支持完整版本或主版本号）：

```json
{ "keyword": "illustrator", "version": "29" }
```

成功响应：`{ "code": 200, "data": { "message": "软件已启动或已在运行" }, "msg": "started" }`。

关键字匹配到多个版本且未指定 `version` 时返回 `500200`，错误信息会列出可选版本。
打开方式由[软件路由表](#软件路由表)决定：CorelDRAW 走 COM，VectorCraft 走线上包下载，
其余（含未命中路由表的关键字）走注册表 + 本地 exe。

### `post_status`

请求同 `post_start`，成功响应：

```json
{ "code": 200, "data": { "status": "已启动" }, "msg": "success" }
```

`OpenMethod::Package`（如 VectorCraft）只检查本地 `designsoft/versions.json` 与进程：
未下载或文件缺失时返回 `500200`；其余软件按注册表安装信息检测。

## 目录结构

```
src/
  lib.rs              crate 入口与 FFI 导出
  ffi/
    common.rs         入参解析与校验
    info.rs           post_info
    start.rs          post_start
    status.rs         post_status
  utils/
    apps.rs           设计软件路由表（COM / 线上包 / 本地 exe）
    cdr.rs            CorelDRAW COM 接口（cdrsdk）
    cjson.rs          C 缓冲区 JSON 输出
    data.rs           可执行文件名推断
    pkg.rs            线上 design_soft 包查询与下载启动
    soft.rs           注册表查询
    status.rs         进程检测与启动
```

## 许可证

MIT，见 [LICENSE](LICENSE)。
