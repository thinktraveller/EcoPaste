//! 去格式规则校验与预览的短错误；只输出稳定原因和规则序号。

use crate::core::AppError;
use crate::settings::Language;

/// 规则序号使用零基索引传入；错误不回显样本、查找模式或替换正文。
pub fn error(language: Language, reason: &str, rule_index: Option<usize>) -> AppError {
    let message = match (language, reason) {
        (Language::ZhCN, "too many rules") => "规则不能超过 32 条。",
        (Language::EnUS, "too many rules") => "Use no more than 32 rules.",
        (Language::ZhCN, "invalid rule id") => "规则标识必须是有效的 UUID。",
        (Language::EnUS, "invalid rule id") => "The rule ID must be a valid UUID.",
        (Language::ZhCN, "duplicate rule id") => "规则标识不能重复。",
        (Language::EnUS, "duplicate rule id") => "Rule IDs must be unique.",
        (Language::ZhCN, "empty rule name") => "请输入规则名称。",
        (Language::EnUS, "empty rule name") => "Enter a rule name.",
        (Language::ZhCN, "rule name too long") => "规则名称不能超过 80 个字符。",
        (Language::EnUS, "rule name too long") => "Use no more than 80 characters in the rule name.",
        (Language::ZhCN, "empty pattern") => "查找内容不能为空；替换内容可以为空。",
        (Language::EnUS, "empty pattern") => "The search pattern cannot be empty; the replacement may be empty.",
        (Language::ZhCN, "pattern too long") => "查找内容不能超过 4 KiB UTF-8。",
        (Language::EnUS, "pattern too long") => "The search pattern cannot exceed 4 KiB of UTF-8 text.",
        (Language::ZhCN, "replacement too long") => "替换内容不能超过 4 KiB UTF-8。",
        (Language::EnUS, "replacement too long") => "The replacement cannot exceed 4 KiB of UTF-8 text.",
        (Language::ZhCN, "null character") => "查找、替换内容和自定义处理结果不能包含空字符（NUL），以免系统剪贴板截断文本。",
        (Language::EnUS, "null character") => "Search patterns, replacements, and custom output cannot contain null characters (NUL), because the system clipboard may truncate the text.",
        (Language::ZhCN, "invalid regex") => "正则表达式无效或超过编译限制；不支持环视和模式回溯引用。",
        (Language::EnUS, "invalid regex") => "The regular expression is invalid or exceeds compilation limits; lookaround and pattern backreferences are unsupported.",
        (Language::ZhCN, "invalid capture") => "替换内容引用了无效捕获组；请使用 $1、${1}、$name、${name} 或 $$。",
        (Language::EnUS, "invalid capture") => "The replacement has an invalid capture reference; use $1, ${1}, $name, ${name}, or $$.",
        (Language::ZhCN, "input too large") => "自定义替换的初始文本不能超过 1 MiB。此次转换已取消。",
        (Language::EnUS, "input too large") => "The initial text for custom replacements cannot exceed 1 MiB. Conversion was cancelled.",
        (Language::ZhCN, "output too large") => "规则处理结果不能超过 4 MiB。此次转换已取消。",
        (Language::EnUS, "output too large") => "A rule's output cannot exceed 4 MiB. Conversion was cancelled.",
        (Language::ZhCN, "too many matches") => "单条规则不能匹配超过 10,000 次。此次转换已取消。",
        (Language::EnUS, "too many matches") => "A rule cannot exceed 10,000 matches. Conversion was cancelled.",
        (Language::ZhCN, "empty result") => "自定义替换的最终结果为空或全为空白。此次转换已取消，剪贴板保持不变。",
        (Language::EnUS, "empty result") => "Custom replacements produced only whitespace or empty text. Conversion was cancelled and the clipboard was preserved.",
        (Language::ZhCN, "empty extraction") => "未提取到可用正文，请调整示例或规则。",
        (Language::EnUS, "empty extraction") => "No usable text was extracted. Adjust the example or rules.",
        (Language::ZhCN, "preview unavailable") => "暂时无法完成预览，请重试。",
        (Language::EnUS, "preview unavailable") => "The preview could not be completed. Try again.",
        (Language::ZhCN, "conversion unavailable") => "暂时无法完成转换，请重试。",
        (Language::EnUS, "conversion unavailable") => "The conversion could not be completed. Try again.",
        (Language::ZhCN, _) => "去格式规则配置无效，请检查后重试。",
        (Language::EnUS, _) => "The plain-text conversion configuration is invalid. Review it and try again.",
    };
    let message = match (language, rule_index) {
        (Language::ZhCN, Some(index)) => format!("第 {} 条规则：{message}", index + 1),
        (Language::EnUS, Some(index)) => format!("Rule {}: {message}", index + 1),
        (_, None) => message.to_owned(),
    };

    AppError::Other(anyhow::anyhow!(message))
}
