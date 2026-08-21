use crate::utils::cjson::{write_error, write_success};
use crate::utils::soft::{find_software_all, get_install_path_by_name};  // 新增导入
use serde::Deserialize;
use serde_json::json;
use std::ffi::{c_char, c_int, CStr};

/// 拆分版本号：主版本号（第一个点之前）和次版本号（剩余部分）
fn split_version(version: &str) -> (String, String) {
    let parts: Vec<&str> = version.split('.').collect();
    if parts.is_empty() {
        return (version.to_string(), "".to_string());
    }
    let major = parts[0].to_string();
    let minor = if parts.len() > 1 {
        parts[1..].join(".")
    } else {
        "".to_string()
    };
    (major, minor)
}

#[unsafe(no_mangle)]
pub extern "C" fn post_info(json_in: *const c_char, json_out: *mut c_char) -> c_int {
    if json_in.is_null() {
        return write_error(json_out, 400100, "json_in is null");
    }

    let body = unsafe {
        match CStr::from_ptr(json_in).to_str() {
            Ok(s) => s,
            Err(_) => return write_error(json_out, 400101, "invalid utf-8"),
        }
    };

    #[derive(Deserialize)]
    struct InputData {
        keyword: String,
    }

    let req: InputData = match serde_json::from_str(body) {
        Ok(v) => v,
        Err(e) => return write_error(json_out, 400102, &format!("invalid json: {}", e)),
    };

    if req.keyword.is_empty() {
        return write_error(json_out, 400103, "keyword is empty");
    }

    let results = find_software_all(&req.keyword);
    let list: Vec<_> = results
        .into_iter()
        .map(|(name, version)| {
            let install_path = get_install_path_by_name(&name).unwrap_or_default();
            let (major_version, minor_version) = split_version(&version);
            json!({
                "name": name,
                "version": version,
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