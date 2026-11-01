//! 声明式图床：`*.uploader.json` → 一个可用的 Provider。
//!
//! 文件放 `%APPDATA%/fun.lxzt.snipforge/plugins/` 下即可生效，改完点"重新加载"。
//! 示例见项目 `plugins/` 目录。

use crate::state::AppState;
use crate::types::{ConfigField, FieldOption, ProviderMeta};
use crate::uploader::providers::DeclarativeProvider;
use crate::uploader::UploaderRegistry;
use std::sync::Arc;
use tauri::{AppHandle, Manager};

/// 一个声明式图床文件的完整结构。
///
/// 只声明**会被用到**的字段：`version` / `author` / `hooks` 以前也解析进来了，
/// 但没有任何代码读它们（插件页展示的是原始 JSON，不走这个结构体）。
/// 声明式插件没有脚本能力，挂不了钩子 —— 写进 JSON 也不会生效，
/// 与其让作者以为能用，不如根本不声明。
#[derive(Debug, Clone, serde::Deserialize)]
pub struct UploaderSpec {
    pub id: String,
    pub name: String,
    pub description: Option<String>,
    /// 图床定义本体
    pub provider: ProviderSpec,
}

#[derive(Debug, Clone, serde::Deserialize)]
pub struct ProviderSpec {
    pub id: String,
    pub name: String,
    pub url: String,
    #[serde(default = "default_method")]
    pub method: String,
    #[serde(default = "default_field")]
    pub field_name: String,
    #[serde(default)]
    pub response_path: String,
    #[serde(default)]
    pub url_prefix: String,
    #[serde(default)]
    pub headers: std::collections::HashMap<String, String>,
    #[serde(default)]
    pub fields: std::collections::HashMap<String, String>,
    #[serde(default)]
    pub config_fields: Vec<SpecField>,
}

fn default_method() -> String {
    "POST".into()
}
fn default_field() -> String {
    "file".into()
}

#[derive(Debug, Clone, serde::Deserialize)]
pub struct SpecField {
    pub key: String,
    pub label: String,
    #[serde(default = "default_type")]
    pub r#type: String,
    #[serde(default)]
    pub placeholder: Option<String>,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub required: bool,
    #[serde(default)]
    pub options: Option<Vec<SpecOption>>,
}

fn default_type() -> String {
    "string".into()
}

#[derive(Debug, Clone, serde::Deserialize)]
pub struct SpecOption {
    pub label: String,
    pub value: String,
}

/// 扫描目录并注册全部声明式图床，返回成功加载的数量
pub fn load_all(app: &AppHandle, state: &Arc<AppState>) -> usize {
    let Some(registry) = app.try_state::<Arc<UploaderRegistry>>() else {
        log::warn!("图床注册表未初始化，跳过声明式图床加载");
        return 0;
    };

    let mut count = 0;
    for dir in [state.paths.builtin_plugins.clone(), state.paths.plugins.clone()] {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if !path.is_file() {
                continue;
            }
            let name = path.file_name().and_then(|n| n.to_str()).unwrap_or_default();
            if !name.ends_with(".uploader.json") {
                continue;
            }

            match std::fs::read_to_string(&path)
                .map_err(|e| e.to_string())
                .and_then(|txt| serde_json::from_str::<UploaderSpec>(&txt).map_err(|e| e.to_string()))
            {
                Ok(spec) => {
                    let enabled = state
                        .settings
                        .read()
                        .plugin_enabled
                        .get(&spec.id)
                        .copied()
                        .unwrap_or(true);
                    if !enabled {
                        log::debug!("声明式图床 {} 已禁用，跳过", spec.id);
                        continue;
                    }

                    let fields: Vec<ConfigField> = spec
                        .provider
                        .config_fields
                        .iter()
                        .map(|cf| ConfigField {
                            key: cf.key.clone(),
                            label: cf.label.clone(),
                            field_type: cf.r#type.clone(),
                            placeholder: cf.placeholder.clone(),
                            description: cf.description.clone(),
                            required: cf.required,
                            default: None,
                            options: cf.options.as_ref().map(|opts| {
                                opts.iter()
                                    .map(|o| FieldOption {
                                        label: o.label.clone(),
                                        value: o.value.clone(),
                                    })
                                    .collect()
                            }),
                            visible_when: None,
                        })
                        .collect();

                    let meta = ProviderMeta {
                        id: spec.provider.id.clone(),
                        name: spec.provider.name.clone(),
                        origin: "declarative".into(),
                        description: spec.description.clone(),
                        requires_network: true,
                        supports_dir: false,
                        can_browse: false,
                        can_list_images: false,
                        fields,
                    };

                    let payload = serde_json::json!({
                        "url": spec.provider.url,
                        "method": spec.provider.method,
                        "fieldName": spec.provider.field_name,
                        "responsePath": spec.provider.response_path,
                        "urlPrefix": spec.provider.url_prefix,
                        "headers": spec.provider.headers,
                        "fields": spec.provider.fields,
                    });

                    registry.register(Arc::new(DeclarativeProvider {
                        meta,
                        spec: payload,
                    }));
                    log::info!("声明式图床已加载: {} ({})", spec.name, spec.provider.id);
                    count += 1;
                }
                Err(e) => log::warn!("解析 {} 失败: {e}", path.display()),
            }
        }
    }
    count
}

/// 卸载所有声明式图床（重新加载时先清空，避免残留旧定义）
pub fn unload_all(registry: &Arc<UploaderRegistry>, _state: &Arc<AppState>) {
    let ids: Vec<String> = registry
        .list()
        .into_iter()
        .filter(|m| m.origin == "declarative")
        .map(|m| m.id)
        .collect();
    for id in ids {
        registry.unregister(&id);
    }
}
