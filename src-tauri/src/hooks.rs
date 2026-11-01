//! 钩子系统 —— 插件介入主流程的唯一入口。
//!
//! 设计取舍：
//!   Rust 没有 JS 那种随手挂回调的便利，所以这里用 `serde_json::Value` 做通用载荷，
//!   换来的是**任何插件（内置或第三方）都能用同一种方式挂钩子**，且不会因为
//!   载荷类型演进就要改钩子签名。
//!
//! 语义与文档严格一致：
//!   - 串行执行，前一个的改写成为后一个的输入
//!   - 任一返回 Cancel 即中断
//!   - 单个插件 panic 被捕获，不影响其它插件

use parking_lot::RwLock;
use serde_json::Value;
use std::collections::HashMap;
use std::sync::Arc;

/// 钩子返回的指令。
///
/// 这两个变体只由**插件处理器**构造（应用内部没有处理器），
/// 所以编译器在单 crate 视角下会认为"从未构造"。
#[allow(dead_code)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HookAction {
    /// 继续，把（可能被改写过的）载荷交给下一个
    Continue,
    /// 中断整个流程
    Cancel,
}

pub type HookHandler = Arc<dyn Fn(&mut Value) -> HookAction + Send + Sync>;

#[derive(Clone)]
struct Entry {
    plugin_id: String,
    handler: HookHandler,
}

/// 已知的钩子点。
///
/// **只允许出现真正被调用的点。** 以前这里有 8 个常量，其中 4 个（app:ready /
/// app:quit / editor:beforeExport 之类）从来没被调用过 —— 看着是扩展点，
/// 实际挂上去也不会触发，比没有更坑。加新点必须同时把它接到调用处。
pub mod names {
    pub const CAPTURE_BEFORE: &str = "capture:before";
    pub const CAPTURE_AFTER: &str = "capture:after";
    pub const CLIPBOARD_CAPTURED: &str = "clipboard:captured";
    pub const IMAGE_BEFORE_UPLOAD: &str = "image:beforeUpload";
    pub const IMAGE_AFTER_UPLOAD: &str = "image:afterUpload";

    pub const ALL: &[&str] = &[
        CAPTURE_BEFORE,
        CAPTURE_AFTER,
        CLIPBOARD_CAPTURED,
        IMAGE_BEFORE_UPLOAD,
        IMAGE_AFTER_UPLOAD,
    ];
}

/// 一个钩子点的对外描述（插件页展示"能挂哪些地方"用）
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HookPoint {
    pub name: &'static str,
    /// 当前挂了几处理器
    pub handlers: usize,
}

pub struct HookRegistry {
    handlers: RwLock<HashMap<String, Vec<Entry>>>,
}

impl HookRegistry {
    pub fn new() -> Self {
        Self {
            handlers: RwLock::new(HashMap::new()),
        }
    }

    /// 注册一个钩子处理器。
    ///
    /// 这是给**原生插件**（重新编译接入，或第三方 crate）准备的公开 API，
    /// 应用内部的调用点看不到使用者，所以允许 dead_code。
    /// 声明式插件（JSON）没有脚本能力，挂不了钩子 —— 那条路只做图床。
    #[allow(dead_code)]
    pub fn register(&self, plugin_id: &str, hook: &str, handler: HookHandler) {
        let mut guard = self.handlers.write();
        guard
            .entry(hook.to_string())
            .or_default()
            .push(Entry {
                plugin_id: plugin_id.to_string(),
                handler,
            });
        log::debug!("插件 {plugin_id} 挂载钩子 {hook}");
    }

    /// 卸载某个插件的全部钩子（插件被禁用/重新加载时用）
    #[allow(dead_code)]
    pub fn unregister_plugin(&self, plugin_id: &str) {
        let mut guard = self.handlers.write();
        for list in guard.values_mut() {
            list.retain(|e| e.plugin_id != plugin_id);
        }
    }

    pub fn count(&self, hook: &str) -> usize {
        self.handlers.read().get(hook).map(|v| v.len()).unwrap_or(0)
    }

    /// 全部钩子点及其挂载数量（插件页展示用）
    pub fn points(&self) -> Vec<HookPoint> {
        names::ALL
            .iter()
            .map(|&n| HookPoint {
                name: n,
                handlers: self.count(n),
            })
            .collect()
    }

    /// 执行钩子链。返回 (是否被取消, 最终载荷)
    pub fn run(&self, hook: &str, mut payload: Value) -> (bool, Value) {
        let list = match self.handlers.read().get(hook) {
            Some(l) if !l.is_empty() => l.clone(),
            _ => return (false, payload),
        };

        for entry in list {
            let handler = entry.handler.clone();
            // 插件代码不可信，panic 不能让宿主陪葬
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| handler(&mut payload)));
            match result {
                Ok(HookAction::Continue) => {}
                Ok(HookAction::Cancel) => {
                    log::info!("插件 {} 在 {} 处中断流程", entry.plugin_id, hook);
                    return (true, payload);
                }
                Err(_) => log::error!("插件 {} 的 {} 钩子 panic，已跳过", entry.plugin_id, hook),
            }
        }
        (false, payload)
    }
}

impl Default for HookRegistry {
    fn default() -> Self {
        Self::new()
    }
}
