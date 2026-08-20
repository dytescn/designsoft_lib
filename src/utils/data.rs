// src/utils/data.rs
use regex::Regex;

/// 根据软件的显示名称（DisplayName）生成可能的可执行文件名列表
pub fn get_candidate_exe_names(display_name: &str) -> Vec<String> {
    let lower = display_name.to_lowercase();
    let mut names: Vec<String> = Vec::new();

    // 提取显示名称中的年份（四位数字，如 2025）
    let year = extract_year(display_name);

    // 已知软件映射（按需扩展）
    if lower.contains("illustrator") {
        names.extend(vec![
            "Illustrator.exe".to_string(),
            "Adobe Illustrator.exe".to_string(),
            "AI.exe".to_string(),
        ]);
        if let Some(y) = year {
            names.push(format!("Illustrator {}.exe", y));
            names.push(format!("Adobe Illustrator {}.exe", y));
        }
    } else if lower.contains("photoshop") {
        names.extend(vec![
            "Photoshop.exe".to_string(),
            "Adobe Photoshop.exe".to_string(),
            "PS.exe".to_string(),
        ]);
        if let Some(y) = year {
            names.push(format!("Photoshop {}.exe", y));
            names.push(format!("Adobe Photoshop {}.exe", y));
        }
    } else if lower.contains("coreldraw") {
        names.extend(vec![
            "CorelDRAW.exe".to_string(),
            "CorelDRW.exe".to_string(),
        ]);
        if let Some(y) = year {
            names.push(format!("CorelDRAW {}.exe", y));
            names.push(format!("CorelDRW {}.exe", y));
        }
    } else if lower.contains("autocad") {
        names.extend(vec![
            "acad.exe".to_string(),
            "AutoCAD.exe".to_string(),
        ]);
        if let Some(y) = year {
            names.push(format!("AutoCAD {}.exe", y));
            names.push(format!("acad{}.exe", y));
        }
    } else if lower.contains("cinema 4d") || lower.contains("c4d") {
        names.extend(vec![
            "Cinema 4D.exe".to_string(),
            "C4D.exe".to_string(),
            "Cinema4D.exe".to_string(),
        ]);
        if let Some(y) = year {
            names.push(format!("Cinema 4D {}.exe", y));
        }
    } else if lower.contains("sketchup") {
        names.push("SketchUp.exe".to_string());
        if let Some(y) = year {
            names.push(format!("SketchUp {}.exe", y));
        }
    } else if lower.contains("blender") {
        names.push("blender.exe".to_string());
        // Blender 通常不包含年份
    } else if lower.contains("3ds max") || lower.contains("3dsmax") {
        names.extend(vec![
            "3dsmax.exe".to_string(),
            "3ds Max.exe".to_string(),
        ]);
        if let Some(y) = year {
            names.push(format!("3dsmax{}.exe", y));
        }
    } else {
        // 通用逻辑：尝试从显示名称中推测
        let parts: Vec<&str> = display_name.split_whitespace().collect();
        if !parts.is_empty() {
            names.push(format!("{}.exe", parts[0]));
            names.push(format!("{}.exe", display_name.replace(" ", "")));
            if lower.contains("adobe") && !lower.contains("illustrator") && !lower.contains("photoshop") {
                names.push(format!("Adobe{}.exe", display_name.replace(" ", "")));
            }
            if let Some(y) = year {
                names.push(format!("{}{}.exe", parts[0], y));
                names.push(format!("{}_{}.exe", parts[0], y));
            }
        }
    }

    // 去重
    names.sort();
    names.dedup();
    names
}

/// 从显示名称中提取年份（四位数字，范围 2015–2030），返回 Option<u32>
fn extract_year(display_name: &str) -> Option<u32> {
    let re = Regex::new(r"\b(20[1-9][0-9]|20[0-9]{2})\b").unwrap();
    for cap in re.captures_iter(display_name) {
        if let Some(m) = cap.get(1) {
            if let Ok(y) = m.as_str().parse::<u32>() {
                if y >= 2015 && y <= 2030 {
                    return Some(y);
                }
            }
        }
    }
    None
}