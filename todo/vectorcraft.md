# VectorCraft：发布与打开

> 状态：打开与状态查看已端到端验证；未单独导出 FFI
> 更新日期：2026-10-28
> 相关实现：`src/utils/pkg.rs`（客户端）、`src/utils/apps.rs`（路由）、`docs/soft.md`（发布规则）

## 1. 现状

| 能力 | 状态 | 入口 |
| --- | --- | --- |
| 线上版本查询 | 已实现并验证 | `pkg::lookup("vectorcraft-cli")` |
| 线上包索引 | 已发布 | `design_soft/packages.json` |
| 包清单 | 已发布 | `design_soft/vectorcraft-cli/dytes.json` |
| 下载并记录版本 | 已实现并验证 | `pkg::open` / `pkg::open_in` |
| 启动 exe | 已实现并验证 | `pkg::open` 下载完成后 `Command::spawn` |
| `post_start` 路由 | 已实现并验证 | 关键字含 `vectorcraft` → `pkg::open("vectorcraft-cli")` |
| 状态查看 | 已实现并验证 | `pkg::status` / `pkg::status_in`，`post_status` 路由 |
| 独立 FFI 导出 | 未实现（当前决定只做 Rust API） | — |
| 更新 / 回滚 / 卸载 | 未实现 | — |

截至 2026-10-28 的线上内容：

`design_soft/packages.json`：

```json
{ "packages": { "vectorcraft-cli": "0.9.0" }, "updated_at": "2026-10-23T17:09:01.944272100+00:00" }
```

`design_soft/vectorcraft-cli/dytes.json`：

```json
{
  "name": "vectorcraft-cli",
  "version": "0.9.0",
  "type": "design_soft",
  "exports": {
    "./target/release/vectorcraft-cli.exe": "vectorcraft-cli.exe"
  }
}
```

## 2. 已验证行为

2026-10-28 实测：

- 查询：
  - `pkg::lookup("vectorcraft-cli")` 返回 `Ok(Some("0.9.0"))`；
  - `pkg::lookup("this-package-should-not-exist-xyz")` 返回 `Ok(None)`。
- 打开（`pkg::open_in`，临时目录）：
  - 下载 `vectorcraft-cli.exe`（53,991,936 字节，
    sha256 `d83d6c842ed03034c908a176217da83260593078f40bf18ce0675324c670e09f`）；
  - 返回 `version = "0.9.0"`、`downloaded = true`，`versions.json` 内容为
    `{"packages":{"vectorcraft-cli":{"version":"0.9.0","exe":"vectorcraft-cli.exe"}}}`；
  - exe 启动后打印 CLI 帮助并退出（约 1 秒内），进程映像名为 `vectorcraft-cli.exe`；
    启动瞬间可以检测到进程，随后回到未运行。
- 状态（`pkg::status_in`）：返回 `version = "0.9.0"`、`running = false`。
- FFI：
  - `post_start {"keyword":"vectorcraft"}` →
    `{"code":200,"data":{"message":"软件已启动或已在运行"},"msg":"started"}`；
  - `post_status {"keyword":"vectorcraft"}` →
    `{"code":200,"data":{"status":"已安装但未启动"},"msg":"success"}`。
- 测试：`tests/pkg_test.rs`，默认 `#[ignore]`，需访问线上 S3：

  ```bash
  cargo test --test pkg_test -- --ignored --nocapture --test-threads=1
  ```

- 纯本地逻辑（`pick_exe`、平台 exports 选择、`versions.json` 读写、本地状态查询）
  有单元测试覆盖，`cargo test` 全绿。

## 3. 打开流程

```rust
use designsoft::utils::pkg;

let version = pkg::lookup("vectorcraft-cli")?;      // 只查索引
let opened = pkg::open("vectorcraft-cli")?;         // 必要时下载到 ./designsoft 并启动
println!("{} {} -> {}", opened.name, opened.version, opened.exe_path.display());
```

1. 读取 `design_soft/packages.json`，包不存在返回 `Ok(None)` / 错误；
2. 读取 `design_soft/vectorcraft-cli/dytes.json`，校验 `version` 与索引一致；
3. 本地 `designsoft/versions.json` 版本一致且 exe 存在 → 直接启动；
4. 否则按 `exports`（或 `platform-exports[平台]`）把文件下载到 `designsoft/`，
   写入 `versions.json` 后启动；
5. 任一文件下载/写入失败会清理本次已写入的文件，版本记录不更新。

## 4. 待办（未完成）

- [ ] 下载完整性校验：`dytes.json` 增加 sha256/size，客户端校验后再启动
- [ ] 版本升级策略：本地版本落后时是否覆盖运行中的 exe、是否需要临时文件 + 重命名
- [ ] 并发与重入：多进程同时 `open` 同一包时对 `designsoft/` 加文件锁
- [ ] 失败恢复：下载中断后残留文件、`versions.json` 与磁盘内容不一致的处理
- [ ] 多平台发布：目前 `exports` 只有 Windows `.exe`，`platform-exports` 未使用
- [ ] 目录型产物：当前导出项只允许单层文件名，资源目录需先打包为单文件
- [ ] 卸载 / 清理：删除 `designsoft/<name>` 相关文件并更新版本记录

## 5. 技术要点与风险

- **本地目录基于当前工作目录**：宿主若在只读目录或系统目录下加载 DLL，`designsoft/`
  可能创建失败；需要时可让宿主通过 `open_in` 指定可写目录。
- **索引与清单两次请求**：`remote_package` 先查索引再取 `dytes.json`，二者版本不一致
  会直接报错，避免发布中途状态被使用。
- **覆盖风险**：同名导出文件会直接覆盖本地文件；若 exe 正在运行，Windows 上写入会失败。
- **无签名/哈希**：当前信任 S3 通道与发布流程，未校验产物来源。
- **CLI 的“打开”是运行一次**：`vectorcraft-cli` 无参数时打印帮助后立即退出，
  因此 `post_status` 通常返回“已安装但未启动”；它没有常驻进程可供检测。
- **`post_status` 与 `pkg::status` 不访问网络**：状态只读本地 `versions.json`
  和进程表；未下载或文件缺失时返回 `500200`。
- **仅 Windows**：整个 crate 以 `#![cfg(windows)]` 编译，`current_platform()` 仍保留
  `PIKSEL_PLATFORM` 覆盖以便未来扩展。
