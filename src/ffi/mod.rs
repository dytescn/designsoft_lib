pub mod common;
pub mod info;
pub mod start;
pub mod status;

/// 写入响应 JSON `code` 字段的业务状态码。
pub mod codes {
    /// 成功。
    pub const OK: i32 = 200;

    /// `json_in` 为空指针。
    pub const INVALID_INPUT_NULL: i32 = 400100;
    /// 入参不是合法的 UTF-8 字符串。
    pub const INVALID_INPUT_UTF8: i32 = 400101;
    /// 入参不是合法的 JSON 或字段不匹配。
    pub const INVALID_INPUT_JSON: i32 = 400102;
    /// `keyword` 为空。
    pub const EMPTY_KEYWORD: i32 = 400103;
    /// 查询或启动软件失败。
    pub const OPERATION_FAILED: i32 = 500200;
}
