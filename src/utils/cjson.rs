//! C ABI 输出缓冲区的 JSON 序列化。
//!
//! FFI 契约：调用方负责分配输出缓冲区，容量至少为 [`OUTPUT_BUFFER_CAPACITY`]
//! 字节（包含结尾的 NUL）。本模块不会写入超过该容量的数据，超长时返回
//! [`WRITE_ERR_BUFFER_TOO_SMALL`] 且不保证缓冲区内容。

use serde_json::{json, Value};
use std::ffi::{c_char, c_int};

/// 输出缓冲区的最小容量（字节，含结尾 NUL）。
pub const OUTPUT_BUFFER_CAPACITY: usize = 64 * 1024;

/// 写入成功。
pub const WRITE_OK: c_int = 0;
/// `json_out` 为空指针。
pub const WRITE_ERR_NULL_BUFFER: c_int = -1;
/// 序列化结果超出 [`OUTPUT_BUFFER_CAPACITY`]。
pub const WRITE_ERR_BUFFER_TOO_SMALL: c_int = -2;

/// 把 JSON 写入 C 缓冲区，并追加结尾 NUL。
#[allow(clippy::not_unsafe_ptr_arg_deref)] // FFI 边界：指针有效性由调用方按契约保证。
pub fn write_json_out(json_out: *mut c_char, value: Value) -> c_int {
    if json_out.is_null() {
        return WRITE_ERR_NULL_BUFFER;
    }

    let out_str = value.to_string();
    let bytes = out_str.as_bytes();

    // 需要为结尾 NUL 预留 1 字节。
    if bytes.len() >= OUTPUT_BUFFER_CAPACITY {
        return WRITE_ERR_BUFFER_TOO_SMALL;
    }

    // SAFETY: 调用方保证 json_out 至少可写 OUTPUT_BUFFER_CAPACITY 字节；
    // 上面已确认写入内容（含结尾 NUL）不超过该容量。
    unsafe {
        std::ptr::copy_nonoverlapping(bytes.as_ptr(), json_out.cast::<u8>(), bytes.len());
        *json_out.add(bytes.len()) = 0;
    }

    WRITE_OK
}

/// 写入统一格式的错误响应：`{ code, data: {}, msg }`。
pub fn write_error(json_out: *mut c_char, code: i32, msg: &str) -> c_int {
    write_json_out(
        json_out,
        json!({
            "code": code,
            "data": {},
            "msg": msg
        }),
    )
}

/// 写入统一格式的成功响应：`{ code: 200, data, msg }`。
pub fn write_success(json_out: *mut c_char, msg: &str, data: Value) -> c_int {
    write_json_out(
        json_out,
        json!({
            "code": 200,
            "data": data,
            "msg": msg
        }),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn writes_json_with_nul_terminator() {
        let mut buffer = vec![0 as c_char; 128];

        let ret = write_json_out(buffer.as_mut_ptr(), json!({ "code": 200 }));

        assert_eq!(ret, WRITE_OK);
        // SAFETY: 缓冲区已由 write_json_out 写入 NUL。
        let written = unsafe { std::ffi::CStr::from_ptr(buffer.as_ptr()) };
        assert_eq!(written.to_str().unwrap(), r#"{"code":200}"#);
    }

    #[test]
    fn rejects_null_buffer() {
        assert_eq!(
            write_json_out(std::ptr::null_mut(), json!({})),
            WRITE_ERR_NULL_BUFFER
        );
    }

    #[test]
    fn rejects_payload_larger_than_contract() {
        let mut buffer = vec![0 as c_char; OUTPUT_BUFFER_CAPACITY];
        let value = Value::String("x".repeat(OUTPUT_BUFFER_CAPACITY));

        assert_eq!(
            write_json_out(buffer.as_mut_ptr(), value),
            WRITE_ERR_BUFFER_TOO_SMALL
        );
    }

    #[test]
    fn error_and_success_use_unified_shape() {
        let mut buffer = vec![0 as c_char; 256];

        assert_eq!(write_error(buffer.as_mut_ptr(), 400100, "boom"), WRITE_OK);
        let written = unsafe { std::ffi::CStr::from_ptr(buffer.as_ptr()) };
        assert_eq!(
            written.to_str().unwrap(),
            r#"{"code":400100,"data":{},"msg":"boom"}"#
        );
    }
}
