//! 从 Windows 注册表的 Uninstall 键读取已安装软件信息。

use std::collections::HashSet;
use std::ffi::OsString;
use std::os::windows::ffi::OsStringExt;

use windows::core::{w, PCSTR, PCWSTR, PWSTR};
use windows::Win32::Foundation::ERROR_NO_MORE_ITEMS;
use windows::Win32::System::LibraryLoader::{GetModuleHandleW, GetProcAddress, LoadLibraryW};
use windows::Win32::System::Registry::{
    RegCloseKey, RegEnumKeyExW, RegOpenKeyExW, RegQueryValueExW, HKEY, HKEY_CURRENT_USER,
    HKEY_LOCAL_MACHINE, KEY_READ, KEY_WOW64_32KEY, KEY_WOW64_64KEY, REG_EXPAND_SZ, REG_SAM_FLAGS,
    REG_SZ, REG_VALUE_TYPE,
};

/// 卸载信息所在的注册表路径。
const UNINSTALL_KEY: &str = r"SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall";

/// 需要枚举的三个视图：64 位、32 位、当前用户。
const UNINSTALL_VIEWS: [(HKEY, REG_SAM_FLAGS); 3] = [
    (HKEY_LOCAL_MACHINE, KEY_WOW64_64KEY),
    (HKEY_LOCAL_MACHINE, KEY_WOW64_32KEY),
    (HKEY_CURRENT_USER, REG_SAM_FLAGS(0)),
];

/// 未提供版本号时使用的占位值。
const UNKNOWN_VERSION: &str = "未知版本";

/// 注册表 Uninstall 条目中与本库相关的信息。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InstalledSoftware {
    /// 显示名称（注册表的 `DisplayName`）。
    pub display_name: String,
    /// 版本号（优先 MSI 的 `ProductVersion`，其次 `DisplayVersion`）。
    pub version: String,
    /// 注册表的 `InstallLocation`，可能为空或缺失。
    pub install_location: Option<String>,
    /// 注册表的 `DisplayIcon`，可能包含 `,0` 之类的图标索引后缀。
    pub display_icon: Option<String>,
}

impl InstalledSoftware {
    /// 安装目录：优先 `InstallLocation`，缺失时由 `DisplayIcon` 的可执行文件路径推断。
    pub fn install_path(&self) -> Option<String> {
        if let Some(location) = self
            .install_location
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
        {
            return Some(location.to_string());
        }

        let icon = self.display_icon.as_deref()?;
        let exe = icon.split(',').next()?.trim();
        let dir = std::path::Path::new(exe).parent()?;
        (!dir.as_os_str().is_empty()).then(|| dir.to_string_lossy().into_owned())
    }
}

// ---------- 注册表辅助 ----------

fn wide(value: &str) -> Vec<u16> {
    value.encode_utf16().chain(Some(0)).collect()
}

fn wide_to_string(wide: &[u16]) -> String {
    let len = wide.iter().position(|&c| c == 0).unwrap_or(wide.len());
    OsString::from_wide(&wide[..len])
        .to_string_lossy()
        .into_owned()
}

/// 已打开的注册表键，Drop 时自动关闭。
struct RegKey(HKEY);

impl RegKey {
    fn open(root: HKEY, path: &str, sam: REG_SAM_FLAGS) -> Option<Self> {
        let path = wide(path);
        let mut hkey = HKEY::default();
        let ret = unsafe { RegOpenKeyExW(root, PCWSTR(path.as_ptr()), None, sam, &mut hkey) };
        (ret.0 == 0).then_some(Self(hkey))
    }

    fn open_subkey(&self, name: &str) -> Option<Self> {
        Self::open(self.0, name, KEY_READ)
    }

    /// 读取字符串值（`REG_SZ` / `REG_EXPAND_SZ`）。
    fn string(&self, value_name: &str) -> Option<String> {
        let name = wide(value_name);
        let mut value_type = REG_VALUE_TYPE(0);
        let mut size = 0u32;

        // 第一次调用只取所需字节数。
        let ret = unsafe {
            RegQueryValueExW(
                self.0,
                PCWSTR(name.as_ptr()),
                None,
                Some(&mut value_type as *mut REG_VALUE_TYPE),
                None,
                Some(&mut size),
            )
        };
        if ret.0 != 0 || (value_type != REG_SZ && value_type != REG_EXPAND_SZ) || size == 0 {
            return None;
        }

        let mut data = vec![0u16; (size as usize).div_ceil(2)];
        let mut size = (data.len() * std::mem::size_of::<u16>()) as u32;
        let ret = unsafe {
            RegQueryValueExW(
                self.0,
                PCWSTR(name.as_ptr()),
                None,
                Some(&mut value_type as *mut REG_VALUE_TYPE),
                Some(data.as_mut_ptr().cast()),
                Some(&mut size),
            )
        };
        if ret.0 != 0 {
            return None;
        }

        Some(wide_to_string(&data))
    }
}

impl Drop for RegKey {
    fn drop(&mut self) {
        unsafe {
            let _ = RegCloseKey(self.0);
        }
    }
}

/// 依次枚举三个视图的全部 Uninstall 子键，对每个条目调用 `visit`。
fn visit_uninstall_entries(mut visit: impl FnMut(&str, &RegKey)) {
    for (root, view) in UNINSTALL_VIEWS {
        let Some(uninstall) = RegKey::open(root, UNINSTALL_KEY, KEY_READ | view) else {
            continue;
        };

        let mut index = 0u32;
        let mut name_buf = [0u16; 256];
        loop {
            let mut name_len = name_buf.len() as u32;
            let ret = unsafe {
                RegEnumKeyExW(
                    uninstall.0,
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
            if ret.0 == 0 {
                let key_name = wide_to_string(&name_buf[..name_len as usize]);
                if let Some(sub) = uninstall.open_subkey(&key_name) {
                    visit(&key_name, &sub);
                }
            }
            index += 1;
        }
    }
}

fn read_entry(key_name: &str, key: &RegKey) -> Option<InstalledSoftware> {
    let display_name = key.string("DisplayName")?;
    if display_name.trim().is_empty() {
        return None;
    }

    Some(InstalledSoftware {
        version: resolve_version(key_name, key),
        display_name,
        install_location: key.string("InstallLocation"),
        display_icon: key.string("DisplayIcon"),
    })
}

/// 版本号：MSI 产品的 `ProductVersion` 更完整，优先于 `DisplayVersion`。
fn resolve_version(key_name: &str, key: &RegKey) -> String {
    // Uninstall 子键名通常就是 MSI 的 ProductCode。
    let product_code = key
        .string("ProductCode")
        .unwrap_or_else(|| key_name.to_string());

    get_msi_product_version(&product_code)
        .or_else(|| key.string("DisplayVersion"))
        .filter(|v| !v.trim().is_empty())
        .unwrap_or_else(|| UNKNOWN_VERSION.to_string())
}

// ---------- MSI API 动态调用 ----------

/// 与 `MsiGetProductInfoW` 原型一致，避免静态链接 msi.lib。
type MsiGetProductInfoWFunc = unsafe extern "system" fn(PCWSTR, PCWSTR, *mut u16, *mut u32) -> u32;

fn get_msi_product_version(product_code: &str) -> Option<String> {
    // 产品码不是 GUID 时 MSI 必然查不到，先粗筛掉非 MSI 条目。
    if !product_code.starts_with('{') {
        return None;
    }

    // SAFETY: 函数指针取自 msi.dll 的导出，签名与 MsiGetProductInfoW 一致；
    // product_code 与输出缓冲区在调用期间保持有效。
    unsafe {
        // msi.dll 通常已随 Windows 加载；未加载时显式加载，句柄随进程常驻。
        let module = GetModuleHandleW(w!("msi.dll"))
            .or_else(|_| LoadLibraryW(w!("msi.dll")))
            .ok()?;
        let address = GetProcAddress(module, PCSTR(c"MsiGetProductInfoW".as_ptr().cast()))?;
        let msi_get_product_info: MsiGetProductInfoWFunc = std::mem::transmute(address);

        let product_wide = wide(product_code);
        let mut buffer = [0u16; 256];
        let mut len = buffer.len() as u32;

        let result = msi_get_product_info(
            PCWSTR(product_wide.as_ptr()),
            w!("ProductVersion"),
            buffer.as_mut_ptr(),
            &mut len,
        );

        (result == 0).then(|| wide_to_string(&buffer[..len as usize]))
    }
}

// ---------- 公开查询接口 ----------

/// 按关键字查询匹配的软件（一次遍历返回完整信息）。
///
/// 名称包含 `" - "` 的子组件会被过滤，结果按名称、版本排序并去重。
pub fn find_software_details(keyword: &str) -> Vec<InstalledSoftware> {
    let keyword = keyword.to_lowercase();
    let mut entries = Vec::new();

    visit_uninstall_entries(|key_name, key| {
        let Some(entry) = read_entry(key_name, key) else {
            return;
        };
        if !entry.display_name.to_lowercase().contains(&keyword) {
            return;
        }
        // 过滤 "Foo - 组件" 这类子组件。
        if entry.display_name.contains(" - ") {
            return;
        }
        entries.push(entry);
    });

    let mut seen = HashSet::new();
    let mut results: Vec<_> = entries
        .into_iter()
        .filter(|entry| seen.insert((entry.display_name.clone(), entry.version.clone())))
        .collect();
    results.sort_by(|a, b| {
        a.display_name
            .cmp(&b.display_name)
            .then_with(|| a.version.cmp(&b.version))
    });
    results
}

/// 按完整 DisplayName 精确查询一个条目。
pub fn find_software_by_name(display_name: &str) -> Option<InstalledSoftware> {
    let mut found = None;

    visit_uninstall_entries(|key_name, key| {
        if found.is_some() {
            return;
        }
        if key.string("DisplayName").as_deref() == Some(display_name) {
            found = read_entry(key_name, key);
        }
    });

    found
}

/// 按关键字返回全部匹配的 `(DisplayName, Version)`。
pub fn find_software_all(keyword: &str) -> Vec<(String, String)> {
    find_software_details(keyword)
        .into_iter()
        .map(|entry| (entry.display_name, entry.version))
        .collect()
}

/// 旧接口：按关键字返回单个最佳匹配，推荐改用 [`find_software_all`]。
pub fn find_software(keyword: &str) -> Vec<(String, String)> {
    find_software_all(keyword).into_iter().take(1).collect()
}

/// 根据完整 DisplayName 获取安装目录。
pub fn get_install_path_by_name(display_name: &str) -> Option<String> {
    find_software_by_name(display_name)?.install_path()
}

/// 根据完整 DisplayName 获取 `DisplayIcon` 原始值。
pub fn get_display_icon(display_name: &str) -> Option<String> {
    find_software_by_name(display_name)?.display_icon
}

#[cfg(test)]
mod tests {
    use super::InstalledSoftware;

    fn software(install_location: Option<&str>, display_icon: Option<&str>) -> InstalledSoftware {
        InstalledSoftware {
            display_name: "Foo".to_string(),
            version: "1.0.0".to_string(),
            install_location: install_location.map(str::to_string),
            display_icon: display_icon.map(str::to_string),
        }
    }

    #[test]
    fn install_path_prefers_install_location() {
        let entry = software(Some(r"C:\Foo"), Some(r"C:\Bar\foo.exe,0"));
        assert_eq!(entry.install_path().as_deref(), Some(r"C:\Foo"));
    }

    #[test]
    fn install_path_falls_back_to_display_icon() {
        let entry = software(Some("   "), Some(r"C:\Bar\foo.exe,0"));
        assert_eq!(entry.install_path().as_deref(), Some(r"C:\Bar"));
    }

    #[test]
    fn install_path_is_none_without_location_and_icon() {
        assert!(software(None, None).install_path().is_none());
    }
}
