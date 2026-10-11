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

Current-clipboard plain-text conversion intentionally does not register with
`WritebackGuard`: its newly generated result must pass through ordinary watcher
capture and persistence. History copy/paste still uses the shared guard. Do not
apply a blanket suppression rule to every application-originated clipboard write.
Before conversion writes, `allow_capture` removes a stale suppression for the
final trimmed Text hash, including a leftover plain fallback from a rich history
write. Other unexpired hashes remain suppressed; expired entries may be pruned.

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

`clipboard/plain_text.rs` converts current OS content without requiring a history
ID or a visible window. Native conversion itself is independent of capture
filters; storing its result follows normal watcher policy. This is different
from plain-pasting a selected history row.

### 2. Signatures

- `convert_current_clipboard_to_plain_text(&WritebackGuard, &PlainTextConversionSettings, Language) -> Result<PlainTextConversionResult>`
- Tauri command `convert_current_clipboard_to_plain_text(AppHandle)` uses the shared
  success-feedback entry point and the existing shared Guard state.
- `WritebackGuard::allow_capture(&self, content_hash: &str)` removes matching
  suppression and expired entries while retaining other unexpired hashes.
- Results serialize as `converted | unchanged | unsupported | changed | busy`.
- The existing `persist_and_notify(&AppHandle, &SqlitePool, &ClipboardItem,
  Option<&ClipboardApp>) -> Result<UpsertResult>` path emits `clipboard://updated`
  with `{ id, kind, deduplicated }` after upsert.

### 3. Contracts

Explicit Markdown/HTML in native Text is parsed; HTML-only uses structured DOM extraction. Native Text without markup keeps exact whitespace before custom replacement. With no enabled custom rules, text-only ordinary/empty content is preserved; images/files/mixed non-text and RTF-only remain unsupported regardless of rules. No history ID,
simulated paste, or direct database write is required by the conversion service.

AtomicBool/RAII prevents concurrent work. OS clipboard generation plus a second
snapshot and final pre-write generation check cancel detected changes. This is
not a cross-process atomic transaction; `set` may partially change contents on
failure. Do not retry or overwrite a newer clipboard with a recovery snapshot.

Conversion does not register or confirm its own WritebackGuard token. Rich history
writes can register both rich and plain fallback hashes; consuming one watcher
echo may leave the other pending. Before native write, conversion calls
`allow_capture` for its final Text hash using the same trim rule as ingest, then
performs the final generation check before `set`. This releases a matching stale
fallback without clearing other valid suppressions. Conversion uses no tracked
token cancellation or written-generation confirmation mechanism.

The native write triggers the ordinary watcher flow: pause/source/capture checks,
sensitive content policy, ingestion, content-hash dedupe, persistence, and then the
existing update event. Ingest still computes captured item hashes normally. Do not
directly insert a second history row or emit an update before persistence to make
the result visible.

When capture allows saving, new plain text is stored and existing identical text
uses normal reuse semantics. Visible lists react to the event; hidden lists use
the existing pending refresh when reopened. Preserve search, filters, groups, and
sort order; a row excluded by them is not forced visible. Paused capture, disabled
text capture, excluded source apps, or sensitive skip policy may prevent saving
even though native conversion succeeds. Do not bypass those policies or promise
backfill after capture resumes. The success hint means the system write succeeded,
not that a history row was saved. Persistence failure follows watcher logging and
must not retry or roll back native conversion.

History copy/paste and conversion use the single `Arc<WritebackGuard>` managed state
shared with the watcher. Tauri uses exact type identity: retrieve
`Arc<WritebackGuard>` and pass `guard.inner().as_ref()`, not a fresh guard or bare
`WritebackGuard`. History write registration stays unchanged; only explicit
conversion releases suppression for its own final Text hash.

### 4. Validation & Error Matrix

- Current content changed -> `changed`, no write; global path reports cancellation.
- Conversion in flight -> `busy`, no write.
- Unsupported formats -> `unsupported`, no write; native localized feedback.
- Read/write failure -> AppError; report possible partial write without retry.
- Stale suppression matches final Text hash -> remove it before final generation
  check/write; keep other live hashes and do not register conversion suppression.
- Converted with capture allowed -> watcher persists/deduplicates and emits update.
- Converted with capture paused/filtered/disabled -> native success; no forced save.
- Converted while current list filters exclude the row -> no forced filter change.
- Watcher persistence failure -> log; successful native conversion is not undone.

### 5. Good/Base/Bad Cases

- Good: native Text plus HTML becomes only Text with original line endings, then
  normal capture persists it and updates a matching visible history list.
- Base: plain Text remains unchanged when no custom rule changes it.
- Bad: HTML-only markup must never be returned as purported plain Text.
- Bad: a conversion Guard swallows the only watcher event, or direct DB insertion
  bypasses sensitive/source/capture policies to force a visible row.

### 6. Tests Required

Check read/write errors, changed generations/snapshots, and concurrent calls.
Assert conversion does not register the history writeback guard; history writes
still suppress their watcher echoes. Check rich/plain fallback registration:
after the rich echo is consumed, explicit conversion releases the final plain
hash while unrelated unexpired hashes still suppress. Capture checks cover new text insertion,
identical-text dedupe, update events after persistence, paused/disabled/excluded/
sensitive skips, and existing visible/hidden-list refresh without filter changes.
Real OS format/focus/list assertions require Windows and macOS desktop sessions.
If the user owns those checks, do not send keys or access the clipboard on their
behalf; record behavior as pending. Builds and artifact checks cannot replace it.

### 7. Wrong vs Correct

Wrong: route current conversion through `paste_clipboard_item(history_id, true)`.
Wrong: suppress conversion writeback, then force an extra DB write to refresh UI.
Wrong: remove conversion registration but leave a matching rich-write plain
fallback pending, or clear the entire shared guard.
Correct: use the shared Guard only to release the final Text hash before write,
without registering conversion suppression;
allow the existing watcher to persist permitted content and emit its normal event.

### Markup parsing contract

`markup::convert_markup(&str, &PlainTextConversionSettings) -> Option<String>` recognizes explicit CommonMark
syntax plus tables/strikethrough/task lists. None means preserve the native Text
exactly before custom replacements. `html_to_text(&str, &PlainTextConversionSettings) -> String` traverses an HTML5 DOM without executing
scripts or loading resources. `pulldown-cmark = 0.13` (locked 0.13.4, upstream
rust-version 1.71.1) and existing transitive `dom_query = 0.27` (now direct,
rust-version 1.75) are compiled on the project Rust 1.96 baseline; those upstream
MSRVs are not a newly verified project minimum.

Default rules retain link titles and image alt, use table tabs, and keep list indentation
without generated bullets or ordered numbers. Built-in options can change these
presentations outside protected pre/code ranges. Remove markers through DOM
list structure, never by deleting dashes from rendered text. Escaped Markdown
markers are literal body text; `a-b`, negative values, and dashes inside code remain.
Fenced/inline code is escaped by the Markdown renderer,
so `<b>` / `**` inside code remain literal; pre preserves internal whitespace.
Ordinary comparison/generic/path text stays intact. HTML whitespace collapses
outside pre; entities decode, block boundaries/br produce line breaks, and
script/style/head/template/noscript/iframe/object/embed subtrees are skipped.
Native readable Text takes precedence over a rich HTML fallback; empty/missing
native Text may fall back to HTML. With no enabled custom replacements, empty
rendered output is unsupported and never replaces the clipboard. Custom stages
may consume intermediate empty output, subject to final nonblank protection.
Unknown Text-only tags are conservatively preserved.

Parse before the second snapshot/generation check. Native write retains output
whitespace; conversion uses the trimmed Text hash only to release stale suppression,
and subsequent watcher ingest owns captured item hashing and dedupe.
Good: `**重要**` and `<b>重要</b>` -> `重要`; HTML-only -> Text. Bad: regex removal
of stars/angle brackets or parsing escaped code as HTML. Verification must cover
markup samples plus write failure, snapshot mutation, in-flight protection, and
normal capture of the conversion result. Historical copy/paste Guard assertions
remain separate from conversion; OS focus/format/list checks remain separate.

## Scenario: Configurable Formatting Rules

### 1. Scope / Trigger

`clipboard/conversion_rules.rs` is the shared text core for active current-clipboard
conversion and manual-example preview. Rules do not affect history copyPlain or
pastePlain, capture classification, search, content preview, or export. Persisted
model, recovery, and editor revision contracts live in settings-window-platform.md.

### 2. Signatures

- `validate_plain_text_conversion(&PlainTextConversionSettings, Language) -> Result<()>`.
- `convert_text(&str, PlainTextInputFormat, &PlainTextConversionSettings, Language)`
  returns output plus extraction/custom-stage facts internally.
- `preview_plain_text_conversion(String, PlainTextInputFormat, PlainTextConversionSettings, Language)`
  returns `{ output: String, changed: bool }`; input format is `text | html`.
- The AppHandle command and global shortcut share one settings/language snapshot
  through `convert_current_clipboard_with_feedback`.

### 3. Contracts

First extract CommonMark/HTML with built-in options, then apply enabled custom rules
in array order. Each performs one global non-overlapping replacement against the
previous output; do not iterate to a fixed point or parse inserted markup again.
Plain Text also reaches this stage. Custom rules cover the full output including
code, while built-in choices retain the previous default extraction within pre/code.
Collapsing body blank lines retains at most one blank line outside protected code
ranges; unmarked native Text does not use this built-in normalization.

Literal patterns are case-sensitive and replacements are exact text. Regex uses
Rust regex, including inline flags and its empty-match iteration. Replacement
supports `$1`, `${1}`, `$name`, `${name}`, and `$$`; unknown captures and `\1`
are invalid. A valid optional group that did not match expands to empty. Do not
interpret literal `\n` as a line break or trim pattern/replacement whitespace.

All saved rules, including disabled entries, require a unique parsed UUID,
nonempty trimmed name of at most 80 characters, nonempty pattern, and no NUL in
pattern/replacement. Rule count is at most 32; pattern/replacement are each at
most 4 KiB UTF-8. RegexBuilder compiled and DFA size limits are each 1 MiB,
with nesting 64. Disabled rules are validated but never executed.

With at least one enabled custom rule, the first input is at most 1 MiB, each
rule output and next input at most 4 MiB, and each rule at most 10,000 matches.
Bound output while appending, before allocation growth exceeds its budget; do not
use unbounded replace_all followed by a length check. Final empty/all-whitespace
or NUL output cancels before OS write. Intermediate empty output may feed later
rules. With no enabled rules preserve old base empty-content semantics, including
whitespace Text with rich formats that only needs rich-format removal.

Preview uses the same core and budgets on explicitly entered text/HTML and a draft
configuration. It does not read/write the system clipboard, persist history or
settings, emit list events, or show the native success hint. The UI displays pure
text and discards stale responses. Preview does not simulate native format choice
or OS generation checks. Blocking workers avoid running synchronous rule work in
async command execution; they cannot hard-cancel a running regex search.

### 4. Validation & Error Matrix

- Invalid saved draft, including disabled regex/capture -> localized rule-index
  error; no persistence or native write.
- Enabled custom runtime budget, final blank result, or NUL -> cancel entire
  conversion; do not write partial output or emit success.
- Valid output equal to native Text with no rich formats -> unchanged.
- Rule error from the global shortcut -> localized safe rule message; native
  clipboard read/write errors retain the existing generic feedback.
- Logs must not include sample/clipboard/pattern/replacement text or underlying
  regex compilation details; use stable reasons and numeric context only.

### 5. Good/Base/Bad Cases

Good: a literal rule changes ordinary `old` to `new`, and the normal watcher captures
the successfully written result if policy permits. Base: absent configuration has
default built-in output and no replacements. Bad: a custom regex replaces HTML
parsing, modifies history paste, or runs unbounded before checking output size.

### 6. Tests Required

Behavioral coverage includes default output, all built-ins, code protection,
Unicode/real whitespace, rule order and disables, valid/invalid captures, optional
and empty matches, UUID/NUL constraints, and exact/over-budget limits. Separately
verify preview parity and absence of OS/persistence side effects. Real native
conversion, settings interaction, focus, feedback, and capture acceptance remain
user-owned when requested; static checks and artifact hashes do not establish them.

Required observable assertions, pending independent functional verification:

- Defaults: `**重要**` -> `重要`; a plain `a-b` retains identical text.
- Ordered literal rules `old -> middle`, then `middle -> new` produce `new`;
  disabling the second produces `middle`.
- Link titleAndUrl on `[标题](https://example.com)` produces
  `标题 (https://example.com)`; default produces `标题`.
- A regex with a real capture accepts `$1`; an unknown `$missing` rejects even
  on a disabled rule. Literal `$1` remains the two characters `$1`.
- 32 valid rules save; 33 reject. A 4 KiB pattern saves and a larger one rejects.
- A final whitespace or NUL custom result rejects before native set; the prior
  clipboard and absence of a success hint are checked separately on desktop.
- Preview and active conversion core return the same output for the same selected
  source/format/config; preview causes no clipboard write, DB upsert, or list event.

### 7. Wrong vs Correct

Wrong: execute user regex as the markup parser or reparse replacement text.
Correct: structured extraction first, bounded ordered replacements second, then
the existing native snapshot/generation checks and normal watcher capture path.
