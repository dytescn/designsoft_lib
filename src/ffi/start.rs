//! `post_start`：启动指定软件（可指定版本）。

use crate::ffi::codes;
use crate::ffi::common::{ensure_keyword, parse_request};
use crate::utils::apps::{self, ComApp, OpenMethod};
use crate::utils::cdr;
use crate::utils::cjson::{write_error, write_success};
use crate::utils::pkg;
use crate::utils::status::launch_software_with_version;
use serde::Deserialize;
use serde_json::json;
use std::ffi::{c_char, c_int};

/// 请求体：`{ "keyword": "illustrator", "version": "2025", "entry": "gui" }`，
/// `version` 与 `entry` 可省略。
#[derive(Deserialize)]
struct StartRequest {
    keyword: String,
    version: Option<String>,
    /// 线上包的启动入口：`gui`（默认）或 `cli`。
    entry: Option<String>,
}

/// 启动匹配的软件；已在运行时直接返回成功。
///
/// 打开方式由 [`crate::utils::apps::APPS`] 路由表决定：COM 接口（CorelDRAW）、
/// 线上包下载（VectorCraft）或本地注册表 + exe；未命中路由表的关键字按本地
/// 安装软件处理。
#[unsafe(no_mangle)]
pub extern "C" fn post_start(json_in: *const c_char, json_out: *mut c_char) -> c_int {
    let req: StartRequest = match parse_request(json_in, json_out) {
        Ok(req) => req,
        Err(ret) => return ret,
    };

    if let Err(ret) = ensure_keyword(json_out, &req.keyword) {
        return ret;
    }

    let version = req.version.as_deref();
    let entry = match req.entry.as_deref() {
        None => None,
        Some(value) => match pkg::Entry::parse(value) {
            Some(entry) => Some(entry),
            None => {
                return write_error(
                    json_out,
                    codes::INVALID_INPUT_JSON,
                    "invalid entry: expected gui or cli",
                )
            }
        },
    };

    let method = apps::resolve(&req.keyword)
        .map(|entry| entry.open)
        .unwrap_or(OpenMethod::Installed);

    let result: Result<Option<&'static str>, String> = match method {
        OpenMethod::Com(ComApp::CorelDraw) => {
            // CorelDRAW 通过 cdrsdk 的 COM 接口打开，未运行时由 COM 启动。
            cdr::open_software(version).map(|_| None)
        }
        OpenMethod::Package(name) => {
            // 便携包：线上存在时下载到本地目录再启动；版本由线上索引决定。
            let opened = match entry {
                Some(entry) => pkg::open_with(name, entry),
                None => pkg::open(name),
            };
            opened.map(|opened| Some(opened.entry.as_str()))
        }
        OpenMethod::Installed => {
            launch_software_with_version(&req.keyword, version).map(|_| None)
        }
    };

    match result {
        Ok(entry) => {
            let mut data = json!({ "message": "软件已启动或已在运行" });
            if let Some(entry) = entry {
                data["entry"] = json!(entry);
            }
            write_success(json_out, "started", data)
        }
        Err(e) => write_error(json_out, codes::OPERATION_FAILED, &e),
    }
}
