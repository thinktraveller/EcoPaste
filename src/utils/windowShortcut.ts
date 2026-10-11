import {
  WINDOW_ACTION_DEFAULTS,
  type WindowShortcutAction,
} from "@/constants/shortcuts";
import { settingsState } from "@/stores/settings";
import { matchesShortcut } from "@/utils/shortcut";

/** Read the backend settings mirror; empty bindings stay disabled. */
export const getWindowShortcut = (action: WindowShortcutAction) => {
  return (
    settingsState.shortcuts.windowActions[action] ??
    WINDOW_ACTION_DEFAULTS[action]
  );
};

/** Match a configured action without allowing key-repeat side effects. */
export const matchesWindowShortcut = (
  event: KeyboardEvent,
  action: WindowShortcutAction,
) => {
  return !event.repeat && matchesShortcut(event, getWindowShortcut(action));
};
