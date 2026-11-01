//! 启动装配：读配置 → 建状态 → 装钩子/托盘/快捷键 → 显示窗口。
//!
//! 从 `lib.rs` 抽出来的原因：`lib.rs` 应该只剩"注册什么"，不该有
//! "启动时按什么顺序做什么"。启动顺序是会有 bug 的地方（比如必须在
//! 日志就位之后再干别的），单独成文件才看得清。

use crate::autostart;
use crate::clipboard;
use crate::config::{self, AppSettings};
use crate::gesture;
use crate::logger;
use crate::panel;
use crate::paths::Paths;
use crate::plugins;
use crate::state::AppState;
use crate::update;
use crate::uploader;
use crate::win32;
use std::sync::Arc;
use tauri::Manager;

/// 历代内置手势表。用户的 bindings 如果跟其中任意一版**完全一致**，
/// 说明从来没自己动过 —— 可以安全地换成当前默认。
///
/// 第二版是"DLD 当 S 形截图"那版：现在改成真正画一个 S（曲线），
/// 老用户没动过默认的话要一起升级，否则他会发现"截图手势没了"。
const LEGACY_GESTURE_SETS: [&[&str]; 4] = [
    &["L", "R", "U", "D", "DR", "DL", "DU", "UD", "LU", "RU", "LD", "RD", "UDR", "UR", "UL"],
    &[
        "L", "R", "U", "D", "DR", "DL", "DU", "UD", "LU", "RU", "LD", "RD", "UDR", "DLD", "DRUL",
        "RDLU",
    ],
    // 上一版：S 已经是曲线了，但还有一对"画出来一模一样"的方框（DRUL / RDLU）
    // 和一对镜像的竖划（UD / DU）
    &[
        "L", "R", "U", "D", "DR", "DL", "DU", "UD", "LU", "RU", "LD", "RD", "UDR", "S", "DRUL",
        "RDLU",
    ],
    // 再上一版：去掉同形手势后（取字改开口框 DRU），还没有 H = 编辑 hosts
    &[
        "L", "R", "U", "D", "DR", "DL", "UD", "LU", "RU", "LD", "RD", "UDR", "S", "DRUL", "DRU",
    ],
];

fn is_legacy_default(bindings: &[crate::config::GestureBinding]) -> bool {
    LEGACY_GESTURE_SETS.iter().any(|set| {
        bindings.len() == set.len()
            && set
                .iter()
                .all(|p| bindings.iter().any(|b| b.pattern == *p))
    })
}

fn load_settings(paths: &Paths) -> AppSettings {
    let file = paths.settings_file();
    if !file.exists() {
        let fresh = AppSettings::default();
        if let Ok(text) = serde_json::to_string_pretty(&fresh) {
            let _ = std::fs::write(&file, text);
        }
        return fresh;
    }
    match std::fs::read_to_string(&file) {
        Ok(text) => match serde_json::from_str::<AppSettings>(&text) {
            Ok(cfg) => {
                // 老配置文件里没有新加的字段（比如 gestures）。内存里有默认值、
                // 功能能用，但用户打开 json 会以为"这功能不存在"，
                // 也没法手改。所以补写一次完整配置。
                let mut cfg = cfg;
                let mut dirty = false;
                if !text.contains("\"gestures\"") {
                    dirty = true;
                    log::info!("配置补写：新增 gestures 段");
                }
                // 新增的 profiles（程序专用集）也要落一次盘：只在内存里默认出来，
                // 用户打开 json 会以为"这功能不存在"，也没法手改。
                if !text.contains("\"profiles\"") {
                    dirty = true;
                    log::info!("配置补写：手势新增 profiles（程序专用集）");
                }
                // 手势的默认值变过（多了 S 形截图 / 框选贴图 / 框选取字）。
                // 只在"用户完全没动过默认"时才替换，改过的绝不去碰。
                if is_legacy_default(&cfg.gestures.bindings) {
                    cfg.gestures.bindings = crate::config::GestureConfig::default().bindings;
                    dirty = true;
                    log::info!("手势默认绑定已升级（画 S 截图 / 框选贴图 / 框选取字）");
                }
                // 用户自己改过手势表、但里面还留着"DLD 当 S 形截图"那一条：
                // 单条也要升级，否则他会发现截图手势彻底没了（S 是曲线，DLD 划不出来）。
                for b in &mut cfg.gestures.bindings {
                    if b.pattern.eq_ignore_ascii_case("DLD")
                        && matches!(&b.action, crate::config::GestureAction::Snip { id } if id == "region")
                    {
                        b.pattern = "S".into();
                        dirty = true;
                        log::info!("手势升级：DLD（折线）→ S（曲线）截图");
                    }
                }
                if dirty {
                    if let Ok(full) = serde_json::to_string_pretty(&cfg) {
                        let _ = std::fs::write(&file, full);
                    }
                }
                cfg
            }
            Err(e) => {
                log::error!("配置解析失败，回退默认值: {e}");
                let backup = file.with_extension(format!(
                    "json.corrupt.{}",
                    chrono::Utc::now().timestamp()
                ));
                let _ = std::fs::copy(&file, backup);
                // 备份完必须把默认值写回去。以前只备份不重写，于是：
                // 每次启动都解析失败、每次都多一个 .corrupt 文件，
                // 配置永远停在坏状态，用户只能自己去删。
                let fresh = AppSettings::default();
                if let Ok(text) = serde_json::to_string_pretty(&fresh) {
                    if let Err(e) = std::fs::write(&file, text) {
                        log::error!("重写默认配置失败: {e}");
                    }
                }
                fresh
            }
        },
        Err(e) => {
            log::error!("配置读取失败: {e}");
            AppSettings::default()
        }
    }
}


/// `tauri::Builder::setup` 的实现。
pub fn setup(app: &mut tauri::App) -> Result<(), Box<dyn std::error::Error>> {

    let handle = app.handle().clone();

    let data_dir = handle
        .path()
        .app_data_dir()
        .unwrap_or_else(|_| std::path::PathBuf::from("."));
    let resource_dir = handle.path().resource_dir().ok();
    let pictures = handle
        .path()
        .picture_dir()
        .unwrap_or_else(|_| data_dir.clone())
        .join("SnipForge");

    let paths = Paths::new(data_dir, resource_dir, pictures);
    if let Err(e) = paths.ensure() {
        log::error!("目录初始化失败: {e}");
    }
    // 日志必须最先就位，后面任何一步炸了都要有记录
    logger::init(&paths.logs);
    log::info!("数据目录: {}", paths.data.display());

    let mut settings = load_settings(&paths);
    let needs_migrate = settings.version < 2;
    config::migrate(&mut settings);

    // 剪贴板历史载入
    let clipboard_service = Arc::new(clipboard::ClipboardService::new(&paths));
    clipboard_service.store.load(settings.clipboard.max_items);
    app.manage(clipboard_service);

    // 图床注册表
    app.manage(Arc::new(uploader::build_registry()));

    // 主状态
    let state = AppState::new(settings, paths);
    app.manage(state.clone());

    // 开机自启：配置说开着就重写一次 Run 项。
    // 换版本、改安装目录、把绿色版挪了位置之后，旧路径指向的文件已经没了，
    // 表现就是"明明开了却不自启" —— 每次启动校正一遍最省事。
    if state.settings.read().general.launch_at_login {
        if let Err(e) = autostart::apply(true) {
            log::warn!("校正开机自启失败: {e}");
        }
    }

    // 迁移完必须落盘，否则下次启动又按老配置走一遍
    if needs_migrate {
        if let Err(e) = state.persist_settings() {
            log::error!("配置迁移写盘失败: {e}");
        }
    }

    win32::window::set_dpi_aware();

    if let Err(e) = plugins::init_runtime(&handle, &state) {
        log::error!("运行时初始化失败: {e}");
    }

    // Win+V 只能靠键盘钩子拿，注册表那条路走不通（实测）
    win32::keyhook::install(app.handle().clone());
    win32::keyhook::set_active(state.settings.read().clipboard.takeover_win_v);

    // 鼠标手势：钩子常驻，配置关掉时只放行不做事
    gesture::install(
        app.handle(),
        state.settings.read().gestures.clone(),
    );

    // 预建剪贴板面板（隐藏）：否则第一次按 Win+V 要等 webview 起来，
    // 那几百毫秒里面板是空白的，用户敲回车毫无反应。
    //
    // 必须放到后台线程：建窗口内部要把请求投递给主线程再等结果，
    // 在 setup 里（就在主线程上）直接调会自锁。延迟 400ms 是让启动
    // 优先把主窗口画出来，用户感知不到。
    {
        let h = app.handle().clone();
        std::thread::spawn(move || {
            std::thread::sleep(std::time::Duration::from_millis(400));
            panel::prewarm(&h);
        });
    }

    // 检查更新：延后 6 秒再联网（启动瞬间抢带宽会让首帧更慢），
    // 失败只写 debug 日志 —— 公司网络出不去是常态，不该弹窗。
    update::spawn_startup_check(app.handle().clone(), state.clone());

    // 首次启动展示主窗口，之后只在托盘里待命
    if let Some(win) = app.get_webview_window("main") {
        let _ = win.show();
        let _ = win.set_focus();
    }

    Ok(())
}

/// 主窗口关闭 = 收到托盘，不是退出。
pub fn on_window_event(window: &tauri::Window, event: &tauri::WindowEvent) {
    if let tauri::WindowEvent::CloseRequested { api, .. } = event {
        if window.label() == "main" {
            let app = window.app_handle();
            let close_to_tray = app
                .try_state::<Arc<AppState>>()
                .map(|s| s.settings.read().general.close_to_tray)
                .unwrap_or(true);
            if close_to_tray {
                api.prevent_close();
                let _ = window.hide();
            }
        }
    }
}
