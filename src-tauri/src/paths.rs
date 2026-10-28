use std::path::PathBuf;

/// 全部落盘位置的唯一出口。
/// 启动时构造一次，之后只在 State 里被读取，避免各处自己拼路径拼歪。
#[derive(Debug, Clone)]
pub struct Paths {
    /// 用户数据根目录（%APPDATA%/fun.lxzt.snipforge）
    pub data: PathBuf,
    /// 剪贴板历史索引
    pub clipboard_index: PathBuf,
    /// 剪贴板图片 blob 目录
    pub blobs: PathBuf,
    /// 第三方插件目录
    pub plugins: PathBuf,
    /// 内置插件目录（随包分发，只读）
    pub builtin_plugins: PathBuf,
    pub logs: PathBuf,
    pub temp: PathBuf,
    /// 默认图片保存目录
    pub pictures: PathBuf,
}

impl Paths {
    pub fn new(data_dir: PathBuf, resource_dir: Option<PathBuf>, pictures: PathBuf) -> Self {
        // 开发模式直接用仓库里的 plugins/ 目录，改完重新加载就生效；
        // 打包后走 resources/plugins
        let builtin_plugins = if cfg!(debug_assertions) {
            std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .parent()
                .map(|p| p.join("plugins"))
                .unwrap_or_else(|| data_dir.join("builtin-plugins"))
        } else {
            // Tauri 打包时，`../plugins` 这类越出 crate 目录的资源会被放到 `_up_/` 下，
            // 两个位置都探一下，省得以后改配置又踩一次
            let mut found = data_dir.join("builtin-plugins");
            if let Some(res) = resource_dir {
                for candidate in [res.join("plugins"), res.join("_up_").join("plugins")] {
                    if candidate.exists() {
                        found = candidate;
                        break;
                    }
                }
            }
            found
        };

        Self {
            clipboard_index: data_dir.join("clipboard").join("history.json"),
            blobs: data_dir.join("clipboard").join("blobs"),
            plugins: data_dir.join("plugins"),
            builtin_plugins,
            logs: data_dir.join("logs"),
            temp: std::env::temp_dir().join("snipforge"),
            pictures,
            data: data_dir,
        }
    }

    pub fn ensure(&self) -> std::io::Result<()> {
        for dir in [
            &self.data,
            &self.blobs,
            &self.plugins,
            &self.logs,
            &self.temp,
            &self.pictures,
        ] {
            std::fs::create_dir_all(dir)?;
        }
        Ok(())
    }

    pub fn settings_file(&self) -> PathBuf {
        self.data.join("settings.json")
    }

    /// 上传历史（图床页的"上传记录"）
    pub fn uploads_file(&self) -> PathBuf {
        self.data.join("uploads.json")
    }
}
