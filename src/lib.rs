//! `designsoft`：查询、检测并启动本机安装的设计软件。
//!
//! 数据来源为 Windows 注册表的 Uninstall 键，进程检测与启动依赖 Win32 API，
//! 因此整个 crate 仅在 Windows 上编译。

#![cfg(windows)]

pub mod ffi;
pub mod utils;

pub use crate::ffi::info::post_info;
pub use crate::ffi::start::post_start;
pub use crate::ffi::status::post_status;
