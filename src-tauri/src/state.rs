use parking_lot::{Mutex, RwLock};
use std::sync::Arc;

use crate::capture::screen::CaptureFrame;
use crate::config::AppSettings;
use crate::hooks::HookRegistry;
use crate::paths::Paths;
use crate::types::HotkeyResult;
use crate::vault::{self, Vault};

/// 一次截图会话的临时状态。用完即弃，不落盘。
pub struct CaptureSession {
    pub frame: CaptureFrame,
    /// region | window | fullscreen | active-window | scroll
    pub mode: String,
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
    /// 密钥/密码收藏（和剪贴板历史分开存）
    pub secrets: Mutex<crate::secrets::SecretStore>,
    /// 主密码保险箱：设了主密码后，敏感数据在磁盘上只有密文
    pub vault: Arc<Vault>,
    /// 最近一次版本检查的结果（内存里，不落盘）：
    /// 界面打开「关于」时先拿它，避免每次进页面都打一次 GitHub API
    pub update: RwLock<Option<crate::update::UpdateInfo>>,
}

/// 翻译配置里算"凭据"的两个字段。
///
/// 一起进保险箱：`deepl_key` 是 API key，`custom_headers` 里常塞 `Authorization`。
/// 其余字段（provider / url / target / body）不敏感，保持可读 ——
/// 整个 translate 段都加密的话，锁着的时候连"用哪家翻译"都看不出来，没这个必要。
#[derive(serde::Serialize, serde::Deserialize, Default)]
#[serde(rename_all = "camelCase")]
struct TranslateSecrets {
    deepl_key: String,
    custom_headers: String,
}

impl AppState {
    pub fn new(settings: AppSettings, paths: Paths) -> Arc<Self> {
        let uploads =
            crate::uploader::history::UploadHistory::load(paths.uploads_file());

        // 保险箱要在读收藏之前建好：设了主密码时，收藏不读明文文件
        let vault = Arc::new(Vault::new(paths.data.join("vault.json")));
        vault.load();

        let secrets = crate::secrets::SecretStore::load(paths.secrets_file(), vault.clone());
        Arc::new(Self {
            settings: RwLock::new(settings),
            paths,
            hotkey_results: RwLock::new(Vec::new()),
            hooks: Arc::new(HookRegistry::new()),
            capture: Mutex::new(None),
            pending: Mutex::new(None),
            uploads: Mutex::new(uploads),
            secrets: Mutex::new(secrets),
            vault,
            update: RwLock::new(None),
        })
    }

    pub fn settings_snapshot(&self) -> AppSettings {
        self.settings.read().clone()
    }

    /// 原子写：先写 tmp 再 rename，断电也不会留下半截 JSON。
    ///
    /// **设了主密码时，凭据不进这个文件**：先把 providers / 翻译凭据写进保险箱，
    /// 落盘的 settings.json 里这几个字段是空的。这样"打开设置文件看一眼"
    /// 拿不到 token —— 那是最容易被顺手拷走的泄漏方式。
    pub fn persist_settings(&self) -> crate::error::AppResult<()> {
        let mut snapshot = self.settings.read().clone();
        if self.vault.is_enabled() {
            if self.vault.is_unlocked() {
                let providers = std::mem::take(&mut snapshot.uploader.providers);
                let tr = TranslateSecrets {
                    deepl_key: std::mem::take(&mut snapshot.translate.deepl_key),
                    custom_headers: std::mem::take(&mut snapshot.translate.custom_headers),
                };
                self.vault.write_slots(&[
                    (vault::SLOT_PROVIDERS, serde_json::to_vec(&providers)?),
                    (vault::SLOT_TRANSLATE, serde_json::to_vec(&tr)?),
                ])?;
            } else {
                // 锁着的时候绝不把凭据落到明文上
                snapshot.uploader.providers.clear();
                snapshot.translate.deepl_key.clear();
                snapshot.translate.custom_headers.clear();
            }
        }

        let file = self.paths.settings_file();
        let tmp = file.with_extension("json.tmp");
        let text = serde_json::to_string_pretty(&snapshot)?;
        std::fs::write(&tmp, text)?;
        std::fs::rename(&tmp, &file)?;
        Ok(())
    }

    /* ---------------- 主密码 ---------------- */

    /// 首次设置主密码：把内存里的敏感数据搬进保险箱，再把明文抹掉。
    pub fn enable_vault(&self, password: &str) -> crate::error::AppResult<()> {
        self.vault.create(password)?;

        let secrets = serde_json::to_vec(&self.secrets.lock().list())?;
        let (providers, tr) = {
            let s = self.settings.read();
            (
                serde_json::to_vec(&s.uploader.providers)?,
                TranslateSecrets {
                    deepl_key: s.translate.deepl_key.clone(),
                    custom_headers: s.translate.custom_headers.clone(),
                },
            )
        };
        self.vault.write_slots(&[
            (vault::SLOT_SECRETS, secrets),
            (vault::SLOT_PROVIDERS, providers),
            (vault::SLOT_TRANSLATE, serde_json::to_vec(&tr)?),
        ])?;

        // 明文收藏文件必须删掉，否则加密等于没做
        let secrets_file = self.paths.secrets_file();
        if secrets_file.exists() {
            if let Err(e) = std::fs::remove_file(&secrets_file) {
                log::warn!("删除明文 secrets.json 失败（请手动删）: {e}");
            }
        }
        // persist_settings 会把凭据从 settings.json 里摘出去
        self.persist_settings()?;
        log::info!("主密码已启用，敏感数据已加密");
        Ok(())
    }

    pub fn unlock_vault(&self, password: &str) -> crate::error::AppResult<()> {
        self.vault.unlock(password)?;
        self.hydrate_from_vault()
    }

    /// 把保险箱里的内容灌回内存
    fn hydrate_from_vault(&self) -> crate::error::AppResult<()> {
        if let Some(bytes) = self.vault.read_slot(vault::SLOT_SECRETS)? {
            let items: Vec<crate::types::Secret> = serde_json::from_slice(&bytes)?;
            self.secrets.lock().replace_all(items);
        }
        if let Some(bytes) = self.vault.read_slot(vault::SLOT_PROVIDERS)? {
            let map = serde_json::from_slice(&bytes)?;
            self.settings.write().uploader.providers = map;
        }
        if let Some(bytes) = self.vault.read_slot(vault::SLOT_TRANSLATE)? {
            let t: TranslateSecrets = serde_json::from_slice(&bytes).unwrap_or_default();
            let mut s = self.settings.write();
            s.translate.deepl_key = t.deepl_key;
            s.translate.custom_headers = t.custom_headers;
        }
        Ok(())
    }

    /// 立即锁定：key 从内存里丢掉，凭据也一起清掉（不然锁定只是换个图标）
    pub fn lock_vault(&self) {
        self.vault.lock();
        self.secrets.lock().replace_all(Vec::new());
        let mut s = self.settings.write();
        s.uploader.providers.clear();
        s.translate.deepl_key.clear();
        s.translate.custom_headers.clear();
    }

    /// 关掉主密码：先写回明文，再删保险箱。
    ///
    /// 顺序不能反：反了的话中间崩一次，数据就只剩保险箱里那份，文件已被删 → 全丢。
    pub fn disable_vault(&self) -> crate::error::AppResult<()> {
        if !self.vault.is_enabled() {
            return Ok(());
        }
        // 1. 明文写回（绕过保险箱）
        self.secrets.lock().save_plaintext()?;
        // 2. 撤掉保险箱（此后 persist_settings 会把凭据写回 settings.json）
        self.vault.destroy()?;
        // 3. 重新落盘设置
        self.persist_settings()?;
        log::info!("主密码已关闭");
        Ok(())
    }

    pub fn change_vault_password(&self, new_password: &str) -> crate::error::AppResult<()> {
        self.vault.change_password(new_password)
    }

    /// 忘记密码时的兜底：清空保险箱，回到明文模式（**加密数据全部丢失**）。
    ///
    /// 没有它的话，忘了密码的用户会永远卡在"锁着但打不开"的状态 ——
    /// 应用还能用，但收藏和图床凭据永远拿不回来，也没有任何补救入口。
    /// 这里不做任何"偷偷绕过密码"的事：数据是真的删掉。
    pub fn reset_vault(&self) -> crate::error::AppResult<()> {
        self.lock_vault();
        self.vault.destroy()?;
        // 明文文件也一并清掉，避免留下"上一轮的明文"造成误解
        let secrets_file = self.paths.secrets_file();
        if secrets_file.exists() {
            let _ = std::fs::remove_file(&secrets_file);
        }
        self.persist_settings()?;
        log::warn!("保险箱已被清空（忘记密码后的重置）");
        Ok(())
    }

    /// 释放截图占用的原始位图（几块 4K 屏加起来上百 MB，必须及时放）
    pub fn release_capture(&self) {
        *self.capture.lock() = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_paths(name: &str) -> Paths {
        let dir = std::env::temp_dir().join(format!("snipforge-state-test-{name}"));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let paths = Paths::new(dir.clone(), None, dir.join("pictures"));
        paths.ensure().unwrap();
        paths
    }

    fn reread(settings_file: &std::path::Path) -> AppSettings {
        serde_json::from_str(&std::fs::read_to_string(settings_file).unwrap()).unwrap()
    }

    /// 这个测试守的是一句承诺：**设了主密码之后，磁盘上没有明文凭据**。
    ///
    /// 它同时覆盖了最容易忘的两处：
    ///   1. `secrets.json` 必须被删掉（否则加密等于没做）；
    ///   2. 图床 token 必须从 `settings.json` 摘出去（"打开设置文件看一眼"是最常见的泄漏路径）。
    #[test]
    fn enabling_vault_leaves_no_plaintext_on_disk() {
        let paths = temp_paths("no-plaintext");

        let mut settings = AppSettings::default();
        settings
            .uploader
            .providers
            .entry("github".into())
            .or_default()
            .insert("token".into(), serde_json::json!("SUPERSECRET_TOKEN"));
        settings.translate.deepl_key = "DEEPL-KEY-123".into();

        let st = AppState::new(settings, paths.clone());
        st.secrets
            .lock()
            .upsert("", "银行密码", "hunter2-plaintext", "")
            .unwrap();
        st.persist_settings().unwrap();

        // 加密前：明文确实在盘上（这是我们要消除的状态）
        assert!(paths.secrets_file().exists(), "加密前应当是明文文件");

        st.enable_vault("a-strong-password").unwrap();
        assert!(st.vault.is_unlocked());

        // 1. 明文收藏文件必须消失
        assert!(
            !paths.secrets_file().exists(),
            "启用保险箱后 secrets.json 必须删除"
        );

        // 2. settings.json 里不能出现 token / key
        let settings_raw = std::fs::read_to_string(paths.settings_file()).unwrap();
        assert!(
            !settings_raw.contains("SUPERSECRET_TOKEN"),
            "settings.json 里还残留图床 token"
        );
        assert!(
            !settings_raw.contains("DEEPL-KEY-123"),
            "settings.json 里还残留翻译 key"
        );

        // 3. vault.json 是密文，也不该出现明文串
        let vault_raw = std::fs::read_to_string(paths.data.join("vault.json")).unwrap();
        assert!(!vault_raw.contains("hunter2-plaintext"));
        assert!(!vault_raw.contains("SUPERSECRET_TOKEN"));

        // 4. 模拟重启：锁着时内存里什么都拿不到
        let restarted = AppState::new(reread(&paths.settings_file()), paths.clone());
        assert!(restarted.vault.is_enabled());
        assert!(!restarted.vault.is_unlocked());
        assert!(restarted.secrets.lock().list().is_empty());
        assert!(restarted.settings.read().uploader.providers.is_empty());

        // 5. 解锁后数据回来了
        restarted.unlock_vault("a-strong-password").unwrap();
        let secrets = restarted.secrets.lock().list();
        assert_eq!(secrets.len(), 1);
        assert_eq!(secrets[0].value, "hunter2-plaintext");
        assert_eq!(
            restarted
                .settings
                .read()
                .uploader
                .providers
                .get("github")
                .unwrap()
                .get("token")
                .unwrap(),
            &serde_json::json!("SUPERSECRET_TOKEN")
        );
        assert_eq!(restarted.settings.read().translate.deepl_key, "DEEPL-KEY-123");
    }

    /// 关掉主密码要把数据写回明文，且顺序不能反（先写后删）
    #[test]
    fn disabling_vault_writes_plaintext_back() {
        let paths = temp_paths("disable");

        let mut settings = AppSettings::default();
        settings
            .uploader
            .providers
            .entry("github".into())
            .or_default()
            .insert("token".into(), serde_json::json!("T0KEN"));
        let st = AppState::new(settings, paths.clone());
        st.secrets.lock().upsert("", "名字", "值", "").unwrap();

        st.enable_vault("password-1234").unwrap();
        st.disable_vault().unwrap();

        assert!(!st.vault.is_enabled());
        // 明文回写：文件存在且内容完整
        let raw = std::fs::read_to_string(paths.secrets_file()).unwrap();
        assert!(raw.contains("值"));
        let settings_raw = std::fs::read_to_string(paths.settings_file()).unwrap();
        assert!(settings_raw.contains("T0KEN"));
        // 保险箱文件已删除
        assert!(!paths.data.join("vault.json").exists());
    }
}
