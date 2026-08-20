use crate::utils::cjson::{write_error, write_success};
use crate::utils::soft::find_software_all;
use serde::Deserialize;
use serde_json::json;
use std::ffi::{c_char, c_int, CStr};

#[unsafe(no_mangle)]
pub extern "C" fn post_software_info(json_in: *const c_char, json_out: *mut c_char) -> c_int {
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
        .map(|(name, version)| json!({ "name": name, "version": version }))
        .collect();

    if list.is_empty() {
        write_success(json_out, "not_found", json!({ "list": [] }))
    } else {
        write_success(json_out, "success", json!({ "list": list }))
    }
}