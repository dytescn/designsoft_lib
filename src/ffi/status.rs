//! `post_status`：查询指定软件的运行状态。

use crate::ffi::codes;
use crate::ffi::common::{ensure_keyword, parse_request};
use crate::utils::apps::{self, OpenMethod};
use crate::utils::cjson::{write_error, write_success};
use crate::utils::pkg;
use crate::utils::status::software_status_with_version;
use serde::Deserialize;
use serde_json::json;
use std::ffi::{c_char, c_int};

/// 请求体：`{ "keyword": "illustrator", "version": "2025" }`，`version` 可省略。
#[derive(Deserialize)]
struct StatusRequest {
    keyword: String,
    version: Option<String>,
}

/// 查询软件状态，`status` 取值为 `"已启动"` 或 `"已安装但未启动"`。
///
/// 线上包（`OpenMethod::Package`）检查本地下载记录与进程，并在 `data.entries`
/// 中返回 GUI / CLI 两个入口的明细；其余软件按注册表安装信息检测。
#[unsafe(no_mangle)]
pub extern "C" fn post_status(json_in: *const c_char, json_out: *mut c_char) -> c_int {
    let req: StatusRequest = match parse_request(json_in, json_out) {
        Ok(req) => req,
        Err(ret) => return ret,
    };

    if let Err(ret) = ensure_keyword(json_out, &req.keyword) {
        return ret;
    }

    let method = apps::resolve(&req.keyword)
        .map(|entry| entry.open)
        .unwrap_or(OpenMethod::Installed);

    let result: Result<(String, Option<Vec<serde_json::Value>>), String> = match method {
        OpenMethod::Package(name) => pkg::status(name).map(|status| {
            let status_text = if status.running {
                "已启动".to_string()
            } else {
                "已安装但未启动".to_string()
            };
            let entries = status
                .entries
                .iter()
                .map(|entry| {
                    json!({
                        "entry": entry.entry.as_str(),
                        "exe": entry.exe,
                        "running": entry.running,
                    })
                })
                .collect();
            (status_text, Some(entries))
        }),
        _ => software_status_with_version(&req.keyword, req.version.as_deref())
            .map(|status| (status, None)),
    };

    match result {
        Ok((status, entries)) => {
            let mut data = json!({ "status": status });
            if let Some(entries) = entries {
                data["entries"] = json!(entries);
            }
            write_success(json_out, "success", data)
        }
        Err(e) => write_error(json_out, codes::OPERATION_FAILED, &e),
    }
}
