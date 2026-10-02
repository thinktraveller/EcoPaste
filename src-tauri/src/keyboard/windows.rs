use std::collections::HashSet;
use std::ptr::null_mut;
use std::sync::atomic::{AtomicBool, AtomicIsize, Ordering};
use std::sync::{Mutex, OnceLock};

use serde_json::json;
use tauri::{AppHandle, Emitter, Manager};
use winapi::shared::minwindef::{LPARAM, LRESULT, UINT, WPARAM};
use winapi::um::processthreadsapi::GetCurrentThreadId;
use winapi::um::winuser::{
    CallNextHookEx, GetAsyncKeyState, GetForegroundWindow, GetMessageW, PostThreadMessageW,
    SetWindowsHookExW, UnhookWindowsHookEx, KBDLLHOOKSTRUCT, LLKHF_INJECTED, MSG, VK_BACK,
    VK_CONTROL, VK_DELETE, VK_DOWN, VK_ESCAPE, VK_LCONTROL, VK_LEFT, VK_LWIN, VK_MENU, VK_RCONTROL,
    VK_RETURN, VK_RIGHT, VK_RWIN, VK_SHIFT, VK_SPACE, VK_TAB, VK_UP, WH_KEYBOARD_LL, WM_KEYDOWN,
    WM_KEYUP, WM_QUIT, WM_SYSKEYDOWN, WM_SYSKEYUP,
};

use super::NAV_EVENT;
use crate::settings::{
    shortcuts::{Binding, WINDOW_ACTION_DEFAULTS},
    SettingsStore,
};

static NAV_ENABLED: AtomicBool = AtomicBool::new(false);
static PINNED_NAV_INTERACTION: AtomicBool = AtomicBool::new(true);
static NAV_FOREGROUND: AtomicIsize = AtomicIsize::new(0);
static HOOK_THREAD_ID: Mutex<Option<u32>> = Mutex::new(None);
static APP_HANDLE: OnceLock<AppHandle> = OnceLock::new();

fn consumed_keys() -> &'static Mutex<HashSet<u32>> {
    static SET: OnceLock<Mutex<HashSet<u32>>> = OnceLock::new();
    SET.get_or_init(|| Mutex::new(HashSet::new()))
}

/// Map supported virtual keys to the semantic key contract used by React and stored bindings.
fn event_key(vk: u32) -> Option<String> {
    let key = match vk as i32 {
        0x30..=0x39 | 0x41..=0x5A => {
            return Some((vk as u8 as char).to_ascii_lowercase().to_string())
        }
        0x70..=0x7B => return Some(format!("F{}", vk - 0x70 + 1)),
        VK_RETURN => "Enter",
        VK_BACK => "Backspace",
        VK_DELETE => "Delete",
        VK_LEFT => "ArrowLeft",
        VK_RIGHT => "ArrowRight",
        VK_UP => "ArrowUp",
        VK_DOWN => "ArrowDown",
        VK_ESCAPE => "Escape",
        VK_SPACE => " ",
        VK_TAB => "Tab",
        0x24 => "Home",
        0x23 => "End",
        0x2D => "Insert",
        0x21 => "PageUp",
        0x22 => "PageDown",
        0xBC => ",",
        0xBE => ".",
        0xBA => ";",
        0xBF => "/",
        0xDC => "\\",
        0xDB => "[",
        0xDD => "]",
        0xBD => "-",
        0xBB => "=",
        0xC0 => "`",
        0xDE => "'",
        _ => return None,
    };
    Some(key.into())
}

pub fn enable_navigation_keys(app: &AppHandle) {
    let _ = APP_HANDLE.set(app.clone());
    set_pinned_navigation_interaction(true);
    NAV_ENABLED.store(true, Ordering::Relaxed);
    if HOOK_THREAD_ID
        .lock()
        .expect("hook thread id poisoned")
        .is_some()
    {
        return;
    }

    std::thread::spawn(|| unsafe {
        let hook = SetWindowsHookExW(WH_KEYBOARD_LL, Some(hook_proc), null_mut(), 0);
        if hook.is_null() {
            log::error!("SetWindowsHookExW failed");
            return;
        }
        *HOOK_THREAD_ID.lock().expect("hook thread id poisoned") = Some(GetCurrentThreadId());
        let mut msg: MSG = std::mem::zeroed();
        while GetMessageW(&mut msg, null_mut(), 0, 0) > 0 {}
        UnhookWindowsHookEx(hook);
        *HOOK_THREAD_ID.lock().expect("hook thread id poisoned") = None;
        consumed_keys()
            .lock()
            .expect("consumed keys poisoned")
            .clear();
    });
}

/// 固定窗口失焦后仍可见，但外部点击与应用切换必须把输入还给前台应用。
/// 保留hook和已消费的keyup配对，窗内点击或显式重新打开才恢复导航。
pub fn set_pinned_navigation_interaction(active: bool) {
    PINNED_NAV_INTERACTION.store(active, Ordering::Relaxed);
    if active {
        NAV_FOREGROUND.store(unsafe { GetForegroundWindow() } as isize, Ordering::Relaxed);
    } else if let Some(app) = APP_HANDLE.get() {
        let _ = app.emit(NAV_EVENT, json!({ "type": "keyup", "key": "Control", "ctrlKey": false, "shiftKey": false, "altKey": false, "metaKey": false }));
    }
}

pub fn disable_navigation_keys() {
    NAV_ENABLED.store(false, Ordering::Relaxed);
    if let Some(app) = APP_HANDLE.get() {
        let _ = app.emit(NAV_EVENT, json!({ "type": "keyup", "key": "Control", "ctrlKey": false, "shiftKey": false, "altKey": false, "metaKey": false }));
    }
    if let Some(tid) = HOOK_THREAD_ID
        .lock()
        .expect("hook thread id poisoned")
        .take()
    {
        unsafe {
            PostThreadMessageW(tid, WM_QUIT, 0, 0);
        }
    }
}

/// Fixed gestures and configured actions share exact matching before the hook consumes a key.
fn matches_window_action(app: &AppHandle, binding: &Binding) -> bool {
    let settings = app.state::<SettingsStore>().snapshot();
    WINDOW_ACTION_DEFAULTS.iter().any(|(action, _)| {
        settings
            .shortcuts
            .window_actions
            .get(*action)
            .and_then(|value| Binding::parse(value))
            .as_ref()
            == Some(binding)
    })
}

fn fixed_binding(binding: &Binding) -> bool {
    if !binding.has_modifier() {
        return matches!(
            binding.key.as_str(),
            "escape" | "space" | "arrowup" | "arrowdown" | "arrowleft" | "arrowright" | "tab"
        );
    }
    if binding.shift && !binding.ctrl && !binding.alt && !binding.meta {
        return binding.key == "tab";
    }
    binding.ctrl
        && !binding.shift
        && !binding.alt
        && !binding.meta
        && matches!(
            binding.key.as_str(),
            "f" | "q"
                | "n"
                | "p"
                | "k"
                | ","
                | "0"
                | "1"
                | "2"
                | "3"
                | "4"
                | "5"
                | "6"
                | "7"
                | "8"
                | "9"
        )
}

unsafe fn pressed(vk: i32) -> bool {
    (GetAsyncKeyState(vk) as u16) & 0x8000 != 0
}

unsafe extern "system" fn hook_proc(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if code < 0 || !NAV_ENABLED.load(Ordering::Relaxed) {
        return CallNextHookEx(null_mut(), code, wparam, lparam);
    }
    let kbd = &*(lparam as *const KBDLLHOOKSTRUCT);
    if kbd.flags & LLKHF_INJECTED != 0 {
        return CallNextHookEx(null_mut(), code, wparam, lparam);
    }
    let vk = kbd.vkCode;
    let msg = wparam as UINT;
    let down = msg == WM_KEYDOWN || msg == WM_SYSKEYDOWN;
    let up = msg == WM_KEYUP || msg == WM_SYSKEYUP;
    if !down && !up {
        return CallNextHookEx(null_mut(), code, wparam, lparam);
    }
    let Some(app) = APP_HANDLE.get() else {
        return CallNextHookEx(null_mut(), code, wparam, lparam);
    };
    let ctrl = pressed(VK_CONTROL);
    let shift = pressed(VK_SHIFT);
    let alt = pressed(VK_MENU);
    let meta = pressed(VK_LWIN) || pressed(VK_RWIN);
    let pinned_input_active = !crate::window::is_clipboard_window_pinned()
        || (PINNED_NAV_INTERACTION.load(Ordering::Relaxed)
            && NAV_FOREGROUND.load(Ordering::Relaxed) == GetForegroundWindow() as isize);
    if matches!(vk as i32, VK_CONTROL | VK_LCONTROL | VK_RCONTROL) {
        if !pinned_input_active {
            return CallNextHookEx(null_mut(), code, wparam, lparam);
        }
        let _ = app.emit(NAV_EVENT, json!({ "type": if down {"keydown"} else {"keyup"}, "key": "Control", "ctrlKey": down, "shiftKey": shift, "altKey": alt, "metaKey": meta }));
        return CallNextHookEx(null_mut(), code, wparam, lparam);
    }
    let Some(key) = event_key(vk) else {
        return CallNextHookEx(null_mut(), code, wparam, lparam);
    };
    let binding = Binding {
        key: if key == " " {
            "space".into()
        } else {
            key.to_ascii_lowercase()
        },
        ctrl,
        shift,
        alt,
        meta,
    };
    let mut consumed = consumed_keys().lock().expect("consumed keys poisoned");
    if up {
        if !consumed.remove(&vk) {
            return CallNextHookEx(null_mut(), code, wparam, lparam);
        }
        drop(consumed);
        let _ = app.emit(NAV_EVENT, json!({ "type": "keyup", "key": key, "code": if vk as i32 == VK_SPACE { "Space" } else { "" }, "ctrlKey": ctrl, "shiftKey": shift, "altKey": alt, "metaKey": meta }));
        return 1;
    }
    if !pinned_input_active && !consumed.contains(&vk) {
        return CallNextHookEx(null_mut(), code, wparam, lparam);
    }
    if !consumed.contains(&vk) && !fixed_binding(&binding) && !matches_window_action(app, &binding)
    {
        return CallNextHookEx(null_mut(), code, wparam, lparam);
    }
    let repeat = !consumed.insert(vk);
    drop(consumed);
    let _ = app.emit(NAV_EVENT, json!({ "type": "keydown", "key": key, "code": if vk as i32 == VK_SPACE { "Space" } else { "" }, "ctrlKey": ctrl, "shiftKey": shift, "altKey": alt, "metaKey": meta, "repeat": repeat }));
    1
}
