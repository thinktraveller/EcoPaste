//! 设置持久化。
//!
//! - 落盘位置：`<app_data_dir>/config/settings.json`（dev/prod 由 `core::paths` 的环境子目录隔离）。
//! - 写入流程：先写到 `settings.json.tmp`，再原子替换主文件，避免中途断电留下半截 JSON。
//! - 缺字段兼容：`Settings` 各结构体都 `#[serde(default)]`，新版本新增字段不影响旧文件。

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard, RwLock};

use anyhow::Context;
use tauri::AppHandle;

use crate::core::{AppError, Result};

use super::model::{
    Language, Settings, WINDOW_OPEN_GROUP_PREFIX, WINDOW_OPEN_SELECTION_ALL,
    WINDOW_OPEN_SELECTION_PRESERVE,
};

const FILENAME: &str = "settings.json";

pub struct SettingsStore {
    path: RwLock<PathBuf>,
    current: RwLock<Settings>,
    operation: Mutex<()>,
    shortcut_diagnostics: RwLock<Vec<&'static str>>,
}

impl SettingsStore {
    pub fn new(app: &AppHandle) -> Result<Self> {
        let dir = crate::core::paths::config_dir(app)?;
        fs::create_dir_all(&dir).with_context(|| format!("failed to create dir at {dir:?}"))?;

        let path = dir.join(FILENAME);

        let (current, diagnostics) = match load_from_disk(&path) {
            Some(settings) => settings,
            None => {
                // 真·首次启动：用系统 locale 推导默认语言并落盘，之后所有读取都走常规分支。
                let settings = default_settings_with_system_locale();
                if let Err(err) = write_atomic(&path, &settings) {
                    log::warn!("persist first-run settings failed: {err}");
                }
                (settings, Vec::new())
            }
        };
        log::info!("settings store ready at {path:?}");

        Ok(Self {
            path: RwLock::new(path),
            current: RwLock::new(current),
            operation: Mutex::new(()),
            shortcut_diagnostics: RwLock::new(diagnostics),
        })
    }

    pub fn snapshot(&self) -> Settings {
        self.current.read().expect("settings poisoned").clone()
    }

    /// Return session-only disabled binding diagnostics; never persist them as settings.
    pub fn shortcut_diagnostics(&self) -> Vec<&'static str> {
        self.shortcut_diagnostics
            .read()
            .expect("shortcut diagnostics poisoned")
            .clone()
    }

    /// Clear an acknowledged load warning only after its explicit setting write succeeds.
    pub fn acknowledge_shortcut_patch(&self, patch: &serde_json::Value) {
        let Some(shortcuts) = patch.get("shortcuts") else {
            return;
        };
        self.shortcut_diagnostics
            .write()
            .expect("shortcut diagnostics poisoned")
            .retain(|action| {
                if *action == crate::shortcut::ACTION_CONVERT_TO_PLAIN_TEXT {
                    shortcuts.get("convertToPlainText").is_none()
                } else {
                    shortcuts
                        .get("windowActions")
                        .and_then(|value| value.get(*action))
                        .is_none()
                }
            });
    }

    /// 恢复默认设置并落盘，返回新的完整快照。
    pub fn reset(&self) -> Result<Settings> {
        let _operation = self.begin_update();
        let next = default_settings_with_system_locale();

        let path = self.path();
        write_atomic(&path, &next)?;
        *self.current.write().expect("settings poisoned") = next.clone();
        self.shortcut_diagnostics
            .write()
            .expect("shortcut diagnostics poisoned")
            .clear();
        Ok(next)
    }

    /// 用 JSON patch 深度合并到当前设置，落盘后返回新快照。
    /// patch 必须是 object；非 object 视为「整个替换」语义不友好，直接报错。
    pub fn update(&self, patch: serde_json::Value) -> Result<Settings> {
        let _operation = self.begin_update();
        let next = self.prepare_update(patch.clone())?;
        let next = self.commit_snapshot(next)?;
        self.acknowledge_shortcut_patch(&patch);
        Ok(next)
    }

    /// Validate a candidate before any native registration or persisted state changes.
    pub fn prepare_update(&self, patch: serde_json::Value) -> Result<Settings> {
        super::shortcuts::validate_shortcut_json(&patch, self.snapshot().appearance.language)?;
        if !patch.is_object() {
            return Err(AppError::Other(anyhow::anyhow!(
                "settings patch must be a JSON object"
            )));
        }

        let guard = self.current.read().expect("settings poisoned");

        let mut merged = serde_json::to_value(&*guard)
            .context("failed to serialize current settings for merge")?;
        deep_merge(&mut merged, patch);

        let next: Settings = serde_json::from_value(merged)
            .map_err(|err| AppError::Other(anyhow::anyhow!("invalid settings patch: {err}")))?;

        validate_settings(&next)?;

        Ok(next)
    }

    /// Serialize settings read-modify-write operations, including prepared native registration.
    pub fn begin_update(&self) -> MutexGuard<'_, ()> {
        self.operation.lock().expect("settings operation poisoned")
    }

    /// Commit an already validated snapshot after native side effects are prepared.
    pub fn commit_snapshot(&self, next: Settings) -> Result<Settings> {
        validate_settings(&next)?;
        write_atomic(&self.path(), &next)?;
        *self.current.write().expect("settings poisoned") = next.clone();
        self.shortcut_diagnostics
            .write()
            .expect("shortcut diagnostics poisoned")
            .retain(|action| {
                if *action == crate::shortcut::ACTION_CONVERT_TO_PLAIN_TEXT {
                    next.shortcuts.convert_to_plain_text.is_empty()
                } else {
                    next.shortcuts
                        .window_actions
                        .get(*action)
                        .is_none_or(String::is_empty)
                }
            });
        Ok(next)
    }

    /// 用完整设置文件替换当前设置；覆盖导入专用。
    pub fn replace_from_file(&self, path: &Path) -> Result<Settings> {
        let _operation = self.begin_update();
        let content =
            fs::read_to_string(path).with_context(|| format!("failed to read {path:?}"))?;
        let raw: serde_json::Value = serde_json::from_str(&content)
            .map_err(|err| AppError::Other(anyhow::anyhow!("invalid settings file: {err}")))?;
        super::shortcuts::validate_shortcut_json(&raw, self.snapshot().appearance.language)?;
        let next: Settings = serde_json::from_str(&content)
            .map_err(|err| AppError::Other(anyhow::anyhow!("invalid settings file: {err}")))?;

        validate_settings(&next)?;

        let path = self.path();
        write_atomic(&path, &next)?;
        *self.current.write().expect("settings poisoned") = next.clone();
        self.shortcut_diagnostics
            .write()
            .expect("shortcut diagnostics poisoned")
            .clear();
        Ok(next)
    }

    /// 数据目录热切换后重新绑定设置文件，并把新路径里的设置加载进内存。
    pub fn rebase(&self, app: &AppHandle) -> Result<Settings> {
        let _operation = self.begin_update();
        let dir = crate::core::paths::config_dir(app)?;
        fs::create_dir_all(&dir).with_context(|| format!("failed to create dir at {dir:?}"))?;
        let path = dir.join(FILENAME);
        let (current, diagnostics) = match load_from_disk(&path) {
            Some(settings) => settings,
            None => {
                let settings = default_settings_with_system_locale();
                write_atomic(&path, &settings)?;
                (settings, Vec::new())
            }
        };

        *self.path.write().expect("settings path poisoned") = path;
        *self.current.write().expect("settings poisoned") = current.clone();
        *self
            .shortcut_diagnostics
            .write()
            .expect("shortcut diagnostics poisoned") = diagnostics;
        Ok(current)
    }

    fn path(&self) -> PathBuf {
        self.path.read().expect("settings path poisoned").clone()
    }
}

/// 生成默认设置，并沿用首次启动的系统语言推导规则。
fn default_settings_with_system_locale() -> Settings {
    let mut settings = Settings::default();
    if let Some(tag) = tauri_plugin_os::locale() {
        settings.appearance.language = Language::from_system_locale(&tag);
        log::info!(
            "default settings language from locale {tag}: {:?}",
            settings.appearance.language
        );
    }
    settings
}

/// 返回 `None` 表示主文件不存在（首次启动），调用方据此走「初始化默认」分支；
/// 读取过程中遇到 IO/解析错误会打 warn，并返回 `Settings::default()` 包装在 `Some` 里——
/// 这条路径表示「文件存在但坏了」，不要当成首次启动覆盖系统 locale。
fn load_from_disk(path: &Path) -> Option<(Settings, Vec<&'static str>)> {
    if !path.exists() {
        return None;
    }

    match fs::read_to_string(path).and_then(|content| {
        let raw: serde_json::Value = serde_json::from_str(&content)
            .map_err(|err| std::io::Error::new(std::io::ErrorKind::InvalidData, err))?;
        serde_json::from_value::<Settings>(raw.clone())
            .map(|settings| (settings, super::shortcuts::malformed_loaded_shortcuts(&raw)))
            .map_err(|err| std::io::Error::new(std::io::ErrorKind::InvalidData, err))
    }) {
        Ok((mut settings, mut diagnostics)) => {
            diagnostics.extend(super::shortcuts::sanitize_loaded_shortcuts(
                &mut settings.shortcuts,
            ));
            diagnostics.sort_unstable();
            diagnostics.dedup();
            Some((settings, diagnostics))
        }
        Err(err) => {
            log::warn!("settings file {path:?} unreadable, using defaults: {err}");
            Some((Settings::default(), Vec::new()))
        }
    }
}

/// 写入策略：把新内容写到 tmp 后 rename 成主文件；rename 在同一文件系统下是原子的。
fn write_atomic(path: &Path, settings: &Settings) -> Result<()> {
    let json = serde_json::to_string_pretty(settings).context("failed to serialize settings")?;

    let tmp = path.with_extension("json.tmp");
    {
        let mut file = fs::File::create(&tmp)
            .with_context(|| format!("failed to create tmp settings at {tmp:?}"))?;
        file.write_all(json.as_bytes())
            .with_context(|| format!("failed to write tmp settings at {tmp:?}"))?;
        file.sync_all().ok();
    }
    fs::rename(&tmp, path)
        .with_context(|| format!("failed to promote tmp settings to {path:?}"))?;
    Ok(())
}

/// 校验设置之间的跨字段约束，避免非法配置写入磁盘。
fn validate_settings(settings: &Settings) -> Result<()> {
    validate_window_open_group(&settings.clipboard.window.select_group_on_open)?;

    super::shortcuts::validate_shortcuts(&settings.shortcuts, settings.appearance.language)
}

/// 校验打开剪贴板窗口时选中分组的字符串编码，避免非法设置值落盘。
fn validate_window_open_group(value: &str) -> Result<()> {
    if value == WINDOW_OPEN_SELECTION_PRESERVE || value == WINDOW_OPEN_SELECTION_ALL {
        return Ok(());
    }

    let Some(group_id) = value.strip_prefix(WINDOW_OPEN_GROUP_PREFIX) else {
        return Err(AppError::Other(anyhow::anyhow!(
            "open group selection is invalid"
        )));
    };

    if group_id.trim().is_empty() {
        return Err(AppError::Other(anyhow::anyhow!(
            "open group selection is invalid"
        )));
    }

    Ok(())
}

/// 深合并对象，数组和叶节点按整体替换。
fn deep_merge(base: &mut serde_json::Value, patch: serde_json::Value) {
    match (base, patch) {
        (serde_json::Value::Object(base_map), serde_json::Value::Object(patch_map)) => {
            for (k, v) in patch_map {
                match base_map.get_mut(&k) {
                    Some(existing) if existing.is_object() && v.is_object() => {
                        deep_merge(existing, v);
                    }
                    _ => {
                        base_map.insert(k, v);
                    }
                }
            }
        }
        (slot, patch) => {
            *slot = patch;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deep_merge_overrides_leaves_and_recurses_objects() {
        let mut base = serde_json::json!({
            "general": {"autoStart": false, "trayIcon": true},
            "clipboard": {"history": {"maxCount": 0}},
        });
        let patch = serde_json::json!({
            "general": {"autoStart": true},
            "clipboard": {"history": {"maxCount": 500}},
        });
        deep_merge(&mut base, patch);
        assert_eq!(
            base,
            serde_json::json!({
                "general": {"autoStart": true, "trayIcon": true},
                "clipboard": {"history": {"maxCount": 500}},
            })
        );
    }

    #[test]
    fn deep_merge_replaces_arrays_wholesale() {
        let mut base = serde_json::json!({"itemActions": ["copy", "star", "delete"]});
        let patch = serde_json::json!({"itemActions": ["copy", "pastePlain"]});
        deep_merge(&mut base, patch);
        assert_eq!(
            base,
            serde_json::json!({"itemActions": ["copy", "pastePlain"]})
        );
    }

    #[test]
    fn validate_settings_rejects_duplicate_global_shortcuts() {
        let mut settings = Settings::default();
        settings.shortcuts.open_preference = settings.shortcuts.open_clipboard.clone();

        assert!(validate_settings(&settings).is_err());
    }

    #[test]
    fn validate_settings_allows_empty_global_shortcuts() {
        let mut settings = Settings::default();
        settings.shortcuts.open_preference = String::new();

        assert!(validate_settings(&settings).is_ok());
    }

    #[test]
    fn missing_fields_fall_back_to_defaults() {
        let partial = r#"{"general": {"autoStart": true}}"#;
        let parsed: Settings = serde_json::from_str(partial).unwrap();
        assert!(parsed.general.auto_start);
        assert!(!parsed.general.run_as_admin);
        assert!(parsed.general.tray_icon, "default kept");
        assert_eq!(parsed.shortcuts.open_clipboard, "Alt+C");
        assert_eq!(
            parsed.update.frequency,
            crate::settings::UpdateFrequency::Daily
        );
        assert_eq!(
            parsed.clipboard.content.sort,
            crate::db::models::ClipboardItemSort::UpdatedAt
        );
        assert!(!parsed.clipboard.content.copy_then_hide_window);
        assert!(
            parsed
                .clipboard
                .content
                .delete_favorite_items_only_in_favorite_group
        );
        assert!(!parsed.clipboard.content.delete_favorite_items);
        assert!(parsed.clipboard.content.delete_favorite_confirm);
        assert!(!parsed.clipboard.content.delete_pinned_items);
        assert!(parsed.clipboard.content.delete_pinned_confirm);
        assert!(!parsed.clipboard.content.update_on_reuse);
        assert_eq!(parsed.clipboard.history.cleanup_interval_hours, 0);
        assert!(parsed.clipboard.window.scroll_to_top_on_open);
        assert_eq!(
            parsed.clipboard.window.select_range_on_open,
            crate::settings::WindowOpenRangeSelection::Preserve
        );
        assert_eq!(
            parsed.clipboard.window.select_category_on_open,
            crate::settings::WindowOpenCategorySelection::Preserve
        );
        assert_eq!(
            parsed.clipboard.window.select_group_on_open,
            crate::settings::WINDOW_OPEN_SELECTION_PRESERVE
        );
    }

    #[test]
    fn validate_settings_rejects_invalid_open_group_selection() {
        let mut settings = Settings::default();
        settings.clipboard.window.select_group_on_open = "invalid".to_owned();

        assert!(validate_settings(&settings).is_err());
    }
}
