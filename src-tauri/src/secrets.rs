//! 密钥 / 密码收藏。
//!
//! 和剪贴板历史分开存（`secrets.json`）：历史会被容量淘汰、会被「清空」清掉，
//! 而收藏是用户主动维护的东西，不该跟着历史一起滚动消失。
//!
//! 明文存本地 —— 这是本机工具，加密就得让用户记一个主密码，
//! 每次粘贴前输一遍的摩擦远大于收益；但这点必须诚实：
//! **能读到这个数据目录的人就能拿到里面的内容**。

use crate::error::AppResult;
use crate::types::Secret;
use std::path::PathBuf;

pub struct SecretStore {
    path: PathBuf,
    /// 按 updated_at 倒序保存（最近用过的/改过的在前）
    items: Vec<Secret>,
}

fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

impl SecretStore {
    pub fn load(path: PathBuf) -> Self {
        // 文件坏了就当空的：收藏丢了大不了重录，不该拦住应用启动
        let mut items = std::fs::read_to_string(&path)
            .ok()
            .and_then(|t| serde_json::from_str::<Vec<Secret>>(&t).ok())
            .unwrap_or_default();
        items.sort_by(|a, b| b.updated_at.cmp(&a.updated_at));
        Self { path, items }
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

    fn save(&self) -> AppResult<()> {
        let text = serde_json::to_string_pretty(&self.items)?;
        let tmp = self.path.with_extension("json.tmp");
        std::fs::write(&tmp, text)?;
        std::fs::rename(&tmp, &self.path)?;
        Ok(())
    }
}
