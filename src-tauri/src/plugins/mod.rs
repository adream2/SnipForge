//! 插件运行时：声明式图床加载 + 资源目录扫描。
//!
//! 第三方扩展的两条路：
//!   A. 声明式（推荐）：`plugins/xxx.uploader.json` —— 零代码、零信任风险
//!   B. 原生插件：需要重新编译，适合深度定制
//!
//! 这里只负责 A。选 A 作为主推方案不是偷懒 —— 一个能配 JSON 就接上的图床，
//! 比一个要装 SDK、要签名、能读你全盘文件的 DLL 安全一个量级。

pub mod declarative;

use crate::error::AppResult;
use crate::state::AppState;
use crate::types::{PluginInfo, PluginState};
use std::sync::Arc;
use tauri::{AppHandle, Manager};

/// 启动时装配：声明式图床 + 全局快捷键 + 托盘菜单 + 剪贴板监听
pub fn init_runtime(app: &AppHandle, state: &Arc<AppState>) -> AppResult<()> {
    // 1. 声明式图床（"启用插件"关掉时整体跳过，这是那个开关唯一该有的含义）
    if state.settings.read().general.enable_plugins {
        let loaded = declarative::load_all(app, state);
        log::info!("加载声明式图床 {loaded} 个");
    } else {
        log::info!("插件总开关已关闭，跳过声明式图床");
    }

    // 2. 快捷键
    super::shortcuts::register_all(app, state)?;

    // 3. 托盘菜单
    super::tray::setup(app, state)?;

    // 4. 剪贴板监听。
    // 线程常驻、每轮现读配置：开关/间隔/是否存图都在设置页改完立即生效。
    super::clipboard::start_monitor(app.clone(), state.clone());

    Ok(())
}

/// 扫描插件目录，汇总成前端要的状态
pub fn state_of(state: &Arc<AppState>) -> PluginState {
    let mut plugins = Vec::new();

    let dirs = [
        (state.paths.builtin_plugins.clone(), "builtin"),
        (state.paths.plugins.clone(), "user"),
    ];

    for (dir, origin) in dirs {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if !path.is_file() {
                continue;
            }
            let name = path.file_name().and_then(|n| n.to_str()).unwrap_or_default();
            if !name.ends_with(".uploader.json") && !name.ends_with(".plugin.json") {
                continue;
            }

            match std::fs::read_to_string(&path)
                .ok()
                .and_then(|txt| serde_json::from_str::<serde_json::Value>(&txt).ok())
            {
                Some(spec) => {
                    let id = spec["id"].as_str().unwrap_or(name).to_string();
                    let enabled = state
                        .settings
                        .read()
                        .plugin_enabled
                        .get(&id)
                        .copied()
                        .unwrap_or(true);
                    let provider_ids: Vec<String> = spec["provider"]
                        .as_object()
                        .map(|_| vec![spec["provider"]["id"].as_str().unwrap_or(&id).to_string()])
                        .unwrap_or_default();
                    let display_id = id.clone();
                    plugins.push(PluginInfo {
                        id: display_id,
                        name: spec["name"].as_str().unwrap_or(name).to_string(),
                        version: spec["version"].as_str().unwrap_or("1.0.0").to_string(),
                        description: spec["description"].as_str().map(|s| s.to_string()),
                        author: spec["author"].as_str().map(|s| s.to_string()),
                        origin: origin.to_string(),
                        enabled,
                        loaded: enabled,
                        error: None,
                        hooks: spec["hooks"]
                            .as_array()
                            .map(|a| {
                                a.iter()
                                    .filter_map(|v| v.as_str().map(|s| s.to_string()))
                                    .collect()
                            })
                            .unwrap_or_default(),
                        providers: provider_ids,
                        dir: path.to_string_lossy().to_string(),
                    });
                }
                None => plugins.push(PluginInfo {
                    id: name.to_string(),
                    name: name.to_string(),
                    version: "-".into(),
                    description: None,
                    author: None,
                    origin: origin.to_string(),
                    enabled: false,
                    loaded: false,
                    error: Some("JSON 解析失败".into()),
                    hooks: Vec::new(),
                    providers: Vec::new(),
                    dir: path.to_string_lossy().to_string(),
                }),
            }
        }
    }

    plugins.sort_by(|a, b| {
        a.origin
            .cmp(&b.origin)
            .then_with(|| a.name.cmp(&b.name))
    });

    PluginState {
        plugins,
        builtin_dir: state.paths.builtin_plugins.to_string_lossy().to_string(),
        user_dir: state.paths.plugins.to_string_lossy().to_string(),
    }
}

pub fn reload(app: &AppHandle, state: &Arc<AppState>) -> PluginState {
    if let Some(registry) = app.try_state::<Arc<crate::uploader::UploaderRegistry>>() {
        declarative::unload_all(&registry, state);
    }
    if state.settings.read().general.enable_plugins {
        declarative::load_all(app, state);
    }
    state_of(state)
}
