//! 功能注册表与手势出厂默认值的 IPC 入口。
//!
//! 前端要的只是"有哪些功能、叫什么"，但这份名单必须在后端定义 ——
//! 前端再抄一份，改一处漏一处就是老问题（下拉里有、真划没反应）。

use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FeatureDto {
    pub id: String,
    pub label: String,
    /// 会抓屏幕：前端用来说明"这个动作会先收起轨迹"，不需要的话忽略即可
    pub grabs_screen: bool,
}

/// 全部可被手势/托盘触发的功能，顺序即界面顺序
#[tauri::command]
pub fn features_list() -> Vec<FeatureDto> {
    crate::features::FEATURES
        .iter()
        .map(|f| FeatureDto {
            id: f.id.to_string(),
            label: f.label.to_string(),
            grabs_screen: f.grabs_screen,
        })
        .collect()
}

/// 手势出厂默认（通用绑定 + 程序专用集）。
///
/// "恢复默认"必须和后端 `GestureConfig::default()` 完全一致，所以直接问后端，
/// 前端不留第二份表。
#[tauri::command]
pub fn gesture_defaults() -> crate::config::GestureDefaults {
    crate::config::GestureDefaults::load()
}
