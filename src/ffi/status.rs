use crate::utils::cjson::{write_error, write_success};
use crate::utils::status::software_status_with_version;
use serde::Deserialize;
use serde_json::json;
use std::ffi::{c_char, c_int, CStr};

#[unsafe(no_mangle)]
pub extern "C" fn post_status(json_in: *const c_char, json_out: *mut c_char) -> c_int {
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
        version: Option<String>,  // 可选
    }

    let req: InputData = match serde_json::from_str(body) {
        Ok(v) => v,
        Err(e) => return write_error(json_out, 400102, &format!("invalid json: {}", e)),
    };

    if req.keyword.is_empty() {
        return write_error(json_out, 400103, "keyword is empty");
    }

    match software_status_with_version(&req.keyword, req.version.as_deref()) {
        Ok(status) => write_success(json_out, "success", json!({ "status": status })),
        Err(e) => write_error(json_out, 500200, &e),
    }
}