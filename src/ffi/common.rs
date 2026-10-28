//! FFI 入口共用的入参解析与校验。

use crate::ffi::codes;
use crate::utils::cjson::write_error;
use serde::de::DeserializeOwned;
use std::ffi::{c_char, c_int, CStr};

/// 解析 C 字符串入参。
///
/// 成功时返回反序列化后的请求体；失败时已把错误 JSON 写入 `json_out`，
/// 并把 `write_json_out` 的返回值作为 `Err`，调用方可直接 `return`。
pub(crate) fn parse_request<T: DeserializeOwned>(
    json_in: *const c_char,
    json_out: *mut c_char,
) -> Result<T, c_int> {
    if json_in.is_null() {
        return Err(write_error(
            json_out,
            codes::INVALID_INPUT_NULL,
            "json_in is null",
        ));
    }

    // SAFETY: json_in 已确认非空，按 FFI 契约指向以 NUL 结尾的字符串。
    let body = match unsafe { CStr::from_ptr(json_in) }.to_str() {
        Ok(body) => body,
        Err(_) => {
            return Err(write_error(
                json_out,
                codes::INVALID_INPUT_UTF8,
                "invalid utf-8",
            ))
        }
    };

    serde_json::from_str(body).map_err(|e| {
        write_error(
            json_out,
            codes::INVALID_INPUT_JSON,
            &format!("invalid json: {e}"),
        )
    })
}

/// 校验 `keyword` 非空；为空时写入错误 JSON 并返回其写入结果。
pub(crate) fn ensure_keyword(json_out: *mut c_char, keyword: &str) -> Result<(), c_int> {
    if keyword.is_empty() {
        Err(write_error(
            json_out,
            codes::EMPTY_KEYWORD,
            "keyword is empty",
        ))
    } else {
        Ok(())
    }
}
