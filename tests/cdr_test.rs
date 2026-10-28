//! CorelDRAW COM 接口的集成测试。
//!
//! 这些用例会启动/关闭 CorelDRAW，默认忽略，需显式运行：
//!
//! ```bash
//! cargo test --test cdr_test -- --ignored --nocapture
//! ```

#![cfg(windows)]

use std::ffi::{c_char, CStr, CString};
use std::time::Duration;

use designsoft::post_start;
use designsoft::utils::cdr::{self, CdrSession};

#[test]
#[ignore = "会启动并关闭 CorelDRAW"]
fn test_open_status_close_coreldraw() {
    // 1. 打开 COM 接口（未运行时自动启动）
    let session = CdrSession::open(None).expect("无法打开 CorelDRAW 接口");
    let info = session.info();

    println!("version       = {:?}", info.version);
    println!("version_major = {:?}", info.version_major);
    println!("program_path  = {:?}", info.program_path);
    println!("document_count= {:?}", info.document_count);
    println!("visible       = {:?}", info.visible);

    assert!(info.version.is_some(), "绑定成功后应能读取版本号");

    // session 可继续使用 cdrsdk 的完整接口
    assert!(session.application().documents().is_some());

    // 2. 状态查询（进程已在运行）
    let status = cdr::status(None).expect("状态查询失败");
    assert!(status.running, "CorelDRAW 应处于运行状态");
    assert_eq!(
        status.info.and_then(|i| i.version),
        info.version,
        "状态查询读到的版本应与 session 一致"
    );

    drop(session);

    // 3. 关闭（Quit + 超时后强制结束）
    cdr::close(None, Duration::from_secs(10)).expect("关闭 CorelDRAW 失败");

    let after = cdr::status(None).expect("关闭后状态查询失败");
    assert!(!after.running, "关闭后不应再有 CorelDRAW 主进程");
}

#[test]
#[ignore = "会启动并显示 CorelDRAW"]
fn test_post_start_opens_coreldraw_via_cdrsdk() {
    let request = CString::new(r#"{"keyword":"CorelDRAW"}"#).expect("请求 JSON");
    let mut output = vec![0 as c_char; 64 * 1024];

    let ret = post_start(request.as_ptr(), output.as_mut_ptr());
    assert_eq!(ret, 0, "post_start 应写入合法 JSON");

    // SAFETY: post_start 已写入以 NUL 结尾的 JSON。
    let response = unsafe { CStr::from_ptr(output.as_ptr()) }
        .to_str()
        .expect("响应应为 UTF-8");
    println!("post_start -> {response}");
    assert!(response.contains("\"code\":200"), "响应: {response}");

    // 由 cdrsdk 打开的实例应能被进程检测识别。
    let status = cdr::status(None).expect("状态查询失败");
    assert!(status.running, "post_start 后 CorelDRAW 应在运行");

    cdr::close_with_default_timeout(None).expect("关闭失败");
}

#[test]
#[ignore = "会启动并显示 CorelDRAW，保持 15 秒"]
fn test_open_software_keeps_window_visible() {
    let info = cdr::open_software(None).expect("open_software");
    println!("open_software info = {info:?}");
    assert_eq!(info.visible, Some(true), "打开后窗口应可见");

    // COM 客户端释放后，窗口仍应保持可见。
    let session = CdrSession::open(None).expect("rebind");
    println!("rebound visible = {:?}", session.info().visible);
    assert_eq!(
        session.info().visible,
        Some(true),
        "COM 释放后窗口应保持可见"
    );
    drop(session);

    println!("保持可见 15 秒...");
    std::thread::sleep(Duration::from_secs(15));

    cdr::close_with_default_timeout(None).expect("关闭失败");
}

#[test]
#[ignore = "会启动并关闭 CorelDRAW"]
fn test_close_is_idempotent() {
    // 未运行时关闭应直接返回成功。
    cdr::close_with_default_timeout(None).expect("未运行时关闭应成功");
    cdr::close_with_default_timeout(None).expect("重复关闭应成功");
}
