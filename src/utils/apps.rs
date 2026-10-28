//! 设计软件路由表。
//!
//! 支持的设计软件数量有限，集中放在 [`APPS`] 数组里声明：哪些通过 COM
//! 接口打开，哪些从线上 `design_soft` 包下载打开，其余按注册表安装信息
//! 启动本地 exe。新增软件只需改数组，`post_start` 不需要改动。

/// COM 打开方式的具体实现。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ComApp {
    /// `utils::cdr`：cdrsdk 的 late-binding 接口。
    CorelDraw,
}

/// 软件的打开方式。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OpenMethod {
    /// 通过 COM 接口打开。
    Com(ComApp),
    /// 从线上 `design_soft` 包下载并启动，值为包名。
    Package(&'static str),
    /// 按注册表安装信息启动本地 exe（默认方式）。
    Installed,
}

/// 一个受支持的设计软件。
#[derive(Debug, Clone, Copy)]
pub struct AppEntry {
    /// 规范名称，用于日志和文档。
    pub name: &'static str,
    /// 关键字别名（小写）；`keyword` 包含任一别名即命中。
    pub aliases: &'static [&'static str],
    /// 打开方式。
    pub open: OpenMethod,
}

/// 支持的设计软件清单。
///
/// 匹配按数组顺序进行，命中即返回；未命中的关键字仍按
/// [`OpenMethod::Installed`] 处理，以兼容任意已安装软件。
pub const APPS: &[AppEntry] = &[
    AppEntry {
        name: "CorelDRAW",
        aliases: &["coreldraw", "corel draw"],
        open: OpenMethod::Com(ComApp::CorelDraw),
    },
    AppEntry {
        name: "CadCraft",
        aliases: &["cadcraft"],
        open: OpenMethod::Package("cadcraft"),
    },
    AppEntry {
        name: "DesignCraft",
        aliases: &["designcraft"],
        open: OpenMethod::Package("designcraft"),
    },
    AppEntry {
        name: "EffectCraft",
        aliases: &["effectcraft"],
        open: OpenMethod::Package("effectcraft"),
    },
    AppEntry {
        name: "FilmCraft",
        aliases: &["filmcraft"],
        open: OpenMethod::Package("filmcraft"),
    },
    AppEntry {
        name: "LightCraft",
        aliases: &["lightcraft"],
        open: OpenMethod::Package("lightcraft"),
    },
    AppEntry {
        name: "PDFCraft",
        aliases: &["pdfcraft"],
        open: OpenMethod::Package("pdfcraft"),
    },
    AppEntry {
        name: "VectorCraft",
        aliases: &["vectorcraft"],
        open: OpenMethod::Package("vectorcraft"),
    },
    AppEntry {
        name: "Adobe Illustrator",
        aliases: &["illustrator"],
        open: OpenMethod::Installed,
    },
    AppEntry {
        name: "Adobe Photoshop",
        aliases: &["photoshop"],
        open: OpenMethod::Installed,
    },
    AppEntry {
        name: "Adobe InDesign",
        aliases: &["indesign"],
        open: OpenMethod::Installed,
    },
    AppEntry {
        name: "AutoCAD",
        aliases: &["autocad"],
        open: OpenMethod::Installed,
    },
    AppEntry {
        name: "3ds Max",
        aliases: &["3ds max", "3dsmax"],
        open: OpenMethod::Installed,
    },
    AppEntry {
        name: "Blender",
        aliases: &["blender"],
        open: OpenMethod::Installed,
    },
    AppEntry {
        name: "SketchUp",
        aliases: &["sketchup"],
        open: OpenMethod::Installed,
    },
    AppEntry {
        name: "Affinity Designer",
        aliases: &["affinity designer"],
        open: OpenMethod::Installed,
    },
    AppEntry {
        name: "Affinity Photo",
        aliases: &["affinity photo"],
        open: OpenMethod::Installed,
    },
    AppEntry {
        name: "Affinity Publisher",
        aliases: &["affinity publisher"],
        open: OpenMethod::Installed,
    },
    AppEntry {
        name: "Cinema 4D",
        aliases: &["cinema 4d", "cinema4d"],
        open: OpenMethod::Installed,
    },
    AppEntry {
        name: "SolidWorks",
        aliases: &["solidworks"],
        open: OpenMethod::Installed,
    },
];

/// 按关键字查找路由；未命中返回 `None`。
///
/// 匹配规则为不区分大小写的包含匹配，按 [`APPS`] 顺序取第一个命中项。
pub fn resolve(keyword: &str) -> Option<&'static AppEntry> {
    let keyword = keyword.trim().to_lowercase();
    if keyword.is_empty() {
        return None;
    }

    APPS.iter()
        .find(|app| app.aliases.iter().any(|alias| keyword.contains(alias)))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn open_of(keyword: &str) -> Option<OpenMethod> {
        resolve(keyword).map(|entry| entry.open)
    }

    #[test]
    fn coreldraw_uses_com() {
        assert_eq!(
            open_of("CorelDRAW"),
            Some(OpenMethod::Com(ComApp::CorelDraw))
        );
        assert_eq!(
            open_of("corel draw 2024"),
            Some(OpenMethod::Com(ComApp::CorelDraw))
        );
    }

    #[test]
    fn vectorcraft_uses_package() {
        assert_eq!(
            open_of("VectorCraft"),
            Some(OpenMethod::Package("vectorcraft"))
        );
        assert_eq!(
            open_of("vectorcraft-cli"),
            Some(OpenMethod::Package("vectorcraft"))
        );
    }

    #[test]
    fn craft_suite_uses_packages() {
        for name in [
            "cadcraft",
            "designcraft",
            "effectcraft",
            "filmcraft",
            "lightcraft",
            "pdfcraft",
        ] {
            assert_eq!(
                open_of(name),
                Some(OpenMethod::Package(match name {
                    "cadcraft" => "cadcraft",
                    "designcraft" => "designcraft",
                    "effectcraft" => "effectcraft",
                    "filmcraft" => "filmcraft",
                    "lightcraft" => "lightcraft",
                    "pdfcraft" => "pdfcraft",
                    _ => unreachable!(),
                })),
                "{name} 应路由到线上包"
            );
        }
    }

    #[test]
    fn installed_software_uses_local_exe() {
        assert_eq!(
            open_of("Adobe Illustrator 2025"),
            Some(OpenMethod::Installed)
        );
        assert_eq!(open_of("autocad"), Some(OpenMethod::Installed));
    }

    #[test]
    fn unknown_keyword_has_no_route() {
        assert!(resolve("this-software-does-not-exist").is_none());
        assert!(resolve("   ").is_none());
    }

    #[test]
    fn aliases_are_lowercase() {
        for app in APPS {
            for alias in app.aliases {
                assert_eq!(*alias, alias.to_lowercase(), "别名必须小写: {alias}");
            }
        }
    }
}
