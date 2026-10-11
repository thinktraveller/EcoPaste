//! Localized recovery and concurrency feedback for the conversion rule editor.

use crate::settings::Language;

pub fn malformed(lang: Language) -> &'static str {
    match lang {
        Language::ZhCN => "去格式规则配置格式无效，请修正规则或恢复默认。",
        Language::EnUS => {
            "The formatting rule configuration is invalid. Correct it or restore its defaults."
        }
    }
}

pub fn recovered(lang: Language) -> &'static str {
    match lang {
        Language::ZhCN => "已暂时使用默认去格式规则，原配置已保留；请修复后保存或恢复默认。",
        Language::EnUS => "Default formatting rules are being used temporarily. The original configuration was preserved; correct and save it or restore its defaults.",
    }
}

pub fn conflict(lang: Language) -> &'static str {
    match lang {
        Language::ZhCN => "去格式规则已在其它窗口或配置中更新，请重新加载后再保存。",
        Language::EnUS => "Formatting rules changed in another window or configuration. Reload them before saving.",
    }
}

pub fn unavailable(lang: Language) -> &'static str {
    match lang {
        Language::ZhCN => "暂时无法处理去格式规则，请重试。",
        Language::EnUS => "Formatting rules could not be processed. Try again.",
    }
}
