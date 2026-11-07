//! SnipForge 的纯逻辑层。
//!
//! 这里只放**不碰 GUI / 平台 API** 的东西：配置结构、路径消毒、模板渲染。
//! 目的是让 `cargo test` 真能跑 —— 主 crate 链接了完整的 Tauri/WinRT，
//! 它的 test harness 在部分机器上加载不起来（0xc0000139），
//! 那些 `#[cfg(test)]` 断言等于没有。

pub mod atom;
pub mod config;
pub mod pattern;
pub mod safe;
pub mod shape;
pub mod version;
