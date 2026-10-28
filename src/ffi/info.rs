//! `post_info`：按关键字列出已安装的设计软件。

use crate::ffi::common::{ensure_keyword, parse_request};
use crate::utils::cjson::write_success;
use crate::utils::soft::find_software_details;
use serde::Deserialize;
use serde_json::json;
use std::ffi::{c_char, c_int};

/// 请求体：`{ "keyword": "illustrator" }`。
#[derive(Deserialize)]
struct InfoRequest {
    keyword: String,
}

/// 拆分版本号为 `(主版本号, 其余部分)`。
fn split_version(version: &str) -> (String, String) {
    match version.split_once('.') {
        Some((major, minor)) => (major.to_string(), minor.to_string()),
        None => (version.to_string(), String::new()),
    }
}

/// 列出名称匹配 `keyword` 的软件及其版本、安装路径。
///
/// 成功响应：`{ "code": 200, "data": { "list": [...] }, "msg": "success" }`；
/// 无匹配时 `msg` 为 `"not_found"` 且 `list` 为空数组。
#[unsafe(no_mangle)]
pub extern "C" fn post_info(json_in: *const c_char, json_out: *mut c_char) -> c_int {
    let req: InfoRequest = match parse_request(json_in, json_out) {
        Ok(req) => req,
        Err(ret) => return ret,
    };

    if let Err(ret) = ensure_keyword(json_out, &req.keyword) {
        return ret;
    }

    let results = find_software_details(&req.keyword);
    let list: Vec<_> = results
        .into_iter()
        .map(|software| {
            let install_path = software.install_path().unwrap_or_default();
            let (major_version, minor_version) = split_version(&software.version);
            json!({
                "name": software.display_name,
                "version": software.version,
                "major_version": major_version,
                "minor_version": minor_version,
                "install_path": install_path,
            })
        })
        .collect();

    if list.is_empty() {
        write_success(json_out, "not_found", json!({ "list": [] }))
    } else {
        write_success(json_out, "success", json!({ "list": list }))
    }
}

#[cfg(test)]
mod tests {
    use super::split_version;

    #[test]
    fn splits_full_version() {
        assert_eq!(
            split_version("26.1.0.143"),
            ("26".to_string(), "1.0.143".to_string())
        );
    }

    #[test]
    fn handles_version_without_dot() {
        assert_eq!(split_version("2025"), ("2025".to_string(), String::new()));
    }

    #[test]
    fn handles_empty_version() {
        assert_eq!(split_version(""), (String::new(), String::new()));
    }
}
