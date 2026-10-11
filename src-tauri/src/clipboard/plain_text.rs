//! 当前系统剪贴板的独立纯文本转换，转换结果由正常监听链路按采集配置入库。
//!
//! 原生代次与快照复核降低读写竞态；最后复核和系统写入之间仍不是跨进程事务。
//! 系统写入可能先清空再失败，此时不重试或覆盖恢复旧内容。

use std::sync::atomic::{AtomicBool, Ordering};

use clipboard_rs::{Clipboard, ClipboardContent, ClipboardContext, ContentFormat};
use serde::Serialize;

use super::conversion_rules::{convert_text, PlainTextInputFormat};
use super::guard::WritebackGuard;
use crate::core::{AppError, Result};
use crate::db::items::content_hash;
use crate::db::models::ClipboardKind;
use crate::settings::{Language, PlainTextConversionSettings};

static CONVERSION_IN_FLIGHT: AtomicBool = AtomicBool::new(false);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum PlainTextConversionResult {
    Converted,
    Unchanged,
    Unsupported,
    Changed,
    Busy,
}

#[derive(Debug, PartialEq, Eq)]
struct TextSnapshot {
    formats: Vec<String>,
    text: Option<String>,
    html: Option<String>,
    rtf: Option<String>,
    has_non_text: bool,
}

struct InFlight;

impl Drop for InFlight {
    fn drop(&mut self) {
        CONVERSION_IN_FLIGHT.store(false, Ordering::Release);
    }
}

/// 读取当时系统内容，去除Markdown/HTML标记或原生富格式，写成唯一Text格式。
/// 所有原生句柄均在同步段内使用，调用方不得跨 await 持有它们。
pub fn convert_current_clipboard_to_plain_text(
    guard: &WritebackGuard,
    config: &PlainTextConversionSettings,
    language: Language,
) -> Result<PlainTextConversionResult> {
    if CONVERSION_IN_FLIGHT
        .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
        .is_err()
    {
        return Ok(PlainTextConversionResult::Busy);
    }
    let _in_flight = InFlight;
    let ctx = ClipboardContext::new().map_err(clip_err)?;
    let generation = clipboard_generation();
    let snapshot = read_snapshot(&ctx)?;
    if generation != clipboard_generation() {
        return Ok(PlainTextConversionResult::Changed);
    }

    let text = match conversion_text(&snapshot, config, language)? {
        Ok(text) => text,
        Err(outcome) => return Ok(outcome),
    };

    let current = read_snapshot(&ctx)?;
    if snapshot != current || generation != clipboard_generation() {
        return Ok(PlainTextConversionResult::Changed);
    }

    // 富文本历史写回可能留下纯文本回退指纹；显式转换只解除产物的抑制，供监听采集。
    guard.allow_capture(&content_hash(ClipboardKind::Text, text.trim()));
    if generation != clipboard_generation() {
        return Ok(PlainTextConversionResult::Changed);
    }

    if let Err(err) = ctx.set(vec![ClipboardContent::Text(text)]) {
        log::error!(
            "current clipboard plain text write failed; clipboard may be partially changed: {err}"
        );
        return Err(clip_err(err));
    }

    Ok(PlainTextConversionResult::Converted)
}

/// 图片或文件的 Text 回退不是富文本，不能丢掉这些原生内容。
fn conversion_text(
    snapshot: &TextSnapshot,
    config: &PlainTextConversionSettings,
    language: Language,
) -> Result<std::result::Result<String, PlainTextConversionResult>> {
    if snapshot.has_non_text {
        return Ok(Err(PlainTextConversionResult::Unsupported));
    }
    let has_rich = snapshot.html.is_some() || snapshot.rtf.is_some();
    let (input, format) = if let Some(text) = snapshot.text.as_ref().filter(|text| !text.is_empty())
    {
        (text.as_str(), PlainTextInputFormat::Text)
    } else if let Some(html) = &snapshot.html {
        (html.as_str(), PlainTextInputFormat::Html)
    } else if snapshot.rtf.is_some() || (snapshot.text.is_none() && !snapshot.formats.is_empty()) {
        return Ok(Err(PlainTextConversionResult::Unsupported));
    } else if let Some(text) = &snapshot.text {
        (text.as_str(), PlainTextInputFormat::Text)
    } else {
        return Ok(Err(PlainTextConversionResult::Unchanged));
    };

    let converted = convert_text(input, format, config, language)?;
    if !converted.custom_enabled && converted.extracted && converted.output.trim().is_empty() {
        return Ok(Err(PlainTextConversionResult::Unsupported));
    }
    if !has_rich && snapshot.text.as_deref() == Some(converted.output.as_str()) {
        return Ok(Err(PlainTextConversionResult::Unchanged));
    }

    Ok(Ok(converted.output))
}

/// 与历史采集的单一载荷读取分离；任何所需格式读取失败都不能当作空内容成功。
fn read_snapshot(ctx: &ClipboardContext) -> Result<TextSnapshot> {
    let mut formats = ctx.available_formats().map_err(clip_err)?;
    formats.sort();
    let has_non_text = ctx.has(ContentFormat::Image) || ctx.has(ContentFormat::Files);
    if has_non_text {
        return Ok(TextSnapshot {
            formats,
            text: None,
            html: None,
            rtf: None,
            has_non_text,
        });
    }

    let text = if ctx.has(ContentFormat::Text) {
        Some(ctx.get_text().map_err(clip_err)?)
    } else {
        None
    };
    let html = if ctx.has(ContentFormat::Html) {
        Some(ctx.get_html().map_err(clip_err)?)
    } else {
        None
    };
    let rtf = if ctx.has(ContentFormat::Rtf) {
        Some(ctx.get_rich_text().map_err(clip_err)?)
    } else {
        None
    };

    Ok(TextSnapshot {
        formats,
        text,
        html,
        rtf,
        has_non_text,
    })
}

/// 包括同内容再次复制的原生代次；Windows 延迟呈现也可能使代次变化并安全取消。
#[cfg(target_os = "windows")]
pub(super) fn clipboard_generation() -> u64 {
    unsafe { winapi::um::winuser::GetClipboardSequenceNumber() as u64 }
}

#[cfg(target_os = "macos")]
pub(super) fn clipboard_generation() -> u64 {
    objc2_app_kit::NSPasteboard::generalPasteboard().changeCount() as u64
}

fn clip_err<E: std::fmt::Display>(err: E) -> AppError {
    AppError::Clipboard(err.to_string())
}
