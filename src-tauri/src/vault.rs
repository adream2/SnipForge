//! 主密码保险箱。
//!
//! 保护两类**真的算秘密**的东西：收藏的密钥/密码、图床凭据、翻译服务的 key。
//! 没设主密码时行为完全不变（明文，界面上明说）；设了之后磁盘上只剩密文。
//!
//! ## 为什么这么设计
//!
//! - **KDF 用 Argon2id**（memory-hard），参数取 OWASP 建议下限
//!   `m=19MiB, t=2, p=1`。用 PBKDF2-SHA256 也能上，但 GPU 集群打 Argon2 更贵。
//! - **加密用 AES-256-GCM**：自带完整性校验，"密码对不对"不用额外判断 ——
//!   解不开就是不对，也不存在"密文被改了一位还能解出垃圾"。
//! - **每个 slot 独立 nonce**，每次写盘都重新生成 nonce。
//!   GCM 最致命的误用是同一个 key 复用 nonce，那样两条密文一异或就泄漏明文。
//! - **AAD 里放 slot 名**：防止把 `secrets` 的密文挪到 `providers` 的位置上
//!   还能解出来（槽位互换攻击）。
//! - **`check` 槽**：建库时写一个固定串。没有它的话，"新建但还没存过数据"
//!   的库用任何密码都能解锁 —— 因为没有任何密文可供验证。
//!
//! ## 文件格式（`vault.json`）
//!
//! ```json
//! {
//!   "version": 1,
//!   "kdf": { "alg": "argon2id", "m": 19456, "t": 2, "p": 1, "salt": "<b64>" },
//!   "slots": {
//!     "check":     { "nonce": "<b64>", "ct": "<b64>" },
//!     "secrets":   { "nonce": "<b64>", "ct": "<b64>" },
//!     "providers": { "nonce": "<b64>", "ct": "<b64>" }
//!   }
//! }
//! ```
//!
//! ## 边界（必须诚实）
//!
//! 这是**静态数据保护**：能防住"翻你 AppData 目录的人/程序"、防住备份和同步盘泄漏。
//! 防不住"已经在你机器上以你的身份运行的恶意程序"——它可以在你解锁时读内存。
//! 界面上要把这句话写出来，不能让人以为设了密码就刀枪不入。

use crate::error::{AppError, AppResult};
use aes_gcm::aead::{Aead, KeyInit, Payload};
use aes_gcm::{Aes256Gcm, Nonce};
use parking_lot::Mutex;
use rand::RngCore;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::PathBuf;
use zeroize::Zeroizing;

/// 收藏的密钥/密码
pub const SLOT_SECRETS: &str = "secrets";
/// 图床凭据（token / secret key / cookie）
pub const SLOT_PROVIDERS: &str = "providers";
/// 翻译服务的敏感字段（DeepL key、自定义请求头）
pub const SLOT_TRANSLATE: &str = "translate";
const SLOT_CHECK: &str = "check";

/// 建库时写进 `check` 槽的固定串，用来验证密码是否正确
const CHECK_MAGIC: &[u8] = b"snipforge-vault-v1";

const KEY_LEN: usize = 32;
const NONCE_LEN: usize = 12;
const SALT_LEN: usize = 16;

/// Argon2id 参数（OWASP 建议下限）。写进文件，以后调了也能解开老库。
const KDF_M: u32 = 19 * 1024; // 19 MiB
const KDF_T: u32 = 2;
const KDF_P: u32 = 1;

#[derive(Debug, Clone, Serialize, Deserialize)]
struct KdfMeta {
    alg: String,
    m: u32,
    t: u32,
    p: u32,
    salt: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct SlotData {
    nonce: String,
    ct: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct VaultFile {
    version: u32,
    kdf: KdfMeta,
    #[serde(default)]
    slots: BTreeMap<String, SlotData>,
}

enum Inner {
    /// 没设主密码
    Disabled,
    /// 设了，但还没解锁（启动时就是这个状态）
    Locked(VaultFile),
    /// 已解锁，key 在内存里
    Unlocked(VaultFile, Zeroizing<[u8; KEY_LEN]>),
}

pub struct Vault {
    path: PathBuf,
    inner: Mutex<Inner>,
}

/// 给前端的状态
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VaultStatus {
    /// 设了主密码
    pub enabled: bool,
    /// 当前已解锁
    pub unlocked: bool,
}

fn b64(bytes: &[u8]) -> String {
    use base64::Engine;
    base64::engine::general_purpose::STANDARD.encode(bytes)
}

fn from_b64(s: &str) -> AppResult<Vec<u8>> {
    use base64::Engine;
    base64::engine::general_purpose::STANDARD
        .decode(s)
        .map_err(|e| AppError::Msg(format!("保险箱数据损坏: {e}")))
}

fn random_bytes<const N: usize>() -> [u8; N] {
    let mut buf = [0u8; N];
    rand::rngs::OsRng.fill_bytes(&mut buf);
    buf
}

/// 密码 → 32 字节密钥
fn derive(password: &str, meta: &KdfMeta) -> AppResult<Zeroizing<[u8; KEY_LEN]>> {
    use argon2::{Algorithm, Argon2, Params, Version};

    let salt = from_b64(&meta.salt)?;
    let params = Params::new(meta.m, meta.t, meta.p, Some(KEY_LEN))
        .map_err(|e| AppError::Msg(format!("KDF 参数不合法: {e}")))?;
    let argon = Argon2::new(Algorithm::Argon2id, Version::V0x13, params);

    let mut key = Zeroizing::new([0u8; KEY_LEN]);
    argon
        .hash_password_into(password.as_bytes(), &salt, key.as_mut())
        .map_err(|e| AppError::Msg(format!("密钥派生失败: {e}")))?;
    Ok(key)
}

fn cipher(key: &[u8]) -> AppResult<Aes256Gcm> {
    Aes256Gcm::new_from_slice(key).map_err(|_| AppError::Msg("密钥长度不对".into()))
}

fn encrypt_slot(key: &[u8], slot: &str, plaintext: &[u8]) -> AppResult<SlotData> {
    let nonce_bytes = random_bytes::<NONCE_LEN>();
    let nonce = Nonce::from_slice(&nonce_bytes);
    let ct = cipher(key)?
        .encrypt(
            nonce,
            Payload {
                msg: plaintext,
                // slot 名进 AAD：换个槽位就解不开
                aad: slot.as_bytes(),
            },
        )
        .map_err(|_| AppError::Msg("加密失败".into()))?;
    Ok(SlotData {
        nonce: b64(&nonce_bytes),
        ct: b64(&ct),
    })
}

fn decrypt_slot(key: &[u8], slot: &str, data: &SlotData) -> AppResult<Vec<u8>> {
    let nonce_bytes = from_b64(&data.nonce)?;
    if nonce_bytes.len() != NONCE_LEN {
        return Err(AppError::Msg("保险箱数据损坏（nonce 长度不对）".into()));
    }
    let ct = from_b64(&data.ct)?;
    cipher(key)?
        .decrypt(
            Nonce::from_slice(&nonce_bytes),
            Payload {
                msg: &ct,
                aad: slot.as_bytes(),
            },
        )
        // GCM 校验失败 = 密码不对，或者文件被改过。两者对用户都是"打不开"。
        .map_err(|_| AppError::Msg("主密码不正确（或保险箱文件损坏）".into()))
}

impl Vault {
    pub fn new(path: PathBuf) -> Self {
        Self {
            path,
            inner: Mutex::new(Inner::Disabled),
        }
    }

    /// 启动时调用：文件在就把状态置为"已锁"。
    ///
    /// 读不出来（损坏）不拦启动 —— 应用还能用，只是保险箱里的东西取不出来，
    /// 界面上会显示"已锁定"，用户可以选重设。
    pub fn load(&self) {
        if !self.path.exists() {
            return;
        }
        match std::fs::read_to_string(&self.path)
            .ok()
            .and_then(|t| serde_json::from_str::<VaultFile>(&t).ok())
        {
            Some(file) => {
                log::info!("检测到主密码保险箱（已锁定）");
                *self.inner.lock() = Inner::Locked(file);
            }
            None => {
                log::error!("保险箱文件无法解析，忽略: {}", self.path.display());
            }
        }
    }

    pub fn status(&self) -> VaultStatus {
        let guard = self.inner.lock();
        VaultStatus {
            enabled: !matches!(&*guard, Inner::Disabled),
            unlocked: matches!(&*guard, Inner::Unlocked(..)),
        }
    }

    pub fn is_enabled(&self) -> bool {
        !matches!(&*self.inner.lock(), Inner::Disabled)
    }

    pub fn is_unlocked(&self) -> bool {
        matches!(&*self.inner.lock(), Inner::Unlocked(..))
    }

    /// 首次设置主密码。已经在用就报错（改密码走 `change_password`）。
    ///
    /// 建库时**立刻写 check 槽**：没有它，"新建但没存过数据"的库
    /// 用任何密码都能解锁（没有密文可供验证）。
    pub fn create(&self, password: &str) -> AppResult<()> {
        let mut guard = self.inner.lock();
        if !matches!(&*guard, Inner::Disabled) {
            return Err(AppError::Msg("主密码已经设置过了".into()));
        }

        let meta = KdfMeta {
            alg: "argon2id".into(),
            m: KDF_M,
            t: KDF_T,
            p: KDF_P,
            salt: b64(&random_bytes::<SALT_LEN>()),
        };
        let key = derive(password, &meta)?;

        let mut file = VaultFile {
            version: 1,
            kdf: meta,
            slots: BTreeMap::new(),
        };
        file.slots
            .insert(SLOT_CHECK.into(), encrypt_slot(key.as_ref(), SLOT_CHECK, CHECK_MAGIC)?);
        write_file(&self.path, &file)?;

        *guard = Inner::Unlocked(file, key);
        log::info!("主密码已设置，保险箱已启用");
        Ok(())
    }

    pub fn unlock(&self, password: &str) -> AppResult<()> {
        let mut guard = self.inner.lock();
        let file = match &*guard {
            Inner::Disabled => return Err(AppError::Msg("还没有设置主密码".into())),
            Inner::Unlocked(..) => return Ok(()),
            Inner::Locked(f) => f.clone(),
        };

        let key = derive(password, &file.kdf)?;
        // 用 check 槽验证密码；老库没有 check 就退而验证任意一个槽
        match file.slots.get(SLOT_CHECK) {
            Some(data) => {
                let plain = decrypt_slot(key.as_ref(), SLOT_CHECK, data)?;
                if plain != CHECK_MAGIC {
                    return Err(AppError::Msg("主密码不正确".into()));
                }
            }
            None => {
                if let Some((name, data)) = file.slots.iter().next() {
                    decrypt_slot(key.as_ref(), name, data)?;
                }
            }
        }

        *guard = Inner::Unlocked(file, key);
        log::info!("保险箱已解锁");
        Ok(())
    }

    pub fn lock(&self) {
        let mut guard = self.inner.lock();
        if let Inner::Unlocked(file, _) = &*guard {
            *guard = Inner::Locked(file.clone());
            log::info!("保险箱已锁定");
        }
    }

    /// 读一个槽。未解锁 / 没有这个槽都返回 None。
    pub fn read_slot(&self, slot: &str) -> AppResult<Option<Vec<u8>>> {
        let guard = self.inner.lock();
        let Inner::Unlocked(file, key) = &*guard else {
            return Ok(None);
        };
        match file.slots.get(slot) {
            Some(data) => Ok(Some(decrypt_slot(key.as_ref(), slot, data)?)),
            None => Ok(None),
        }
    }

    /// 批量写槽（一次落盘）。必须已解锁。
    ///
    /// 一次写多个槽而不是逐个写：每个槽都要重新加密并重写整个文件，
    /// 分开写会让"图床已更新、翻译还是旧值"这种半更新状态出现在磁盘上。
    pub fn write_slots(&self, entries: &[(&str, Vec<u8>)]) -> AppResult<()> {
        let mut guard = self.inner.lock();
        let Inner::Unlocked(file, key) = &mut *guard else {
            return Err(AppError::Msg("保险箱未解锁".into()));
        };
        for (slot, plain) in entries {
            file.slots
                .insert((*slot).to_string(), encrypt_slot(key.as_ref(), slot, plain)?);
        }
        write_file(&self.path, file)
    }

    /// 换密码：用当前 key 解出所有槽，再用新盐新 key 重新加密。
    pub fn change_password(&self, new_password: &str) -> AppResult<()> {
        let mut guard = self.inner.lock();
        let Inner::Unlocked(old_file, old_key) = &*guard else {
            return Err(AppError::Msg("保险箱未解锁".into()));
        };

        // 先把明文全部解出来（内存里，不落盘）
        let mut plain: Vec<(String, Vec<u8>)> = Vec::with_capacity(old_file.slots.len());
        for (name, data) in &old_file.slots {
            plain.push((name.clone(), decrypt_slot(old_key.as_ref(), name, data)?));
        }

        let meta = KdfMeta {
            alg: "argon2id".into(),
            m: KDF_M,
            t: KDF_T,
            p: KDF_P,
            salt: b64(&random_bytes::<SALT_LEN>()),
        };
        let key = derive(new_password, &meta)?;

        let mut file = VaultFile {
            version: 1,
            kdf: meta,
            slots: BTreeMap::new(),
        };
        for (name, bytes) in &plain {
            file.slots
                .insert(name.clone(), encrypt_slot(key.as_ref(), name, bytes)?);
        }
        if !file.slots.contains_key(SLOT_CHECK) {
            file.slots
                .insert(SLOT_CHECK.into(), encrypt_slot(key.as_ref(), SLOT_CHECK, CHECK_MAGIC)?);
        }
        write_file(&self.path, &file)?;

        *guard = Inner::Unlocked(file, key);
        log::info!("主密码已更换");
        Ok(())
    }

    /// 关掉主密码：删文件、回到明文模式。
    ///
    /// **调用方必须先把明文写回各自的文件**，否则数据就只剩这个保险箱里
    /// 那一份，删了就没了。这里只负责把状态归零。
    pub fn destroy(&self) -> AppResult<()> {
        let mut guard = self.inner.lock();
        if matches!(&*guard, Inner::Disabled) {
            return Ok(());
        }
        if self.path.exists() {
            std::fs::remove_file(&self.path)
                .map_err(|e| AppError::Msg(format!("删除保险箱失败: {e}")))?;
        }
        *guard = Inner::Disabled;
        log::info!("主密码已关闭，回到明文存储");
        Ok(())
    }
}

fn write_file(path: &std::path::Path, file: &VaultFile) -> AppResult<()> {
    let text = serde_json::to_string_pretty(file)?;
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, text)?;
    std::fs::rename(&tmp, path)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_vault(name: &str) -> (Vault, PathBuf) {
        let dir = std::env::temp_dir().join(format!("snipforge-vault-test-{name}"));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("vault.json");
        (Vault::new(path.clone()), path)
    }

    #[test]
    fn roundtrip_and_wrong_password() {
        let (vault, _path) = temp_vault("roundtrip");
        vault.create("correct horse").unwrap();

        let before = vault.status();
        assert!(before.enabled && before.unlocked);

        vault
            .write_slots(&[(SLOT_SECRETS, b"[{\"name\":\"token\"}]".to_vec())])
            .unwrap();

        // 模拟重启：新建实例从磁盘读
        let vault2 = Vault::new(vault.path.clone());
        vault2.load();
        assert!(!vault2.is_unlocked(), "重启后必须是锁着的");

        assert!(vault2.unlock("wrong").is_err(), "错密码必须解不开");
        vault2.unlock("correct horse").unwrap();
        let got = vault2.read_slot(SLOT_SECRETS).unwrap().unwrap();
        assert_eq!(got, b"[{\"name\":\"token\"}]");
    }

    /// 没存过数据的库也必须能验证密码 —— check 槽就是为了这个
    #[test]
    fn empty_vault_still_verifies_password() {
        let (vault, _path) = temp_vault("empty");
        vault.create("pw-1").unwrap();

        let v2 = Vault::new(vault.path.clone());
        v2.load();
        assert!(v2.unlock("pw-2").is_err(), "空库也不能用任意密码解锁");
        assert!(v2.unlock("pw-1").is_ok());
    }

    #[test]
    fn change_password_keeps_data() {
        let (vault, _path) = temp_vault("change");
        vault.create("old").unwrap();
        vault
            .write_slots(&[(SLOT_PROVIDERS, b"{\"github\":{\"token\":\"t\"}}".to_vec())])
            .unwrap();
        vault.change_password("new").unwrap();

        let v2 = Vault::new(vault.path.clone());
        v2.load();
        assert!(v2.unlock("old").is_err());
        v2.unlock("new").unwrap();
        assert_eq!(
            v2.read_slot(SLOT_PROVIDERS).unwrap().unwrap(),
            b"{\"github\":{\"token\":\"t\"}}"
        );
    }

    /// 槽名进 AAD：把一个槽的密文挪到另一个槽，必须解不开
    #[test]
    fn slot_name_is_bound_to_ciphertext() {
        let (vault, path) = temp_vault("aad");
        vault.create("pw").unwrap();
        vault.write_slots(&[(SLOT_SECRETS, b"aaa".to_vec())]).unwrap();

        // 手工把 secrets 的密文塞进 providers
        let mut file: VaultFile =
            serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        let stolen = file.slots.get(SLOT_SECRETS).unwrap().clone();
        file.slots.insert(SLOT_PROVIDERS.into(), stolen);
        write_file(&path, &file).unwrap();

        let v2 = Vault::new(path);
        v2.load();
        v2.unlock("pw").unwrap();
        assert!(v2.read_slot(SLOT_PROVIDERS).is_err(), "换槽位必须解不开");
    }
}
