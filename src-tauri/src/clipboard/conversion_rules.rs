//! 无系统副作用的去格式文本核心：结构提取后执行受限、有序的全文替换。

use std::collections::HashSet;

use regex::{Captures, Regex, RegexBuilder};
use serde::{Deserialize, Serialize};

use super::markup::{convert_markup, html_to_text};
use crate::core::Result;
use crate::i18n::plain_text_conversion::error;
use crate::settings::{Language, PlainTextConversionSettings, ReplacementMode};

pub const MAX_CONVERSION_RULES: usize = 32;
pub const MAX_RULE_TEXT_BYTES: usize = 4 * 1024;
pub const MAX_RULE_NAME_CHARS: usize = 80;
pub const MAX_INITIAL_INPUT_BYTES: usize = 1024 * 1024;
pub const MAX_RULE_OUTPUT_BYTES: usize = 4 * 1024 * 1024;
pub const MAX_RULE_MATCHES: usize = 10_000;
pub const MAX_REGEX_SIZE_BYTES: usize = 1024 * 1024;
pub const MAX_REGEX_NEST_LIMIT: u32 = 64;

#[derive(Debug, Clone, Copy, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum PlainTextInputFormat {
    Text,
    Html,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PlainTextPreview {
    pub output: String,
    pub changed: bool,
}

pub(super) struct ConvertedText {
    pub output: String,
    pub extracted: bool,
    pub custom_enabled: bool,
}

enum ReplacementPart {
    Text(String),
    Capture(usize),
}

enum PreparedReplacement {
    Literal {
        pattern: String,
        replacement: String,
    },
    Regex {
        pattern: Regex,
        parts: Vec<ReplacementPart>,
    },
}

struct PreparedRule {
    index: usize,
    replacement: PreparedReplacement,
}

/// 保存与运行共用完整校验，停用规则仍须有效；正则编译错误正文不会外泄。
pub fn validate_plain_text_conversion(
    config: &PlainTextConversionSettings,
    language: Language,
) -> Result<()> {
    prepare(config, language)?;

    Ok(())
}

/// 仅处理传入样例；不访问系统剪贴板、持久化、窗口或事件。
pub fn preview_plain_text_conversion(
    input: String,
    input_format: PlainTextInputFormat,
    config: PlainTextConversionSettings,
    language: Language,
) -> Result<PlainTextPreview> {
    let converted = convert_text(&input, input_format, &config, language)?;
    if converted.extracted && converted.output.trim().is_empty() {
        return Err(error(language, "empty extraction", None));
    }

    Ok(PlainTextPreview {
        changed: converted.output != input || input_format == PlainTextInputFormat::Html,
        output: converted.output,
    })
}

/// 原生转换与示例预览共享文本语义；仅启用自定义规则时应用运行预算和空输出保护。
pub(super) fn convert_text(
    input: &str,
    input_format: PlainTextInputFormat,
    config: &PlainTextConversionSettings,
    language: Language,
) -> Result<ConvertedText> {
    let rules = prepare(config, language)?;
    let extracted = match input_format {
        PlainTextInputFormat::Text => convert_markup(input, config),
        PlainTextInputFormat::Html => Some(html_to_text(input, config)),
    };
    let marked = extracted.is_some();
    let mut output = extracted.unwrap_or_else(|| input.to_owned());
    let custom_enabled = !rules.is_empty();
    if custom_enabled && output.len() > MAX_INITIAL_INPUT_BYTES {
        return Err(error(language, "input too large", None));
    }
    for rule in rules {
        output = apply_rule(&output, &rule, language)?;
    }
    if custom_enabled && output.contains('\0') {
        return Err(error(language, "null character", None));
    }
    if custom_enabled && output.trim().is_empty() {
        return Err(error(language, "empty result", None));
    }

    Ok(ConvertedText {
        output,
        extracted: marked,
        custom_enabled,
    })
}

/// 全量校验后仅返回启用规则，禁止用停用状态绕过持久化配置校验。
fn prepare(config: &PlainTextConversionSettings, language: Language) -> Result<Vec<PreparedRule>> {
    if config.rules.len() > MAX_CONVERSION_RULES {
        return Err(error(language, "too many rules", None));
    }
    let mut ids = HashSet::new();
    let mut prepared = Vec::new();
    for (index, rule) in config.rules.iter().enumerate() {
        let id = uuid::Uuid::parse_str(&rule.id)
            .map_err(|_| error(language, "invalid rule id", Some(index)))?;
        if !ids.insert(id) {
            return Err(error(language, "duplicate rule id", Some(index)));
        }
        if rule.name.trim().is_empty() {
            return Err(error(language, "empty rule name", Some(index)));
        }
        if rule.name.chars().count() > MAX_RULE_NAME_CHARS {
            return Err(error(language, "rule name too long", Some(index)));
        }
        if rule.pattern.is_empty() {
            return Err(error(language, "empty pattern", Some(index)));
        }
        if rule.pattern.len() > MAX_RULE_TEXT_BYTES {
            return Err(error(language, "pattern too long", Some(index)));
        }
        if rule.replacement.len() > MAX_RULE_TEXT_BYTES {
            return Err(error(language, "replacement too long", Some(index)));
        }
        if rule.pattern.contains('\0') || rule.replacement.contains('\0') {
            return Err(error(language, "null character", Some(index)));
        }
        let replacement = match rule.mode {
            ReplacementMode::Literal => PreparedReplacement::Literal {
                pattern: rule.pattern.clone(),
                replacement: rule.replacement.clone(),
            },
            ReplacementMode::Regex => {
                let pattern = RegexBuilder::new(&rule.pattern)
                    .size_limit(MAX_REGEX_SIZE_BYTES)
                    .dfa_size_limit(MAX_REGEX_SIZE_BYTES)
                    .nest_limit(MAX_REGEX_NEST_LIMIT)
                    .build()
                    .map_err(|_| error(language, "invalid regex", Some(index)))?;
                let parts = parse_replacement(&rule.replacement, &pattern)
                    .ok_or_else(|| error(language, "invalid capture", Some(index)))?;

                PreparedReplacement::Regex { pattern, parts }
            }
        };
        if rule.enabled {
            prepared.push(PreparedRule { index, replacement });
        }
    }

    Ok(prepared)
}

/// 按 Rust regex 的最长 ASCII 非花括号引用规则分词；捕获先解析为索引以便受限拼接。
fn parse_replacement(replacement: &str, regex: &Regex) -> Option<Vec<ReplacementPart>> {
    let bytes = replacement.as_bytes();
    if bytes
        .windows(2)
        .any(|pair| pair[0] == b'\\' && pair[1].is_ascii_digit())
    {
        return None;
    }
    let mut parts = Vec::new();
    let mut literal = String::new();
    let mut cursor = 0;
    while cursor < bytes.len() {
        if bytes[cursor] != b'$' {
            let ch = replacement[cursor..].chars().next()?;
            literal.push(ch);
            cursor += ch.len_utf8();
            continue;
        }
        cursor += 1;
        if bytes.get(cursor) == Some(&b'$') {
            literal.push('$');
            cursor += 1;
            continue;
        }
        let start = cursor;
        let reference = if bytes.get(cursor) == Some(&b'{') {
            cursor += 1;
            let end = replacement[cursor..].find('}')? + cursor;
            let reference = &replacement[cursor..end];
            cursor = end + 1;
            reference
        } else {
            while bytes
                .get(cursor)
                .is_some_and(|byte| byte.is_ascii_alphanumeric() || *byte == b'_')
            {
                cursor += 1;
            }
            if cursor == start {
                literal.push('$');
                continue;
            }
            &replacement[start..cursor]
        };
        let capture =
            if reference.bytes().all(|byte| byte.is_ascii_digit()) && !reference.is_empty() {
                let index = reference.parse::<usize>().ok()?;
                if index >= regex.captures_len() {
                    return None;
                }
                index
            } else {
                regex
                    .capture_names()
                    .position(|name| name == Some(reference))?
            };
        if !literal.is_empty() {
            parts.push(ReplacementPart::Text(std::mem::take(&mut literal)));
        }
        parts.push(ReplacementPart::Capture(capture));
    }
    if !literal.is_empty() {
        parts.push(ReplacementPart::Text(literal));
    }

    Some(parts)
}

/// 单轮非重叠匹配，超过预算立即丢弃整条产物，不构造无界替换串。
fn apply_rule(input: &str, rule: &PreparedRule, language: Language) -> Result<String> {
    let mut output = String::new();
    let mut end = 0;
    match &rule.replacement {
        PreparedReplacement::Literal {
            pattern,
            replacement,
        } => {
            for (count, (start, matched)) in input.match_indices(pattern).enumerate() {
                check_match_count(count, rule.index, language)?;
                append_bounded(&mut output, &input[end..start], rule.index, language)?;
                append_bounded(&mut output, replacement, rule.index, language)?;
                end = start + matched.len();
            }
        }
        PreparedReplacement::Regex { pattern, parts } => {
            for (count, captures) in pattern.captures_iter(input).enumerate() {
                check_match_count(count, rule.index, language)?;
                let matched = captures
                    .get(0)
                    .expect("regex captures contain the full match");
                append_bounded(
                    &mut output,
                    &input[end..matched.start()],
                    rule.index,
                    language,
                )?;
                append_captures(&mut output, &captures, parts, rule.index, language)?;
                end = matched.end();
            }
        }
    }
    append_bounded(&mut output, &input[end..], rule.index, language)?;

    Ok(output)
}

fn check_match_count(count: usize, index: usize, language: Language) -> Result<()> {
    if count >= MAX_RULE_MATCHES {
        return Err(error(language, "too many matches", Some(index)));
    }

    Ok(())
}

/// 可选组未参与当前匹配时展开为空；组名和索引已经在准备阶段确认存在。
fn append_captures(
    output: &mut String,
    captures: &Captures<'_>,
    parts: &[ReplacementPart],
    index: usize,
    language: Language,
) -> Result<()> {
    for part in parts {
        let value = match part {
            ReplacementPart::Text(text) => text.as_str(),
            ReplacementPart::Capture(index) => {
                captures.get(*index).map_or("", |value| value.as_str())
            }
        };
        append_bounded(output, value, index, language)?;
    }

    Ok(())
}

/// 拼接前按 UTF-8 字节检查预算，避免先分配超限结果再拒绝。
fn append_bounded(
    output: &mut String,
    value: &str,
    index: usize,
    language: Language,
) -> Result<()> {
    if value.len() > MAX_RULE_OUTPUT_BYTES.saturating_sub(output.len()) {
        return Err(error(language, "output too large", Some(index)));
    }
    output.push_str(value);

    Ok(())
}
