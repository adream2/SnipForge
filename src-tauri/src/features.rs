//! 功能注册表：全软件"能被触发的东西"只在这里定义一次。
//!
//! 之前同一批功能被抄了好几遍 —— 手势的 `describe` / `run_snip` /
//! `needs_clean_screen`、托盘菜单、前端手势动作下拉。加一个功能要改四处，
//! 漏掉一处就出现"下拉里有、真划没反应"这种只有用户才会发现的问题。
//!
//! 所以这里收成一张表，每条写明：
//!   - `id`：手势配置里存的那个字符串（也是前端下拉的 value），**不能改**
//!   - `label`：界面上怎么叫它
//!   - `grabs_screen`：会不会抓屏幕。会抓的在动手前必须先把手势轨迹收干净，
//!     否则用户刚画的那道 S 会被一起截进图里（这个 bug 真出现过）
//!   - `run`：真正干活的那一步
//!
//! 新增功能 = 在这里加一行 + 在 `commands::mod` 里挂命令，别的地方不用动。

use tauri::AppHandle;

pub struct Feature {
    pub id: &'static str,
    pub label: &'static str,
    pub grabs_screen: bool,
    pub run: fn(&AppHandle),
}

/// 顺序就是手势动作下拉里的顺序：先常做的事，再边角料。
pub const FEATURES: &[Feature] = &[
    Feature {
        id: "region",
        label: "区域截图",
        grabs_screen: true,
        run: |a| crate::commands::capture::trigger(a, "region"),
    },
    Feature {
        id: "fullscreen",
        label: "全屏截图",
        grabs_screen: true,
        run: |a| crate::commands::capture::trigger(a, "fullscreen"),
    },
    Feature {
        id: "scroll",
        label: "滚动长截图",
        grabs_screen: true,
        run: |a| crate::commands::capture::trigger(a, "scroll"),
    },
    Feature {
        id: "panel",
        label: "剪贴板面板",
        grabs_screen: false,
        run: |a| crate::panel::toggle(a),
    },
    Feature {
        id: "pin",
        label: "剪贴板贴图",
        grabs_screen: true,
        run: |a| {
            let h = a.clone();
            tauri::async_runtime::spawn(async move {
                if let Err(e) = crate::commands::clipboard::clipboard_pin_current(h).await {
                    log::warn!("贴剪贴板图片失败: {e}");
                }
            });
        },
    },
    Feature {
        id: "record",
        label: "录屏（视频）",
        grabs_screen: true,
        run: |a| crate::commands::record::toggle(a, crate::commands::record::Fmt::Mp4),
    },
    Feature {
        id: "record-gif",
        label: "录屏（GIF）",
        grabs_screen: true,
        run: |a| crate::commands::record::toggle(a, crate::commands::record::Fmt::Gif),
    },
    Feature {
        id: "pin-region",
        label: "框选贴图",
        grabs_screen: true,
        run: |a| crate::commands::capture::trigger(a, "pin-region"),
    },
    Feature {
        id: "ocr-region",
        label: "框选取字",
        grabs_screen: true,
        run: |a| crate::commands::capture::trigger(a, "ocr-region"),
    },
];

pub fn find(id: &str) -> Option<&'static Feature> {
    FEATURES.iter().find(|f| f.id == id)
}

/// 界面上叫它什么。不认识的 id 原样返回 —— 比显示"未知操作"好排查。
pub fn label(id: &str) -> &str {
    find(id).map(|f| f.label).unwrap_or(id)
}

/// 执行前要不要先收掉手势轨迹。
///
/// 不认识的 id 一律当成"会抓屏"：宁可多等几毫秒，也别把轨迹截进图里。
pub fn grabs_screen(id: &str) -> bool {
    find(id).map(|f| f.grabs_screen).unwrap_or(true)
}

/// 触发。返回 false 表示这个 id 没有登记（老配置里可能有已删掉的功能）。
pub fn run(app: &AppHandle, id: &str) -> bool {
    match find(id) {
        Some(f) => {
            (f.run)(app);
            true
        }
        None => {
            log::warn!("配置里引用了未知功能: {id}");
            false
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_are_unique() {
        let mut seen = std::collections::HashSet::new();
        for f in FEATURES {
            assert!(seen.insert(f.id), "功能 id 重复: {}", f.id);
            assert!(!f.label.is_empty(), "{} 没有名字", f.id);
        }
    }

    #[test]
    fn unknown_id_is_safe() {
        assert_eq!(label("nope"), "nope");
        // 未知 id 保守当成会抓屏，宁可多等一会儿
        assert!(grabs_screen("nope"));
        // 老配置里被删掉的 "window" 不会再让程序崩
        assert!(!grabs_screen("panel"));
    }
}
