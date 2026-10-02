# Clipboard Pipeline

## Capture Flow

The main capture flow is:

```text
OS clipboard change
  -> clipboard::watcher
  -> clipboard::read::ClipboardReader
  -> clipboard::payload::ClipboardPayload
  -> clipboard::ingest::build_item_with_settings
  -> db::items::upsert_item
  -> clipboard://updated
  -> React list fetches enriched item/page
```

Manual `read_clipboard` reuses the same `build_item_with_settings` and
`persist_and_notify` path, so manual and automatic capture share dedupe,
source-app upsert, copy sound, and event semantics.

Reference files:
- `src-tauri/src/clipboard/watcher.rs`
- `src-tauri/src/clipboard/read.rs`
- `src-tauri/src/clipboard/payload.rs`
- `src-tauri/src/clipboard/ingest.rs`
- `src-tauri/src/db/items.rs`
- `src-tauri/src/commands/clipboard.rs`

## Transient Clipboard Read Failures

On Windows, a clipboard change notification can arrive while another listener
briefly holds the clipboard open. The watcher must treat a reader `Err` as
transient and retry within a bounded window before dropping that update. The
current policy performs one initial read followed by delays of 15 ms, 35 ms,
and 75 ms (four attempts and at most 125 ms total delay).

`read_with_retry` returns immediately for both `Ok(Some(payload))` and
`Ok(None)`. Only `Err` is retried, and an exhausted sequence is returned to the
existing watcher error branch so it emits one final warning rather than one
warning per attempt.

```rust
read_with_retry(&CLIPBOARD_READ_RETRY_DELAYS, || {
    reader.read_with_capture(capture_settings)
})
```

Tests must assert the attempt count for immediate success, transient recovery,
empty content, and exhaustion. Use zero-duration delays in unit tests; native
clipboard contention belongs in ignored desktop-session tests or manual Windows
validation with another clipboard listener enabled.

## Payload and Storage Semantics

`ClipboardPayload` is a typed read result, not a database model. The read layer
only classifies one copy as text, image, or files based on the user-configured
capture order.

`build_item_with_settings` decides persisted fields:

- Text `content` is the selected source representation: HTML source, RTF source,
  or plain text.
- Text `search_text` is the OS plain text representation for FTS and plain paste.
- Text `summary` is a trimmed, capped list preview.
- Plain text subtypes (`url`, `email`, `color`, `path`) are detected in Rust by
  `clipboard/detect.rs`.
- Images are stored as PNG resources; the database `content` is the image file
  name.
- Files store newline-separated paths in `content` and compact file kind markers
  in `file_types`.

Do not parse HTML/RTF, infer color safety, or split file entries in React when a
command can return a prepared field. `ClipboardItem` already includes
`fileEntries`, `filesPreviewKind`, `colorPreview`, `availableActions`,
`displayCreatedAt`, `imageThumbnailPath`, and source app icon paths after command
enrichment.

## Dedupe and Reuse

`db::items::content_hash` hashes `"<kind>:<content>"` with blake3. `upsert_item`
checks this hash:

- New content inserts a row.
- Existing content increments `use_count`, refreshes `updated_at`, and returns
  `deduplicated = true`.

Metadata edits such as notes, favorites, pins, and groups must not refresh
`updated_at`; that timestamp means "content was reused." This is called out in
`db::items::update_item_group` and the project rules.

## Loop Suppression

Writing history back to the system clipboard would normally trigger the watcher.
`clipboard::write` calls `WritebackGuard::suppress` before writing, and the
watcher calls `should_skip` after rebuilding the item hash. The guard stores
content hashes with a short TTL, not a global boolean, so a real unrelated copy
is not swallowed.

If writeback behavior changes, keep the hash computed by the write path aligned
with the hash the watcher will compute after reading the OS clipboard.

Reference files:
- `src-tauri/src/clipboard/write.rs`
- `src-tauri/src/clipboard/guard.rs`

## Sensitive Content

Secret detection lives in `clipboard/secrets.rs` and is applied during ingest.
Settings can skip saving secrets or save them with `is_sensitive = true`.
List and preview commands apply redaction based on the current settings before
returning payloads.

React should display `isSensitive` and render the provided text. It should not
try to run its own secret detector.

## Source Applications and Icons

The watcher captures the frontmost app before reading the clipboard because
foreground state can change by the time async persistence runs. Source apps are
materialized through `AppIconStore` and `AppsRegistry`, then upserted into
`clipboard_apps`. App upsert failure logs a warning and clears `source_app_id`
instead of dropping the clipboard item.

Source app IDs are platform-specific:

- macOS: bundle id.
- Windows: executable path.

The command layer resolves icon file names into absolute paths for React.

## Tests

Clipboard logic has focused Rust tests for pure parsing, ingestion, storage,
loop suppression, and repository behavior. Real system clipboard round trips are
marked `#[ignore = "touches the real system clipboard; ..."]` in
`clipboard/read.rs`, `clipboard/write.rs`, and `clipboard/watcher.rs`. Run those
ignored tests only in a desktop session when changing native clipboard behavior.

## Scenario: Current Clipboard Plain-Text Conversion

### 1. Scope / Trigger

`clipboard/plain_text.rs` converts current OS content independently of history,
capture filters, and window visibility. This is different from plain-pasting a
selected history row.

### 2. Signatures

- `convert_current_clipboard_to_plain_text(&WritebackGuard) -> Result<PlainTextConversionResult>`
- Tauri command `convert_current_clipboard_to_plain_text(AppHandle)` delegates directly.
- Results serialize as `converted | unchanged | unsupported | changed | busy`.

### 3. Contracts

Explicit Markdown/HTML in native Text is parsed; HTML-only uses structured DOM extraction. Native Text without markup keeps exact whitespace when rich formats are stripped. Text-only ordinary/empty content, images/files/mixed non-text and RTF-only are preserved. No history ID,
database insertion, reuse timestamp, simulated paste, or window side effect.

AtomicBool/RAII prevents concurrent work. OS clipboard generation plus a second
snapshot and final pre-write generation check cancel detected changes. This is
not a cross-process atomic transaction; `set` may partially change contents on
failure. Do not retry or overwrite a newer clipboard with a recovery snapshot.

Tracked Guard tokens cancel only the current registration on failure. Hash the
same trimmed content as ingest while writing the original untrimmed Text.
Successful conversion tokens also require the written clipboard generation,
so a later real copy of identical text is not suppressed after paused capture.

The watcher registers `Arc<WritebackGuard>` as Tauri managed state. Command and
global conversion entry points must retrieve that exact type and pass
`guard.inner().as_ref()` to the conversion service. Tauri state uses exact type
identity: retrieving `WritebackGuard` directly panics before conversion, and a
fresh guard would break suppression shared with the watcher. Low-level service
tests alone cannot verify this application-state wiring.

### 4. Validation & Error Matrix

- Current content changed -> `changed`, no write; global path reports cancellation.
- Conversion in flight -> `busy`, no write.
- Unsupported formats -> `unsupported`, no write; native localized feedback.
- Read/write failure -> AppError; cancel this Guard token, report possible partial write.

### 5. Good/Base/Bad Cases

- Good: native Text plus HTML becomes only Text with original line endings.
- Base: plain Text remains unchanged.
- Bad: HTML-only markup must never be returned as purported plain Text.

### 6. Tests Required

Fault-inject read/write errors, changed generations/snapshots, concurrent calls,
tracked cancellation, trimmed hashes, and later identical real copy. Real OS
format/focus assertions require Windows and macOS desktop sessions. Unavailable
toolchains/devices must be recorded as pending, not counted as passing.

### 7. Wrong vs Correct

Wrong: route current conversion through `paste_clipboard_item(history_id, true)`.
Correct: call the independent conversion service with the shared WritebackGuard.

### Markup parsing contract (2026-10-02)

`markup::convert_markup(&str) -> Option<String>` recognizes explicit CommonMark
syntax plus tables/strikethrough/task lists. None means preserve the native Text
exactly. `html_to_text(&str) -> String` traverses an HTML5 DOM without executing
scripts or loading resources. `pulldown-cmark = 0.13` (locked 0.13.4, upstream
rust-version 1.71.1) and existing transitive `dom_query = 0.27` (now direct,
rust-version 1.75) are compiled on the project Rust 1.96 baseline; those upstream
MSRVs are not a newly verified project minimum.

Links retain titles, images retain alt, table cells use tabs, and list indentation
remains without generated bullets or ordered numbers. Remove markers through DOM
list structure, never by deleting dashes from rendered text. Escaped Markdown
markers are literal body text; `a-b`, negative values, and dashes inside code remain.
Fenced/inline code is escaped by the Markdown renderer,
so `<b>` / `**` inside code remain literal; pre preserves internal whitespace.
Ordinary comparison/generic/path text stays intact. HTML whitespace collapses
outside pre; entities decode, block boundaries/br produce line breaks, and
script/style/head/template/noscript/iframe/object/embed subtrees are skipped.
Native readable Text takes precedence over a rich HTML fallback; empty/missing
native Text may fall back to HTML. Empty rendered output is unsupported and never
replaces the clipboard. Unknown Text-only tags are conservatively preserved.

Parse before the second snapshot/generation check. Guard hashes the final output
using the ingest trim rule, while the native write retains the output whitespace.
Good: `**重要**` and `<b>重要</b>` -> `重要`; HTML-only -> Text. Bad: regex removal
of stars/angle brackets or parsing escaped code as HTML. Verification must cover
markup samples plus the existing write failure, snapshot mutation, in-flight and
same-content real-copy Guard assertions; OS focus/format checks remain separate.
