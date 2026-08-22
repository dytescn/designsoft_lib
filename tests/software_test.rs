// tests/soft_test.rs
use designsoft::utils::soft::{
    find_software,
    find_software_all,
    get_install_path_by_name,
    get_display_icon,
};

#[test]
fn test_find_software_existing() {
    // 测试一个已知安装的软件（如 CorelDRAW）
    let results = find_software("CorelDRAW");
    if results.is_empty() {
        eprintln!("⚠️ CorelDRAW 未安装，跳过测试");
        return;
    }
    println!("{:?}",results);
    assert_eq!(results.len(), 1);    
    let (name, version) = &results[0];
    assert!(name.contains("CorelDRAW"));
    assert!(!version.is_empty());
}

#[test]
fn test_find_software_not_existing() {
    let results = find_software("ThisSoftwareDoesNotExistXYZ");
    assert!(results.is_empty());
}

#[test]
fn test_find_software_all() {
    let results = find_software_all("Illustrator");
    if results.is_empty() {
        eprintln!("⚠️ Illustrator 未安装，跳过测试");
        return;
    }
    // 可能返回多个版本，每个条目都应包含 Illustrator
    for (name, version) in results {
        assert!(name.contains("Illustrator") || name.contains("illustrator"));
        assert!(!version.is_empty());
    }
}

#[test]
fn test_get_install_path_by_name() {
    // 先找一个已知软件
    let results = find_software("CorelDRAW");
    if results.is_empty() {
        eprintln!("⚠️ CorelDRAW 未安装，跳过测试");
        return;
    }
    let (name, _) = &results[0];
    let path = get_install_path_by_name(name);
    assert!(path.is_some());
    let path_str = path.unwrap();
    assert!(std::path::Path::new(&path_str).exists());
}


#[test]
fn test_find_software_all_filters_subcomponents() {
    // 此测试验证 find_software_all 是否过滤掉包含 " - " 的子组件
    // 使用一个已知有子组件的软件（如 CorelDRAW）
    let results = find_software_all("CorelDRAW");
    if results.is_empty() {
        eprintln!("⚠️ CorelDRAW 未安装，跳过过滤测试");
        return;
    }
    for (name, _) in results {
        // 确保没有子组件（名称包含 " - "）
        assert!(!name.contains(" - "), "发现子组件: {}", name);
    }
}

#[test]
fn test_get_install_path_by_name_not_found() {
    let path = get_install_path_by_name("NonExistentSoftwareXYZ");
    assert!(path.is_none());
}

#[test]
fn test_get_display_icon() {
    // 先找一个已知软件
    let results = find_software("CorelDRAW");
    if results.is_empty() {
        eprintln!("⚠️ CorelDRAW 未安装，跳过 DisplayIcon 测试");
        return;
    }
    let (name, _) = &results[0];
    let icon = get_display_icon(name);
    // 某些软件可能没有 DisplayIcon，所以这里只检查如果存在则有效
    if let Some(icon_path) = icon {
        assert!(!icon_path.is_empty());
        // 可能指向 .exe 或 .ico，不做进一步检查
    } else {
        eprintln!("⚠️ 软件 {} 没有 DisplayIcon 注册表值", name);
    }
}

#[test]
fn test_get_display_icon_not_found() {
    let icon = get_display_icon("NonExistentSoftwareXYZ");
    assert!(icon.is_none());
}