//! Native feedback available even without a clipboard WebView.
use crate::settings::Language;

pub fn converted(lang: Language) -> &'static str {
    match lang {
        Language::ZhCN => "已经转换格式",
        Language::EnUS => "Formatting converted",
    }
}

pub fn title(lang: Language) -> &'static str {
    match lang {
        Language::ZhCN => "转换剪贴板为纯文本",
        Language::EnUS => "Convert clipboard to plain text",
    }
}

pub fn unsupported(lang: Language) -> &'static str {
    match lang {
        Language::ZhCN => "当前剪贴板包含图片、文件或不支持的文本格式，内容保持不变。",
        Language::EnUS => "The clipboard contains images, files, or unsupported text formats. Its contents were preserved.",
    }
}

pub fn changed(lang: Language) -> &'static str {
    match lang {
        Language::ZhCN => "剪贴板在转换期间已发生变化，本次转换已取消。",
        Language::EnUS => "The clipboard changed during conversion. This conversion was cancelled.",
    }
}

pub fn failed(lang: Language) -> &'static str {
    match lang {
        Language::ZhCN => "无法读取或写入剪贴板。若写入已经开始，剪贴板内容可能已部分改变，请重新复制原内容。",
        Language::EnUS => "The clipboard could not be read or written. A started write may have partially changed its contents; copy the original content again.",
    }
}

/// Translate stable validation reasons at the settings/registration boundary.
pub fn validation_error(lang: Language, reason: &str) -> &'static str {
    match (lang, reason) {
        (Language::ZhCN, "loaded binding disabled") => "配置中的快捷键损坏、无效或冲突，已禁用；请重新设置或恢复默认。",
        (Language::EnUS, "loaded binding disabled") => "The stored shortcut was malformed, unsupported, or conflicting and was disabled. Set it again or restore its default.",
        (Language::ZhCN, "unsupported global shortcut") => "不支持此全局快捷键。",
        (Language::ZhCN, "unsupported window shortcut") => "不支持此窗口快捷键。",
        (Language::ZhCN, "global shortcut requires a modifier") => "全局快捷键需要修饰键或功能键。",
        (Language::ZhCN, "window shortcut requires a modifier or function key") => "窗口快捷键需要修饰键、功能键或支持的操作键。",
        (Language::ZhCN, "shortcut conflicts with another action") => "此快捷键与另一动作或保留窗口动作冲突。",
        (Language::ZhCN, "plain-text shortcut must be a string") => "纯文本转换快捷键格式无效。",
        (Language::ZhCN, "window shortcuts must be an object") => "窗口快捷键配置格式无效。",
        (Language::ZhCN, "window shortcut must be a string") => "窗口快捷键格式无效。",
        (Language::ZhCN, "unregister") => "无法注销上一组快捷键，请重新启动应用后再试。",
        (Language::EnUS, "unsupported global shortcut") => "This global shortcut is not supported.",
        (Language::EnUS, "unsupported window shortcut") => "This window shortcut is not supported.",
        (Language::EnUS, "global shortcut requires a modifier") => "A global shortcut requires a modifier or function key.",
        (Language::EnUS, "window shortcut requires a modifier or function key") => "A window shortcut requires a modifier, function key, or supported action key.",
        (Language::EnUS, "shortcut conflicts with another action") => "This shortcut conflicts with another action or reserved window gesture.",
        (Language::EnUS, "plain-text shortcut must be a string") => "The plain-text shortcut has an invalid format.",
        (Language::EnUS, "window shortcuts must be an object") => "The window shortcut configuration has an invalid format.",
        (Language::EnUS, "window shortcut must be a string") => "The window shortcut has an invalid format.",
        (Language::EnUS, "unregister") => "Previous shortcuts could not be unregistered. Restart the app and try again.",
        (Language::ZhCN, _) => "快捷键无效。",
        (Language::EnUS, _) => "The shortcut is invalid.",
    }
}
