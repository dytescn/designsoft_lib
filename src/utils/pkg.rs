//! `design_soft` 便携包管理。
//!
//! 打开流程：
//!
//! 1. 调用 `dytes install design-soft <name> --dir <root> --json`，由 dytes
//!    负责版本比较、下载、安装与锁文件；
//! 2. 读取 `apps/<name>/dytes.json`，选择 GUI / CLI 入口；
//! 3. 启动 `apps/<name>/<exe>`。
//!
//! 本地目录统一为 dytes 的 `apps/<name>/`，不再维护 `designsoft/versions.json`，
//! 本库也不再直接访问 S3。
//!
//! 只处理便携型 `design_soft` 包；CorelDRAW 等安装型软件仍走
//! `utils::cdr` / `utils::status` 的 COM 与注册表流程。

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use serde::Deserialize;
use serde_json::Value;

use crate::utils::dytes;
use crate::utils::status::is_software_running;

/// 设计软件的安装目录（相对安装根目录）。
pub const LOCAL_DIR: &str = "apps";

/// 包的启动入口。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Entry {
    /// 图形界面。
    Gui,
    /// 命令行。
    Cli,
}

impl Entry {
    /// 入口的稳定名称。
    pub fn as_str(self) -> &'static str {
        match self {
            Entry::Gui => "gui",
            Entry::Cli => "cli",
        }
    }

    /// 解析入口名；接受 `gui`/`desktop` 与 `cli`/`command`。
    pub fn parse(value: &str) -> Option<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "gui" | "desktop" => Some(Entry::Gui),
            "cli" | "command" => Some(Entry::Cli),
            _ => None,
        }
    }
}

/// 本地已安装包的入口信息（由 `apps/<name>/dytes.json` 推导）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LocalPackage {
    /// 本地文件对应的包版本。
    pub version: String,
    /// 默认入口（GUI 优先，其次 CLI）。
    pub exe: String,
    /// GUI 入口文件名。
    pub gui: Option<String>,
    /// CLI 入口文件名。
    pub cli: Option<String>,
}

impl LocalPackage {
    fn entry_file(&self, entry: Entry) -> Option<&str> {
        match entry {
            Entry::Gui => self.gui.as_deref(),
            Entry::Cli => self.cli.as_deref(),
        }
    }

    /// 默认入口：GUI 优先，其次 CLI。
    fn default_entry(&self) -> Option<(Entry, &str)> {
        self.gui
            .as_deref()
            .map(|file| (Entry::Gui, file))
            .or_else(|| self.cli.as_deref().map(|file| (Entry::Cli, file)))
    }

    fn resolve_entry(&self, requested: Option<Entry>) -> Result<(Entry, &str), String> {
        match requested {
            Some(entry) => self
                .entry_file(entry)
                .map(|file| (entry, file))
                .ok_or_else(|| format!("软件包没有 {} 入口", entry.as_str())),
            None => self
                .default_entry()
                .ok_or_else(|| "软件包没有可执行入口".to_string()),
        }
    }

    fn from_manifest(
        name: &str,
        manifest: &Manifest,
        platform: &str,
    ) -> Result<Self, String> {
        let (exports, _) = select_exports(manifest, platform)
            .ok_or_else(|| format!("包 '{name}' 未发布平台 '{platform}'"))?;
        let (gui, cli) = select_entries(exports);
        let exe = gui
            .clone()
            .or_else(|| cli.clone())
            .ok_or_else(|| format!("包 '{name}' 没有导出可执行文件（.exe）"))?;

        Ok(Self {
            version: manifest.version.clone(),
            exe,
            gui,
            cli,
        })
    }
}

/// 打开结果。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OpenedPackage {
    pub name: String,
    pub version: String,
    /// 本次启动的入口。
    pub entry: Entry,
    /// 入口可执行文件完整路径。
    pub exe_path: PathBuf,
    /// 本次调用是否执行了下载（dytes 返回 `skipped = false`）。
    pub downloaded: bool,
}

/// 单个入口的状态。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EntryStatus {
    pub entry: Entry,
    /// 可执行文件名。
    pub exe: String,
    /// 可执行文件完整路径。
    pub path: PathBuf,
    /// 该入口的进程是否正在运行。
    pub running: bool,
}

/// 本地包状态。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PackageStatus {
    pub version: String,
    /// 默认入口的完整路径。
    pub exe_path: PathBuf,
    /// 默认入口的进程是否正在运行。
    pub running: bool,
    /// 全部入口（GUI、CLI），至少一个。
    pub entries: Vec<EntryStatus>,
}

/// 远程 / 本地 `dytes.json` 中本模块需要的字段。
#[derive(Debug, Clone, Deserialize)]
struct Manifest {
    version: String,
    #[serde(default)]
    exports: HashMap<String, String>,
    #[serde(default, rename = "platform-exports")]
    platform_exports: HashMap<String, HashMap<String, String>>,
}

/// 查询本地已安装包版本；未安装时返回 `Ok(None)`。
///
/// 线上版本由 `dytes update --check` 查询，本库不再直接读取 `packages.json`。
pub fn lookup(name: &str) -> Result<Option<String>, String> {
    let name = name.trim();
    if name.is_empty() {
        return Err("软件包名不能为空".to_string());
    }

    let package_dir = Path::new(".").join(LOCAL_DIR).join(name);
    if !package_dir.join("dytes.json").is_file() {
        return Ok(None);
    }
    Ok(Some(read_manifest(&package_dir)?.version))
}

/// 打开设计软件：先由 dytes 确保安装，再启动本地入口。
///
/// 默认入口为 GUI，包只有 CLI 时自动使用 CLI。本地目录为当前工作目录下的
/// `apps/<name>/`。
pub fn open(name: &str) -> Result<OpenedPackage, String> {
    open_in_with(name, Path::new("."), None)
}

/// 指定入口打开，本地目录为当前工作目录下的 `apps/<name>/`。
pub fn open_with(name: &str, entry: Entry) -> Result<OpenedPackage, String> {
    open_in_with(name, Path::new("."), Some(entry))
}

/// 与 [`open`] 相同，但可以指定安装根目录（便于测试或宿主自定义位置）。
pub fn open_in(name: &str, root: &Path) -> Result<OpenedPackage, String> {
    open_in_with(name, root, None)
}

/// 与 [`open_in`] 相同，并可以指定入口；`entry = None` 时使用默认入口。
pub fn open_in_with(
    name: &str,
    root: &Path,
    entry: Option<Entry>,
) -> Result<OpenedPackage, String> {
    let name = name.trim();
    if name.is_empty() {
        return Err("软件包名不能为空".to_string());
    }

    // 1. 由 dytes 保证包已安装 / 已更新
    let skipped = install_with_dytes(root, name)?;

    // 2. 读取本地清单并选择入口
    let package_dir = root.join(LOCAL_DIR).join(name);
    let manifest = read_manifest(&package_dir)?;
    let local = LocalPackage::from_manifest(name, &manifest, &current_platform())?;
    let (resolved, file) = local.resolve_entry(entry)?;
    let exe_path = package_dir.join(file);
    if !exe_path.is_file() {
        return Err(format!(
            "软件包 '{name}' 的文件缺失: {}",
            exe_path.display()
        ));
    }

    // 3. 启动
    launch(&exe_path, &package_dir)?;

    Ok(OpenedPackage {
        name: name.to_string(),
        version: local.version,
        entry: resolved,
        exe_path,
        downloaded: !skipped,
    })
}

/// 查询本地包状态：只检查 `apps/<name>`、清单、文件与进程，不访问网络、不启动软件。
pub fn status(name: &str) -> Result<PackageStatus, String> {
    status_in(name, Path::new("."))
}

/// 与 [`status`] 相同，但可以指定安装根目录。
pub fn status_in(name: &str, root: &Path) -> Result<PackageStatus, String> {
    let name = name.trim();
    if name.is_empty() {
        return Err("软件包名不能为空".to_string());
    }

    let package_dir = root.join(LOCAL_DIR).join(name);
    let manifest = read_manifest(&package_dir)
        .map_err(|_| format!("软件包 '{name}' 尚未安装到 {}", package_dir.display()))?;
    let local = LocalPackage::from_manifest(name, &manifest, &current_platform())?;

    let mut entries = Vec::new();
    for (entry, file) in [
        (Entry::Gui, local.gui.as_deref()),
        (Entry::Cli, local.cli.as_deref()),
    ] {
        let Some(file) = file else { continue };
        let path = package_dir.join(file);
        if !path.is_file() {
            return Err(format!(
                "软件包 '{name}' 的文件缺失: {}",
                path.display()
            ));
        }
        entries.push(EntryStatus {
            entry,
            exe: file.to_string(),
            path,
            running: is_software_running(file),
        });
    }

    let (default_entry, _) = local
        .default_entry()
        .ok_or_else(|| format!("软件包 '{name}' 没有可执行入口"))?;
    let default = entries
        .iter()
        .find(|status| status.entry == default_entry)
        .ok_or_else(|| format!("软件包 '{name}' 没有可执行入口"))?;

    Ok(PackageStatus {
        version: local.version.clone(),
        exe_path: default.path.clone(),
        running: default.running,
        entries,
    })
}

/// 调用 dytes 安装 / 更新设计软件，返回 dytes 的 `skipped`。
fn install_with_dytes(root: &Path, name: &str) -> Result<bool, String> {
    let root_arg = root.to_string_lossy().to_string();
    let output = dytes::run(
        root,
        &["install", "design-soft", name, "--dir", &root_arg, "--json"],
    )?;

    if output.code != Some(0) {
        return Err(format!(
            "dytes install {name} 失败: {}",
            output.stderr.trim()
        ));
    }

    let value = dytes::parse_json(&output)?;
    Ok(value
        .get("skipped")
        .and_then(Value::as_bool)
        .unwrap_or(false))
}

fn read_manifest(package_dir: &Path) -> Result<Manifest, String> {
    let path = package_dir.join("dytes.json");
    let text = fs::read_to_string(&path)
        .map_err(|error| format!("读取 {} 失败: {error}", path.display()))?;
    serde_json::from_str(&text).map_err(|error| format!("解析 {} 失败: {error}", path.display()))
}

fn select_exports<'a>(
    manifest: &'a Manifest,
    platform: &'a str,
) -> Option<(&'a HashMap<String, String>, Option<&'a str>)> {
    if manifest.platform_exports.is_empty() {
        Some((&manifest.exports, None))
    } else {
        manifest
            .platform_exports
            .get(platform)
            .map(|exports| (exports, Some(platform)))
    }
}

/// 按文件名区分 GUI 与 CLI：`<name>-cli.exe` 为 CLI，其余 `.exe` 为 GUI。
fn classify_entry(file: &str) -> Entry {
    if file.to_ascii_lowercase().ends_with("-cli.exe") {
        Entry::Cli
    } else {
        Entry::Gui
    }
}

/// 从导出表中选择 GUI 与 CLI 入口；按文件名排序，保证结果稳定。
fn select_entries(exports: &HashMap<String, String>) -> (Option<String>, Option<String>) {
    let mut gui = None;
    let mut cli = None;
    let mut files: Vec<&String> = exports
        .values()
        .filter(|dest| dest.to_ascii_lowercase().ends_with(".exe"))
        .collect();
    files.sort();

    for file in files {
        match classify_entry(file) {
            Entry::Gui if gui.is_none() => gui = Some(file.clone()),
            Entry::Cli if cli.is_none() => cli = Some(file.clone()),
            _ => {}
        }
    }

    (gui, cli)
}

fn current_platform() -> String {
    std::env::var("DYTES_PLATFORM")
        .or_else(|_| std::env::var("PIKSEL_PLATFORM"))
        .ok()
        .map(|value| value.trim().to_lowercase())
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| std::env::consts::OS.to_string())
}

fn launch(exe_path: &Path, workdir: &Path) -> Result<(), String> {
    Command::new(exe_path)
        .current_dir(workdir)
        .spawn()
        .map_err(|error| format!("启动 {} 失败: {error}", exe_path.display()))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn sample_manifest() -> Manifest {
        Manifest {
            version: "1.2.3".to_string(),
            exports: HashMap::from([("./demo.dll".to_string(), "demo.dll".to_string())]),
            platform_exports: HashMap::new(),
        }
    }

    #[test]
    fn selects_gui_and_cli_entries() {
        let exports = HashMap::from([
            ("./demo.dll".to_string(), "demo.dll".to_string()),
            ("./demo.exe".to_string(), "demo.exe".to_string()),
            ("./demo-cli.exe".to_string(), "demo-cli.exe".to_string()),
        ]);
        assert_eq!(
            select_entries(&exports),
            (
                Some("demo.exe".to_string()),
                Some("demo-cli.exe".to_string())
            )
        );
    }

    #[test]
    fn cli_only_package_uses_cli_entry() {
        let exports = HashMap::from([("demo-cli.exe".to_string(), "demo-cli.exe".to_string())]);
        assert_eq!(
            select_entries(&exports),
            (None, Some("demo-cli.exe".to_string()))
        );

        let local = LocalPackage {
            version: "1.2.3".to_string(),
            exe: "demo-cli.exe".to_string(),
            gui: None,
            cli: Some("demo-cli.exe".to_string()),
        };
        assert_eq!(local.default_entry(), Some((Entry::Cli, "demo-cli.exe")));
        assert_eq!(
            local.resolve_entry(Some(Entry::Gui)),
            Err("软件包没有 gui 入口".to_string())
        );
    }

    #[test]
    fn returns_none_without_exe() {
        let exports = HashMap::from([("./demo.dll".to_string(), "demo.dll".to_string())]);
        assert_eq!(select_entries(&exports), (None, None));
    }

    #[test]
    fn selects_platform_exports_when_present() {
        let mut manifest = sample_manifest();
        manifest.platform_exports = HashMap::from([(
            "windows".to_string(),
            HashMap::from([("./demo.exe".to_string(), "demo.exe".to_string())]),
        )]);

        let (exports, tag) = select_exports(&manifest, "windows").expect("windows exports");
        assert_eq!(tag, Some("windows"));
        assert_eq!(
            exports.get("./demo.exe").map(String::as_str),
            Some("demo.exe")
        );

        assert!(select_exports(&manifest, "linux").is_none());
    }

    #[test]
    fn status_reports_local_package() {
        let root =
            std::env::temp_dir().join(format!("designsoft-pkg-status-test-{}", std::process::id()));
        let package_dir = root.join(LOCAL_DIR).join("demo");
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&package_dir).expect("create package dir");

        let manifest = json!({
            "name": "demo",
            "version": "1.2.3",
            "exports": {
                "./demo.exe": "demo.exe",
                "./demo-cli.exe": "demo-cli.exe"
            }
        });
        fs::write(
            package_dir.join("dytes.json"),
            serde_json::to_string(&manifest).expect("serialize manifest"),
        )
        .expect("write manifest");
        fs::write(package_dir.join("demo.exe"), b"not a real exe").expect("write exe");
        fs::write(package_dir.join("demo-cli.exe"), b"not a real exe").expect("write cli exe");

        let status = status_in("demo", &root).expect("query status");
        assert_eq!(status.version, "1.2.3");
        assert_eq!(status.exe_path, package_dir.join("demo.exe"));
        assert!(!status.running);
        assert_eq!(status.entries.len(), 2);
        assert_eq!(status.entries[0].entry, Entry::Gui);
        assert_eq!(status.entries[1].entry, Entry::Cli);

        fs::remove_file(package_dir.join("demo.exe")).expect("remove exe");
        assert!(status_in("demo", &root).is_err(), "文件缺失时应报错");
        assert!(status_in("missing", &root).is_err(), "未安装时应报错");

        let _ = fs::remove_dir_all(&root);
    }
}
