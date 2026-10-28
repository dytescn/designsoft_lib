//! 软件运行状态检测与启动。

use std::path::Path;
use std::process::Command;

use windows::core::{Error, HRESULT};
use windows::Win32::Foundation::CloseHandle;
use windows::Win32::System::ProcessStatus::{EnumProcesses, GetModuleBaseNameW};
use windows::Win32::System::Threading::{OpenProcess, PROCESS_QUERY_INFORMATION, PROCESS_VM_READ};

use crate::utils::data::get_candidate_exe_names;
use crate::utils::soft::{find_software_all, find_software_by_name};

/// 首次枚举进程时分配的 PID 数量，不足以容纳全部进程时自动扩容。
const INITIAL_PID_CAPACITY: usize = 1024;
/// PID 缓冲区扩容上限，避免异常环境下无限增长。
const MAX_PID_CAPACITY: usize = 64 * 1024;

/// 启动软件时优先搜索的子目录（按优先级排序，空串表示安装根目录）。
const SEARCH_SUBDIRS: [&str; 7] = [
    "Programs64",                      // CorelDRAW
    "Programs",                        // 常见
    r"Support Files\Contents\Windows", // Adobe Illustrator
    r"Contents\Windows",               // Adobe
    "Program Files",                   // 部分软件
    "Application",                     // 部分软件
    "",                                // 根目录
];

/// 根据关键字和可选的版本号解析出唯一的软件条目 `(DisplayName, Version)`。
///
/// 版本号可以是完整版本，也可以是主版本号（第一个点之前的部分）。
pub fn resolve_software(keyword: &str, version: Option<&str>) -> Result<(String, String), String> {
    let candidates = find_software_all(keyword);
    if candidates.is_empty() {
        return Err("未找到匹配的软件".to_string());
    }

    match version {
        Some(required) => candidates
            .into_iter()
            .find(|(_, ver)| ver == required || ver.split('.').next() == Some(required))
            .ok_or_else(|| format!("未找到版本 '{required}' 的软件")),
        None if candidates.len() == 1 => Ok(candidates[0].clone()),
        None => {
            let versions: Vec<String> = candidates.iter().map(|(_, v)| v.clone()).collect();
            Err(format!(
                "存在多个版本，请指定版本号: {}",
                versions.join(", ")
            ))
        }
    }
}

/// 判断一个文件路径是否是合理的可执行文件（排除卸载/修复程序）。
fn is_valid_exe_path(path: &Path) -> bool {
    let file_name = path.file_name().and_then(|s| s.to_str()).unwrap_or("");
    let lower = file_name.to_lowercase();

    if lower.contains("uninstall")
        || lower.contains("setup")
        || lower.contains("install")
        || lower.contains("repair")
        || lower.contains("patch")
        || lower.contains("update")
    {
        return false;
    }

    path.extension()
        .and_then(|e| e.to_str())
        .is_some_and(|ext| ext.eq_ignore_ascii_case("exe"))
}

/// 根据 DisplayName 查找可执行文件完整路径。
fn find_executable_path(display_name: &str) -> Result<String, String> {
    let software = find_software_by_name(display_name)
        .ok_or_else(|| format!("未在注册表中找到 '{}'", display_name))?;

    // 1. DisplayIcon 直接指向有效 exe 时优先使用。
    if let Some(icon) = software.display_icon.as_deref() {
        let exe = icon.split(',').next().unwrap_or(icon).trim();
        let path = Path::new(exe);
        if path.exists() && is_valid_exe_path(path) {
            return Ok(exe.to_string());
        }
    }

    // 2. 在安装目录中按候选文件名搜索。
    let install_path = software
        .install_path()
        .ok_or_else(|| format!("未找到 '{}' 的安装目录", display_name))?;
    let install_dir = Path::new(&install_path);
    if !install_dir.exists() {
        return Err(format!("安装目录不存在: {install_path}"));
    }

    let candidate_names = get_candidate_exe_names(display_name);
    for sub in SEARCH_SUBDIRS {
        let search_dir = install_dir.join(sub);
        if !search_dir.is_dir() {
            continue;
        }
        for name in &candidate_names {
            let full_path = search_dir.join(name);
            if full_path.exists() && is_valid_exe_path(&full_path) {
                return Ok(full_path.to_string_lossy().into_owned());
            }
        }
    }

    // 3. 浅层递归搜索。
    if let Some(found) = deep_search_exe(install_dir, &candidate_names, 3) {
        return Ok(found);
    }

    Err(format!("在 '{install_path}' 下未找到匹配的可执行文件"))
}

/// 在目录及其子目录中递归搜索（限制深度），跳过无关目录。
fn deep_search_exe(dir: &Path, names: &[String], max_depth: usize) -> Option<String> {
    if max_depth == 0 {
        return None;
    }

    let entries = std::fs::read_dir(dir).ok()?;
    for entry in entries.filter_map(Result::ok) {
        let path = entry.path();
        if path.is_dir() {
            let dir_name = path.file_name().and_then(|s| s.to_str()).unwrap_or("");
            let lower = dir_name.to_lowercase();
            if lower.contains("resource")
                || lower.contains("_common")
                || lower.contains("help")
                || lower.contains("sample")
                || lower.contains("redist")
                || lower.contains("directx")
                || lower.contains("fonts")
                || lower.contains("licenses")
            {
                continue;
            }
            if let Some(found) = deep_search_exe(&path, names, max_depth - 1) {
                return Some(found);
            }
        } else if path.is_file() {
            if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
                if names.iter().any(|n| n.eq_ignore_ascii_case(name)) && is_valid_exe_path(&path) {
                    return Some(path.to_string_lossy().into_owned());
                }
            }
        }
    }

    None
}

/// 判断是否有进程的可执行文件名与 `exe_name` 相同（忽略大小写）。
pub fn is_software_running(exe_name: &str) -> bool {
    !find_process_ids(exe_name).is_empty()
}

/// 返回所有映像名与 `exe_name` 相同的进程 PID。
pub(crate) fn find_process_ids(exe_name: &str) -> Vec<u32> {
    let mut pids = vec![0u32; INITIAL_PID_CAPACITY];

    loop {
        let mut bytes_returned = 0u32;
        // SAFETY: pids 容量与传入的字节数一致。
        let result = unsafe {
            EnumProcesses(
                pids.as_mut_ptr(),
                (pids.len() * std::mem::size_of::<u32>()) as u32,
                &mut bytes_returned,
            )
        };
        if result.is_err() {
            return Vec::new();
        }

        let count = bytes_returned as usize / std::mem::size_of::<u32>();

        // 缓冲区被填满说明可能还有进程，扩容后重试。
        if count == pids.len() && pids.len() < MAX_PID_CAPACITY {
            pids.resize((pids.len() * 2).min(MAX_PID_CAPACITY), 0);
            continue;
        }

        return pids[..count.min(pids.len())]
            .iter()
            .copied()
            .filter(|&pid| pid != 0 && process_matches(pid, exe_name))
            .collect();
    }
}

/// 判断单个 PID 的映像名是否与 `exe_name` 相同。
fn process_matches(pid: u32, exe_name: &str) -> bool {
    // SAFETY: 句柄在使用后立即关闭；name_buf 在调用期间有效。
    unsafe {
        let Ok(handle) = OpenProcess(PROCESS_QUERY_INFORMATION | PROCESS_VM_READ, false, pid)
        else {
            return false;
        };

        let mut name_buf = [0u16; 260];
        let len = GetModuleBaseNameW(handle, None, &mut name_buf);
        let _ = CloseHandle(handle);

        len > 0
            && String::from_utf16_lossy(&name_buf[..len as usize]).eq_ignore_ascii_case(exe_name)
    }
}

/// 综合状态查询（支持版本指定），返回 `"已启动"` 或 `"已安装但未启动"`。
pub fn software_status_with_version(
    keyword: &str,
    version: Option<&str>,
) -> Result<String, String> {
    let (display_name, _) = resolve_software(keyword, version)?;
    let exe_path = find_executable_path(&display_name)?;
    let exe_name = Path::new(&exe_path)
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("unknown.exe");

    if is_software_running(exe_name) {
        Ok("已启动".to_string())
    } else {
        Ok("已安装但未启动".to_string())
    }
}

/// 启动软件（支持版本指定）；已在运行时直接返回成功。
pub fn launch_software_with_version(keyword: &str, version: Option<&str>) -> Result<(), String> {
    let (display_name, _) = resolve_software(keyword, version)?;
    let exe_path = find_executable_path(&display_name)?;

    let path = Path::new(&exe_path);
    if !path.exists() || !is_valid_exe_path(path) {
        return Err(format!("可执行文件无效或不存在: {exe_path}"));
    }

    let exe_name = path
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("unknown.exe");
    if is_software_running(exe_name) {
        return Ok(());
    }

    // 工作目录优先使用安装目录，缺失时退回可执行文件所在目录。
    let working_dir = find_software_by_name(&display_name)
        .and_then(|software| software.install_path())
        .or_else(|| path.parent().map(|p| p.to_string_lossy().into_owned()));

    let mut command = Command::new(&exe_path);
    if let Some(dir) = working_dir {
        command.current_dir(dir);
    }
    command.spawn().map_err(|e| format!("启动失败: {e}"))?;

    Ok(())
}

// ---------- 旧版兼容接口 ----------

#[deprecated(
    since = "0.1.0",
    note = "请使用 software_status_with_version(keyword, None) 代替"
)]
pub fn software_status(keyword: &str, exe_name: &str) -> String {
    if find_software_all(keyword).is_empty() {
        return "未安装".to_string();
    }
    if is_software_running(exe_name) {
        "已启动".to_string()
    } else {
        "已安装但未启动".to_string()
    }
}

#[deprecated(
    since = "0.1.0",
    note = "请使用 launch_software_with_version(keyword, None) 代替"
)]
pub fn launch_software(keyword: &str) -> Result<(), Error> {
    launch_software_with_version(keyword, None).map_err(|e| Error::new(HRESULT(1), e))
}
