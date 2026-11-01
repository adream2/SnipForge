//! 密钥 / 密码收藏。
//!
//! 和剪贴板历史分开存：历史会被容量淘汰、会被「清空」清掉，
//! 而收藏是用户主动维护的东西，不该跟着历史一起滚动消失。
//!
//! **落盘位置取决于有没有设主密码**：
//!   - 没设：明文写 `secrets.json`（本机工具的取舍，界面上写明了）；
//!   - 设了：进保险箱（`vault.json`），这里是密文，明文文件会被删掉。
//!
//! 加解密和 KDF 都在 `vault.rs`，这个文件只管"收藏列表"这件事本身。

use crate::error::AppResult;
use crate::types::Secret;
use crate::vault::{Vault, SLOT_SECRETS};
use std::path::PathBuf;
use std::sync::Arc;

pub struct SecretStore {
    path: PathBuf,
    /// 按 updated_at 倒序保存（最近用过的/改过的在前）
    items: Vec<Secret>,
    vault: Arc<Vault>,
}

fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

impl SecretStore {
    pub fn load(path: PathBuf, vault: Arc<Vault>) -> Self {
        // 设了主密码就是锁着的：此刻磁盘上只有密文，先把列表留空，
        // 解锁时由 AppState 从保险箱灌回来。
        let mut items = if vault.is_enabled() {
            Vec::new()
        } else {
            std::fs::read_to_string(&path)
                .ok()
                .and_then(|t| serde_json::from_str::<Vec<Secret>>(&t).ok())
                .unwrap_or_default()
        };
        items.sort_by(|a, b| b.updated_at.cmp(&a.updated_at));
        Self {
            path,
            items,
            vault,
        }
    }

    /// 解锁后替换整个列表
    pub fn replace_all(&mut self, items: Vec<Secret>) {
        self.items = items;
        self.items.sort_by(|a, b| b.updated_at.cmp(&a.updated_at));
    }

    pub fn list(&self) -> Vec<Secret> {
        self.items.clone()
    }

    pub fn find(&self, id: &str) -> Option<Secret> {
        self.items.iter().find(|s| s.id == id).cloned()
    }

    /// 新建或更新。`id` 为空 = 新建。
    ///
    /// 更新时保留 created_at —— 排序和"这份东西什么时候加的"都靠它，
    /// 改个错别字不该让它变成新条目。
    pub fn upsert(
        &mut self,
        id: &str,
        name: &str,
        value: &str,
        note: &str,
    ) -> AppResult<Secret> {
        let name = name.trim();
        if name.is_empty() {
            return Err(crate::error::AppError::Msg("名称不能为空".into()));
        }
        if value.is_empty() {
            return Err(crate::error::AppError::Msg("内容不能为空".into()));
        }
        let now = now_ms();

        if !id.is_empty() {
            if let Some(item) = self.items.iter_mut().find(|s| s.id == id) {
                item.name = name.to_string();
                item.value = value.to_string();
                item.note = note.trim().to_string();
                item.updated_at = now;
                let out = item.clone();
                self.sort_and_save()?;
                return Ok(out);
            }
        }

        // 新建时如果这份内容已经收藏过，直接返回已有的那条。
        // 从剪贴板列表连点两次收藏是很自然的操作，不该出现两条一模一样的。
        if let Some(existing) = self.items.iter().find(|s| s.value == value) {
            return Ok(existing.clone());
        }

        let item = Secret {
            id: format!("s_{now}_{}", self.items.len()),
            name: name.to_string(),
            value: value.to_string(),
            note: note.trim().to_string(),
            created_at: now,
            updated_at: now,
        };
        self.items.push(item.clone());
        self.sort_and_save()?;
        Ok(item)
    }

    /// 只改名字。
    ///
    /// 不复用 `upsert` 是因为那会把 `updated_at` 推到现在，
    /// 列表按它倒序排 —— 改个名字条目就跳到最前面，找起来更费劲。
    /// 改名字不该改变"这条是什么时候加的"。
    pub fn rename(&mut self, id: &str, name: &str) -> AppResult<Secret> {
        let name = name.trim();
        if name.is_empty() {
            return Err(crate::error::AppError::Msg("名称不能为空".into()));
        }
        let (out, changed) = {
            let Some(item) = self.items.iter_mut().find(|s| s.id == id) else {
                return Err(crate::error::AppError::Msg("收藏不存在".into()));
            };
            let changed = item.name != name;
            if changed {
                item.name = name.to_string();
            }
            (item.clone(), changed)
        };
        // 用的是 save 而不是 sort_and_save：位置保持不变
        if changed {
            self.save()?;
        }
        Ok(out)
    }

    pub fn remove(&mut self, id: &str) -> AppResult<bool> {
        let before = self.items.len();
        self.items.retain(|s| s.id != id);
        let removed = self.items.len() != before;
        if removed {
            self.save()?;
        }
        Ok(removed)
    }

    /// 用过一次就往前排，下次更容易找到
    pub fn touch(&mut self, id: &str) -> AppResult<()> {
        if let Some(item) = self.items.iter_mut().find(|s| s.id == id) {
            item.updated_at = now_ms();
            self.sort_and_save()?;
        }
        Ok(())
    }

    fn sort_and_save(&mut self) -> AppResult<()> {
        self.items.sort_by(|a, b| b.updated_at.cmp(&a.updated_at));
        self.save()
    }

    /// 落盘。设了主密码时进保险箱，否则写明文文件。
    fn save(&self) -> AppResult<()> {
        let json = serde_json::to_vec(&self.items)?;
        if self.vault.is_enabled() {
            if !self.vault.is_unlocked() {
                return Err(crate::error::AppError::Msg("保险箱未解锁".into()));
            }
            self.vault.write_slots(&[(SLOT_SECRETS, json)])?;
            // 从明文模式迁过来的老文件不能留着
            if self.path.exists() {
                let _ = std::fs::remove_file(&self.path);
            }
            return Ok(());
        }
        self.write_plaintext(&json)
    }

    /// 强制写明文（关掉主密码时用；此时保险箱可能已不可用）
    pub fn save_plaintext(&self) -> AppResult<()> {
        self.write_plaintext(&serde_json::to_vec_pretty(&self.items)?)
    }

    fn write_plaintext(&self, bytes: &[u8]) -> AppResult<()> {
        let tmp = self.path.with_extension("json.tmp");
        std::fs::write(&tmp, bytes)?;
        std::fs::rename(&tmp, &self.path)?;
        Ok(())
    }
}
