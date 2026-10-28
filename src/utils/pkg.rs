//! `design_soft` 便携包管理。
//!
//! 打开流程：
//!
//! 1. 读取线上 `design_soft/packages.json`，确认包存在；
//! 2. 本地 `designsoft/` 已有且版本一致时直接启动；
//! 3. 否则读取 `design_soft/<name>/dytes.json`，把导出文件下载到 `designsoft/`；
//! 4. 版本记录写入 `designsoft/versions.json`，然后启动 exe。
//!
//! 只处理便携型 `design_soft` 包；CorelDRAW 等安装型软件仍走
//! `utils::cdr` / `utils::status` 的 COM 与注册表流程。

use std::collections::{BTreeMap, HashMap};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use aws_config::BehaviorVersion;
use aws_credential_types::Credentials;
use aws_sdk_s3::config::{Config, Region};
use aws_sdk_s3::Client;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::utils::status::is_software_running;

/// S3 兼容存储（与 `publish` / `update` 使用同一个 bucket）。
const ENDPOINT_URL: &str = "https://s3.api.upyun.com";
const ACCESS_KEY: &str = "195cc6d12c2d45389a5e52e5138f7250";
const SECRET_KEY: &str = "7ab145ae97310ca9f65db3dff07beac2";
const BUCKET_NAME: &str = "packages";

/// 本地包目录（相对当前工作目录）。
pub const LOCAL_DIR: &str = "designsoft";
/// 本地版本记录文件（位于 [`LOCAL_DIR`] 内）。
pub const VERSION_FILE: &str = "versions.json";

/// 本地已下载包的记录。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LocalPackage {
    /// 本地文件对应的包版本。
    pub version: String,
    /// 默认入口（GUI 优先，其次 CLI）。
    pub exe: String,
    /// GUI 入口文件名。
    #[serde(default)]
    pub gui: Option<String>,
    /// CLI 入口文件名。
    #[serde(default)]
    pub cli: Option<String>,
}

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

    /// 兼容旧记录：缺少 gui/cli 时按 `exe` 文件名推断。
    fn normalize(mut self) -> Self {
        if self.gui.is_none() && self.cli.is_none() {
            if self.exe.to_ascii_lowercase().ends_with("-cli.exe") {
                self.cli = Some(self.exe.clone());
            } else {
                self.gui = Some(self.exe.clone());
            }
        }
        if self.exe.is_empty() {
            if let Some(gui) = &self.gui {
                self.exe = gui.clone();
            } else if let Some(cli) = &self.cli {
                self.exe = cli.clone();
            }
        }
        self
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
    /// 本次调用是否执行了下载。
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

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct VersionBook {
    #[serde(default)]
    packages: BTreeMap<String, LocalPackage>,
}

/// 远程 `dytes.json` 中本模块需要的字段。
#[derive(Debug, Clone, Deserialize)]
struct Manifest {
    version: String,
    #[serde(default)]
    exports: HashMap<String, String>,
    #[serde(default, rename = "platform-exports")]
    platform_exports: HashMap<String, HashMap<String, String>>,
}

/// 查询线上 `design_soft` 包版本；包不存在时返回 `Ok(None)`。
pub fn lookup(name: &str) -> Result<Option<String>, String> {
    remote_index_version(name.trim())
}

/// 打开设计软件：线上必须存在该包；本地缺失或版本落后时先下载。
///
/// 默认入口为 GUI，包只有 CLI 时自动使用 CLI。本地目录为当前工作目录下的
/// [`LOCAL_DIR`]。
pub fn open(name: &str) -> Result<OpenedPackage, String> {
    open_in_with(name, Path::new(LOCAL_DIR), None)
}

/// 指定入口打开，本地目录为当前工作目录下的 [`LOCAL_DIR`]。
pub fn open_with(name: &str, entry: Entry) -> Result<OpenedPackage, String> {
    open_in_with(name, Path::new(LOCAL_DIR), Some(entry))
}

/// 与 [`open`] 相同，但可以指定本地目录（便于测试或宿主自定义位置）。
pub fn open_in(name: &str, local_dir: &Path) -> Result<OpenedPackage, String> {
    open_in_with(name, local_dir, None)
}

/// 与 [`open_in`] 相同，并可以指定入口；`entry = None` 时使用默认入口。
pub fn open_in_with(
    name: &str,
    local_dir: &Path,
    entry: Option<Entry>,
) -> Result<OpenedPackage, String> {
    let name = name.trim();
    if name.is_empty() {
        return Err("软件包名不能为空".to_string());
    }

    // 1. 线上查包
    let Some(manifest) = remote_package(name)? else {
        return Err(format!("线上没有找到设计软件包 '{name}'，无法打开"));
    };

    // 2. 本地已有且版本一致，入口文件存在时直接打开
    let book = read_versions(local_dir);
    if let Some(local) = book.packages.get(name) {
        if local.version == manifest.version {
            if let Ok((resolved, file)) = local.resolve_entry(entry) {
                let exe_path = local_dir.join(file);
                if exe_path.is_file() {
                    launch(&exe_path, local_dir)?;
                    return Ok(OpenedPackage {
                        name: name.to_string(),
                        version: local.version.clone(),
                        entry: resolved,
                        exe_path,
                        downloaded: false,
                    });
                }
            }
        }
    }

    // 3. 下载到本地并记录版本
    let package = download_package(name, &manifest, local_dir)?;
    let version = package.version.clone();
    let (resolved, file) = package.resolve_entry(entry)?;
    let exe_path = local_dir.join(file);
    launch(&exe_path, local_dir)?;

    Ok(OpenedPackage {
        name: name.to_string(),
        version,
        entry: resolved,
        exe_path,
        downloaded: true,
    })
}

/// 查询本地包状态：只检查 `versions.json`、文件与进程，不访问网络、不启动软件。
///
/// 本地目录为当前工作目录下的 [`LOCAL_DIR`]。
pub fn status(name: &str) -> Result<PackageStatus, String> {
    status_in(name, Path::new(LOCAL_DIR))
}

/// 与 [`status`] 相同，但可以指定本地目录。
pub fn status_in(name: &str, local_dir: &Path) -> Result<PackageStatus, String> {
    let name = name.trim();
    if name.is_empty() {
        return Err("软件包名不能为空".to_string());
    }

    let book = read_versions(local_dir);
    let local = book
        .packages
        .get(name)
        .ok_or_else(|| format!("软件包 '{name}' 尚未下载到 {}", local_dir.display()))?;

    let mut entries = Vec::new();
    for (entry, file) in [
        (Entry::Gui, local.gui.as_deref()),
        (Entry::Cli, local.cli.as_deref()),
    ] {
        let Some(file) = file else { continue };
        let path = local_dir.join(file);
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

/// 读取线上包索引，并校验 `dytes.json` 的版本与索引一致。
fn remote_package(name: &str) -> Result<Option<Manifest>, String> {
    let Some(index_version) = remote_index_version(name)? else {
        return Ok(None);
    };

    let manifest_text = fetch_text(&format!("design_soft/{name}/dytes.json"))?;
    let manifest: Manifest = serde_json::from_str(&manifest_text)
        .map_err(|error| format!("解析 design_soft/{name}/dytes.json 失败: {error}"))?;

    if manifest.version != index_version {
        return Err(format!(
            "包 '{name}' 的索引版本 {index_version} 与 dytes.json 版本 {} 不一致",
            manifest.version
        ));
    }

    Ok(Some(manifest))
}

fn remote_index_version(name: &str) -> Result<Option<String>, String> {
    let index_text = fetch_text("design_soft/packages.json")?;
    let index: Value = serde_json::from_str(&index_text)
        .map_err(|error| format!("解析 design_soft/packages.json 失败: {error}"))?;

    Ok(index
        .get("packages")
        .and_then(Value::as_object)
        .and_then(|packages| packages.get(name))
        .and_then(Value::as_str)
        .map(str::to_string))
}

fn download_package(
    name: &str,
    manifest: &Manifest,
    local_dir: &Path,
) -> Result<LocalPackage, String> {
    let platform = current_platform();
    let (exports, platform_tag) = select_exports(manifest, &platform)
        .ok_or_else(|| format!("包 '{name}' 未发布平台 '{platform}'"))?;

    if exports.is_empty() {
        return Err(format!("包 '{name}' 没有导出文件"));
    }

    let (gui, cli) = select_entries(exports);
    let exe = gui
        .clone()
        .or_else(|| cli.clone())
        .ok_or_else(|| format!("包 '{name}' 没有导出可执行文件（.exe）"))?;

    fs::create_dir_all(local_dir)
        .map_err(|error| format!("创建目录 {} 失败: {error}", local_dir.display()))?;

    let base = match platform_tag {
        Some(tag) => format!("design_soft/{name}/{}/{tag}/", manifest.version),
        None => format!("design_soft/{name}/{}/", manifest.version),
    };

    let mut written: Vec<PathBuf> = Vec::new();
    for dest in exports.values() {
        if dest.contains('/') || dest.contains('\\') {
            cleanup(&written);
            return Err(format!("不支持的导出路径: {dest}"));
        }

        let object = format!("{base}{dest}");
        let bytes = match fetch_bytes(&object) {
            Ok(bytes) => bytes,
            Err(error) => {
                cleanup(&written);
                return Err(error);
            }
        };

        let target = local_dir.join(dest);
        if let Err(error) = fs::write(&target, bytes) {
            cleanup(&written);
            return Err(format!("写入 {} 失败: {error}", target.display()));
        }
        written.push(target);
    }

    let package = LocalPackage {
        version: manifest.version.clone(),
        exe,
        gui,
        cli,
    };
    write_versions(local_dir, name, &package)?;

    Ok(package)
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
    std::env::var("PIKSEL_PLATFORM")
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

fn cleanup(written: &[PathBuf]) {
    for path in written {
        let _ = fs::remove_file(path);
    }
}

fn versions_path(local_dir: &Path) -> PathBuf {
    local_dir.join(VERSION_FILE)
}

fn read_versions(local_dir: &Path) -> VersionBook {
    let Ok(text) = fs::read_to_string(versions_path(local_dir)) else {
        return VersionBook::default();
    };
    let mut book: VersionBook = serde_json::from_str(&text).unwrap_or_default();
    for package in book.packages.values_mut() {
        *package = package.clone().normalize();
    }
    book
}

fn write_versions(local_dir: &Path, name: &str, package: &LocalPackage) -> Result<(), String> {
    let mut book = read_versions(local_dir);
    book.packages.insert(name.to_string(), package.clone());

    let text = serde_json::to_string_pretty(&book)
        .map_err(|error| format!("序列化版本记录失败: {error}"))?;
    fs::write(versions_path(local_dir), text).map_err(|error| format!("写入版本记录失败: {error}"))
}

// ---------- S3 访问 ----------

fn runtime() -> Result<tokio::runtime::Runtime, String> {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|error| format!("创建 tokio 运行时失败: {error}"))
}

async fn s3_client() -> Client {
    let credentials = Credentials::new(ACCESS_KEY, SECRET_KEY, None, None, "designsoft");
    let config = aws_config::defaults(BehaviorVersion::latest())
        .endpoint_url(ENDPOINT_URL)
        .region(Region::new("us-east-1"))
        .credentials_provider(credentials)
        .load()
        .await;

    let config = Config::from(&config)
        .to_builder()
        .force_path_style(true)
        .build();

    Client::from_conf(config)
}

fn fetch_bytes(object_name: &str) -> Result<Vec<u8>, String> {
    let runtime = runtime()?;

    runtime.block_on(async {
        let client = s3_client().await;
        let response = client
            .get_object()
            .bucket(BUCKET_NAME)
            .key(object_name)
            .send()
            .await
            .map_err(|error| format!("S3 读取 {object_name} 失败: {error:?}"))?;

        let bytes = response
            .body
            .collect()
            .await
            .map_err(|error| format!("读取 S3 响应失败: {error:?}"))?;

        Ok(bytes.into_bytes().to_vec())
    })
}

fn fetch_text(object_name: &str) -> Result<String, String> {
    let bytes = fetch_bytes(object_name)?;
    String::from_utf8(bytes).map_err(|error| format!("{object_name} 不是 UTF-8: {error}"))
}

#[cfg(test)]
mod tests {
    use super::*;

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
    fn version_book_roundtrip() {
        let dir = std::env::temp_dir().join(format!("designsoft-pkg-test-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).expect("create temp dir");

        let package = LocalPackage {
            version: "1.2.3".to_string(),
            exe: "demo.exe".to_string(),
            gui: Some("demo.exe".to_string()),
            cli: Some("demo-cli.exe".to_string()),
        };
        write_versions(&dir, "demo", &package).expect("write versions");

        let book = read_versions(&dir);
        assert_eq!(book.packages.get("demo"), Some(&package));

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn status_reports_local_package() {
        let dir =
            std::env::temp_dir().join(format!("designsoft-pkg-status-test-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).expect("create temp dir");
        fs::write(dir.join("demo.exe"), b"not a real exe").expect("write exe");
        fs::write(dir.join("demo-cli.exe"), b"not a real exe").expect("write cli exe");

        let package = LocalPackage {
            version: "1.2.3".to_string(),
            exe: "demo.exe".to_string(),
            gui: Some("demo.exe".to_string()),
            cli: Some("demo-cli.exe".to_string()),
        };
        write_versions(&dir, "demo", &package).expect("write versions");

        let status = status_in("demo", &dir).expect("query status");
        assert_eq!(status.version, "1.2.3");
        assert_eq!(status.exe_path, dir.join("demo.exe"));
        assert!(!status.running);
        assert_eq!(status.entries.len(), 2);
        assert_eq!(status.entries[0].entry, Entry::Gui);
        assert_eq!(status.entries[1].entry, Entry::Cli);

        fs::remove_file(dir.join("demo.exe")).expect("remove exe");
        assert!(status_in("demo", &dir).is_err(), "文件缺失时应报错");
        assert!(status_in("missing", &dir).is_err(), "未下载时应报错");

        let _ = fs::remove_dir_all(&dir);
    }
}
