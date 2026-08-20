// src/utils/status.rs
use std::process::Command;
use std::path::Path;
use windows::core::{Error, HRESULT};
use windows::Win32::System::ProcessStatus::{EnumProcesses, GetModuleBaseNameW};
use windows::Win32::System::Threading::{OpenProcess, PROCESS_QUERY_INFORMATION, PROCESS_VM_READ};

use crate::utils::soft::{find_software, find_software_all, get_install_path_by_name, get_display_icon};
use crate::utils::data::get_candidate_exe_names;  // 从 data 模块导入

/// 根据关键字和可选的版本号解析出唯一的软件条目（DisplayName, Version）
pub fn resolve_software(keyword: &str, version: Option<&str>) -> Result<(String, String), String> {
    let candidates = find_software_all(keyword);
    if candidates.is_empty() {
        return Err("未找到匹配的软件".to_string());
    }

    match version {
        Some(v) => {
            for (name, ver) in candidates {
                if ver == v {
                    return Ok((name, ver));
                }
            }
            Err(format!("未找到版本 '{}' 的软件", v))
        }
        None => {
            if candidates.len() == 1 {
                return Ok(candidates[0].clone());
            }
            let versions: Vec<String> = candidates.iter().map(|(_, v)| v.clone()).collect();
            Err(format!("存在多个版本，请指定版本号: {}", versions.join(", ")))
        }
    }
}

/// 根据 DisplayName 查找可执行文件完整路径（内部使用）
fn find_executable_path(display_name: &str) -> Result<String, Error> {
    // 1. 从 DisplayIcon 获取
    if let Some(icon) = get_display_icon(display_name) {
        let exe = icon.split(',').next().unwrap_or(&icon).trim();
        let path = Path::new(exe);
        if path.exists() {
            if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
                if ext.eq_ignore_ascii_case("exe") || ext.eq_ignore_ascii_case("com") {
                    return Ok(exe.to_string());
                } else {
                    eprintln!("警告: DisplayIcon 指向非 .exe 文件: {}", exe);
                }
            }
        } else {
            eprintln!("警告: DisplayIcon 路径不存在: {}", exe);
        }
    }

    // 2. 从 InstallLocation 搜索
    let install_path = get_install_path_by_name(display_name)
        .ok_or_else(|| Error::new(HRESULT(1), "未找到 InstallLocation"))?;

    let install_dir = Path::new(&install_path);
    if !install_dir.exists() {
        return Err(Error::new(HRESULT(1), format!("InstallLocation 目录不存在: {}", install_path)));
    }

    // 使用 data 模块中的候选名生成函数
    let candidate_names = get_candidate_exe_names(display_name);
    eprintln!("候选文件名: {:?}", candidate_names);

    // 定义子目录
    let subdirs = vec![
        "",
        "Programs",
        "Support Files\\Contents\\Windows",
        "Contents\\Windows",
        "Program Files",
        "Application",
    ];

    for sub in subdirs {
        let search_dir = if sub.is_empty() {
            install_dir.to_path_buf()
        } else {
            install_dir.join(sub)
        };
        if !search_dir.exists() { continue; }

        for name in &candidate_names {
            let full_path = search_dir.join(name);
            if full_path.exists() {
                return Ok(full_path.to_string_lossy().to_string());
            }
        }
    }

    // 深度搜索（2层）
    if let Some(found) = deep_search_exe(install_dir, &candidate_names, 2) {
        return Ok(found);
    }

    // 未找到，打印帮助信息
    eprintln!("在安装目录 '{}' 下未找到匹配的可执行文件", install_path);
    eprintln!("安装目录中的 .exe 文件（可能相关）:");
    if let Ok(entries) = std::fs::read_dir(install_dir) {
        for entry in entries.filter_map(|e| e.ok()) {
            let path = entry.path();
            if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
                if ext.eq_ignore_ascii_case("exe") {
                    eprintln!("  {}", path.file_name().unwrap_or_default().to_string_lossy());
                }
            }
        }
    }

    Err(Error::new(HRESULT(1), format!("未找到 '{}' 的可执行文件", display_name)))
}

/// 在目录及其子目录中递归搜索（深度限制）
fn deep_search_exe(dir: &Path, names: &[String], max_depth: usize) -> Option<String> {
    if max_depth == 0 { return None; }
    if let Ok(entries) = std::fs::read_dir(dir) {
        for entry in entries.filter_map(|e| e.ok()) {
            let path = entry.path();
            if path.is_dir() {
                if let Some(found) = deep_search_exe(&path, names, max_depth - 1) {
                    return Some(found);
                }
            } else if path.is_file() {
                if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
                    if names.iter().any(|n| n.eq_ignore_ascii_case(name)) {
                        return Some(path.to_string_lossy().to_string());
                    }
                }
            }
        }
    }
    None
}

/// 判断软件是否正在运行（通过可执行文件名）
pub fn is_software_running(exe_name: &str) -> bool {
    let mut pids = [0u32; 1024];
    let mut bytes_returned = 0u32;
    unsafe {
        if EnumProcesses(pids.as_mut_ptr(), (pids.len() * 4) as u32, &mut bytes_returned).is_ok() {
            let count = bytes_returned as usize / 4;
            for i in 0..count {
                let pid = pids[i];
                if pid == 0 { continue; }
                if let Ok(handle) = OpenProcess(PROCESS_QUERY_INFORMATION | PROCESS_VM_READ, false, pid) {
                    let mut name_buf = [0u16; 260];
                    let len = GetModuleBaseNameW(handle, None, &mut name_buf);
                    if len > 0 {
                        let name = String::from_utf16_lossy(&name_buf[..len as usize]);
                        if name.to_lowercase() == exe_name.to_lowercase() {
                            return true;
                        }
                    }
                }
            }
        }
    }
    false
}

/// 综合状态查询（支持版本指定）
pub fn software_status_with_version(keyword: &str, version: Option<&str>) -> Result<String, String> {
    let (display_name, _) = resolve_software(keyword, version)?;
    let exe_path = find_executable_path(&display_name).map_err(|e| e.to_string())?;
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

/// 启动软件（支持版本指定）
pub fn launch_software_with_version(keyword: &str, version: Option<&str>) -> Result<(), String> {
    let (display_name, _) = resolve_software(keyword, version)?;
    let exe_path = find_executable_path(&display_name).map_err(|e| e.to_string())?;
    let exe_name = Path::new(&exe_path)
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("unknown.exe");
    if is_software_running(exe_name) {
        return Ok(());
    }
    Command::new(&exe_path)
        .spawn()
        .map_err(|e| format!("启动失败: {}", e))?;
    Ok(())
}

// ========== 以下为旧版兼容函数（已标记 deprecated）==========

#[deprecated(since = "0.1.0", note = "请使用 software_status_with_version(keyword, None) 代替")]
pub fn software_status(keyword: &str, exe_name: &str) -> String {
    let installed = !find_software(keyword).is_empty();
    if !installed {
        return "未安装".to_string();
    }
    if is_software_running(exe_name) {
        "已启动".to_string()
    } else {
        "已安装但未启动".to_string()
    }
}

#[deprecated(since = "0.1.0", note = "请使用 launch_software_with_version(keyword, None) 代替")]
pub fn launch_software(keyword: &str) -> Result<(), Error> {
    let candidates = find_software(keyword);
    if candidates.is_empty() {
        return Err(Error::new(HRESULT(1), format!("未找到与 '{}' 匹配的软件", keyword)));
    }
    let (display_name, _) = &candidates[0];
    let exe_path = find_executable_path(display_name)?;
    let exe_name = Path::new(&exe_path)
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("unknown.exe");
    if is_software_running(exe_name) {
        return Ok(());
    }
    Command::new(&exe_path)
        .spawn()
        .map_err(|e| {
            let msg = format!("启动 '{}' 失败: {}", exe_path, e);
            Error::new(HRESULT(e.raw_os_error().unwrap_or(1) as i32), msg)
        })?;
    Ok(())
}