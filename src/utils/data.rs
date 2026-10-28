//! 根据软件显示名称推断可能的可执行文件名。
//!
//! 已知软件使用内置映射，未知软件按 DisplayName 套用通用规则。

use std::sync::LazyLock;

use regex::Regex;

/// 匹配四位年份（如 2025）。
static YEAR_PATTERN: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\b20\d{2}\b").expect("内置年份正则表达式始终合法"));

/// 只有落在此范围内的数字才会被当作版本年份。
fn is_valid_year(year: u32) -> bool {
    (2015..=2030).contains(&year)
}

/// 已知软件的可执行文件名映射。
///
/// `year_executables` 中的 `{}` 会替换为年份（如 `Illustrator 2025.exe`）。
struct KnownSoftware {
    /// 用于匹配 DisplayName 的关键字（不区分大小写）。
    keywords: &'static [&'static str],
    /// 与年份无关的可执行文件名。
    executables: &'static [&'static str],
    /// 带年份的可执行文件名模板。
    year_executables: &'static [&'static str],
}

const KNOWN_SOFTWARE: &[KnownSoftware] = &[
    KnownSoftware {
        keywords: &["illustrator"],
        executables: &["Illustrator.exe", "Adobe Illustrator.exe", "AI.exe"],
        year_executables: &["Illustrator {}.exe", "Adobe Illustrator {}.exe"],
    },
    KnownSoftware {
        keywords: &["photoshop"],
        executables: &["Photoshop.exe", "Adobe Photoshop.exe", "PS.exe"],
        year_executables: &["Photoshop {}.exe", "Adobe Photoshop {}.exe"],
    },
    KnownSoftware {
        keywords: &["coreldraw"],
        executables: &["CorelDRAW.exe", "CorelDRW.exe"],
        year_executables: &["CorelDRAW {}.exe", "CorelDRW {}.exe"],
    },
    KnownSoftware {
        keywords: &["autocad"],
        executables: &["acad.exe", "AutoCAD.exe"],
        year_executables: &["AutoCAD {}.exe", "acad{}.exe"],
    },
    KnownSoftware {
        keywords: &["cinema 4d", "c4d"],
        executables: &["Cinema 4D.exe", "C4D.exe", "Cinema4D.exe"],
        year_executables: &["Cinema 4D {}.exe"],
    },
    KnownSoftware {
        keywords: &["sketchup"],
        executables: &["SketchUp.exe"],
        year_executables: &["SketchUp {}.exe"],
    },
    KnownSoftware {
        keywords: &["blender"],
        executables: &["blender.exe"],
        // Blender 的安装路径通常不含年份。
        year_executables: &[],
    },
    KnownSoftware {
        keywords: &["3ds max", "3dsmax"],
        executables: &["3dsmax.exe", "3ds Max.exe"],
        year_executables: &["3dsmax{}.exe"],
    },
];

/// 根据软件的显示名称（DisplayName）生成可能的可执行文件名列表。
pub fn get_candidate_exe_names(display_name: &str) -> Vec<String> {
    let lower = display_name.to_lowercase();
    let year = extract_year(display_name);

    let mut names = match KNOWN_SOFTWARE.iter().find(|software| {
        software
            .keywords
            .iter()
            .any(|keyword| lower.contains(keyword))
    }) {
        Some(software) => known_candidates(software, year),
        None => generic_candidates(display_name, year),
    };

    names.sort();
    names.dedup();
    names
}

fn known_candidates(software: &KnownSoftware, year: Option<u32>) -> Vec<String> {
    let mut names: Vec<String> = software
        .executables
        .iter()
        .map(|name| (*name).to_string())
        .collect();

    if let Some(year) = year {
        names.extend(
            software
                .year_executables
                .iter()
                .map(|template| template.replace("{}", &year.to_string())),
        );
    }

    names
}

/// 未知软件的通用规则：按首个单词、压缩空格后的全名和年份组合猜测。
fn generic_candidates(display_name: &str, year: Option<u32>) -> Vec<String> {
    let Some(first_word) = display_name.split_whitespace().next() else {
        return Vec::new();
    };
    let compact = display_name.replace(' ', "");

    let mut names = vec![format!("{first_word}.exe"), format!("{compact}.exe")];

    if display_name.to_lowercase().contains("adobe") {
        names.push(format!("Adobe{compact}.exe"));
    }
    if let Some(year) = year {
        names.push(format!("{first_word}{year}.exe"));
        names.push(format!("{first_word}_{year}.exe"));
    }

    names
}

/// 从显示名称中提取版本年份，返回 `2015..=2030` 范围内的第一个四位数字。
fn extract_year(display_name: &str) -> Option<u32> {
    YEAR_PATTERN
        .find_iter(display_name)
        .filter_map(|m| m.as_str().parse::<u32>().ok())
        .find(|year| is_valid_year(*year))
}
