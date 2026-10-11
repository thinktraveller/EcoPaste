# Settings, Windowing, and Platform

## Settings Model

Settings live in Rust under `src-tauri/src/settings/`.

- `model.rs` defines the serialized shape, defaults, enums, and helper methods.
- Top-level and grouped settings use `#[serde(default, rename_all = "camelCase")]`,
  so newly added fields fall back to defaults. Individual custom replacement
  rules require complete fields rather than completing malformed saved entries.
- `store.rs` persists `<app_data_dir>/config/settings.json` with a temp file and
  atomic rename.
- `SettingsStore::update` accepts only object patches, deep-merges objects,
  replaces arrays wholesale, validates cross-field constraints, writes to disk,
  and updates the in-memory snapshot.

When adding a setting, update:

- Rust model and defaults.
- Validation if the field interacts with another field.
- TypeScript `src/types/settings.ts`.
- Preference schema in `src/pages/Preference/config/preferenceSchema.ts` if the
  setting is user-visible.
- zh-CN and en-US locale entries.
- Side effects in `commands/settings.rs` or
  `src/pages/Preference/services/preferenceSettings.ts` when OS state changes
  immediately.

### Window Open Selection Settings

#### 1. Scope / Trigger

Changing the default filter applied when the clipboard window opens is a
settings contract change. Rust owns the persisted shape; React only mirrors it
and applies the transient `clipboardViewState` side effect on `window://visibility`.

#### 2. Signatures

- Rust fields:
  - `Window.select_range_on_open: WindowOpenRangeSelection`
  - `Window.select_category_on_open: WindowOpenCategorySelection`
  - `Window.select_group_on_open: String`
- TypeScript mirror:
  - `Window.selectRangeOnOpen: "preserve" | ClipboardRange`
  - `Window.selectCategoryOnOpen: "preserve" | "all" | ClipboardCategory`
  - `Window.selectGroupOnOpen: "preserve" | "all" | \`group:${string}\``

#### 3. Contracts

- `preserve` means keep the user's last selected value for that dimension.
- `all` means clear that dimension to the built-in All state.
- `group:<id>` means select the custom clipboard group with that id.
- Range options are `preserve`, `all`, and `favorite`.
- Category options are `preserve`, `all`, `text`, `image`, and `files`.

#### 4. Validation & Error Matrix

- Invalid range/category literals fail serde deserialization.
- `select_group_on_open = "preserve"` -> valid.
- `select_group_on_open = "all"` -> valid.
- `select_group_on_open = "group:<non-empty id>"` -> valid.
- Any other group string, including `group:` -> `AppError::Other("open group selection is invalid")`.

#### 5. Good/Base/Bad Cases

- Good: selecting `favorite`, `image`, and `group:<uuid>` opens into favorites,
  image category, and that custom group.
- Base: all three fields default to `preserve`, so opening the window keeps the
  user's last filters.
- Bad: storing a raw group UUID without the `group:` prefix is rejected.

#### 6. Tests Required

- Missing-field defaults assert all three fields default to `preserve`.
- Settings validation rejects invalid `select_group_on_open` strings.
- Frontend type-check covers the mirrored selection unions and the dynamic group
  Select value shape.

#### 7. Wrong vs Correct

Wrong:

```json
{ "clipboard": { "window": { "selectGroupOnOpen": "550e8400-e29b" } } }
```

Correct:

```json
{ "clipboard": { "window": { "selectGroupOnOpen": "group:550e8400-e29b" } } }
```

## Settings Events and Mirrors

Rust is the source of truth. `commands::settings::update_settings` and
`reset_settings` emit `settings://updated` after persistence and side effects.
`src/stores/settings.ts` listens to that event and updates the Valtio mirror.

## Scenario: Custom Formatting Rule Settings

### 1. Scope / Trigger

Active current-clipboard conversion has a new additive
`clipboard.plainTextConversion` settings subtree. Rust owns persistence,
validation, recovery, and execution. A dedicated preference action under the
Reuse tab (Chinese display label 操作) opens the rule editor; React holds only a
local draft. Clipboard-pipeline.md defines execution and resource budgets.

### 2. Signatures

- `PlainTextConversionSettings` fields serialize as `linkStyle`, `imageStyle`,
  `listStyle`, `taskStyle`, `tableSeparator`, `collapseBlankLines`, and `rules`.
- Enum values: `title | titleAndUrl | url`, `alt | omit`,
  `textOnly | readableMarkers`, `status | textOnly`, and `tab | space`.
- Rules: `{ id, name, enabled, mode: literal | regex, pattern, replacement }`.
- `get_plain_text_conversion_editor(AppHandle)` returns
  `{ config, recovery: JSON | null, diagnostics: string[], revision: string }`.
- `save_plain_text_conversion_settings(AppHandle, config, expected_revision)`
  returns the complete `Settings`; JS arguments use `expectedRevision`.
- `preview_plain_text_conversion(AppHandle, input, input_format, config)` returns
  `{ output, changed }`; JS arguments use `inputFormat: text | html`.
- `SettingsStore::prepare_from_file(&Path)` validates a full replacement without
  writing. `commit_snapshot_with_patch` preserves explicit-repair intent through
  ordinary and shortcut updates.

### 3. Contracts

Missing fields use serde defaults: link title, image alt, text-only list indentation,
task status, tab columns, no blank-line collapse, and no custom rules. No schema
or migration change is needed; this does not claim old-binary round-trip retention
of the new subtree. `CustomReplacementRule` requires complete typed fields rather
than silently completing a damaged rule. TS types/default builder mirror Rust.

Startup/rebase remove the new subtree before deserializing unrelated settings,
then validate it separately. Valid rules become the runtime snapshot. A malformed
or semantically invalid subtree falls back only this feature to defaults, retains
the original JSON in session recovery state, and records a localized editor
diagnostic. Startup never writes the fallback over the source file. If the raw
subtree is typed, the editor returns it for repair even when regex validation
fails; otherwise it returns safe defaults plus the raw JSON. JSON null is a valid
preserved damaged value, so the UI uses nonempty diagnostics to show recovery,
not merely a `recovery != null` check.

Unrelated settings updates validate the safe effective candidate and write the
preserved damaged subtree back unchanged. Do not make invalid loaded rules block
unrelated saves or disappear on the next save. Explicit updates to this subtree
are strict and clear recovery only after their successful disk write. Dedicated
save/default-save, global reset, and successful full replacement also clear it.
Rebase replaces recovery with the new location's load result.

Editor get/save serialize access with the existing operation gate. Revision is
blake3 over the editable raw/effective subtree with recursive object-key sorting;
array order remains significant. Unrelated updates do not conflict. A dedicated
save compares expected revision while holding the gate, validates the complete
config, atomically persists, updates memory, then emits settings://updated.
Conflict or validation/write failure preserves effective settings, recovery, and
draft. Ordinary update/shortcut prepare and commit also validate complete rules.

Backup merge validates its settings candidate through prepare_update; full
overwrite uses prepare_from_file. This preflight runs before changing production
history, live database, or resources, so invalid new rules cannot partially import
history before rejection. Actual settings application remains strict. This is
pre-validation of rule/settings errors, not a new all-files backup transaction or
a guarantee against later IO/native registration failure.

Dedicated editor get/save and preview use blocking workers for synchronous
regex/IO work. Acquire State inside the moved AppHandle closure, and never retain
locks across await. Join failures use stable logs and localized short errors.

The Ant Design Modal edits an independent draft; cancel leaves runtime unchanged,
restore-defaults edits only this draft, and save failure retains it. The rule
Virtuoso list lives inside a container with an explicit UnoCSS height; its default
percentage height otherwise collapses in the auto-sized modal body. Adding and
reopening rules must render editable cards, including in a small viewport.
Pattern and replacement preserve exact whitespace through dedicated textareas. Rule cards use
Virtuoso, with UUID keys and at most 32 entries. Each modal instance uses its own
window dirty owner to prevent idle destruction, with paired late-request cleanup.
Preview only processes manually entered examples using the draft; stale responses
cannot overwrite newer inputs, and output is displayed as text.

### 4. Validation & Error Matrix

- Missing new subtree/fields -> compatible defaults.
- Malformed loaded subtree/invalid regex -> local recovery, other settings intact,
  no startup rewrite; original JSON available for repair.
- Unrelated save during recovery -> succeeds under normal validation and keeps
  raw damaged subtree and diagnostics.
- Invalid explicit patch/full import -> reject before persistence; backup preflight
  rejects before history/DB/resource side effects.
- Stale revision -> localized conflict, no overwrite.
- Successful explicit repair/default-save/reset/full replacement -> clear recovery.
- Settings-file/patch typed parse errors for the new subtree -> localized generic configuration error,
  never a serde message echoing raw rule text.

### 5. Good/Base/Bad Cases

Good: bad regex is isolated on load; a theme save keeps its raw source; the user
repairs and explicitly saves it. Base: an old file without the subtree produces
the old conversion output. Bad: sanitize to defaults and discard the raw subtree
when the next unrelated preference is saved, or validate imported rules only
after replacing live history.

### 6. Tests Required

Use independent settings copies for missing-field compatibility, malformed and
semantic recovery, retained original JSON after unrelated updates, explicit repair,
reset, import preflight, rebase, and revision conflicts. UI acceptance covers draft
cancel/failure/defaults, saved reload, virtual scrolling, preview parity, recovery
display including raw null, and window dirty-owner lifecycle. When the user owns
functional testing, record it as pending; lint/typecheck/build are separate evidence.

Required observable assertions, pending independent functional verification:

- Loading `{ "general": { "autoStart": true } }` keeps autoStart true and yields
  default title/alt/textOnly/status/tab, collapse false, and an empty rules array.
- A copied valid settings file with only a bad new regex retains its original
  unrelated fields, exposes nonempty diagnostics/raw recovery, and is byte-identical
  on disk immediately after load.
- Saving an unrelated theme change retains JSON equality of the original bad
  subtree and keeps recovery; successful explicit correction clears diagnostics
  and persists the corrected complete subtree.
- A draft opened at revision A is rejected after another save produces revision B;
  changing only a theme does not alter the rule revision.
- An invalid-rule backup rejects before any production history row/resource/database
  is changed; a valid replacement clears prior recovery after persistence.
- Cancel/default-only edit leaves saved config unchanged; saving defaults changes
  only this subtree. Closing an old modal cannot clear a new modal's dirty owner.

### 7. Wrong vs Correct

Wrong: deserialize the full file with invalid new rules, return all defaults, then
write them on the next theme change. Correct: isolate only the new subtree, retain
its raw JSON, preserve it on unrelated saves, and clear recovery after explicit
successful repair.

## Scenario: Configurable Clipboard Shortcuts

### 1. Scope / Trigger

Window action rebinding spans persisted settings, React keyboard events, Windows
non-focusable navigation hooks, native menus, and global shortcut registration.

### 2. Signatures

- `Shortcuts.convert_to_plain_text: String` -> `convertToPlainText`.
- `Shortcuts.window_actions: BTreeMap<String, String>` -> `windowActions`.
- `get_shortcut_status` returns `{ action, binding, active, error }[]`.
- Events: `shortcut://status`, `shortcut://conflict`; Rust constants mirrored in TS.
- Native action IDs: `open_clipboard`, `open_preference`, `convert_to_plain_text`.

### 3. Contracts

Default conversion is Control+Shift+V on both platforms. Missing new fields and
missing known action IDs receive defaults; explicit empty strings remain disabled.
Known window IDs: paste, pastePlain, copy, open, favorite, pin, note, delete,
deleteForward. The two deletion entries retain the original Backspace/Delete
paths. Unknown string-valued IDs survive serialization but never execute.

Window bare keys are Enter/Backspace/Delete and F1—F12. Navigation, Space, Esc,
Tab, digit shortcuts, and fixed helper actions retain dedicated handlers.
Aliases/modifier order normalize; all four modifiers must match exactly.
An isolated modifier is never a shortcut primary key. The window recorder accepts
bare Backspace/Delete as bindings and uses its clear button to disable a binding;
the global recorder retains deletion-key clearing.
Shifted digits/punctuation resolve the same base key as the recorder and Windows
hook; `Control+Shift+1` must match the event whose key is `!` and code is `Digit1`.

Settings updates serialize read/prepare/commit. Native registration has its own
gate; shortcut updates hold both through registration, persistence, and rollback.
Delayed resume reads the latest snapshot inside the native gate. Window-only
updates do not re-register unrelated globals. Recording tests new global
registrations while callbacks remain suspended, then unregisters them until resume.
Each recorder owns one suspension request; blur and unmount release that same
request at most once. Cancellation, blur, refocus, and unmount invalidate pending
key events before they can save. A failed native suspension rolls back only its
own pause count and schedules restoration only when no other recorder holds it.

Import and storage rebase re-apply registrations and emit actual status; failed
registrations retain imported/rebased intent with a visible inactive outcome.
Reset/import/rebase must not equate settings persistence with OS registration.

### 4. Validation & Error Matrix

- Semantic duplicates across global/window actions or Win+V -> reject writes.
- Fixed gesture shadowing, unsupported primary key, bare letter -> reject writes.
- Malformed new field types -> reject update/import; tolerant disk load disables
  damaged new bindings without replacing unrelated settings.
- Invalid loaded new bindings -> disable and retain session-only action diagnostics
  in SettingsStore. `get_shortcut_status` merges a localized error for the native
  converter ID or known window ID, even when its value is empty. Successful explicit
  edits (including intentional disable), restore, or replacement clear acknowledged
  warnings; rebase replaces them with diagnostics for its loaded file. Legacy
  globals remain intact for native registration diagnostics.
- Native prepare/write failure -> retain prior settings and attempt native restore;
  restore failure remains visible in actual status.

### 5. Good/Base/Bad Cases

- Good: copy bound to Control+Shift+Y executes only that exact combination.
- Base: missing fields keep existing Enter/CmdOrCtrl+C behaviors.
- Bad: Control alone must not match Control+F because it appears among modifiers.

### 6. Tests Required

Old custom globals/winV/unrelated fields, explicit disables, partial maps,
unknown IDs, malformed new types, aliases/order, exact modifiers, reserved
actions, and Win+V collisions need assertions. UI checks cover recording,
cancel/blur/unmount, restore, failure rollback, and hint/menu agreement.
Empty disabled converter/window rows display load warnings; successful correction
clears only acknowledged diagnostics. No diagnostic field is persisted to settings.
Native Windows hooks and macOS menus still require real desktop verification.

### 7. Wrong vs Correct

Wrong: `parts.includes(event.key)` or `ctrlKey && key === 'c'` for full matching.
Correct: compare the unique non-modifier primary key and all modifier booleans.

Do not mutate `settingsState` directly from UI components. Even the initiating
window waits for the broadcast so all windows share the same snapshot.

## Window Labels and Lifecycle

Window labels are constants in `window/mod.rs` and mirrored in
`src/constants/windows.ts`. Current labels are:

- `clipboard`
- `preference`
- `clipboard-preview`
- `onboarding`
- `update`
- `context-menu`
- `context-submenu`

When a known window label is displayed in diagnostics UI, map it through
`src/locales/*/preferences.json` instead of rendering the raw internal label.
Keep raw-label fallback only for unknown or unregistered labels so diagnostics
remain useful.

Use `window::show_window`, `hide_window`, and `toggle_window` instead of direct
webview calls. These functions restore/save geometry, apply clipboard-window
position settings, emit `window://visibility`, update lifecycle state, and
handle platform-specific show/hide.

`window/lifecycle/mod.rs` tracks phases such as `ready`, `visible`,
`hiddenWarm`, `dormant`, `destroyPending`, and `destroyed`. Destroyable windows
use descriptors and can rebuild after idle destruction. Frontend windows report
readiness through `notify_window_ready` once `settingsReady` has resolved.

### Scenario: Clipboard Window Label Contract

#### 1. Scope / Trigger

Changing the primary clipboard-history window label touches Tauri config, Rust
window constants, frontend constants, commands, events, lifecycle diagnostics,
and preview payloads. Treat it as a cross-layer contract change.

#### 2. Signatures

- Runtime label:
  - Rust `window::CLIPBOARD_WINDOW_LABEL = "clipboard"`
  - TypeScript `WINDOW_LABEL.CLIPBOARD = "clipboard"`
  - Tauri config window label `"clipboard"`
- Commands:
  - `set_clipboard_window_pinned(AppHandle, pinned: bool) -> ClipboardWindowPinState`
  - `set_clipboard_window_auto_hide_suspended(suspended: bool) -> ()`
- Preview payload:
  - `ClipboardPreviewState.clipboardWindow: PreviewClipboardWindowRect | null`

#### 3. Contracts

- User-visible copy should say clipboard window, not main window.
- User-visible names for concrete window entities should include the window
  suffix, for example `剪贴板窗口` / `Clipboard Window` and
  `偏好设置窗口` / `Preferences Window`.
- Do not use raw `"clipboard"` in React components; compare with
  `WINDOW_LABEL.CLIPBOARD`.
- Keep platform terms such as Rust `main()`, `run_on_main_thread`, and Cocoa
  `can_become_main_window` unchanged.

#### 4. Validation & Error Matrix

- Stale `"main"` label in a consumer -> lifecycle or emit target is ignored.
- Stale command name -> frontend invoke fails with an unknown command.
- Stale preview field name -> preview layout loses clipboard-window geometry.

#### 5. Good/Base/Bad Cases

- Good: tray, shortcut, onboarding, context menu, preview, and copy-after-hide
  all target `CLIPBOARD_WINDOW_LABEL` / `WINDOW_LABEL.CLIPBOARD`.
- Base: secondary windows still use their existing labels and lifecycle rules.
- Bad: component code checks `event.payload.label === "main"` directly.

#### 6. Tests Required

- `rg` verifies no stale `MAIN_WINDOW_LABEL`, `WINDOW_LABEL.MAIN`, raw
  `"main"` label, or old command names remain outside platform terminology.
- Frontend: `pnpm lint` and `pnpm tsc`.
- Backend: `cargo fmt`, `cargo clippy -- -D warnings`, and `cargo test`.
- Manual desktop checks for clipboard show/hide/toggle, pin, auto-hide,
  preview, tray click, context menu dispatch, onboarding completion, and
  shortcuts.

#### 7. Wrong vs Correct

Wrong:

```tsx
if (event.payload.label !== "main") return;
```

Correct:

```tsx
if (event.payload.label !== WINDOW_LABEL.CLIPBOARD) return;
```

## Pending-Slot Pattern

When an event targets a destroyable preference window, use a Rust pending slot
if the window may not exist yet:

- Preference setting highlight: `window::open_preference_with_highlight` and
  `take_pending_preference_highlight`.
- Backup file receive: `backup::emit_received_backup` and
  `take_pending_backup`.

This avoids losing events while a WebView is being rebuilt and before React has
mounted listeners.

## macOS Window Rules

The clipboard window becomes an NSPanel in `window/macos.rs`. It can become key
without activating the app. Hidden constraints:

- The tauri-nspanel plugin must be registered before `to_panel`.
- Clipboard panel show is delayed briefly, then executed on the main thread.
- Visibility and lifecycle events for the clipboard panel are emitted after the panel
  actually shows.
- Pinned-window paste must resign key before simulated paste, then restore key
  after the target app consumes the keystroke.

Changing clipboard-window behavior on macOS requires manual testing of show/hide,
keyboard navigation, auto-hide, pinned paste, and Spaces behavior.

## Windows Window Rules

The clipboard window is non-focusable on Windows. `window/windows.rs` enables low-level
keyboard and mouse hooks while the clipboard window is visible:

- `keyboard/windows.rs` emits `keyboard://nav` for navigation keys and selected
  Ctrl shortcuts.
- `mouse/windows.rs` hides the clipboard window on outside click when auto-hide is
  allowed.
- Hiding the clipboard window disables hooks and hides context menus.

React code uses `useKeyboardEvent` so components do not need to know whether the
event came from the browser or Rust.

### Scenario: Windows Clipboard Editable Focus

#### 1. Scope / Trigger

Changing text input behavior inside the Windows clipboard window touches the
window focusability flag, low-level keyboard hooks, foreground-window restore,
Tauri command registration, and the React keyboard abstraction.

#### 2. Signatures

- Rust command:
  - `set_clipboard_window_editing(app: AppHandle, editing: bool) -> Result<()>`
- Rust platform owner:
  - `window::set_clipboard_window_editing(app_handle: &AppHandle, editing: bool) -> Result<()>`
  - `window/windows.rs::set_clipboard_window_editing(...)`
- TypeScript wrapper:
  - `setClipboardWindowEditing(editing: boolean): Promise<void>`
- Frontend owner:
  - `useClipboardWindowEditableFocus()` mounted by the clipboard page.

#### 3. Contracts

- The clipboard window remains non-focusable by default on Windows and
  `show_window` must force it back to `focusable=false` before showing.
- `editing=true` stores the current foreground HWND when it is not the clipboard
  window, disables Windows navigation-key hooks, sets the clipboard window
  focusable, and focuses it so editable controls can receive normal text input.
- `editing=false` sets the clipboard window non-focusable again, restores the
  navigation and outside-click hooks while the window remains visible, and only
  restores the stored foreground HWND when the clipboard window is still the
  foreground window.
- The frontend reports editable focus at the clipboard page level using
  `focusin`, `focusout`, `pointerdown`, window `blur`, and `visibilitychange`;
  individual input components should not call the command directly.
- `useKeyboardEvent` must let `input`, `textarea`, and `contenteditable`
  targets keep native browser keyboard behavior while editing is active.
- In the Windows clipboard window only, editable controls with
  `data-allow-global-keyboard="true"` are handoff inputs: when they receive
  navigation/function keys such as Arrow keys, Enter, Escape, or Tab,
  `useKeyboardEvent` must blur the control before running the global keyboard
  handler so clipboard shortcuts keep working. macOS keeps its existing browser
  keyboard bubbling path.
- Modifier-only keydown/keyup updates hint state without blurring an editable
  control; subsequent character/copy shortcuts retain native editing behavior.
- Shortcut hint components must not clear their active state on the Windows
  clipboard window blur caused by Ctrl handoff while the modifier key is still
  pressed; they should remain active until the matching modifier keyup arrives.
- Programmatic focus paths such as Ctrl+F must call
  `prepareClipboardWindowEditableFocus()` before focusing the input, so Windows
  can temporarily make the clipboard window focusable first.

#### 4. Validation & Error Matrix

- Missing clipboard window -> command returns `window not found: clipboard`.
- `editing=true` with no foreground HWND or the clipboard already foreground ->
  no foreground HWND is stored.
- Stored HWND no longer exists -> skip restore and clear the stored handle.
- Windows rejects `SetForegroundWindow` -> log debug only; editing mode still
  ends and hooks are restored.
- Non-Windows platform -> command is a no-op success.

#### 5. Good/Base/Bad Cases

- Good: user opens the clipboard window from another app, focuses search or a
  note input, types normally, then blur restores the clipboard window to
  non-focusable and returns focus to the prior app.
- Good: user focuses search, presses ArrowDown, and the search input blurs
  before the list selection shortcut runs.
- Good: user focuses search and holds Ctrl; search remains focused and shortcut
  hints update until Ctrl is released. Copy/text edits remain native.
- Good: user presses Ctrl+F while the Windows clipboard window is non-focusable,
  the window enters editing mode first, and the search input receives focus.
- Base: no editable control is focused; Windows navigation continues through
  `keyboard://nav` and the clipboard window does not steal focus.
- Bad: leaving `WH_KEYBOARD_LL` navigation enabled during editing consumes arrow,
  Tab, Enter, or selected Ctrl shortcuts before the input can handle them.
- Bad: applying `data-allow-global-keyboard` to note or group-name editors makes
  editing keys leave the field unexpectedly.

#### 6. Tests Required

- Backend: `cargo clippy -- -D warnings` covers the Windows command and cfg
  branches; `cargo test` must still pass.
- Frontend: `pnpm tsc` and `pnpm lint` cover command mirrors and hook typing.
- Manual Windows checks: open without focus steal, type in search, type/edit a
  note, type/edit a group name, blur each field, verify the prior app receives
  focus, and verify list navigation still works after blur.

#### 7. Wrong vs Correct

Wrong:

```tsx
<Input onFocus={() => setClipboardWindowEditing(true)} />
```

Correct:

```tsx
const Clipboard = () => {
  useClipboardWindowEditableFocus();

  return <ClipboardWindowContent />;
};
```

## Platform-Specific Tauri Capabilities

Platform-specific plugin permissions must live in their own capability file with
an explicit `platforms` guard. Keep `src-tauri/capabilities/default.json`
limited to permissions valid on both macOS and Windows; otherwise Windows
capability validation can reject macOS-only identifiers before runtime cfgs take
effect.

Wrong:

```json
{
  "identifier": "default",
  "permissions": ["core:default", "macos-permissions:default"],
  "windows": ["*"]
}
```

Correct:

```json
{
  "identifier": "macos-permissions",
  "permissions": ["macos-permissions:default"],
  "platforms": ["macOS"],
  "windows": ["*"]
}
```

## Native Integration Side Effects

Settings changes that affect OS state must be applied in Rust or through the
existing command layer:

- Global shortcuts: `shortcut::apply`, `suspend`, and `resume`.
- Tray menu and language: `tray::apply`.
- Autostart: `autostart::set_enabled`.
- Dock/taskbar icon: `window::show_taskbar_icon`.
- Windows administrator launch: `admin::sync_scheduled_task` and
  `admin::launch_elevated_current_process`.

Failures in side-effect refresh after a successful settings write are logged and
do not roll back the persisted settings unless the called command itself is the
direct user action, such as `set_autostart`.

## Scenario: Windows Autostart Entry Deduplication and Duplicate Launches

### 1. Scope / Trigger

Changing Windows autostart behavior touches registry Run keys, Task Manager
startup approval records, persisted settings, and the administrator relaunch
task. Keep this Rust-owned; React only toggles `general.autoStart`.

### 2. Signatures

- Setting: `Settings.general.auto_start: bool`.
- Commands:
  - `get_autostart() -> bool`
  - `set_autostart(enabled: bool) -> ()`
- Startup sync:
  - `autostart::sync_enabled(app: &AppHandle, enabled: bool) -> Result<()>`
- Launch-source matcher:
  - `autostart::is_autostart_launch(args: &[String]) -> bool`
- Windows Run value name: app package name, currently `EcoPaste`.
- Windows autostart launch arg: `--auto-launch`.

### 3. Contracts

- Normal Windows autostart uses one effective Run entry only.
- New Run entries are written with `WindowsEnableMode::CurrentUser`, so an
  elevated EcoPaste process must not create a fresh HKLM Run value.
- Startup initialization syncs the OS Run entries with
  `settings.general.auto_start`, so upgrades can repair stale entries from older
  versions.
- Registry cleanup cannot prevent two legacy Run entries from racing before the
  primary instance finishes setup. The single-instance callback must ignore a
  secondary process whose arguments contain `--auto-launch`; it must not show
  the preference window for that process.
- A regular second launch without `--auto-launch` remains an explicit user
  action and opens the default foreground window.
- When enabling autostart and an HKLM Run value already exists, Rust first tries
  to delete HKLM. If access is denied, Rust deletes HKCU and keeps HKLM as the
  only effective startup entry instead of creating a duplicate.
- When disabling autostart, Rust must remove HKCU and HKLM Run entries. If HKLM
  cannot be removed because the process is not elevated, the direct
  `set_autostart(false)` action fails instead of persisting a false setting while
  Windows can still start the app.
- Remove matching `StartupApproved\Run` shadow values best-effort when deleting
  Run values so Task Manager and Autoruns do not keep stale EcoPaste rows.
- `admin::sync_scheduled_task` owns the `EcoPasteAdmin` task for administrator
  relaunch support. Do not use that task as a second normal autostart path
  without redesigning the interaction with `general.auto_start`.

### 4. Validation & Error Matrix

- HKCU Run missing -> cleanup succeeds.
- HKLM Run missing -> cleanup succeeds.
- Enable with removable HKLM Run -> delete HKLM, then create/update HKCU.
- Enable with inaccessible HKLM Run -> delete HKCU, return success with HKLM as
  the only startup entry.
- Disable with inaccessible HKLM Run -> command error from registry access;
  frontend should not persist `general.autoStart=false`.
- Startup sync failure after settings load -> log warning and continue app
  startup.
- Secondary instance with `--auto-launch` -> return from the single-instance
  callback without showing a window.
- Secondary instance without `--auto-launch` -> show the default foreground
  window.

### 5. Good/Base/Bad Cases

- Good: user enables autostart from an elevated process and only HKCU Run is
  created.
- Good: user upgrades with both HKCU and HKLM Run values; next startup removes
  the duplicate when permissions allow, while the already-racing secondary
  instance exits without opening preferences.
- Base: user has a locked HKLM Run value from an older install; EcoPaste keeps
  that single entry and avoids adding HKCU.
- Base: user manually launches EcoPaste while it is running; the primary
  instance opens preferences as expected.
- Bad: `auto-launch` stays in dynamic Windows mode and creates HKLM when the app
  happens to be elevated.
- Bad: disabling autostart reports success while an inaccessible HKLM Run value
  remains active.

### 6. Tests Required

- Backend: `cargo check`, `cargo clippy -- -D warnings`, and `cargo test`.
- Unit tests: `is_autostart_launch` returns true when `--auto-launch` is present
  and false for a regular executable-only argument list.
- Windows validation: enable autostart as normal user and elevated user, inspect
  Autoruns for a single EcoPaste Run entry, then disable and verify no Run entry
  remains.
- Upgrade validation: seed both HKCU and HKLM `...\Run\EcoPaste` values, launch
  EcoPaste once, and verify duplicate cleanup or single-entry fallback.
- Administrator validation: with `general.runAsAdmin=true`, confirm
  `EcoPasteAdmin` remains a relaunch helper and does not combine with duplicate
  Run entries to start multiple processes at login.

### 7. Wrong vs Correct

Wrong:

```rust
AutoLaunchBuilder::new()
    .set_app_name(&app_name)
    .set_app_path(&exe_path)
    .set_args(&[AUTO_LAUNCH_ARG])
    .build()?;
```

Correct:

```rust
let mut builder = AutoLaunchBuilder::new();
builder
    .set_app_name(&app_name)
    .set_app_path(&exe_path)
    .set_args(&[AUTO_LAUNCH_ARG]);
builder.set_windows_enable_mode(WindowsEnableMode::CurrentUser);
```

Wrong:

```rust
if backup::backup_path_from_args(&argv).is_none() {
    show_default_foreground_window(app_handle)?;
}
```

Correct:

```rust
if autostart::is_autostart_launch(&argv) {
    return;
}
show_default_foreground_window(app_handle)?;
```

## Scenario: Windows Administrator Launch

### 1. Scope / Trigger

Adding or changing administrator launch behavior touches startup order, Windows
process APIs, scheduled tasks, persisted settings, Tauri commands, TypeScript
command wrappers, and onboarding UI. Treat it as a Rust-owned platform contract,
not a frontend-only permission prompt.

### 2. Signatures

- Setting:
  - Rust `Settings.general.run_as_admin: bool`
  - TypeScript `Settings.general.runAsAdmin: boolean`
- Commands:
  - `get_run_as_admin_status() -> AdminLaunchStatus`
  - `set_run_as_admin(enabled: bool) -> Settings`
  - `restart_as_admin() -> ()`
- Response:
  - `AdminLaunchStatus.configured: bool`
  - `AdminLaunchStatus.runningAsAdmin: bool`
  - `AdminLaunchStatus.taskReady: bool`
- Internal restart marker:
  - `--ecopaste-admin-restarted`

### 3. Contracts

- `runAsAdmin` records persistent user intent. The current Windows access token
  is the source of truth for whether EcoPaste is actually elevated.
- Windows elevation detection uses `OpenProcessToken` and
  `GetTokenInformation(TokenElevation)`.
- Release startup calls the admin auto-elevation check before normal Tauri setup
  initializes windows, tray, database, clipboard watcher, or hooks.
- If `runAsAdmin=true` and the process is already elevated, Rust best-effort
  syncs the highest-privilege scheduled task and continues startup.
- If `runAsAdmin=true` and the process is not elevated, Rust tries to launch an
  elevated process and exits the unelevated process only after launch succeeds.
- Relaunch prefers a valid scheduled task only when current arguments are safe
  for the static task action, meaning no external arguments beyond the internal
  restart marker. Dynamic arguments such as file-open backup paths or
  `--auto-launch` fall back to `ShellExecuteW` with the `runas` verb so they can
  be preserved.
- React renders `AdminLaunchStatus` and sends user intent through command
  wrappers. It must not call Windows APIs or infer token elevation itself.

### 4. Validation & Error Matrix

- `restart_as_admin` on non-Windows -> command error `administrator launch is only available on Windows`.
- UAC cancelled or elevated launch fails -> command error
  `administrator permission request was cancelled or failed`; current process
  remains open.
- Scheduled task path mismatch -> task is not considered ready; fallback to UAC.
- `set_run_as_admin(false)` while elevated -> Rust deletes the scheduled task
  best-effort after settings update.
- Early settings or storage manifest read failure -> auto-elevation is skipped
  and normal startup continues.

### 5. Good/Base/Bad Cases

- Good: onboarding on Windows shows the administrator card as actionable, the
  user grants access, Rust persists `runAsAdmin`, launches an elevated instance,
  exits the old process, and the elevated instance reopens onboarding with the
  card granted.
- Base: app already runs elevated; onboarding reports granted and startup keeps
  the scheduled task aligned with the current executable.
- Bad: frontend marks administrator permission granted because `runAsAdmin=true`
  even though `runningAsAdmin=false`.

### 6. Tests Required

- Backend: unit-test Windows command-line quoting for scheduled task / UAC
  arguments.
- Backend: `cargo clippy -- -D warnings` must pass on Windows-specific code.
- Backend: `cargo test` must include settings default coverage for
  `general.run_as_admin`.
- Frontend: `pnpm tsc` must cover `AdminLaunchStatus` and the mirrored
  `runAsAdmin` setting.
- Manual Windows validation is required for UAC approval/cancel and scheduled
  task launch behavior.

### 7. Wrong vs Correct

Wrong:

```tsx
const granted = settings.general.runAsAdmin;
```

Correct:

```tsx
const status = await getRunAsAdminStatus();
const granted = status.runningAsAdmin;
```

## Scenario: Auto Updater Window

### 1. Scope / Trigger

Adding or changing automatic update behavior touches Tauri plugin setup, Rust
commands, stored update settings, Tauri window lifecycle, TypeScript command
mirrors, and localized UI strings. Treat it as a cross-layer contract change,
not a frontend-only preference action.

### 2. Signatures

- Window label:
  - Rust `window::UPDATE_WINDOW_LABEL = "update"`
  - TypeScript `WINDOW_LABEL.UPDATE = "update"`
- Commands:
  - `get_update_status() -> AppUpdateStatus`
  - `check_for_updates() -> AppUpdateStatus`
  - `download_update(version: String) -> UpdateMetadata`
  - `install_update(version: String) -> ()`
  - `skip_update_version(version: String) -> AppUpdateStatus`
  - `open_update_window() -> ()`
- Event:
  - `update://progress` with `{ downloaded, total, progress }`
- Settings fields:
  - `update.autoCheck: bool`
  - `update.includeBeta: bool`
  - `update.includeNightly: bool`
  - `update.frequency: daily | weekly | monthly`
  - `update.lastCheckedAt: Option<String>`
  - `update.skippedVersion: Option<String>`

### 3. Contracts

- Rust owns update checks, signature verification, downloaded bytes, install,
  auto-check throttling, skipped-version persistence, and automatic update
  window display.
- On macOS, `tauri-plugin-updater` installation APIs replace the application
  files and return without relaunching the process. `install_update` must
  request an application restart after installation succeeds and must not
  restart after an install error. On Windows the updater exits and delegates
  relaunch to the spawned installer before this call can return.
- `update::schedule_auto_check` must run for the whole app session: it performs
  the initial delayed background check, then computes the next due time from
  `update.lastCheckedAt + update.frequency` so the configured cadence is honored
  even when EcoPaste stays open for days. The scheduler may cap long sleeps to
  refresh settings, but network checks must still pass through Rust-side
  `should_auto_check` throttling.
- `update.skippedVersion` suppresses that exact version for both manual and
  automatic checks; a newer different version is shown normally.
- React renders `AppUpdateStatus` and sends user intent through command wrappers
  only. Do not call updater plugin APIs directly from React components.
- Stable endpoint default:
  `https://releases.ecopaste.cn/update?channel=stable`.
- Optional environment overrides:
  - `ECOPASTE_UPDATE_ENDPOINT`: stable channel endpoint.
  - `ECOPASTE_UPDATE_BETA_ENDPOINT`: beta channel endpoint; defaults to
    `https://releases.ecopaste.cn/update?channel=beta` and is checked before
    the stable endpoint when `includeBeta` is true.
  - `ECOPASTE_UPDATE_NIGHTLY_ENDPOINT`: nightly channel endpoint; defaults to
    `https://releases.ecopaste.cn/update?channel=nightly` and is checked before
    beta and stable endpoints when `includeNightly` is true.
  - `TAURI_UPDATER_PUBLIC_KEY` or `TAURI_SIGNING_PUBLIC_KEY`: public updater
    verification key injected into the plugin at runtime.
  - `TAURI_SIGNING_PRIVATE_KEY`: build-time private key used by Tauri bundling
    to sign artifacts; never read it from app runtime code.

### 4. Validation & Error Matrix

- Invalid endpoint URL -> command error `update endpoint is invalid: ...`.
- No configured endpoint or updater build failure -> command error from Rust
  updater setup.
- No pending update when downloading/installing -> `no update is ready`.
- Version mismatch between UI request and Rust pending update ->
  `the selected update is no longer current`.
- Install before successful download -> `update is not downloaded`.
- Install failure -> propagated command error; the current process stays open.
- Successful macOS install -> Rust logs the installed version and requests an
  application restart. Successful Windows install -> the updater exits and the
  installer owns relaunch.
- Signature mismatch -> propagated from `tauri-plugin-updater` download
  verification; frontend should show the command error and remain in error
  state.

### 5. Good/Base/Bad Cases

- Good: user opens About -> Check for Updates; Rust opens the `update` window,
  checks stable/beta endpoint according to settings, downloads through Rust,
  emits progress, verifies signature, installs, then requests an application
  restart.
- Base: no update is available; Rust clears pending update state and the window
  renders the latest-version state.
- Bad: React stores downloaded bytes, performs signature checks, or decides
  auto-check frequency locally.
- Bad: Rust assumes `Update::install` or `Update::download_and_install`
  automatically relaunches the application on macOS.

### 6. Tests Required

- Backend checks: `cargo clippy -- -D warnings` and `cargo test`.
- Auto-check scheduling changes must verify that the background task loops after
  the first check and still delegates frequency/auto-check gating to
  `should_auto_check`.
- Frontend checks: `pnpm tsc` and `pnpm lint`.
- Packaging/build check: `pnpm build` for route and locale integration.
- Manual signed-artifact check must assert that a successful install terminates
  the old process, relaunches EcoPaste, and reports the new version after
  startup.
- Manual update-server checks when release artifacts exist:
  - mock endpoint returns newer signed version -> update found and install path
    starts.
  - mock endpoint returns bad signature -> download refuses install.
  - `includeBeta=false` -> stable endpoint only.
  - `includeBeta=true` -> beta endpoint is attempted before stable fallback.
  - `includeNightly=true` -> nightly endpoint is attempted before beta and
    stable fallback.

### 7. Wrong vs Correct

Wrong:

```tsx
import { check } from "@tauri-apps/plugin-updater";

await check();
```

Correct:

```tsx
import { checkForUpdates } from "@/commands";

const status = await checkForUpdates();
```

Wrong:

```rust
app.state::<UpdateState>().install_downloaded(&version)?;
Ok(())
```

Correct:

```rust
app.state::<UpdateState>().install_downloaded(&version)?;
app.request_restart();
Ok(())
```

## Scenario: Pinned Clipboard Window and Conversion Success Hint

### 1. Scope / Trigger

Window pin is a session fact, distinct from row pinning and temporary file-dialog
auto-hide suspension. Global conversion feedback must work with a hidden clipboard
window and preserve the external application's foreground/input target.

### 2. Signatures

- `get_clipboard_window_pin_state() -> { pinned: bool, revision: u64 }`.
- `set_clipboard_window_pinned(AppHandle, pinned: bool)` returns the same state.
- `window://pinned` mirrors the state; Header rejects lower revisions.
- `convert_current_clipboard_with_feedback(&AppHandle)` shares command/global
  success handling; low-level `convert_current_clipboard_to_plain_text` stays UI-free.
- `get_conversion_feedback_state() -> Option<{ requestId: u64, message: String }>`.
- `present_conversion_feedback(AppHandle, request_id: u64)` accepts rendered IDs.
- `clipboard://conversion-feedback` targets window label `conversion-feedback`.

### 3. Contracts

The existing topmost levels remain active. Pin keeps the clipboard window expanded
on loss of focus and copy/paste; explicit Esc/toggle/close still hides it without
clearing pin. Clicking pin again cancels; restart resets false. No settings field
or migration is introduced. UI pulls facts on mount/visibility and listens to state
events; revision prevents late query/save responses reverting newer state. Pending
save and shortcuts cannot double-toggle; errors retain the last confirmed state.

Windows pinned navigation pauses on outside clicks/foreground changes. Already
consumed keys retain their release pairing; clicking inside or explicitly reopening
resumes interaction. Editing disables the hook, restores the external foreground
before re-enabling navigation, and preserves pause if focus was handed to another
app. The temporary file-dialog suspension is independent of pin.

Only `Converted` requests the exact zh-CN `已经转换格式` / en-US `Formatting converted`
message. A single lazy-created transparent, mouse-transparent window is reused.
Windows uses `SWP_NOACTIVATE`, macOS a non-key NSPanel at Status level. Rust positions
it near the bottom of the cursor's monitor work area and hides after one second
from actual show. New successes replace the one message; no stack of toasts.

Windows native `SetWindowPos(SWP_SHOWWINDOW)` bypasses Tao's cached visibility;
hide through native `ShowWindow(SW_HIDE)` on the main thread as well. Calling
Tauri `hide()` with the cached flag still false can be a no-op. `ShowWindow`'s
return value describes previous visibility, not success; do not treat false as
a hide failure. macOS continues pairing panel show/hide.

The frontend's post-commit effect acknowledges the rendered ID; do not use rAF in a
hidden WebView because Chromium can suspend it. Pull fallback covers a cold window
missing the initial event. Pending IDs and displayed IDs are separate: a new request
that never renders must not cancel the old displayed window's hide timer. Only an
actual successful show updates DISPLAYED_ID. Old/duplicate acks are no-ops; requests
older than ten seconds are not shown. Feedback errors are logged separately and
never cause conversion retry or reinterpret a successful native write as failed.

### 4. Validation & Error Matrix

- Failed pin command -> wrapper toast, retained confirmed state.
- Late lower revision -> ignored.
- Unchanged/Unsupported/Changed/Busy/Err -> no success hint.
- Missing feedback window/build/show failure -> log; successful clipboard remains.
- New pending request without render -> old displayed hint still expires.
- Old displayed timer after newer successful show -> no hide of the new hint.

### 5. Good/Base/Bad Cases

Good: hidden clipboard + external editor + Ctrl+Shift+V converts markup and displays
one small hint without focus movement. Base: ordinary Text-only produces no hint.
Bad: antd message in the hidden clipboard window, native success dialog, or letting
pinned navigation consume an outside application's arrow/delete/copy keys.

### 6. Tests Required

UI covers initialization/remount/revision/save failure/double trigger and feedback
pull/events/old IDs/one ack per ID. Native Windows tests cover outside click and
app switching, search-to-list focus restore, actual clipboard formats, cold/warm
hint show/expiry and unchanged/unsupported feedback. Repeat on macOS; absent device
is pending, never inferred from a platform flag or Windows pass.

### 7. Wrong vs Correct

Wrong: replace pending request ID and use it to decide whether an already displayed
hint may hide; a failed new render can keep the old window visible forever.
Correct: timeouts compare the last successfully displayed ID; pending requests do
not update that ID. No persistence is added for a session-only pin policy.
