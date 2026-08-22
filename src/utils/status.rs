// src/utils/status.rs
use std::process::Command;
use std::path::Path;
use windows::core::{Error, HRESULT};
use windows::Win32::System::ProcessStatus::{EnumProcesses, GetModuleBaseNameW};
use windows::Win32::System::Threading::{OpenProcess, PROCESS_QUERY_INFORMATION, PROCESS_VM_READ};

use crate::utils::soft::{find_software, find_software_all, get_install_path_by_name, get_display_icon};
use crate::utils::data::get_candidate_exe_names;

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

/// 判断一个文件路径是否是合理的可执行文件（非卸载/修复程序）
fn is_valid_exe_path(path: &Path) -> bool {
    let file_name = path.file_name().and_then(|s| s.to_str()).unwrap_or("");
    let lower = file_name.to_lowercase();

    // 排除常见的卸载/修复/安装程序
    if lower.contains("uninstall") || lower.contains("setup")
        || lower.contains("install") || lower.contains("repair")
        || lower.contains("patch") || lower.contains("update") {
        return false;
    }

    // 检查扩展名
    if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
        if ext.eq_ignore_ascii_case("exe") {
            return true;
        }
    }
    false
}

/// 根据 DisplayName 查找可执行文件完整路径（优化版）
fn find_executable_path(display_name: &str) -> Result<String, Error> {
    // 1. 尝试从 DisplayIcon 获取（如果指向有效 exe）
    if let Some(icon) = get_display_icon(display_name) {
        let exe = icon.split(',').next().unwrap_or(&icon).trim();
        let path = Path::new(exe);
        if path.exists() && is_valid_exe_path(path) {
            eprintln!("从 DisplayIcon 找到可执行文件: {}", exe);
            return Ok(exe.to_string());
        } else if path.exists() {
            eprintln!("DisplayIcon 指向无效文件（可能为卸载程序）: {}", exe);
        } else {
            eprintln!("DisplayIcon 路径不存在: {}", exe);
        }
    }

    // 2. 从 InstallLocation 搜索
    let install_path = get_install_path_by_name(display_name)
        .ok_or_else(|| Error::new(HRESULT(1), "未找到 InstallLocation"))?;

    let install_dir = Path::new(&install_path);
    if !install_dir.exists() {
        return Err(Error::new(HRESULT(1), format!("InstallLocation 目录不存在: {}", install_path)));
    }

    eprintln!("安装目录: {}", install_path);

    // 获取候选文件名列表
    let candidate_names = get_candidate_exe_names(display_name);
    eprintln!("候选文件名: {:?}", candidate_names);

    // 定义搜索子目录（按优先级排序）
    let subdirs = vec![
        "Programs64",                       // CorelDRAW 常见
        "Programs",                         // 常见
        "Support Files\\Contents\\Windows", // Adobe Illustrator
        "Contents\\Windows",                // Adobe
        "Program Files",                    // 某些软件
        "Application",                      // 某些软件
        "",                                 // 根目录（最后搜索）
    ];

    // 优先在子目录中精确匹配
    for sub in &subdirs {
        let search_dir = if sub.is_empty() {
            install_dir.to_path_buf()
        } else {
            install_dir.join(sub)
        };
        if !search_dir.exists() {
            eprintln!("目录不存在，跳过: {}", search_dir.display());
            continue;
        }

        eprintln!("搜索目录: {}", search_dir.display());
        for name in &candidate_names {
            let full_path = search_dir.join(name);
            if full_path.exists() && is_valid_exe_path(&full_path) {
                eprintln!("找到可执行文件: {}", full_path.display());
                return Ok(full_path.to_string_lossy().to_string());
            }
        }
    }

    // 如果还没找到，进行深度搜索（跳过无关目录）
    if let Some(found) = deep_search_exe(install_dir, &candidate_names, 3) {
        eprintln!("深度搜索找到: {}", found);
        return Ok(found);
    }

    // 完全找不到，输出帮助信息
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

    Err(Error::new(HRESULT(1), format!("未找到 '{}' 的有效可执行文件", display_name)))
}

/// 在目录及其子目录中递归搜索（深度限制），跳过无关目录
fn deep_search_exe(dir: &Path, names: &[String], max_depth: usize) -> Option<String> {
    if max_depth == 0 { return None; }
    if let Ok(entries) = std::fs::read_dir(dir) {
        for entry in entries.filter_map(|e| e.ok()) {
            let path = entry.path();
            if path.is_dir() {
                let dir_name = path.file_name().and_then(|s| s.to_str()).unwrap_or("");
                let lower = dir_name.to_lowercase();
                // 跳过无关目录
                if lower.contains("resource") || lower.contains("_common")
                    || lower.contains("help") || lower.contains("sample")
                    || lower.contains("redist") || lower.contains("directx")
                    || lower.contains("fonts") || lower.contains("licenses") {
                    continue;
                }
                if let Some(found) = deep_search_exe(&path, names, max_depth - 1) {
                    return Some(found);
                }
            } else if path.is_file() {
                if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
                    if names.iter().any(|n| n.eq_ignore_ascii_case(name)) && is_valid_exe_path(&path) {
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

    // 获取可执行路径
    let exe_path = find_executable_path(&display_name)
        .map_err(|e| e.to_string())?;

    // 获取安装目录作为工作目录
    let install_path = get_install_path_by_name(&display_name)
        .ok_or_else(|| "无法获取安装目录".to_string())?;

    // 再次验证文件有效性
    let path = Path::new(&exe_path);
    if !path.exists() || !is_valid_exe_path(path) {
        return Err(format!("可执行文件无效或不存在: {}", exe_path));
    }

    let exe_name = path.file_name().and_then(|s| s.to_str()).unwrap_or("unknown.exe");
    if is_software_running(exe_name) {
        return Ok(());
    }

    eprintln!("启动: {}", exe_path);
    eprintln!("工作目录: {}", install_path);

    // 启动进程并设置工作目录
    Command::new(&exe_path)
        .current_dir(&install_path)
        .spawn()
        .map_err(|e| format!("启动失败: {}", e))?;
    Ok(())
}

// ========== 旧版兼容函数 ==========

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
    let install_path = get_install_path_by_name(display_name).unwrap_or_default();
    Command::new(&exe_path)
        .current_dir(&install_path)
        .spawn()
        .map_err(|e| {
            let msg = format!("启动 '{}' 失败: {}", exe_path, e);
            Error::new(HRESULT(e.raw_os_error().unwrap_or(1) as i32), msg)
        })?;
    Ok(())
}