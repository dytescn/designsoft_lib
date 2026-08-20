// src/utils/soft.rs
use std::collections::HashSet;
use std::ffi::OsString;
use std::os::windows::ffi::OsStringExt;
use windows::core::{PCWSTR, PWSTR};
use windows::Win32::Foundation::ERROR_NO_MORE_ITEMS;
use windows::Win32::System::Registry::{
    RegCloseKey, RegEnumKeyExW, RegOpenKeyExW, RegQueryValueExW,
    HKEY, HKEY_LOCAL_MACHINE, HKEY_CURRENT_USER,
    KEY_READ, KEY_WOW64_32KEY, KEY_WOW64_64KEY,
    REG_SAM_FLAGS, REG_VALUE_TYPE,
    REG_SZ, REG_EXPAND_SZ,
};

fn wide_to_string(wide: &[u16]) -> String {
    let null_pos = wide.iter().position(|&c| c == 0).unwrap_or(wide.len());
    OsString::from_wide(&wide[..null_pos])
        .to_string_lossy()
        .into_owned()
}

fn query_reg_string(key: HKEY, value_name: &str) -> Option<String> {
    let value_name_wide: Vec<u16> = value_name.encode_utf16().chain(Some(0)).collect();
    let mut data = [0u16; 1024];
    let mut data_size = (data.len() * std::mem::size_of::<u16>()) as u32;
    let mut type_ = REG_VALUE_TYPE(0);

    let ret = unsafe {
        RegQueryValueExW(
            key,
            PCWSTR(value_name_wide.as_ptr()),
            None,
            Some(&mut type_ as *mut REG_VALUE_TYPE),
            Some(data.as_mut_ptr() as *mut u8),
            Some(&mut data_size),
        )
    };
    if ret.0 != 0 {
        return None;
    }
    if type_ != REG_SZ && type_ != REG_EXPAND_SZ {
        return None;
    }
    let len = (data_size / 2) as usize;
    Some(wide_to_string(&data[..len]))
}

/// 内部搜索函数（遍历注册表子键）
fn search_subkey(
    root: HKEY,
    subkey: &str,
    keyword: &str,
    wow64_flag: REG_SAM_FLAGS,
) -> Vec<(String, String)> {
    let mut results = Vec::new();
    let subkey_wide: Vec<u16> = subkey.encode_utf16().chain(Some(0)).collect();
    let mut hkey = HKEY::default();
    let sam = KEY_READ | wow64_flag;

    let ret = unsafe {
        RegOpenKeyExW(
            root,
            PCWSTR(subkey_wide.as_ptr()),
            None,
            sam,
            &mut hkey,
        )
    };
    if ret.0 != 0 {
        return results;
    }

    let mut index = 0u32;
    let mut name_buf = [0u16; 256];
    let mut name_len; // 移除初始赋值

    loop {
        name_len = name_buf.len() as u32; // 在循环开始时赋值
        let ret = unsafe {
            RegEnumKeyExW(
                hkey,
                index,
                Some(PWSTR(name_buf.as_mut_ptr())),
                &mut name_len,
                None,
                None,
                None,
                None,
            )
        };
        if ret.0 == ERROR_NO_MORE_ITEMS.0 {
            break;
        }
        if ret.0 != 0 {
            index += 1;
            continue;
        }

        let key_name = wide_to_string(&name_buf[..name_len as usize]);

        let subkey_wide: Vec<u16> = key_name.encode_utf16().chain(Some(0)).collect();
        let mut sub_hkey = HKEY::default();
        let ret = unsafe {
            RegOpenKeyExW(
                hkey,
                PCWSTR(subkey_wide.as_ptr()),
                None,
                KEY_READ,
                &mut sub_hkey,
            )
        };
        if ret.0 != 0 {
            index += 1;
            continue;
        }

        if let Some(display_name) = query_reg_string(sub_hkey, "DisplayName") {
            if display_name.to_lowercase().contains(&keyword.to_lowercase()) {
                let version = query_reg_string(sub_hkey, "DisplayVersion")
                    .unwrap_or_else(|| "未知版本".to_string());
                results.push((display_name, version));
            }
        }

        // 忽略 RegCloseKey 返回值
        unsafe { let _ = RegCloseKey(sub_hkey); };
        index += 1;
    }

    unsafe { let _ = RegCloseKey(hkey); };
    results
}

/// 对外公开的查找函数：返回主程序（过滤掉子组件）
pub fn find_software(keyword: &str) -> Vec<(String, String)> {
    let subkey = r"SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall";
    let mut all = Vec::new();

    all.extend(search_subkey(HKEY_LOCAL_MACHINE, subkey, keyword, KEY_WOW64_64KEY));
    all.extend(search_subkey(HKEY_LOCAL_MACHINE, subkey, keyword, KEY_WOW64_32KEY));
    all.extend(search_subkey(HKEY_CURRENT_USER, subkey, keyword, REG_SAM_FLAGS(0)));

    let unique: HashSet<_> = all.into_iter().collect();
    let mut results: Vec<_> = unique.into_iter().collect();

    // 过滤掉子组件（名称中包含 " - " 的条目）
    results.retain(|(name, _)| !name.contains(" - "));

    // 如果还有多个（不同版本），取版本号最长的（通常最完整）
    if results.len() > 1 {
        results.sort_by(|a, b| b.1.len().cmp(&a.1.len()));
        results.truncate(1);
    }

    results
}

/// 根据软件的完整 DisplayName 获取安装路径（InstallLocation），
/// 如果 InstallLocation 不存在，尝试从 DisplayIcon 提取目录。
pub fn get_install_path_by_name(display_name: &str) -> Option<String> {
    let subkey = r"SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall";
    let views = [
        (HKEY_LOCAL_MACHINE, KEY_WOW64_64KEY),
        (HKEY_LOCAL_MACHINE, KEY_WOW64_32KEY),
        (HKEY_CURRENT_USER, REG_SAM_FLAGS(0)),
    ];
    for (root, flag) in views {
        if let Some(path) = get_value_for_display_name(root, subkey, display_name, "InstallLocation", flag) {
            if !path.is_empty() {
                return Some(path);
            }
        }
        // 如果 InstallLocation 不存在或为空，尝试 DisplayIcon
        if let Some(icon) = get_value_for_display_name(root, subkey, display_name, "DisplayIcon", flag) {
            let exe_path = icon.split(',').next().unwrap_or(&icon);
            if let Some(dir) = std::path::Path::new(exe_path).parent() {
                return Some(dir.to_string_lossy().to_string());
            }
        }
    }
    None
}

/// 获取软件的 DisplayIcon（可执行文件完整路径，可能包含逗号和参数）
pub fn get_display_icon(display_name: &str) -> Option<String> {
    let subkey = r"SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall";
    let views = [
        (HKEY_LOCAL_MACHINE, KEY_WOW64_64KEY),
        (HKEY_LOCAL_MACHINE, KEY_WOW64_32KEY),
        (HKEY_CURRENT_USER, REG_SAM_FLAGS(0)),
    ];
    for (root, flag) in views {
        if let Some(icon) = get_value_for_display_name(root, subkey, display_name, "DisplayIcon", flag) {
            if !icon.is_empty() {
                return Some(icon);
            }
        }
    }
    None
}

/// 内部辅助：在指定根键和子键下查找 DisplayName 完全匹配的子键，并读取指定值。
fn get_value_for_display_name(
    root: HKEY,
    subkey: &str,
    display_name: &str,
    value_name: &str,
    wow64_flag: REG_SAM_FLAGS,
) -> Option<String> {
    let subkey_wide: Vec<u16> = subkey.encode_utf16().chain(Some(0)).collect();
    let mut hkey = HKEY::default();
    let sam = KEY_READ | wow64_flag;
    unsafe {
        if RegOpenKeyExW(root, PCWSTR(subkey_wide.as_ptr()), None, sam, &mut hkey).0 != 0 {
            return None;
        }
    }

    let mut index = 0u32;
    let mut name_buf = [0u16; 256];
    let mut name_len; // 移除初始赋值

    loop {
        name_len = name_buf.len() as u32; // 循环中赋值
        let ret = unsafe {
            RegEnumKeyExW(
                hkey,
                index,
                Some(PWSTR(name_buf.as_mut_ptr())),
                &mut name_len,
                None,
                None,
                None,
                None,
            )
        };
        if ret.0 == ERROR_NO_MORE_ITEMS.0 {
            break;
        }
        if ret.0 != 0 {
            index += 1;
            continue;
        }

        let key_name = wide_to_string(&name_buf[..name_len as usize]);
        let subkey_wide: Vec<u16> = key_name.encode_utf16().chain(Some(0)).collect();
        let mut sub_hkey = HKEY::default();
        unsafe {
            if RegOpenKeyExW(hkey, PCWSTR(subkey_wide.as_ptr()), None, KEY_READ, &mut sub_hkey).0 != 0 {
                index += 1;
                continue;
            }
        }

        if let Some(dn) = query_reg_string(sub_hkey, "DisplayName") {
            if dn == display_name {
                if let Some(val) = query_reg_string(sub_hkey, value_name) {
                    unsafe { let _ = RegCloseKey(sub_hkey); };
                    return Some(val);
                }
            }
        }
        unsafe { let _ = RegCloseKey(sub_hkey); };
        index += 1;
    }

    unsafe { let _ = RegCloseKey(hkey); };
    None
}

/// 返回所有匹配的主程序（过滤子组件）的列表，不去重，返回所有不同版本
pub fn find_software_all(keyword: &str) -> Vec<(String, String)> {
    let subkey = r"SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall";
    let mut all = Vec::new();

    all.extend(search_subkey(HKEY_LOCAL_MACHINE, subkey, keyword, KEY_WOW64_64KEY));
    all.extend(search_subkey(HKEY_LOCAL_MACHINE, subkey, keyword, KEY_WOW64_32KEY));
    all.extend(search_subkey(HKEY_CURRENT_USER, subkey, keyword, REG_SAM_FLAGS(0)));

    // 去重（相同名称+版本）
    let unique: HashSet<_> = all.into_iter().collect();
    let mut results: Vec<_> = unique.into_iter().collect();

    // 过滤掉子组件（名称中包含 " - " 的条目）
    results.retain(|(name, _)| !name.contains(" - "));

    // 按名称排序，保证顺序稳定
    results.sort_by(|a, b| a.0.cmp(&b.0));
    results
}