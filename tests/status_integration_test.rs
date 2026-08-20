// tests/status_integration_test.rs
use designsoft::utils::status::{resolve_software, software_status_with_version};

#[test]
fn test_full_flow_known_software() {
    let keyword = "Illustrator";
    // 先获取软件列表（通过 soft 模块）
    let all = designsoft::utils::soft::find_software_all(keyword);
    if all.is_empty() {
        eprintln!("⚠️ Illustrator 未安装，跳过集成测试");
        return;
    }

    // 取第一个版本
    let (_, version) = &all[0];
    // 解析该版本
    let resolved = resolve_software(keyword, Some(version));
    assert!(resolved.is_ok());
    let (name, ver) = resolved.unwrap();
    assert_eq!(ver, *version);

    // 查询状态
    let status = software_status_with_version(keyword, Some(version));
    assert!(status.is_ok());
    let s = status.unwrap();
    assert!(s == "已安装但未启动" || s == "已启动");
    println!("{} v{} 状态: {}", name, ver, s);
}