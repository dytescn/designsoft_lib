// src/utils/soft.rs
use std::collections::HashSet;
use std::ffi::OsString;
use std::os::windows::ffi::OsStringExt;
use windows::core::{PCWSTR, PCSTR, PWSTR};
use windows::Win32::Foundation::ERROR_NO_MORE_ITEMS;
use windows::Win32::System::Registry::{
    RegCloseKey, RegEnumKeyExW, RegOpenKeyExW, RegQueryValueExW,
    HKEY, HKEY_LOCAL_MACHINE, HKEY_CURRENT_USER,
    KEY_READ, KEY_WOW64_32KEY, KEY_WOW64_64KEY,
    REG_SAM_FLAGS, REG_VALUE_TYPE,
    REG_SZ, REG_EXPAND_SZ,
};
use windows::Win32::System::LibraryLoader::{GetModuleHandleW, GetProcAddress};


// ---------- 辅助函数 ----------
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

    // ---------- 动态调用 MSI API ----------
    // ... 顶部导入不变

    type MsiGetProductInfoWFunc = unsafe extern "system" fn(
        sz_product: PCWSTR,
        sz_property: PCWSTR,
        lp_value_buf: *mut u16,
        pcch_value_buf: *mut u32,
    ) -> u32;

    fn get_msi_product_version_dynamic(product_code: &str) -> Option<String> {
        unsafe {
            let module_name: Vec<u16> = "msi.dll\0".encode_utf16().collect();
            let h_module = GetModuleHandleW(PCWSTR(module_name.as_ptr())).ok()?;
            if h_module.is_invalid() {
                return None;
            }

            let func_name = "MsiGetProductInfoW\0";
            let proc_addr = GetProcAddress(h_module, PCSTR(func_name.as_ptr()));
            let proc = proc_addr?;
            if proc as usize == 0 {
                return None;
            }
            let func: MsiGetProductInfoWFunc = std::mem::transmute(proc);

            let product_wide: Vec<u16> = product_code.encode_utf16().chain(Some(0)).collect();
            let property_wide: Vec<u16> = "ProductVersion\0".encode_utf16().collect();
            let mut buffer = [0u16; 256];
            let mut len = buffer.len() as u32;

            let result = func(
                PCWSTR(product_wide.as_ptr()),
                PCWSTR(property_wide.as_ptr()),
                buffer.as_mut_ptr(),
                &mut len,
            );

            if result == 0 {
                Some(wide_to_string(&buffer[..len as usize]))
            } else {
                None
            }
        }
    }

// ---------- 内部搜索函数 ----------
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
    let mut name_len;

    loop {
        name_len = name_buf.len() as u32;
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
                let mut version = query_reg_string(sub_hkey, "DisplayVersion")
                    .unwrap_or_else(|| "未知版本".to_string());

                if let Some(product_code) = query_reg_string(sub_hkey, "ProductCode") {
                    if let Some(msi_ver) = get_msi_product_version_dynamic(&product_code) {
                        if msi_ver.len() > version.len() {
                            version = msi_ver;
                        }
                    }
                }

                // 只保留主版本号（第一个点之前）
                let main_version = version.split('.').next().unwrap_or(&version).to_string();
                results.push((display_name, main_version));
            }
        }
        unsafe { let _ = RegCloseKey(sub_hkey); };
        index += 1;
    }

    unsafe { let _ = RegCloseKey(hkey); };
    results
}

// ---------- 公开函数 ----------
pub fn find_software(keyword: &str) -> Vec<(String, String)> {
    let subkey = r"SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall";
    let mut all = Vec::new();

    all.extend(search_subkey(HKEY_LOCAL_MACHINE, subkey, keyword, KEY_WOW64_64KEY));
    all.extend(search_subkey(HKEY_LOCAL_MACHINE, subkey, keyword, KEY_WOW64_32KEY));
    all.extend(search_subkey(HKEY_CURRENT_USER, subkey, keyword, REG_SAM_FLAGS(0)));

    let unique: HashSet<_> = all.into_iter().collect();
    let mut results: Vec<_> = unique.into_iter().collect();
    results.retain(|(name, _)| !name.contains(" - "));

    if results.len() > 1 {
        results.sort_by(|a, b| b.1.len().cmp(&a.1.len()));
        results.truncate(1);
    }
    results
}

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
        if let Some(icon) = get_value_for_display_name(root, subkey, display_name, "DisplayIcon", flag) {
            let exe_path = icon.split(',').next().unwrap_or(&icon);
            if let Some(dir) = std::path::Path::new(exe_path).parent() {
                return Some(dir.to_string_lossy().to_string());
            }
        }
    }
    None
}

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
    let mut name_len;

    loop {
        name_len = name_buf.len() as u32;
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

pub fn find_software_all(keyword: &str) -> Vec<(String, String)> {
    let subkey = r"SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall";
    let mut all = Vec::new();

    all.extend(search_subkey(HKEY_LOCAL_MACHINE, subkey, keyword, KEY_WOW64_64KEY));
    all.extend(search_subkey(HKEY_LOCAL_MACHINE, subkey, keyword, KEY_WOW64_32KEY));
    all.extend(search_subkey(HKEY_CURRENT_USER, subkey, keyword, REG_SAM_FLAGS(0)));

    let unique: HashSet<_> = all.into_iter().collect();
    let mut results: Vec<_> = unique.into_iter().collect();
    results.retain(|(name, _)| !name.contains(" - "));
    results.sort_by(|a, b| a.0.cmp(&b.0));
    results
}