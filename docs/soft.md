# design_soft 发布目录

用于发布设计软件本体，例如：

- VectorCraft（矢量设计）
- pdfcraft（PDF 设计 / 处理）
- 其他基于 Piksel 体系的设计软件

## 发布规则

| 项目 | 值 |
| --- | --- |
| `dytes.json` 的 `type` | `design_soft` |
| S3 包路径 | `design_soft/<name>/<version>/` |
| 包索引 | `design_soft/packages.json` |
| 版本 | 合法 semver，例如 `1.0.0` |

## 目录约定

每个软件一个子目录，各自带 `dytes.json` 和发布产物：

```
design_soft/
├── README.md
├── dytes.json.example
└── vectorcraft/
    ├── dytes.json
    ├── vectorcraft.exe
    └── assets...
```

> 不要在 `design_soft/` 根目录直接放 `dytes.json`。`publish` 读取的是**当前工作目录**的清单。

## 发布步骤

1. 复制模板：

   ```bash
   cp dytes.json.example vectorcraft/dytes.json
   ```

2. 修改 `name`、`version`、`exports`，确认安装包 / 可执行文件已就位；
3. 进入软件目录，运行：

   ```bash
   cd design_soft/vectorcraft
   ../../publish/target/release/publish.exe
   ```

4. 发布工具会上传产物、`dytes.json`，并更新 `design_soft/packages.json`。

## 客户端打开

`designsoft` 的 `utils::pkg` 负责消费上述产物（目前为 Rust API，未导出 FFI）：

```rust
use designsoft::utils::pkg;

let version = pkg::lookup("vectorcraft-cli")?;   // 读 design_soft/packages.json
let opened = pkg::open("vectorcraft-cli")?;      // 必要时下载到 ./designsoft 并启动
```

客户端约定：

- `packages.json` 的 `packages.<name>` 必须与 `design_soft/<name>/dytes.json` 的
  `version` 一致，否则拒绝打开；
- `exports` 的导出目标（右侧文件名）只支持单层文件名（不能带 `/` 或 `\`），
  且至少有一个 `.exe` 作为启动入口；左侧源路径只在发布端使用，客户端不读取；
- 存在 `platform-exports` 时按平台取子表，对象路径为
  `design_soft/<name>/<version>/<platform>/<目标文件名>`，否则为
  `design_soft/<name>/<version>/<目标文件名>`；
- 下载文件与 `versions.json` 记录在客户端当前工作目录的 `designsoft/` 下。

## 产物建议

- Windows / Linux / macOS 安装包或便携包；
- 运行时依赖、资源目录（当前 `exports` 只支持单文件，目录需先打包）；
- 版本号与 `dytes.json` 保持一致；
- 发布前在干净环境验证安装与启动。

## 注意

- `publish` 当前没有 `--dry-run`，测试发布会写入真实 S3；
- `update` 端尚未支持 `design_soft` 依赖，安装联动见
  [`sys/install/publish_todo.md`](../../../dytes_sys_operations/sys/install/publish_todo.md)；
- 安装目录与注册流程尚未定义，发布前需先确定。
