//! 可执行文件名推断的单元测试。

#![cfg(windows)]

use designsoft::utils::data::get_candidate_exe_names;

#[test]
fn test_get_candidate_exe_names_illustrator() {
    let names = get_candidate_exe_names("Adobe Illustrator 2025");
    assert!(names.contains(&"Illustrator.exe".to_string()));
    assert!(names.contains(&"Adobe Illustrator.exe".to_string()));
    assert!(names.contains(&"AI.exe".to_string()));
    assert!(names.contains(&"Illustrator 2025.exe".to_string()));
    assert!(names.contains(&"Adobe Illustrator 2025.exe".to_string()));
}

#[test]
fn test_get_candidate_exe_names_autocad() {
    let names = get_candidate_exe_names("AutoCAD 2024");
    assert!(names.contains(&"acad.exe".to_string()));
    assert!(names.contains(&"AutoCAD.exe".to_string()));
    assert!(names.contains(&"AutoCAD 2024.exe".to_string()));
    assert!(names.contains(&"acad2024.exe".to_string()));
}

#[test]
fn test_get_candidate_exe_names_coreldraw() {
    let names = get_candidate_exe_names("CorelDRAW Graphics Suite 2025");
    assert!(names.contains(&"CorelDRAW.exe".to_string()));
    assert!(names.contains(&"CorelDRW.exe".to_string()));
    assert!(names.contains(&"CorelDRAW 2025.exe".to_string()));
    assert!(names.contains(&"CorelDRW 2025.exe".to_string()));
}

#[test]
fn test_get_candidate_exe_names_unknown() {
    let names = get_candidate_exe_names("Some Unknown Software 2023");
    // 应包含通用规则
    assert!(names.contains(&"Some.exe".to_string()));
    assert!(names.contains(&"SomeUnknownSoftware2023.exe".to_string()));
    assert!(names.contains(&"Some2023.exe".to_string()));
    assert!(names.contains(&"Some_2023.exe".to_string()));
}

#[test]
fn test_get_candidate_exe_names_no_year() {
    let names = get_candidate_exe_names("Blender");
    assert!(names.contains(&"blender.exe".to_string()));
    // 不包含年份候选
    assert!(!names.iter().any(|s| s.contains("202")));
}
