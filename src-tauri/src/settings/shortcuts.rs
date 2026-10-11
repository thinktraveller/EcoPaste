//! Window action IDs and semantic shortcut validation shared by settings and native hooks.

use std::collections::{BTreeMap, HashSet};

use super::{Language, Shortcuts};
use crate::core::{AppError, Result};

pub const WINDOW_ACTION_DEFAULTS: &[(&str, &str)] = &[
    ("paste", "Enter"),
    ("pastePlain", "CmdOrCtrl+Enter"),
    ("copy", "CmdOrCtrl+C"),
    ("open", "CmdOrCtrl+O"),
    ("favorite", "CmdOrCtrl+D"),
    ("pin", "CmdOrCtrl+T"),
    ("note", "CmdOrCtrl+M"),
    ("delete", "CmdOrCtrl+Backspace"),
    ("deleteForward", "CmdOrCtrl+Delete"),
];

pub fn default_window_actions() -> BTreeMap<String, String> {
    WINDOW_ACTION_DEFAULTS
        .iter()
        .map(|(id, key)| ((*id).into(), (*key).into()))
        .collect()
}

/// Deserializes partial old/new maps without re-enabling explicitly disabled actions.
pub fn deserialize_window_actions<'de, D>(
    deserializer: D,
) -> std::result::Result<BTreeMap<String, String>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let value = <serde_json::Value as serde::Deserialize>::deserialize(deserializer)?;
    let custom: BTreeMap<String, String> = value
        .as_object()
        .map(|map| {
            map.iter()
                .map(|(key, value)| (key.clone(), value.as_str().unwrap_or_default().to_owned()))
                .collect()
        })
        .unwrap_or_else(|| {
            WINDOW_ACTION_DEFAULTS
                .iter()
                .map(|(id, _)| ((*id).into(), String::new()))
                .collect()
        });
    let mut actions = default_window_actions();
    actions.extend(custom);
    Ok(actions)
}

/// Corrupt newly introduced fields disable their action instead of resetting unrelated settings.
pub fn deserialize_binding<'de, D>(deserializer: D) -> std::result::Result<String, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let value = <serde_json::Value as serde::Deserialize>::deserialize(deserializer)?;
    Ok(value.as_str().unwrap_or_default().to_owned())
}

/// Update/import paths reject malformed new contracts; tolerant loading is a separate policy.
pub fn validate_shortcut_json(value: &serde_json::Value, lang: Language) -> Result<()> {
    let Some(shortcuts) = value.get("shortcuts") else {
        return Ok(());
    };
    if let Some(binding) = shortcuts.get("convertToPlainText") {
        if !binding.is_string() {
            return Err(invalid("plain-text shortcut must be a string", lang));
        }
    }
    if let Some(actions) = shortcuts.get("windowActions") {
        let Some(actions) = actions.as_object() else {
            return Err(invalid("window shortcuts must be an object", lang));
        };
        if actions.values().any(|binding| !binding.is_string()) {
            return Err(invalid("window shortcut must be a string", lang));
        }
    }
    Ok(())
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Binding {
    pub key: String,
    pub ctrl: bool,
    pub shift: bool,
    pub alt: bool,
    pub meta: bool,
}

impl Binding {
    /// Parse a supported semantic key; aliases and modifier order have identical meaning.
    pub fn parse(value: &str) -> Option<Self> {
        let mut result = Self {
            key: String::new(),
            ctrl: false,
            shift: false,
            alt: false,
            meta: false,
        };
        for part in value.split('+') {
            let part = part.trim().to_ascii_lowercase();
            if part.is_empty() {
                return None;
            }
            match part.as_str() {
                "ctrl" | "control" => {
                    if result.ctrl {
                        return None;
                    }
                    result.ctrl = true;
                }
                "shift" => {
                    if result.shift {
                        return None;
                    }
                    result.shift = true;
                }
                "alt" | "option" => {
                    if result.alt {
                        return None;
                    }
                    result.alt = true;
                }
                "command" | "cmd" | "meta" | "super" => {
                    if result.meta {
                        return None;
                    }
                    result.meta = true;
                }
                "cmdorctrl" | "commandorcontrol" | "mod" => {
                    if cfg!(target_os = "macos") {
                        if result.meta {
                            return None;
                        }
                        result.meta = true;
                    } else {
                        if result.ctrl {
                            return None;
                        }
                        result.ctrl = true;
                    }
                }
                _ => {
                    if !result.key.is_empty() {
                        return None;
                    }
                    result.key = match part.as_str() {
                        "esc" => "escape".into(),
                        "return" => "enter".into(),
                        "up" => "arrowup".into(),
                        "down" => "arrowdown".into(),
                        "left" => "arrowleft".into(),
                        "right" => "arrowright".into(),
                        _ => part,
                    };
                }
            }
        }
        let key = result.key.as_str();
        let function_key = key
            .strip_prefix('f')
            .and_then(|v| v.parse::<u8>().ok())
            .is_some_and(|v| (1..=12).contains(&v) && key == format!("f{v}"));
        if (key.len() == 1
            && (key.as_bytes()[0].is_ascii_alphanumeric() || ";./\\[]-=`,\"'".contains(key)))
            || function_key
            || matches!(
                key,
                "enter"
                    | "backspace"
                    | "delete"
                    | "escape"
                    | "space"
                    | "tab"
                    | "arrowup"
                    | "arrowdown"
                    | "arrowleft"
                    | "arrowright"
                    | "home"
                    | "end"
                    | "insert"
                    | "pageup"
                    | "pagedown"
            )
        {
            Some(result)
        } else {
            None
        }
    }

    pub fn has_modifier(&self) -> bool {
        self.ctrl || self.shift || self.alt || self.meta
    }

    fn is_function_key(&self) -> bool {
        self.key
            .strip_prefix('f')
            .and_then(|value| value.parse::<u8>().ok())
            .is_some_and(|value| (1..=12).contains(&value))
    }

    /// Return a plugin-compatible canonical representation for OS registration and menus.
    pub fn canonical(&self) -> String {
        let mut parts = Vec::new();
        if self.meta {
            parts.push("Super".to_owned());
        }
        if self.ctrl {
            parts.push("Control".to_owned());
        }
        if self.alt {
            parts.push("Alt".to_owned());
        }
        if self.shift {
            parts.push("Shift".to_owned());
        }
        let key = match self.key.as_str() {
            "enter" => "Enter".into(),
            "backspace" => "Backspace".into(),
            "delete" => "Delete".into(),
            "escape" => "Escape".into(),
            "space" => "Space".into(),
            "tab" => "Tab".into(),
            "arrowup" => "ArrowUp".into(),
            "arrowdown" => "ArrowDown".into(),
            "arrowleft" => "ArrowLeft".into(),
            "arrowright" => "ArrowRight".into(),
            "home" => "Home".into(),
            "end" => "End".into(),
            "insert" => "Insert".into(),
            "pageup" => "PageUp".into(),
            "pagedown" => "PageDown".into(),
            _ => self.key.to_ascii_uppercase(),
        };
        parts.push(key);
        parts.join("+")
    }
}

/// Fixed window gestures cannot be shadowed by configurable actions.
fn reserved_window_binding(binding: &Binding) -> bool {
    if matches!(
        binding.key.as_str(),
        "space" | "escape" | "tab" | "arrowup" | "arrowdown" | "arrowleft" | "arrowright"
    ) {
        return true;
    }
    let primary = if cfg!(target_os = "macos") {
        binding.meta
    } else {
        binding.ctrl
    };
    let secondary = if cfg!(target_os = "macos") {
        binding.ctrl
    } else {
        binding.meta
    };
    primary
        && !secondary
        && !binding.alt
        && !binding.shift
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

/// Reject ambiguous or unsupported bindings at every settings write boundary.
pub fn validate_shortcuts(shortcuts: &Shortcuts, lang: Language) -> Result<()> {
    let mut seen = HashSet::new();
    if cfg!(target_os = "windows") && shortcuts.win_v {
        if let Some(binding) = Binding::parse("Super+V") {
            seen.insert(binding);
        }
    }
    for value in [&shortcuts.open_clipboard, &shortcuts.open_preference] {
        if value.is_empty() {
            continue;
        }
        if let Some(binding) = Binding::parse(value) {
            if !seen.insert(binding) {
                return Err(invalid("shortcut conflicts with another action", lang));
            }
        }
    }
    let value = &shortcuts.convert_to_plain_text;
    if !value.is_empty() {
        let binding =
            Binding::parse(value).ok_or_else(|| invalid("unsupported global shortcut", lang))?;
        if !binding.has_modifier() && !binding.is_function_key() {
            return Err(invalid("global shortcut requires a modifier", lang));
        }
        if reserved_window_binding(&binding) || !seen.insert(binding) {
            return Err(invalid("shortcut conflicts with another action", lang));
        }
    }
    for (action, _) in WINDOW_ACTION_DEFAULTS {
        let Some(value) = shortcuts.window_actions.get(*action) else {
            continue;
        };
        if value.is_empty() {
            continue;
        }
        let binding =
            Binding::parse(value).ok_or_else(|| invalid("unsupported window shortcut", lang))?;
        if reserved_window_binding(&binding) || !seen.insert(binding.clone()) {
            return Err(invalid("shortcut conflicts with another action", lang));
        }
        if !binding.has_modifier()
            && !matches!(binding.key.as_str(), "enter" | "backspace" | "delete")
            && !binding.is_function_key()
        {
            return Err(invalid(
                "window shortcut requires a modifier or function key",
                lang,
            ));
        }
    }
    Ok(())
}

fn invalid(message: &str, lang: Language) -> AppError {
    AppError::Other(anyhow::anyhow!(crate::i18n::shortcuts::validation_error(
        lang, message
    )))
}

/// Keep unrelated settings on load; disable only invalid new bindings.
pub fn sanitize_loaded_shortcuts(shortcuts: &mut Shortcuts) -> Vec<&'static str> {
    let mut disabled = Vec::new();
    let mut probe = shortcuts.clone();
    probe.convert_to_plain_text.clear();
    // Legacy errors belong to their OS registration status, not to every newly added action.
    let mut seen_legacy = HashSet::new();
    if cfg!(target_os = "windows") && shortcuts.win_v {
        if let Some(binding) = Binding::parse("Super+V") {
            seen_legacy.insert(binding);
        }
    }
    for value in [&mut probe.open_clipboard, &mut probe.open_preference] {
        if let Some(binding) = Binding::parse(value) {
            if !seen_legacy.insert(binding) {
                value.clear();
            }
        } else {
            value.clear();
        }
    }
    probe.window_actions = default_window_actions()
        .into_keys()
        .map(|key| (key, String::new()))
        .collect();
    let convert = shortcuts.convert_to_plain_text.clone();
    probe.convert_to_plain_text = convert;
    if validate_shortcuts(&probe, Language::default()).is_err() {
        log::warn!("invalid loaded plain-text shortcut disabled");
        probe.convert_to_plain_text.clear();
        shortcuts.convert_to_plain_text.clear();
        disabled.push(crate::shortcut::ACTION_CONVERT_TO_PLAIN_TEXT);
    }
    for (id, _) in WINDOW_ACTION_DEFAULTS {
        let value = shortcuts
            .window_actions
            .get(*id)
            .cloned()
            .unwrap_or_default();
        probe.window_actions.insert((*id).into(), value);
        if validate_shortcuts(&probe, Language::default()).is_err() {
            log::warn!("invalid loaded window shortcut {id} disabled");
            probe.window_actions.insert((*id).into(), String::new());
            shortcuts.window_actions.insert((*id).into(), String::new());
            disabled.push(*id);
        }
    }
    disabled
}

/// Report malformed new fields before tolerant deserialization replaces them with disabled values.
pub fn malformed_loaded_shortcuts(raw: &serde_json::Value) -> Vec<&'static str> {
    let mut disabled = Vec::new();
    let Some(shortcuts) = raw.get("shortcuts") else {
        return disabled;
    };
    if shortcuts
        .get("convertToPlainText")
        .is_some_and(|value| !value.is_string())
    {
        disabled.push(crate::shortcut::ACTION_CONVERT_TO_PLAIN_TEXT);
    }
    if let Some(actions) = shortcuts.get("windowActions") {
        for (id, _) in WINDOW_ACTION_DEFAULTS {
            if !actions.is_object() || actions.get(*id).is_some_and(|value| !value.is_string()) {
                disabled.push(*id);
            }
        }
    }
    disabled
}
