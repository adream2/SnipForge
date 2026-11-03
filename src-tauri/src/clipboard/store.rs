//! 剪贴板历史存储。
//!
//! 落盘策略：元数据进 history.json，图片 blob 按内容哈希独立存文件。
//! 不上 SQLite —— 省掉 native 依赖和迁移负担；单条删除也不用重写大文件。

use crate::error::AppResult;
use crate::paths::Paths;
use crate::types::{ClipboardItem, ClipboardQuery, ClipboardStats};
use parking_lot::Mutex;
use parking_lot::RwLock;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

/// 两次落盘的最小间隔。
///
/// 历史里有 2000 条长文本时，`save` 每次都要序列化整个索引（几 MB）。
/// 连着复制五段话就写五次，界面会明显一顿。真正的数据在内存里，
/// 索引只是重启后的恢复手段，晚 0.5 秒写不影响任何可见行为。
const SAVE_THROTTLE: Duration = Duration::from_millis(800);

#[derive(serde::Serialize, serde::Deserialize, Default)]
struct IndexFile {
    #[serde(default)]
    version: u32,
    #[serde(default)]
    items: Vec<ClipboardItem>,
}

pub struct ClipboardStore {
    /// **单一数据源**。以前还有一个 hash → id 的旁路索引，
    /// 两处状态一旦不同步（并发扫描、单条删除）就会丢映射、失去去重能力；
    /// 去重改成在 `add` 里同一把锁内线性查一遍（2000 条量级，成本可忽略）。
    items: RwLock<Vec<ClipboardItem>>,
    index_file: PathBuf,
    blobs_dir: PathBuf,
    pub max_items: RwLock<usize>,
    /// 上次落盘时刻（节流用）
    save_gate: Mutex<Option<Instant>>,
    /// 被节流跳过的写入：下一轮要补上，否则"最后复制的那条"会一直不落盘
    save_pending: AtomicBool,
}

/// `add` 的结果。
///
/// 常驻监控线程和面板的即时扫描（`sync_now`）会并发调到这里：
/// 以前两边都能先查后插、各插一条，同一份内容进两条。
pub enum AddResult {
    /// 真的新增了一条
    Added(ClipboardItem),
    /// 同 hash 已存在，只把时间和命中数往前挪
    Duplicate,
}

impl ClipboardStore {
    pub fn new(paths: &Paths) -> Self {
        Self {
            items: RwLock::new(Vec::new()),
            index_file: paths.clipboard_index.clone(),
            blobs_dir: paths.blobs.clone(),
            max_items: RwLock::new(2000),
            save_gate: Mutex::new(None),
            save_pending: AtomicBool::new(false),
        }
    }

    pub fn load(&self, max_items: usize) {
        *self.max_items.write() = max_items.max(50);
        if !self.index_file.exists() {
            return;
        }
        match std::fs::read_to_string(&self.index_file) {
            Ok(text) => match serde_json::from_str::<IndexFile>(&text) {
                Ok(idx) => {
                    let mut items = idx.items;
                    // 老数据补分类：以前只有 text / color 两种，链接全被记成 text。
                    // 这里按当前规则重判一次，"链接"分类一上线就有东西看，
                    // 不用等用户重新复制一遍才生效。
                    let mut retagged = 0;
                    for item in items.iter_mut() {
                        if item.kind == "text" {
                            let k = super::detect_kind(item.text.as_deref().unwrap_or(""));
                            if k != "text" {
                                item.kind = k.to_string();
                                retagged += 1;
                            }
                        }
                    }
                    if retagged > 0 {
                        log::info!("补全 {retagged} 条历史的分类（链接/颜色）");
                    }
                    items.sort_by(|a, b| b.last_used_at.cmp(&a.last_used_at));
                    log::info!("载入剪贴板历史 {} 条", items.len());
                    *self.items.write() = items;
                }
                Err(e) => {
                    log::error!("剪贴板索引损坏: {e}");
                    let backup = self.index_file.with_extension(format!("corrupt.{}", chrono::Utc::now().timestamp()));
                    let _ = std::fs::copy(&self.index_file, backup);
                }
            },
            Err(e) => log::error!("读取剪贴板索引失败: {e}"),
        }
    }

    /// 节流版落盘：高频收录走这条。
    ///
    /// 被跳过时会置 `save_pending`，由调用方（剪贴板轮询线程）下一轮
    /// `flush_if_pending` 补写 —— 只跳过不补，最后复制的那条就永远不落盘了。
    pub fn save_throttled(&self) -> AppResult<()> {
        {
            let mut gate = self.save_gate.lock();
            if let Some(at) = *gate {
                if at.elapsed() < SAVE_THROTTLE {
                    self.save_pending.store(true, Ordering::SeqCst);
                    return Ok(());
                }
            }
            *gate = Some(Instant::now());
        }
        self.save_pending.store(false, Ordering::SeqCst);
        self.save()
    }

    /// 有被节流掉的写入就补一次。轮询线程每轮调一次，成本是一次原子读。
    pub fn flush_if_pending(&self) -> AppResult<()> {
        if !self.save_pending.swap(false, Ordering::SeqCst) {
            return Ok(());
        }
        *self.save_gate.lock() = Some(Instant::now());
        self.save()
    }

    /// 无条件立即落盘。退出 / 清空 / 用户主动操作时用。
    pub fn save(&self) -> AppResult<()> {
        *self.save_gate.lock() = Some(Instant::now());
        self.save_pending.store(false, Ordering::SeqCst);
        let payload = IndexFile {
            version: 1,
            items: self.items.read().clone(),
        };
        let text = serde_json::to_string(&payload)?;
        let tmp = self.index_file.with_extension("json.tmp");
        std::fs::write(&tmp, text)?;
        std::fs::rename(&tmp, &self.index_file)?;
        Ok(())
    }

    pub fn find_by_id(&self, id: &str) -> Option<ClipboardItem> {
        self.items.read().iter().find(|i| i.id == id).cloned()
    }

    /// 命中重复内容：更新使用时间并提到最前
    pub fn touch(&self, hash: &str) -> Option<ClipboardItem> {
        let mut items = self.items.write();
        let pos = items.iter().position(|i| i.hash == hash)?;
        let mut item = items.remove(pos);
        item.last_used_at = chrono::Utc::now().timestamp_millis();
        item.hits += 1;
        let updated = item.clone();
        items.insert(0, item);
        Some(updated)
    }

    /// 加入一条历史。查重和插入在同一把锁里完成，两个扫描入口并发也只会进一条。
    pub fn add(&self, item: ClipboardItem) -> AddResult {
        let mut items = self.items.write();

        // 同一内容：更新使用时间、提到最前，不新增
        if let Some(pos) = items.iter().position(|i| i.hash == item.hash) {
            let mut existing = items.remove(pos);
            existing.last_used_at = item.last_used_at.max(existing.last_used_at);
            existing.hits += 1;
            items.insert(0, existing);
            return AddResult::Duplicate;
        }

        items.insert(0, item.clone());
        drop(items);
        self.enforce_limit();
        AddResult::Added(item)
    }

    fn enforce_limit(&self) {
        let limit = *self.max_items.read();
        let mut items = self.items.write();
        if items.len() <= limit {
            return;
        }
        // 置顶的永远不淘汰
        let overflow = items.len() - limit;
        let mut removed = Vec::new();
        let mut idx = items.len();
        let mut need = overflow;
        while need > 0 && idx > 0 {
            idx -= 1;
            if !items[idx].pinned {
                removed.push(items.remove(idx));
                need -= 1;
            }
        }
        drop(items);
        if !removed.is_empty() {
            for item in &removed {
                self.delete_files(item);
            }
            log::debug!("超出上限淘汰 {} 条", removed.len());
        }
    }

    pub fn query(&self, q: &ClipboardQuery) -> Vec<ClipboardItem> {
        let items = self.items.read();
        let keyword = q.keyword.as_ref().map(|k| k.trim().to_lowercase()).filter(|k| !k.is_empty());
        let kind = q.kind.as_deref().unwrap_or("all");
        let limit = q.limit.unwrap_or(100);
        let offset = q.offset.unwrap_or(0);
        // 来源比较忽略大小写和 `.exe` 后缀：用户点的是界面上的 "chrome"，
        // 而历史里存的是 "Chrome.EXE"，先转小写再剪后缀，否则筛不出来。
        let source = q
            .source
            .as_deref()
            .map(|s| {
                let low = s.trim().to_ascii_lowercase();
                low.trim_end_matches(".exe").to_string()
            })
            .filter(|s| !s.is_empty());

        let mut filtered: Vec<&ClipboardItem> = items
            .iter()
            .filter(|i| {
                if q.pinned_only.unwrap_or(false) && !i.pinned {
                    return false;
                }
                if kind != "all" && i.kind != kind {
                    return false;
                }
                if let Some(src) = &source {
                    let hit = i.source_app.as_deref().map_or(false, |s| {
                        s.to_ascii_lowercase().trim_end_matches(".exe") == src.as_str()
                    });
                    if !hit {
                        return false;
                    }
                }
                if let Some(k) = &keyword {
                    let hit_text = i.text.as_ref().map(|t| t.to_lowercase().contains(k)).unwrap_or(false);
                    let hit_files = i
                        .files
                        .as_ref()
                        .map(|f| f.iter().any(|p| p.to_lowercase().contains(k)))
                        .unwrap_or(false);
                    let hit_tags = i.tags.iter().any(|t| t.to_lowercase().contains(k));
                    if !(hit_text || hit_files || hit_tags) {
                        return false;
                    }
                }
                true
            })
            .collect();

        // 置顶优先，其次按最近使用
        filtered.sort_by(|a, b| match (a.pinned, b.pinned) {
            (true, false) => std::cmp::Ordering::Less,
            (false, true) => std::cmp::Ordering::Greater,
            _ => b.last_used_at.cmp(&a.last_used_at),
        });

        filtered.into_iter().skip(offset).take(limit).cloned().collect()
    }

    pub fn toggle_pin(&self, id: &str) -> Option<ClipboardItem> {
        let mut items = self.items.write();
        let item = items.iter_mut().find(|i| i.id == id)?;
        item.pinned = !item.pinned;
        Some(item.clone())
    }

    pub fn remove(&self, id: &str) -> bool {
        let mut items = self.items.write();
        if let Some(pos) = items.iter().position(|i| i.id == id) {
            let item = items.remove(pos);
            drop(items);
            self.delete_files(&item);
            return true;
        }
        false
    }

    pub fn clear(&self, keep_pinned: bool) {
        let mut items = self.items.write();
        let kept: Vec<ClipboardItem> = if keep_pinned {
            items.iter().filter(|i| i.pinned).cloned().collect()
        } else {
            Vec::new()
        };
        for item in items.iter() {
            if !keep_pinned || !item.pinned {
                self.delete_files(item);
            }
        }
        *items = kept;
    }

    pub fn purge_older_than(&self, days: u32) -> usize {
        if days == 0 {
            return 0;
        }
        let cutoff = chrono::Utc::now().timestamp_millis() - (days as i64) * 86_400_000;
        let mut items = self.items.write();
        let doomed: Vec<ClipboardItem> = items
            .iter()
            .filter(|i| !i.pinned && i.last_used_at < cutoff)
            .cloned()
            .collect();
        let ids: std::collections::HashSet<&String> = doomed.iter().map(|d| &d.id).collect();
        items.retain(|i| !ids.contains(&i.id));
        drop(items);
        for item in &doomed {
            self.delete_files(item);
        }
        doomed.len()
    }

    pub fn stats(&self) -> ClipboardStats {
        let items = self.items.read();
        let mut by_kind = HashMap::new();
        for item in items.iter() {
            *by_kind.entry(item.kind.clone()).or_insert(0) += 1;
        }
        let mut db_bytes = std::fs::metadata(&self.index_file).map(|m| m.len()).unwrap_or(0);
        if let Ok(entries) = std::fs::read_dir(&self.blobs_dir) {
            for entry in entries.flatten() {
                if let Ok(meta) = entry.metadata() {
                    if meta.is_file() {
                        db_bytes += meta.len();
                    }
                }
            }
        }
        ClipboardStats {
            total: items.len(),
            by_kind,
            db_bytes,
        }
    }

    pub fn blob_path(&self, item: &ClipboardItem) -> Option<PathBuf> {
        let blob = item.blob.as_ref()?;
        let p = self.blobs_dir.join(format!("{blob}.png"));
        p.exists().then_some(p)
    }

    pub fn thumb_path(&self, item: &ClipboardItem) -> Option<PathBuf> {
        let blob = item.blob.as_ref()?;
        let p = self.blobs_dir.join(format!("{blob}.t.jpg"));
        p.exists().then_some(p)
    }

    fn delete_files(&self, item: &ClipboardItem) {
        if let Some(blob) = &item.blob {
            let _ = std::fs::remove_file(self.blobs_dir.join(format!("{blob}.png")));
            let _ = std::fs::remove_file(self.blobs_dir.join(format!("{blob}.t.jpg")));
        }
    }

    pub fn blobs_dir(&self) -> &PathBuf {
        &self.blobs_dir
    }
}
