//! `design_soft` 线上包查询测试。
//!
//! 需要访问线上 S3，默认忽略：
//!
//! ```bash
//! cargo test --test pkg_test -- --ignored --nocapture
//! ```

#![cfg(windows)]

use designsoft::utils::pkg;

#[test]
#[ignore = "需要访问线上 S3"]
fn test_lookup_unknown_package() {
    match pkg::lookup("this-package-should-not-exist-xyz") {
        Ok(None) => println!("未知包返回 None（符合预期）"),
        Ok(Some(version)) => panic!("不存在的包返回了版本 {version}"),
        Err(error) => panic!("查询失败: {error}"),
    }
}

#[test]
#[ignore = "需要访问线上 S3"]
fn test_lookup_known_package() {
    match pkg::lookup("vectorcraft-cli") {
        Ok(Some(version)) => println!("vectorcraft-cli 线上版本: {version}"),
        Ok(None) => println!("vectorcraft-cli 线上不存在"),
        Err(error) => panic!("查询失败: {error}"),
    }
}

#[test]
#[ignore = "会下载并启动 vectorcraft-cli"]
fn test_open_and_status_vectorcraft() {
    let dir = std::env::temp_dir().join("designsoft-pkg-open-test");
    let _ = std::fs::remove_dir_all(&dir);

    let expected = pkg::lookup("vectorcraft-cli")
        .expect("查询线上版本")
        .expect("vectorcraft-cli 应存在于线上");

    let opened = pkg::open_in("vectorcraft-cli", &dir).expect("打开 vectorcraft-cli");
    println!("opened: {opened:?}");
    assert_eq!(opened.name, "vectorcraft-cli");
    assert_eq!(opened.version, expected);
    assert!(opened.downloaded, "首次打开应执行下载");
    assert!(opened.exe_path.is_file());
    assert!(dir.join(pkg::VERSION_FILE).is_file());

    // CLI 启动后打印帮助并退出，等进程结束后再查状态。
    std::thread::sleep(std::time::Duration::from_secs(2));
    let status = pkg::status_in("vectorcraft-cli", &dir).expect("查询状态");
    println!("status: {status:?}");
    assert_eq!(status.version, expected);
    assert!(status.exe_path.is_file());
    assert!(!status.running, "CLI 打印帮助后应已退出");

    let _ = std::fs::remove_dir_all(&dir);
}
