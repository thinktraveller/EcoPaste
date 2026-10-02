/** Stable window action IDs/defaults, mirrored from Rust settings::shortcuts. */
export const WINDOW_ACTION_DEFAULTS = {
  copy: "CmdOrCtrl+C",
  delete: "CmdOrCtrl+Backspace",
  deleteForward: "CmdOrCtrl+Delete",
  favorite: "CmdOrCtrl+D",
  note: "CmdOrCtrl+M",
  open: "CmdOrCtrl+O",
  paste: "Enter",
  pastePlain: "CmdOrCtrl+Enter",
  pin: "CmdOrCtrl+T",
} as const;

export type WindowShortcutAction = keyof typeof WINDOW_ACTION_DEFAULTS;

/** Native action IDs mirrored from Rust shortcut module constants. */
export const GLOBAL_SHORTCUT_ACTION = {
  convertToPlainText: "convert_to_plain_text",
  openClipboard: "open_clipboard",
  openPreference: "open_preference",
} as const;

export const FIXED_WINDOW_SHORTCUTS = [
  "Space",
  "Escape",
  "Tab",
  "Shift+Tab",
  "ArrowUp",
  "ArrowDown",
  "ArrowLeft",
  "ArrowRight",
  "CmdOrCtrl+F",
  "CmdOrCtrl+Q",
  "CmdOrCtrl+N",
  "CmdOrCtrl+P",
  "CmdOrCtrl+K",
  "CmdOrCtrl+,",
  ...Array.from({ length: 10 }, (_, index) => {
    return `CmdOrCtrl+${index}`;
  }),
] as const;
