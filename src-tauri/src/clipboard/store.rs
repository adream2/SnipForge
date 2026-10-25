//! 剪贴板历史存储。
//!
//! 落盘策略：元数据进 history.json，图片 blob 按内容哈希独立存文件。
//! 不上 SQLite —— 省掉 native 依赖和迁移负担；单条删除也不用重写大文件。

use crate::error::AppResult;
use crate::paths::Paths;
use crate::types::{ClipboardItem, ClipboardQuery, ClipboardStats};
use parking_lot::RwLock;
use std::collections::HashMap;
use std::path::PathBuf;

#[derive(serde::Serialize, serde::Deserialize, Default)]
struct IndexFile {
    #[serde(default)]
    version: u32,
    #[serde(default)]
    items: Vec<ClipboardItem>,
}

pub struct ClipboardStore {
    items: RwLock<Vec<ClipboardItem>>,
    by_hash: RwLock<HashMap<String, String>>,
    index_file: PathBuf,
    blobs_dir: PathBuf,
    pub max_items: RwLock<usize>,
}

impl ClipboardStore {
    pub fn new(paths: &Paths) -> Self {
        Self {
            items: RwLock::new(Vec::new()),
            by_hash: RwLock::new(HashMap::new()),
            index_file: paths.clipboard_index.clone(),
            blobs_dir: paths.blobs.clone(),
            max_items: RwLock::new(2000),
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
                    items.sort_by(|a, b| b.last_used_at.cmp(&a.last_used_at));
                    let mut map = HashMap::new();
                    for item in &items {
                        map.insert(item.hash.clone(), item.id.clone());
                    }
                    log::info!("载入剪贴板历史 {} 条", items.len());
                    *self.items.write() = items;
                    *self.by_hash.write() = map;
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

    pub fn save(&self) -> AppResult<()> {
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

    pub fn find_by_hash(&self, hash: &str) -> Option<ClipboardItem> {
        let map = self.by_hash.read();
        let id = map.get(hash)?;
        self.items.read().iter().find(|i| &i.id == id).cloned()
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

    pub fn add(&self, item: ClipboardItem) -> ClipboardItem {
        let mut items = self.items.write();
        items.insert(0, item.clone());
        self.by_hash.write().insert(item.hash.clone(), item.id.clone());
        drop(items);
        self.enforce_limit();
        item
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
            let mut map = self.by_hash.write();
            for item in &removed {
                map.remove(&item.hash);
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

        let mut filtered: Vec<&ClipboardItem> = items
            .iter()
            .filter(|i| {
                if q.pinned_only.unwrap_or(false) && !i.pinned {
                    return false;
                }
                if kind != "all" && i.kind != kind {
                    return false;
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
            self.by_hash.write().remove(&item.hash);
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
        *items = kept.clone();
        let mut map = self.by_hash.write();
        map.clear();
        for item in &kept {
            map.insert(item.hash.clone(), item.id.clone());
        }
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
        let mut map = self.by_hash.write();
        for item in &doomed {
            map.remove(&item.hash);
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
