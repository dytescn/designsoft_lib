//! CorelDRAW COM 接口封装（基于 [`cdrsdk`]）。
//!
//! 通过 late-binding `IDispatch` 接口绑定 CorelDRAW 应用对象：
//!
//! - [`CdrSession::open`]：绑定已运行的 CorelDRAW；未运行时由 COM 自动启动
//! - [`status`]：只查询状态，不会启动 CorelDRAW
//! - [`close`]：先请求 `Quit`，超时后强制结束进程
//!
//! COM 需要在调用线程初始化。本模块内部使用 STA 初始化，并把初始化守卫
//! 与 [`IvgApplication`] 一起保存在 [`CdrSession`] 中，因此 **session 必须在
//! 创建它的线程上使用和释放**（`cdrsdk` 的类型本身也是 `!Send`）。

use std::path::Path;
use std::thread::sleep;
use std::time::{Duration, Instant};

use cdrsdk::prelude::IvgApplication;
use windows::core::BOOL;
use windows::Win32::Foundation::{CloseHandle, FALSE, HWND, LPARAM, RPC_E_CHANGED_MODE, TRUE};
use windows::Win32::System::Com::{CoInitializeEx, CoUninitialize, COINIT_APARTMENTTHREADED};
use windows::Win32::System::Threading::{OpenProcess, TerminateProcess, PROCESS_TERMINATE};
use windows::Win32::UI::WindowsAndMessaging::{
    EnumWindows, GetWindowTextLengthW, GetWindowThreadProcessId, IsWindowVisible,
    SetForegroundWindow, ShowWindow, SW_RESTORE,
};

use crate::utils::apps::{self, ComApp, OpenMethod};
use crate::utils::status::{find_process_ids, is_software_running};

/// 优雅关闭的默认等待时间。
pub const DEFAULT_CLOSE_TIMEOUT: Duration = Duration::from_secs(10);

/// CorelDRAW 主程序可能的映像名。
const EXE_CANDIDATES: [&str; 2] = ["CorelDRW.exe", "CorelDRAW.exe"];

/// 无版本 ProgID 失败时依次尝试的主版本号（v20 = CorelDRAW 2018）。
const FALLBACK_VERSIONS: [&str; 7] = ["26", "25", "24", "23", "22", "21", "20"];

/// COM 绑定重试次数：CorelDRAW 刚启动或刚退出时类工厂可能短暂不可用。
const CONNECT_ATTEMPTS: usize = 10;
/// 每次绑定重试之间的等待时间。
const CONNECT_RETRY_DELAY: Duration = Duration::from_millis(300);

/// 打开软件后等待主窗口出现并置前的最长时间。
const WINDOW_FOREGROUND_TIMEOUT: Duration = Duration::from_secs(5);
/// 窗口置前重试间隔。
const WINDOW_FOREGROUND_RETRY: Duration = Duration::from_millis(200);

/// CorelDRAW 应用信息快照。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CdrAppInfo {
    /// `version` 属性，例如 `"26.1.0.143"`。
    pub version: Option<String>,
    /// `versionmajor`，例如 `26`。
    pub version_major: Option<i64>,
    /// 程序目录（`ProgramPath`），通常是 `...\Programs64\`。
    pub program_path: Option<String>,
    /// 当前打开的文档数。
    pub document_count: Option<i64>,
    /// 主窗口是否可见。
    pub visible: Option<bool>,
}

/// CorelDRAW 运行状态。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CdrStatus {
    /// 是否有 CorelDRAW 主进程在运行。
    pub running: bool,
    /// 进程在运行时通过 COM 读到的应用信息。
    pub info: Option<CdrAppInfo>,
}

/// COM 初始化守卫。
///
/// 线程已以其它模式初始化（`RPC_E_CHANGED_MODE`）时不接管释放。
struct ComGuard {
    owns_com: bool,
}

impl ComGuard {
    fn init() -> Result<Self, String> {
        // SAFETY: 与 Drop 中的 CoUninitialize 配对。
        let hr = unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED) };

        if hr == RPC_E_CHANGED_MODE {
            // 线程已经初始化过 COM（通常是 MTA），可以继续使用但不归本守卫释放。
            return Ok(Self { owns_com: false });
        }
        if hr.is_err() {
            return Err(format!("COM 初始化失败: 0x{:08X}", hr.0));
        }

        Ok(Self { owns_com: true })
    }
}

impl Drop for ComGuard {
    fn drop(&mut self) {
        if self.owns_com {
            // SAFETY: 与 init 中成功的 CoInitializeEx 配对。
            unsafe { CoUninitialize() };
        }
    }
}

/// 绑定 CorelDRAW 应用对象。
pub struct CdrSession {
    app: IvgApplication,
    _com: ComGuard,
}

impl CdrSession {
    /// 打开 CorelDRAW 的 COM 接口。
    ///
    /// 已运行则复用现有实例；未运行且已安装时由 COM 自动启动。
    /// 指定版本时先尝试对应 ProgID，失败后回退到无版本 ProgID 和已知主版本。
    pub fn open(version: Option<&str>) -> Result<Self, String> {
        let com = ComGuard::init()?;
        let app = connect(version)?;

        Ok(Self { app, _com: com })
    }

    /// 底层的 cdrsdk 应用对象，可继续调用文档、图层、图形等接口。
    pub fn application(&self) -> &IvgApplication {
        &self.app
    }

    /// 读取应用信息快照。
    pub fn info(&self) -> CdrAppInfo {
        CdrAppInfo {
            version: self.app.version(),
            version_major: self.app.version_major(),
            program_path: self.app.program_path(),
            document_count: self.app.documents().and_then(|documents| documents.count()),
            visible: self.app.visible(),
        }
    }

    /// 设置主窗口可见性。
    pub fn set_visible(&self, visible: bool) -> bool {
        self.app.set_visible(visible)
    }

    /// 主程序映像名，用于等待退出或强制结束。
    pub fn exe_name(&self) -> String {
        let Some(program_path) = self.app.program_path() else {
            return EXE_CANDIDATES[0].to_string();
        };
        let path = Path::new(program_path.trim_matches('"'));

        // 某些版本直接返回 exe 路径。
        if path.is_file() {
            if let Some(name) = path.file_name().and_then(|name| name.to_str()) {
                return name.to_string();
            }
        }

        // 常见情况：`ProgramPath` 返回 `...\Programs64\` 目录。
        if let Some(name) = EXE_CANDIDATES.iter().find(|name| path.join(name).is_file()) {
            return (*name).to_string();
        }

        EXE_CANDIDATES[0].to_string()
    }
}

/// 关键字是否指向 CorelDRAW。
///
/// 与路由表保持一致：`utils::apps::APPS` 中路由到
/// [`OpenMethod::Com(ComApp::CorelDraw)`](OpenMethod) 的关键字即视为 CorelDRAW。
pub fn is_coreldraw(keyword: &str) -> bool {
    apps::resolve(keyword).is_some_and(|app| app.open == OpenMethod::Com(ComApp::CorelDraw))
}

/// 打开 CorelDRAW 并显示主窗口。
///
/// 与 [`CdrSession::open`] 的区别是会把 `Visible` 设为 `true`，并等待主窗口
/// 出现后带到前台，适合宿主“打开软件”的语义；已运行的实例也会被置前。
///
/// CorelDRAW 在最后一个自动化客户端断开时会重新隐藏窗口，因此无文档实例会
/// 先创建再关闭一个文档，把它切换到普通文档模式后再返回。
pub fn open_software(version: Option<&str>) -> Result<CdrAppInfo, String> {
    let session = CdrSession::open(version)?;
    let exe_name = session.exe_name();

    session.set_visible(true);

    if session.application().active_document().is_none() {
        if let Some(document) = session.application().create_document() {
            let _ = document.close_without_saving();
        }
    }

    bring_windows_to_front(&exe_name, WINDOW_FOREGROUND_TIMEOUT);

    Ok(session.info())
}

/// 查询 CorelDRAW 状态，不会启动 CorelDRAW。
///
/// 进程未运行时返回 `running = false`，此时不会进行 COM 绑定。
pub fn status(version: Option<&str>) -> Result<CdrStatus, String> {
    if !is_coreldraw_running() {
        return Ok(CdrStatus {
            running: false,
            info: None,
        });
    }

    let session = CdrSession::open(version)?;
    Ok(CdrStatus {
        running: true,
        info: Some(session.info()),
    })
}

/// 关闭 CorelDRAW。
///
/// 先调用 COM 的 `Quit` 优雅退出；超过 `timeout` 仍未退出时强制结束所有
/// 主程序进程。未运行时直接返回成功（幂等）。
///
/// 注意：存在未保存文档时，`Quit` 会弹出保存对话框，可能一直等到超时才被强制结束。
pub fn close(version: Option<&str>, timeout: Duration) -> Result<(), String> {
    if !is_coreldraw_running() {
        return Ok(());
    }

    let session = match CdrSession::open(version) {
        Ok(session) => session,
        // 进程在运行但 COM 绑定失败（例如正在启动/退出），退回按进程名强制结束。
        Err(error) => {
            let exe_name = running_exe_name();
            return force_terminate(&exe_name)
                .map_err(|terminate_error| format!("{error}; 强制结束失败: {terminate_error}"));
        }
    };
    let exe_name = session.exe_name();
    let quit_accepted = session.application().quit();
    // 及时释放 COM 引用，避免 Quit 被未释放的接口阻塞。
    drop(session);

    if quit_accepted && wait_for_exit(&exe_name, timeout) {
        return Ok(());
    }

    force_terminate(&exe_name)
}

/// 用 [`close`] 的默认超时关闭 CorelDRAW。
pub fn close_with_default_timeout(version: Option<&str>) -> Result<(), String> {
    close(version, DEFAULT_CLOSE_TIMEOUT)
}

fn connect(version: Option<&str>) -> Result<IvgApplication, String> {
    let mut last_error = String::new();

    for attempt in 0..CONNECT_ATTEMPTS {
        match try_connect_once(version) {
            Ok(app) => return Ok(app),
            Err(error) => last_error = error,
        }

        if attempt + 1 < CONNECT_ATTEMPTS {
            sleep(CONNECT_RETRY_DELAY);
        }
    }

    Err(last_error)
}

fn try_connect_once(version: Option<&str>) -> Result<IvgApplication, String> {
    let mut last_error = String::new();

    if let Some(progid_version) = version.and_then(progid_version) {
        match IvgApplication::try_new(&progid_version) {
            Ok(app) => return Ok(app),
            Err(error) => last_error = format!("CorelDRAW.Application.{progid_version}: {error}"),
        }
    }

    match IvgApplication::try_new("") {
        Ok(app) => return Ok(app),
        Err(error) if last_error.is_empty() => {
            last_error = format!("CorelDRAW.Application: {error}");
        }
        Err(_) => {}
    }

    for version in FALLBACK_VERSIONS {
        if let Ok(app) = IvgApplication::try_new(version) {
            return Ok(app);
        }
    }

    Err(format!(
        "无法绑定 CorelDRAW（未安装，或 COM 未注册）: {last_error}"
    ))
}

/// 把注册表版本号（`"26.1.0.143"` 或年份 `"2025"`）转换为 ProgID 后缀。
fn progid_version(version: &str) -> Option<String> {
    let first = version.trim().split('.').next()?.trim();
    if first.is_empty() {
        return None;
    }

    // 四位年份：CorelDRAW 2018 = v20，2025 = v27。
    if first.len() == 4 && first.starts_with("20") {
        let year = first.parse::<u32>().ok()?;
        return year_to_major(year).map(|major| major.to_string());
    }

    Some(first.to_string())
}

fn year_to_major(year: u32) -> Option<u32> {
    (2018..=2030).contains(&year).then(|| year - 1998)
}

fn is_coreldraw_running() -> bool {
    EXE_CANDIDATES
        .iter()
        .any(|exe_name| is_software_running(exe_name))
}

/// 当前正在运行的主程序映像名。
fn running_exe_name() -> String {
    EXE_CANDIDATES
        .iter()
        .find(|exe_name| is_software_running(exe_name))
        .copied()
        .unwrap_or(EXE_CANDIDATES[0])
        .to_string()
}

/// 等待主窗口出现并带到前台，返回是否成功。
fn bring_windows_to_front(exe_name: &str, timeout: Duration) -> bool {
    let deadline = Instant::now() + timeout;
    loop {
        if bring_to_front(exe_name) {
            return true;
        }
        if Instant::now() >= deadline {
            return false;
        }
        sleep(WINDOW_FOREGROUND_RETRY);
    }
}

fn bring_to_front(exe_name: &str) -> bool {
    let pids = find_process_ids(exe_name);
    let Some(hwnd) = find_process_window(&pids) else {
        return false;
    };

    // SAFETY: hwnd 来自本次 EnumWindows 的结果，调用期间有效。
    unsafe {
        let _ = ShowWindow(hwnd, SW_RESTORE);
        let _ = SetForegroundWindow(hwnd);
    }
    true
}

struct WindowSearch<'a> {
    pids: &'a [u32],
    /// 第一个属于目标进程的可见顶层窗口。
    first_visible: Option<HWND>,
    /// 第一个带标题的可见顶层窗口（更可能是主窗口）。
    titled: Option<HWND>,
}

fn find_process_window(pids: &[u32]) -> Option<HWND> {
    if pids.is_empty() {
        return None;
    }

    let mut search = WindowSearch {
        pids,
        first_visible: None,
        titled: None,
    };

    // SAFETY: search 在 EnumWindows 调用期间保持有效。
    unsafe {
        let _ = EnumWindows(
            Some(enum_process_windows),
            LPARAM(&mut search as *mut WindowSearch as isize),
        );
    }

    search.titled.or(search.first_visible)
}

unsafe extern "system" fn enum_process_windows(hwnd: HWND, lparam: LPARAM) -> BOOL {
    // SAFETY: lparam 指向存活于 EnumWindows 调用期间的 WindowSearch。
    let search = unsafe { &mut *(lparam.0 as *mut WindowSearch) };

    let mut pid = 0u32;
    // SAFETY: pid 是有效的输出变量。
    unsafe { GetWindowThreadProcessId(hwnd, Some(&mut pid)) };
    if !search.pids.contains(&pid) {
        return TRUE;
    }

    // SAFETY: hwnd 由 EnumWindows 提供。
    if !unsafe { IsWindowVisible(hwnd) }.as_bool() {
        return TRUE;
    }

    if search.first_visible.is_none() {
        search.first_visible = Some(hwnd);
    }

    // 有标题的窗口更可能是主窗口，找到后提前结束枚举。
    if unsafe { GetWindowTextLengthW(hwnd) } > 0 {
        search.titled = Some(hwnd);
        return FALSE;
    }

    TRUE
}

fn wait_for_exit(exe_name: &str, timeout: Duration) -> bool {
    let deadline = Instant::now() + timeout;
    loop {
        if !is_software_running(exe_name) {
            return true;
        }
        if Instant::now() >= deadline {
            return false;
        }
        sleep(Duration::from_millis(100));
    }
}

fn force_terminate(exe_name: &str) -> Result<(), String> {
    for pid in find_process_ids(exe_name) {
        // SAFETY: 句柄在终止后立即关闭。
        unsafe {
            let handle = OpenProcess(PROCESS_TERMINATE, false, pid)
                .map_err(|e| format!("无法打开进程 {pid}: {e}"))?;
            let result = TerminateProcess(handle, 1);
            let _ = CloseHandle(handle);
            result.map_err(|e| format!("终止进程 {pid} 失败: {e}"))?;
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{progid_version, year_to_major};

    #[test]
    fn maps_full_version_to_progid_suffix() {
        assert_eq!(progid_version("26.1.0.143").as_deref(), Some("26"));
    }

    #[test]
    fn maps_plain_major_version() {
        assert_eq!(progid_version("24").as_deref(), Some("24"));
    }

    #[test]
    fn maps_installation_year_to_major_version() {
        assert_eq!(year_to_major(2018), Some(20));
        assert_eq!(year_to_major(2025), Some(27));
        assert_eq!(progid_version("2025").as_deref(), Some("27"));
    }

    #[test]
    fn rejects_empty_or_out_of_range_version() {
        assert_eq!(progid_version(""), None);
        assert_eq!(progid_version("   "), None);
        assert_eq!(year_to_major(2017), None);
        assert_eq!(year_to_major(2031), None);
    }
}
