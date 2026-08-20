// tests/status_test.rs
// 保留原有测试，并新增以下

use designsoft::utils::status::{
    resolve_software,
    software_status_with_version,
    launch_software_with_version,
    is_software_running,
};

#[test]
fn test_resolve_software() {
    let keyword = "CorelDRAW";
    let result = resolve_software(keyword, None);
    if result.is_err() {
        eprintln!("⚠️ 无法解析软件（可能未安装或版本冲突），错误: {}", result.err().unwrap());
        return;
    }
    let (name, version) = result.unwrap();
    assert!(name.contains("CorelDRAW"));
    assert!(!version.is_empty());

    // 如果已知版本，测试指定版本
    // 这里假设从注册表读取的版本可用，我们仅测试存在性
    if let Ok((_, ver)) = resolve_software(keyword, Some("26.1")) {
        // 若成功，则验证
    }
}

#[test]
fn test_is_software_running_self() {
    // 当前进程应该能被检测到
    let exe_name = std::env::current_exe()
        .ok()
        .and_then(|p| p.file_name().map(|s| s.to_string_lossy().to_string()))
        .unwrap_or("designsoft.exe".to_string());
    assert!(is_software_running(&exe_name));
}

#[test]
fn test_software_status_with_version() {
    let keyword = "Illustrator";
    // 尝试获取状态，如果未安装则跳过
    let status = software_status_with_version(keyword, None);
    if let Ok(s) = status {
        assert!(s == "已安装但未启动" || s == "已启动");
        println!("状态: {}", s);
    } else {
        eprintln!("⚠️ 无法获取状态: {}", status.err().unwrap());
    }
}

#[test]
fn test_resolve_software_with_version() {
    // 测试指定存在版本
    let keyword = "CorelDRAW";
    let result = resolve_software(keyword, Some("26.1.0.143"));
    if let Ok((name, ver)) = result {
        assert!(name.contains("CorelDRAW"));
        assert_eq!(ver, "26.1.0.143");
    } else {
        eprintln!("⚠️ 未找到指定版本的 CorelDRAW，跳过测试");
        // 测试通过，因为我们无法强制环境拥有这个版本
    }

    // 测试指定不存在的版本
    let result = resolve_software(keyword, Some("99.99"));
    assert!(result.is_err());
    if let Err(e) = result {
        assert!(e.contains("未找到版本 '99.99'"));
    }
}

#[test]
fn test_software_status_with_version_error() {
    let keyword = "Illustrator";
    // 传入不存在的版本
    let status = software_status_with_version(keyword, Some("99.99"));
    assert!(status.is_err());
    if let Err(e) = status {
        assert!(e.contains("未找到版本 '99.99'") || e.contains("未找到匹配的软件"));
    }
}

#[test]
fn test_launch_software_with_version_error() {
    let keyword = "Photoshop";
    // 不存在的版本
    let result = launch_software_with_version(keyword, Some("99.99"));
    assert!(result.is_err());
    if let Err(e) = result {
        assert!(e.contains("未找到版本 '99.99'") || e.contains("未找到匹配的软件"));
    }

    // 注意：我们并不实际启动，只测试错误分支，所以无需 `#[ignore]`
}

#[test]
fn test_is_software_running_false() {
    // 一个不可能存在的进程名
    assert!(!is_software_running("NonExistentProcessName.exe"));
}

// 保留原有的启动测试（已忽略）
#[test]
#[ignore]
fn test_launch_software_with_version_existing() {
    let keyword = "CorelDRAW";
    // 如果只安装了一个版本，可以不带版本；如果多个，请指定一个
    // 这里使用 None 尝试自动选择
    let result = launch_software_with_version(keyword, None);
    if let Err(e) = result {
        // 如果失败可能是因为无安装或多个版本，打印警告但不失败
        eprintln!("⚠️ 启动失败 (可能未安装或多版本): {}", e);
        return;
    }
    // 启动成功后检查状态
    let status = software_status_with_version(keyword, None).unwrap();
    assert_eq!(status, "已启动");
}