# CorelDRAW：打开 / 关闭 / 状态查看

> 状态：Rust API 与 `post_start` 分流已完成；FFI 仍未单独导出
> 更新日期：2026-10-28
> 实现位置：`src/utils/cdr.rs`（基于 `cdrsdk` 1.0.0）

## 1. 现状

| 能力 | 状态 | 入口 |
| --- | --- | --- |
| 打开（显示界面） | 已实现 | `post_start`（路由表命中 `coreldraw`）/ `cdr::open_software` |
| 打开 COM 接口 | 已实现 | `cdr::CdrSession::open(version)` |
| 状态查看 | 已实现（不会启动 CorelDRAW） | `cdr::status(version)` |
| 关闭 | 已实现（Quit + 超时强制结束） | `cdr::close(version, timeout)` / `close_with_default_timeout` |
| 独立 FFI 导出 | 未实现（当前决定只做 Rust API） | — |

`post_start` 对 CorelDRAW 不再走“注册表查 exe + `Command::spawn`”，而是由
`utils::apps::APPS` 路由表命中后直接调用 `cdr::open_software`，由 cdrsdk 的 COM
接口打开并显示应用；其它软件按路由表选择线上包下载或本地 exe 启动。

## 2. 已验证行为

2026-10-28 在本机 CorelDRAW `26.1.0.143` 上实测：

- 打开：CorelDRAW 未运行时自动启动，`version = "版本 26.1.0.143"`，`version_major = 26`，
  `program_path = D:\Program Files\Corel\CorelDRAW Graphics Suite\26\Programs64\`；
- 窗口显示：`open_software` 会设置 `Visible = true` 并置前；无文档实例会先创建/关闭一个文档，
  COM 客户端释放后 `visible` 仍为 `true`，窗口保持显示 15 秒以上（`tests/cdr_test.rs` 验证）；
- `post_start`：请求 `{"keyword":"CorelDRAW"}` 返回
  `{"code":200,"data":{"message":"软件已启动或已在运行"},"msg":"started"}`；
- 状态：`running = true`，读到的版本与 session 一致；
- 关闭：`Quit` 后进程退出，随后 `status` 返回 `running = false`；
- 幂等：未运行时调用关闭返回成功，重复调用仍成功；
- 测试：`tests/cdr_test.rs`，默认 `#[ignore]`，需显式运行：

  ```bash
  cargo test --test cdr_test -- --ignored --nocapture --test-threads=1
  ```

## 3. 接口

```rust
use designsoft::utils::cdr::{self, CdrSession};

// 打开并显示 CorelDRAW（未运行则自动启动）
let info = cdr::open_software(None)?;

// 或只拿 COM 接口（不改变可见性）
let session = CdrSession::open(None)?;
let app = session.application();   // 可继续使用 cdrsdk 的文档/图层/图形接口
session.set_visible(true);
drop(session);

// 查询状态（不启动）
let status = cdr::status(None)?;

// 关闭：默认等待 10 秒，超时后 TerminateProcess
cdr::close_with_default_timeout(None)?;
```

约束与实现细节：

- COM 以 STA 在调用线程初始化，`CdrSession` 必须在同一线程使用和释放。
- 绑定带重试：10 次、间隔 300 ms，覆盖 CorelDRAW 刚启动/刚退出时类工厂短暂不可用的窗口。
- 关闭时若 COM 绑定失败但进程仍在运行，退回按 `CorelDRW.exe` 强制结束。
- 进程名从 `ProgramPath` 目录下的 `CorelDRW.exe` / `CorelDRAW.exe` 解析，失败回退 `CorelDRW.exe`。

## 4. 待办（未完成）

- [ ] 决定是否新增独立 FFI：`post_cdr_open` / `post_cdr_close` / `post_cdr_status`
- [ ] 清洗 `version` 的本地化前缀（`"版本 26.1.0.143"` → `"26.1.0.143"`）
- [ ] 验证存在未保存文档时的关闭行为（`Quit` 弹窗 + 超时强制）
- [ ] 多版本共存时 ProgID 的选择策略（当前：指定版本 → 无版本 ProgID → v26~v20 回退）
- [ ] 同时打开多个 CorelDRAW 版本时的隔离（COM `!Send`，需每线程一个 session）
- [ ] 重启能力（关闭后立即打开）
- [ ] 按路径打开文档的便捷封装（`app.open_document(path)` 已可用，未封装）

## 5. 技术要点与风险

- **COM 线程模型**：`cdrsdk` 类型是 `!Send` / `!Sync`，每个线程单独创建 `CdrSession`。
- **`ProgramPath` 是目录**：不能直接取 `file_name()`，否则会得到 `Programs64` 并导致关闭时等错进程。
- **启动时序**：刚关闭后立即重新打开时，`CoCreateInstance` 可能短暂失败，已通过重试覆盖。
- **未保存文档**：`Quit` 触发保存提示，进程可能直到超时都不退出；默认 10 秒后强制结束会丢失未保存内容。
- **启动可见性**：`CdrSession::open` 启动的实例默认不可见，且最后一个自动化客户端断开时 CorelDRAW 会重新隐藏窗口；`open_software` 通过 `Visible = true` + 创建/关闭一个文档切换到普通文档模式，并调用 `ShowWindow`/`SetForegroundWindow` 置前。
- **版本号格式**：COM 的 `version` 可能带本地化前缀，程序判断应使用 `version_major`。
