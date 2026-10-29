//! `design_soft` 包集成测试。
//!
//! 需要 `dytes` 可执行文件与线上包，默认忽略：
//!
//! ```bash
//! cargo test --test pkg_test -- --ignored --nocapture
//! ```

#![cfg(windows)]

use designsoft::utils::pkg;

#[test]
#[ignore = "会调用 dytes 下载并启动 vectorcraft-cli"]
fn test_open_and_status_vectorcraft() {
    let root = std::env::temp_dir().join("designsoft-pkg-open-test");
    let _ = std::fs::remove_dir_all(&root);

    let opened = pkg::open_in("vectorcraft-cli", &root).expect("打开 vectorcraft-cli");
    println!("opened: {opened:?}");
    assert_eq!(opened.name, "vectorcraft-cli");
    assert!(opened.exe_path.is_file());
    assert!(
        root.join("apps")
            .join("vectorcraft-cli")
            .join("dytes.json")
            .is_file(),
        "dytes 应把清单保存到 apps/vectorcraft-cli/dytes.json"
    );

    // CLI 启动后打印帮助并退出，等进程结束后再查状态。
    std::thread::sleep(std::time::Duration::from_secs(2));
    let status = pkg::status_in("vectorcraft-cli", &root).expect("查询状态");
    println!("status: {status:?}");
    assert_eq!(status.version, opened.version);
    assert!(!status.running, "CLI 打印帮助后应已退出");

    let _ = std::fs::remove_dir_all(&root);
}
