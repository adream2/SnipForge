//! 上传历史。
//!
//! 以前"刚才传的"只存在前端内存里：切个页面就没了，重启更不用说，
//! 用户想找上周传的那张图只能翻图床后台。这里把它落盘，
//! 和剪贴板历史一样——启动读一次，之后只在写的时候落盘。
//!
//! 容量上限按条数算（500 条），不按体积：记录本身只有几百字节，
//! 真正的图在远端，不占本地空间。

use crate::error::AppResult;
use crate::types::UploadRecord;
use std::path::PathBuf;

/// 保留最近多少条。再多也没什么意义，用户真要找老图会去图床后台。
const MAX_ITEMS: usize = 500;

pub struct UploadHistory {
    path: PathBuf,
    /// 新的在前
    items: Vec<UploadRecord>,
}

impl UploadHistory {
    pub fn load(path: PathBuf) -> Self {
        // 文件坏了就当空的：上传历史丢了大不了重传，不该拦住应用启动
        let items = std::fs::read_to_string(&path)
            .ok()
            .and_then(|t| serde_json::from_str::<Vec<UploadRecord>>(&t).ok())
            .unwrap_or_default();
        Self { path, items }
    }

    /// 关键字匹配文件名 / URL / 图床名，够用了，不做复杂查询
    pub fn list(&self, keyword: Option<&str>, limit: usize) -> Vec<UploadRecord> {
        let kw = keyword
            .map(|s| s.trim().to_lowercase())
            .filter(|s| !s.is_empty());
        self.items
            .iter()
            .filter(|r| match &kw {
                None => true,
                Some(k) => {
                    r.name.to_lowercase().contains(k)
                        || r.url.to_lowercase().contains(k)
                        || r.provider_name.to_lowercase().contains(k)
                }
            })
            .take(limit.max(1))
            .cloned()
            .collect()
    }

    pub fn push(&mut self, rec: UploadRecord) -> AppResult<()> {
        self.items.insert(0, rec);
        self.items.truncate(MAX_ITEMS);
        self.flush()
    }

    pub fn remove(&mut self, id: &str) -> AppResult<bool> {
        let before = self.items.len();
        self.items.retain(|r| r.id != id);
        let removed = self.items.len() != before;
        if removed {
            self.flush()?;
        }
        Ok(removed)
    }

    pub fn clear(&mut self) -> AppResult<()> {
        self.items.clear();
        self.flush()
    }

    fn flush(&self) -> AppResult<()> {
        let text = serde_json::to_string_pretty(&self.items)?;
        let tmp = self.path.with_extension("json.tmp");
        std::fs::write(&tmp, text)?;
        std::fs::rename(&tmp, &self.path)?;
        Ok(())
    }
}
