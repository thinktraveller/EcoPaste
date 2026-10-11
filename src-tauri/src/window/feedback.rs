//! 全局转换成功的非聚焦浮层：前端渲染确认后显示，计时由 Rust 管理。

use std::sync::{
    atomic::{AtomicU64, Ordering},
    Mutex,
};
use std::time::{Duration, Instant};

use tauri::{AppHandle, Emitter, Manager, PhysicalPosition, WebviewUrl, WebviewWindowBuilder};

use super::CONVERSION_FEEDBACK_WINDOW_LABEL;
use crate::core::Result;

const FEEDBACK_EVENT: &str = "clipboard://conversion-feedback";
const FEEDBACK_DURATION: Duration = Duration::from_secs(1);
const MAX_READY_WAIT: Duration = Duration::from_secs(10);
const FEEDBACK_WIDTH: f64 = 320.0;
const FEEDBACK_HEIGHT: f64 = 72.0;

static REQUEST_ID: AtomicU64 = AtomicU64::new(0);
static DISPLAYED_ID: AtomicU64 = AtomicU64::new(0);
static REQUEST: Mutex<Option<FeedbackRequest>> = Mutex::new(None);
static BUILD_LOCK: Mutex<()> = Mutex::new(());

#[derive(Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConversionFeedback {
    pub request_id: u64,
    pub message: String,
}

struct FeedbackRequest {
    payload: ConversionFeedback,
    requested_at: Instant,
    presented: bool,
}

#[cfg(target_os = "macos")]
use tauri_nspanel::{tauri_panel, ManagerExt, PanelLevel, WebviewWindowExt};

#[cfg(target_os = "macos")]
tauri_panel! {
    panel!(ConversionFeedbackPanel {
        config: {
            is_floating_panel: true,
            can_become_key_window: false,
            can_become_main_window: false
        }
    })
}

/// 此函数只在转换实际写回成功后调用，展示失败仅记录日志，不重试剪贴板写回。
pub fn request_conversion_success(app: &AppHandle) {
    let mut request = REQUEST.lock().unwrap_or_else(|err| err.into_inner());
    let request_id = REQUEST_ID.fetch_add(1, Ordering::Relaxed) + 1;
    let message = crate::i18n::shortcuts::converted(crate::i18n::current_language(app)).to_owned();
    *request = Some(FeedbackRequest {
        payload: ConversionFeedback {
            request_id,
            message,
        },
        requested_at: Instant::now(),
        presented: false,
    });
    drop(request);
    let app = app.clone();
    tauri::async_runtime::spawn_blocking(move || {
        if let Err(err) = ensure_feedback_window(&app) {
            log::warn!("build conversion feedback failed after successful write: {err}");
            return;
        }
        if let Some(payload) = conversion_feedback_state() {
            if let Err(err) = app.emit_to(CONVERSION_FEEDBACK_WINDOW_LABEL, FEEDBACK_EVENT, payload)
            {
                log::warn!("emit conversion feedback failed after successful write: {err}");
            }
        }
    });
}

/// 首次WebView加载主动拉取，避免建窗后的事件先于订阅而丢失。
pub fn conversion_feedback_state() -> Option<ConversionFeedback> {
    REQUEST
        .lock()
        .unwrap_or_else(|err| err.into_inner())
        .as_ref()
        .filter(|request| request.requested_at.elapsed() <= MAX_READY_WAIT)
        .map(|request| request.payload.clone())
}

fn ensure_feedback_window(app: &AppHandle) -> anyhow::Result<()> {
    let _build = BUILD_LOCK.lock().unwrap_or_else(|err| err.into_inner());
    if app
        .get_webview_window(CONVERSION_FEEDBACK_WINDOW_LABEL)
        .is_some()
    {
        return Ok(());
    }
    let window = WebviewWindowBuilder::new(
        app,
        CONVERSION_FEEDBACK_WINDOW_LABEL,
        WebviewUrl::App("index.html/#/conversion-feedback".into()),
    )
    .title(crate::i18n::shortcuts::title(
        crate::i18n::current_language(app),
    ))
    .inner_size(FEEDBACK_WIDTH, FEEDBACK_HEIGHT)
    .decorations(false)
    .transparent(true)
    .shadow(false)
    .resizable(false)
    .maximizable(false)
    .minimizable(false)
    .always_on_top(true)
    .skip_taskbar(true)
    .focused(false)
    .focusable(false)
    .disable_drag_drop_handler()
    .visible(false)
    .build()?;
    window.set_ignore_cursor_events(true)?;
    Ok(())
}

/// 前端提交已渲染的request ID；旧请求及重复确认不得重新显示或延长旧提示。
pub fn present_conversion_feedback(app: &AppHandle, request_id: u64) -> Result<()> {
    let handle = app.clone();
    app.run_on_main_thread(move || {
        let mut request = REQUEST.lock().unwrap_or_else(|err| err.into_inner());
        let Some(current) = request.as_mut().filter(|request| {
            request.payload.request_id == request_id
                && !request.presented
                && request.requested_at.elapsed() <= MAX_READY_WAIT
        }) else {
            return;
        };
        if let Err(err) = show_feedback_window(&handle) {
            log::warn!("show conversion feedback failed after successful write: {err}");
            return;
        }
        current.presented = true;
        DISPLAYED_ID.store(request_id, Ordering::Relaxed);
        drop(request);
        std::thread::spawn(move || {
            std::thread::sleep(FEEDBACK_DURATION);
            let app = handle.clone();
            if let Err(err) = handle.run_on_main_thread(move || {
                // 新请求还未渲染时，旧窗口仍按原计时隐藏；只有实际show才替换显示代次。
                if DISPLAYED_ID.load(Ordering::Relaxed) == request_id {
                    #[cfg(target_os = "macos")]
                    if let Ok(panel) = app.get_webview_panel(CONVERSION_FEEDBACK_WINDOW_LABEL) {
                        panel.hide();
                    }
                    #[cfg(target_os = "windows")]
                    if let Some(window) = app.get_webview_window(CONVERSION_FEEDBACK_WINDOW_LABEL) {
                        if let Err(err) = hide_feedback_window(&window) {
                            log::warn!("hide conversion feedback failed: {err}");
                        }
                    }
                }
            }) {
                log::warn!("schedule conversion feedback hide failed: {err}");
            }
        });
    })
    .map_err(|err| anyhow::anyhow!(err))?;
    Ok(())
}

/// 原生显示没有更新Tao的可见性缓存，隐藏也必须直接操作HWND。
#[cfg(target_os = "windows")]
fn hide_feedback_window(window: &tauri::WebviewWindow) -> anyhow::Result<()> {
    use windows::Win32::Foundation::HWND;
    use windows::Win32::UI::WindowsAndMessaging::{ShowWindow, SW_HIDE};

    let hwnd = HWND(window.hwnd()?.0 as isize);
    unsafe {
        // 返回值是此前的可见状态，不是调用成功标志。
        let _ = ShowWindow(hwnd, SW_HIDE);
    }
    Ok(())
}

/// 按鼠标所在显示器工作区定位；平台显示必须不激活窗口，不影响粘贴目标。
fn show_feedback_window(app: &AppHandle) -> anyhow::Result<()> {
    let window = super::get_window(app, CONVERSION_FEEDBACK_WINDOW_LABEL)?;
    let cursor = app.cursor_position()?;
    let monitor = app
        .monitor_from_point(cursor.x, cursor.y)?
        .or(app.primary_monitor()?)
        .ok_or_else(|| anyhow::anyhow!("conversion feedback monitor unavailable"))?;
    let area = monitor.work_area();
    let scale = monitor.scale_factor();
    window.set_size(tauri::LogicalSize::new(FEEDBACK_WIDTH, FEEDBACK_HEIGHT))?;
    window.set_position(PhysicalPosition::new(
        area.position.x + (area.size.width as f64 / 2.0 - FEEDBACK_WIDTH * scale / 2.0) as i32,
        area.position.y + area.size.height as i32 - ((FEEDBACK_HEIGHT + 32.0) * scale) as i32,
    ))?;
    #[cfg(target_os = "macos")]
    {
        let panel = match app.get_webview_panel(CONVERSION_FEEDBACK_WINDOW_LABEL) {
            Ok(panel) => panel,
            Err(_) => window
                .to_panel::<ConversionFeedbackPanel>()
                .map_err(|err| anyhow::anyhow!("feedback panel: {err:?}"))?,
        };
        panel.set_level(PanelLevel::Status.value());
        panel.set_ignores_mouse_events(true);
        panel.show();
    }
    #[cfg(target_os = "windows")]
    {
        use windows::Win32::Foundation::HWND;
        use windows::Win32::UI::WindowsAndMessaging::{
            SetWindowPos, HWND_TOPMOST, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE, SWP_SHOWWINDOW,
        };
        let hwnd = HWND(window.hwnd()?.0 as isize);
        unsafe {
            SetWindowPos(
                hwnd,
                HWND_TOPMOST,
                0,
                0,
                0,
                0,
                SWP_NOACTIVATE | SWP_NOMOVE | SWP_NOSIZE | SWP_SHOWWINDOW,
            )?;
        }
    }
    Ok(())
}
