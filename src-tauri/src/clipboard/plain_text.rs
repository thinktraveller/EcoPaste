//! 当前系统剪贴板的独立纯文本转换，不经过历史记录或采集配置。
//!
//! 原生代次与快照复核降低读写竞态；最后复核和系统写入之间仍不是跨进程事务。
//! 系统写入可能先清空再失败，此时只撤销本次 Guard，不重试或覆盖恢复旧内容。

use std::sync::atomic::{AtomicBool, Ordering};

use clipboard_rs::{Clipboard, ClipboardContent, ClipboardContext, ContentFormat};
use serde::Serialize;

use super::guard::WritebackGuard;
use super::markup::{convert_markup, html_to_text};
use crate::core::{AppError, Result};
use crate::db::items::content_hash;
use crate::db::models::ClipboardKind;

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

    let text = match conversion_text(&snapshot) {
        Ok(text) => text,
        Err(outcome) => return Ok(outcome),
    };

    let current = read_snapshot(&ctx)?;
    if snapshot != current || generation != clipboard_generation() {
        return Ok(PlainTextConversionResult::Changed);
    }

    // ingest 对纯文本内容做 trim；保留系统原文，同时对齐监听入库的哈希。
    let token = guard.suppress_tracked(content_hash(ClipboardKind::Text, text.trim()));
    if generation != clipboard_generation() {
        guard.cancel(token);
        return Ok(PlainTextConversionResult::Changed);
    }

    if let Err(err) = ctx.set(vec![ClipboardContent::Text(text)]) {
        guard.cancel(token);
        log::error!(
            "current clipboard plain text write failed; clipboard may be partially changed: {err}"
        );
        return Err(clip_err(err));
    }
    guard.mark_written(&token, clipboard_generation());

    Ok(PlainTextConversionResult::Converted)
}

/// 图片或文件的 Text 回退不是富文本，不能丢掉这些原生内容。
fn conversion_text(
    snapshot: &TextSnapshot,
) -> std::result::Result<String, PlainTextConversionResult> {
    if snapshot.has_non_text {
        return Err(PlainTextConversionResult::Unsupported);
    }
    let has_rich = snapshot.html.is_some() || snapshot.rtf.is_some();
    if let Some(text) = snapshot.text.as_ref().filter(|text| !text.is_empty()) {
        if let Some(converted) = convert_markup(text) {
            if converted.trim().is_empty() {
                return Err(PlainTextConversionResult::Unsupported);
            }
            if converted != *text || has_rich {
                return Ok(converted);
            }
        }
        return if has_rich {
            Ok(text.clone())
        } else {
            Err(PlainTextConversionResult::Unchanged)
        };
    }
    if let Some(html) = &snapshot.html {
        let converted = html_to_text(html);
        return if converted.trim().is_empty() {
            Err(PlainTextConversionResult::Unsupported)
        } else {
            Ok(converted)
        };
    }
    if snapshot.rtf.is_some() || (snapshot.text.is_none() && !snapshot.formats.is_empty()) {
        return Err(PlainTextConversionResult::Unsupported);
    }
    Err(PlainTextConversionResult::Unchanged)
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
