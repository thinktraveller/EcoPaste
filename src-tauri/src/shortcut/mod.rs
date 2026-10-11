//! 全局快捷键：注册由 Rust 主导，前端在偏好设置里改完通过 `update_settings` 触发重注册。
//!
//! 配置来自 `settings::Shortcuts`；负责全局注册、生效状态与当前剪贴板转换分发。

use std::collections::HashSet;
use std::sync::Mutex;
use std::time::Duration;

use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager};
use tauri_plugin_global_shortcut::{GlobalShortcutExt, Shortcut, ShortcutEvent, ShortcutState};

use crate::core::{AppError, Result};
use crate::settings::{Settings, SettingsStore, Shortcuts};
use crate::window::{self, CLIPBOARD_WINDOW_LABEL, PREFERENCE_WINDOW_LABEL};

#[cfg(target_os = "windows")]
mod win_v;

pub const CONFLICT_EVENT: &str = "shortcut://conflict";
pub const STATUS_EVENT: &str = "shortcut://status";
pub const ACTION_OPEN_CLIPBOARD: &str = "open_clipboard";
pub const ACTION_OPEN_PREFERENCE: &str = "open_preference";
pub const ACTION_CONVERT_TO_PLAIN_TEXT: &str = "convert_to_plain_text";
const RESUME_DEBOUNCE: Duration = Duration::from_millis(160);

#[derive(Debug, Clone, Serialize)]
pub struct ShortcutConflict {
    pub action: &'static str,
    pub binding: String,
    pub reason: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ShortcutStatus {
    pub action: &'static str,
    pub binding: String,
    pub active: bool,
    pub error: Option<String>,
}

#[derive(Default)]
pub struct ShortcutManager {
    active: Mutex<Vec<(&'static str, Shortcut)>>,
    pause: Mutex<ShortcutPause>,
    status: Mutex<Vec<ShortcutStatus>>,
    pressed: Mutex<HashSet<&'static str>>,
    operation: Mutex<()>,
}

#[derive(Default)]
struct ShortcutPause {
    resume_epoch: u64,
    suspend_count: usize,
}

impl ShortcutPause {
    /// 记录一次暂停请求，并返回是否需要实际注销已注册快捷键。
    fn suspend(&mut self) -> bool {
        self.resume_epoch += 1;
        self.suspend_count += 1;

        self.suspend_count == 1
    }

    /// 释放一次暂停请求；返回 `None` 表示没有对应的暂停可释放。
    fn resume(&mut self) -> Option<bool> {
        if self.suspend_count == 0 {
            return None;
        }

        self.suspend_count -= 1;

        Some(self.suspend_count == 0)
    }

    /// 开启新的恢复世代，用于让更早的延迟恢复任务失效。
    fn next_resume_epoch(&mut self) -> u64 {
        self.resume_epoch += 1;
        self.resume_epoch
    }

    /// 判断延迟恢复任务是否仍对当前暂停状态有效。
    fn allows_resume(&self, epoch: u64) -> bool {
        self.resume_epoch == epoch && self.suspend_count == 0
    }

    /// 判断全局快捷键是否处于暂停态。
    fn suspended(&self) -> bool {
        self.suspend_count > 0
    }
}

pub fn init(app: &AppHandle, shortcuts: &Shortcuts) -> Result<()> {
    app.manage(ShortcutManager::default());
    if let Err(err) = apply(app, shortcuts) {
        log::warn!("initial shortcut registration incomplete: {err}");
    }
    Ok(())
}

pub fn status(app: &AppHandle) -> Vec<ShortcutStatus> {
    let mut statuses = app
        .state::<ShortcutManager>()
        .status
        .lock()
        .expect("shortcut state poisoned")
        .clone();
    for action in app.state::<SettingsStore>().shortcut_diagnostics() {
        let error = Some(
            crate::i18n::shortcuts::validation_error(
                crate::i18n::current_language(app),
                "loaded binding disabled",
            )
            .into(),
        );
        if let Some(item) = statuses.iter_mut().find(|item| item.action == action) {
            item.error = error;
        } else {
            statuses.push(ShortcutStatus {
                action,
                binding: String::new(),
                active: false,
                error,
            });
        }
    }
    statuses
}

/// 暂停所有已注册的全局快捷键；用于前端录入快捷键期间避免旧绑定被触发。
pub fn suspend(app: &AppHandle) -> Result<()> {
    let manager = app.state::<ShortcutManager>();
    let should_unregister = {
        let mut pause = manager.pause.lock().expect("shortcut state poisoned");
        pause.suspend()
    };

    if should_unregister {
        let _operation = manager
            .operation
            .lock()
            .expect("shortcut operation poisoned");
        if let Err(err) = unregister_active(app) {
            let should_schedule = manager
                .pause
                .lock()
                .expect("shortcut state poisoned")
                .resume()
                .unwrap_or(false);
            if should_schedule {
                schedule_resume(app);
            }
            return Err(err);
        }
    }

    Ok(())
}

/// 释放一次全局快捷键暂停；只有所有录入器都结束后才按最新设置恢复注册。
pub fn resume(app: &AppHandle) -> Result<()> {
    let manager = app.state::<ShortcutManager>();
    let should_schedule = {
        let mut pause = manager.pause.lock().expect("shortcut state poisoned");

        match pause.resume() {
            Some(should_schedule) => should_schedule,
            None => {
                log::warn!("resume global shortcuts called without active suspend");

                return Ok(());
            }
        }
    };

    if should_schedule {
        schedule_resume(app);
    }

    Ok(())
}

/// 全量替换并记录逐项生效状态；部分注册失败保留成功项并返回错误。
pub fn apply(app: &AppHandle, shortcuts: &Shortcuts) -> Result<()> {
    let manager = app.state::<ShortcutManager>();
    let _operation = manager
        .operation
        .lock()
        .expect("shortcut operation poisoned");
    apply_inner(app, shortcuts, false)
}

/// Serialize native registration and persistence; failed preparation restores the previous intent.
pub fn update_settings(app: &AppHandle, patch: serde_json::Value) -> Result<Settings> {
    let store = app.state::<SettingsStore>();
    let _settings_operation = store.begin_update();
    let previous = store.snapshot();
    let candidate = store.prepare_update(patch.clone())?;
    let manager = app.state::<ShortcutManager>();
    let _operation = manager
        .operation
        .lock()
        .expect("shortcut operation poisoned");
    let global_bindings_changed = candidate.shortcuts.open_clipboard
        != previous.shortcuts.open_clipboard
        || candidate.shortcuts.open_preference != previous.shortcuts.open_preference
        || candidate.shortcuts.convert_to_plain_text != previous.shortcuts.convert_to_plain_text;
    if !global_bindings_changed {
        let next = store.commit_snapshot_with_patch(candidate, &patch)?;
        store.acknowledge_shortcut_patch(&patch);
        sync_win_v(app, &next.shortcuts);
        let _ = app.emit(STATUS_EVENT, status(app));
        return Ok(next);
    }
    let preparation = apply_inner(app, &candidate.shortcuts, true);
    let preparation = if is_suspended(app) {
        unregister_active(app).and(preparation)
    } else {
        preparation
    };
    if let Err(err) = preparation {
        if let Err(restore_error) = apply_inner(app, &previous.shortcuts, false) {
            log::error!("restore previous shortcut registration failed: {restore_error}");
        }
        return Err(err);
    }
    match store.commit_snapshot_with_patch(candidate, &patch) {
        Ok(next) => {
            store.acknowledge_shortcut_patch(&patch);
            sync_win_v(app, &next.shortcuts);
            let _ = app.emit(STATUS_EVENT, status(app));
            Ok(next)
        }
        Err(err) => {
            if let Err(restore_error) = apply_inner(app, &previous.shortcuts, false) {
                log::error!("restore shortcuts after persistence failure failed: {restore_error}");
            }
            Err(err)
        }
    }
}

fn apply_inner(app: &AppHandle, shortcuts: &Shortcuts, ignore_pause: bool) -> Result<()> {
    unregister_active(app)?;

    if is_suspended(app) && !ignore_pause {
        return Ok(());
    }

    let desired: [(&'static str, &str); 3] = [
        (ACTION_OPEN_CLIPBOARD, &shortcuts.open_clipboard),
        (ACTION_OPEN_PREFERENCE, &shortcuts.open_preference),
        (
            ACTION_CONVERT_TO_PLAIN_TEXT,
            &shortcuts.convert_to_plain_text,
        ),
    ];

    if !ignore_pause {
        sync_win_v(app, shortcuts);
    }

    let mut active = Vec::new();
    let mut statuses = Vec::new();
    let mut failure = None;
    for (action, binding) in desired {
        if binding.trim().is_empty() {
            statuses.push(ShortcutStatus {
                action,
                binding: binding.into(),
                active: false,
                error: None,
            });
            continue;
        }
        match register_one(app, action, binding) {
            Ok(shortcut) => {
                active.push((action, shortcut));
                statuses.push(ShortcutStatus {
                    action,
                    binding: binding.into(),
                    active: true,
                    error: None,
                });
            }
            Err(err) => {
                log::warn!("register shortcut {action}={binding} failed: {err}");
                failure = Some(err.to_string());
                statuses.push(ShortcutStatus {
                    action,
                    binding: binding.into(),
                    active: false,
                    error: failure.clone(),
                });
                let _ = app.emit(
                    CONFLICT_EVENT,
                    ShortcutConflict {
                        action,
                        binding: binding.into(),
                        reason: err.to_string(),
                    },
                );
            }
        }
    }

    *app.state::<ShortcutManager>()
        .active
        .lock()
        .expect("shortcut state poisoned") = active;
    *app.state::<ShortcutManager>()
        .status
        .lock()
        .expect("shortcut state poisoned") = statuses;
    let _ = app.emit(STATUS_EVENT, status(app));
    match failure {
        Some(error) => Err(AppError::Other(anyhow::anyhow!(error))),
        None => Ok(()),
    }
}

/// 判断是否仍有录入器持有全局快捷键暂停。
fn is_suspended(app: &AppHandle) -> bool {
    app.state::<ShortcutManager>()
        .pause
        .lock()
        .expect("shortcut state poisoned")
        .suspended()
}

/// 延迟恢复全局快捷键；若用户立即切到另一个录入器，新 suspend 会让本次恢复失效。
fn schedule_resume(app: &AppHandle) {
    let app = app.clone();
    let epoch = {
        let manager = app.state::<ShortcutManager>();
        let mut pause = manager.pause.lock().expect("shortcut state poisoned");
        pause.next_resume_epoch()
    };

    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(RESUME_DEBOUNCE).await;

        if !should_run_scheduled_resume(&app, epoch) {
            return;
        }

        let manager = app.state::<ShortcutManager>();
        let _operation = manager
            .operation
            .lock()
            .expect("shortcut operation poisoned");
        if !should_run_scheduled_resume(&app, epoch) {
            return;
        }
        let settings = app.state::<SettingsStore>().snapshot();
        if let Err(err) = apply_inner(&app, &settings.shortcuts, false) {
            log::warn!("resume delayed global shortcuts failed: {err}");
        }
    });
}

/// 只有仍处于同一恢复世代且暂停计数为 0 时，延迟恢复任务才允许执行。
fn should_run_scheduled_resume(app: &AppHandle, epoch: u64) -> bool {
    let manager = app.state::<ShortcutManager>();
    let pause = manager.pause.lock().expect("shortcut state poisoned");

    pause.allows_resume(epoch)
}

/// 取消当前轮所有已注册快捷键，并清空内部 active 状态。
fn unregister_active(app: &AppHandle) -> Result<()> {
    #[cfg(target_os = "windows")]
    win_v::set_enabled(app, false);
    let plugin = app.global_shortcut();
    let manager = app.state::<ShortcutManager>();

    let previous = {
        let mut guard = manager.active.lock().expect("shortcut state poisoned");
        std::mem::take(&mut *guard)
    };
    let mut remaining = Vec::new();
    for (action, shortcut) in &previous {
        if let Err(err) = plugin.unregister(*shortcut) {
            log::warn!("unregister previous shortcut failed: {err:?}");
            remaining.push((*action, *shortcut));
        }
    }
    let failed = !remaining.is_empty();
    *manager.active.lock().expect("shortcut state poisoned") = remaining;
    manager
        .pressed
        .lock()
        .expect("shortcut state poisoned")
        .clear();
    let remaining_actions: HashSet<_> = manager
        .active
        .lock()
        .expect("shortcut state poisoned")
        .iter()
        .map(|(action, _)| *action)
        .collect();
    for item in manager
        .status
        .lock()
        .expect("shortcut state poisoned")
        .iter_mut()
    {
        item.active = remaining_actions.contains(item.action);
        if item.active {
            item.error = Some(
                crate::i18n::shortcuts::validation_error(
                    crate::i18n::current_language(app),
                    "unregister",
                )
                .into(),
            );
        }
    }
    let _ = app.emit(STATUS_EVENT, status(app));
    if failed {
        Err(AppError::Other(anyhow::anyhow!(
            crate::i18n::shortcuts::validation_error(
                crate::i18n::current_language(app),
                "unregister"
            )
        )))
    } else {
        Ok(())
    }
}

/// Synchronize the separate Windows Win+V hook only after a settings commit or resume.
pub fn sync_win_v(app: &AppHandle, shortcuts: &Shortcuts) {
    #[cfg(target_os = "windows")]
    win_v::set_enabled(app, shortcuts.win_v && !is_suspended(app));
    #[cfg(not(target_os = "windows"))]
    let _ = (app, shortcuts);
}

fn register_one(app: &AppHandle, action: &'static str, binding: &str) -> Result<Shortcut> {
    let plugin = app.global_shortcut();
    let canonical = crate::settings::shortcuts::Binding::parse(binding)
        .map(|value| value.canonical())
        .unwrap_or_else(|| binding.into());
    let shortcut: Shortcut = canonical.parse().map_err(|err| {
        log::warn!("parse shortcut {binding}: {err}");
        AppError::Other(anyhow::anyhow!(crate::i18n::shortcuts::validation_error(
            crate::i18n::current_language(app),
            "unsupported global shortcut"
        )))
    })?;

    if plugin.is_registered(shortcut) {
        plugin
            .unregister(shortcut)
            .map_err(|err| AppError::Other(anyhow::anyhow!(err)))?;
    }

    plugin
        .on_shortcut(shortcut, move |app, _scut, event| {
            handle_event(app, action, event);
        })
        .map_err(|err| AppError::Other(anyhow::anyhow!(err)))?;
    Ok(shortcut)
}

fn handle_event(app: &AppHandle, action: &'static str, event: ShortcutEvent) {
    let manager = app.state::<ShortcutManager>();
    if matches!(event.state(), ShortcutState::Released) {
        manager
            .pressed
            .lock()
            .expect("shortcut state poisoned")
            .remove(action);
        return;
    }
    // Pressed 触发一次即可（Released 是按键松开），避免 toggle 在按下/松开各执行一次回弹。
    if !matches!(event.state(), ShortcutState::Pressed) {
        return;
    }
    if is_suspended(app)
        || !manager
            .pressed
            .lock()
            .expect("shortcut state poisoned")
            .insert(action)
    {
        return;
    }
    if action == ACTION_CONVERT_TO_PLAIN_TEXT {
        let app = app.clone();
        tauri::async_runtime::spawn_blocking(move || {
            use crate::clipboard::{
                convert_current_clipboard_with_feedback, PlainTextConversionResult,
            };
            use tauri_plugin_dialog::DialogExt;
            let lang = crate::i18n::current_language(&app);
            let message = match convert_current_clipboard_with_feedback(&app) {
                Ok(PlainTextConversionResult::Unsupported) => {
                    Some(crate::i18n::shortcuts::unsupported(lang).to_owned())
                }
                Ok(PlainTextConversionResult::Changed) => {
                    Some(crate::i18n::shortcuts::changed(lang).to_owned())
                }
                Err(AppError::Other(err)) => Some(err.to_string()),
                Err(err) => {
                    log::warn!("global clipboard conversion failed: {err}");
                    Some(crate::i18n::shortcuts::failed(lang).to_owned())
                }
                _ => None,
            };
            if let Some(message) = message {
                app.dialog()
                    .message(message)
                    .title(crate::i18n::shortcuts::title(lang))
                    .show(|_| {});
            }
        });
        return;
    }
    let label = match action {
        ACTION_OPEN_CLIPBOARD => CLIPBOARD_WINDOW_LABEL,
        ACTION_OPEN_PREFERENCE => PREFERENCE_WINDOW_LABEL,
        _ => return,
    };
    if let Err(err) = window::toggle_window(app, label) {
        log::warn!("toggle window via shortcut {action} failed: {err}");
    }
}
