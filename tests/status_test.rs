//! 状态查询与启动的集成测试。
//!
//! 部分用例依赖本机安装的设计软件；未安装时会打印提示并跳过断言。

use designsoft::utils::soft::find_software_all;
use designsoft::utils::status::{
    is_software_running, launch_software_with_version, resolve_software,
    software_status_with_version,
};

#[test]
fn test_resolve_software() {
    let keyword = "CorelDRAW";
    let Ok((name, version)) = resolve_software(keyword, None) else {
        eprintln!("⚠️ CorelDRAW 未安装或存在多个版本，跳过测试");
        return;
    };

    assert!(name.contains("CorelDRAW"));
    assert!(!version.is_empty());
}

#[test]
fn test_is_software_running_self() {
    // 当前进程应该能被检测到。
    let exe_name = std::env::current_exe()
        .ok()
        .and_then(|p| p.file_name().map(|s| s.to_string_lossy().into_owned()))
        .unwrap_or_else(|| "designsoft.exe".to_string());

    assert!(is_software_running(&exe_name));
}

#[test]
fn test_software_status_with_version() {
    match software_status_with_version("Illustrator", None) {
        Ok(status) => assert!(
            status == "已安装但未启动" || status == "已启动",
            "意外状态: {status}"
        ),
        Err(e) => eprintln!("⚠️ 无法获取状态（Illustrator 可能未安装）: {e}"),
    }
}

#[test]
fn test_resolve_software_with_version() {
    let keyword = "CorelDRAW";
    let installed = find_software_all(keyword);

    match installed.first() {
        Some((name, version)) => {
            // 完整版本号应能解析。
            let (resolved_name, resolved_version) =
                resolve_software(keyword, Some(version.as_str())).expect("已安装版本应能解析");
            assert_eq!(resolved_name, *name);
            assert_eq!(resolved_version, *version);

            // 主版本号（第一个点之前）也应能匹配。
            let major = version.split('.').next().unwrap();
            assert!(resolve_software(keyword, Some(major)).is_ok());
        }
        None => eprintln!("⚠️ CorelDRAW 未安装，跳过指定版本解析测试"),
    }

    // 不存在的版本始终报错。
    assert!(resolve_software(keyword, Some("99.99")).is_err());
}

#[test]
fn test_software_status_with_version_error() {
    // 不存在的版本：无论软件是否安装，都应返回错误。
    let status = software_status_with_version("Illustrator", Some("99.99"));
    assert!(status.is_err());
    if let Err(e) = status {
        assert!(e.contains("未找到版本 '99.99'") || e.contains("未找到匹配的软件"));
    }
}

#[test]
fn test_launch_software_with_version_error() {
    // 只测试错误分支，不会实际启动软件。
    let result = launch_software_with_version("Photoshop", Some("99.99"));
    assert!(result.is_err());
    if let Err(e) = result {
        assert!(e.contains("未找到版本 '99.99'") || e.contains("未找到匹配的软件"));
    }
}

#[test]
fn test_is_software_running_false() {
    assert!(!is_software_running("NonExistentProcessName.exe"));
}

#[test]
#[ignore = "会真实启动软件"]
fn test_launch_software_with_version_existing() {
    let keyword = "CorelDRAW";
    if let Err(e) = launch_software_with_version(keyword, None) {
        eprintln!("⚠️ 启动失败（可能未安装或多版本）: {e}");
        return;
    }

    let status = software_status_with_version(keyword, None).unwrap();
    assert_eq!(status, "已启动");
}
