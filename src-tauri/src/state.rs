use parking_lot::{Mutex, RwLock};
use std::sync::Arc;

use crate::capture::screen::CaptureFrame;
use crate::config::AppSettings;
use crate::hooks::HookRegistry;
use crate::paths::Paths;
use crate::types::HotkeyResult;

/// 一次截图会话的临时状态。用完即弃，不落盘。
pub struct CaptureSession {
    pub frame: CaptureFrame,
    /// region | window | fullscreen | active-window | scroll
    pub mode: String,
    /// 长截图模式下用户框选的物理矩形
    pub scroll_rect: Option<crate::types::Rect>,
    /// 长截图是否已被用户中止
    pub aborted: bool,
}

impl std::fmt::Debug for CaptureSession {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CaptureSession")
            .field("mode", &self.mode)
            .field("displays", &self.frame.displays.len())
            .finish()
    }
}

pub struct AppState {
    pub settings: RwLock<AppSettings>,
    pub paths: Paths,
    pub hotkey_results: RwLock<Vec<HotkeyResult>>,
    /// 钩子注册表：插件通过它介入主流程
    pub hooks: Arc<HookRegistry>,
    /// 当前截图会话（含原始像素，占内存，用完必须 clear）
    pub capture: Mutex<Option<CaptureSession>>,
    /// 已经裁剪好、等待编辑器取走的截图
    pub pending: Mutex<Option<crate::types::CaptureResult>>,
    /// 上传历史（落盘，重启后还在）
    pub uploads: Mutex<crate::uploader::history::UploadHistory>,
}

impl AppState {
    pub fn new(settings: AppSettings, paths: Paths) -> Arc<Self> {
        let uploads =
            crate::uploader::history::UploadHistory::load(paths.uploads_file());
        Arc::new(Self {
            settings: RwLock::new(settings),
            paths,
            hotkey_results: RwLock::new(Vec::new()),
            hooks: Arc::new(HookRegistry::new()),
            capture: Mutex::new(None),
            pending: Mutex::new(None),
            uploads: Mutex::new(uploads),
        })
    }

    pub fn settings_snapshot(&self) -> AppSettings {
        self.settings.read().clone()
    }

    /// 原子写：先写 tmp 再 rename，断电也不会留下半截 JSON
    pub fn persist_settings(&self) -> crate::error::AppResult<()> {
        let snapshot = self.settings.read().clone();
        let file = self.paths.settings_file();
        let tmp = file.with_extension("json.tmp");
        let text = serde_json::to_string_pretty(&snapshot)?;
        std::fs::write(&tmp, text)?;
        std::fs::rename(&tmp, &file)?;
        Ok(())
    }

    /// 释放截图占用的原始位图（几块 4K 屏加起来上百 MB，必须及时放）
    pub fn release_capture(&self) {
        *self.capture.lock() = None;
    }
}
