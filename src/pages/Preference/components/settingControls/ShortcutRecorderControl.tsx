import { useMount } from "ahooks";
import { Button } from "antd";
import type { FC } from "react";
import { useState } from "react";
import { useTranslation } from "react-i18next";
import { getShortcutStatus, type ShortcutStatus } from "@/commands";
import ShortcutRecorder from "@/components/ShortcutRecorder";
import { TAURI_EVENT } from "@/constants/events";
import {
  FIXED_WINDOW_SHORTCUTS,
  GLOBAL_SHORTCUT_ACTION,
  WINDOW_ACTION_DEFAULTS,
} from "@/constants/shortcuts";
import { useTauriListen } from "@/hooks/useTauriListen";
import { updateSettings } from "@/stores/settings";
import type { Settings } from "@/types/settings";
import { isWin } from "@/utils/is";
import { log } from "@/utils/log";
import type { PreferenceSetting } from "../../types/preferences";
import { translatePreferencePlaceholder } from "../../utils/preferenceI18n";
import type { ControlProps } from "./types";

interface ShortcutRecorderControlProps extends ControlProps {
  setting: PreferenceSetting;
  settings: Settings;
  value: string;
}

/** Record scoped bindings and show the actual Rust registration outcome. */
const ShortcutRecorderControl: FC<ShortcutRecorderControlProps> = (props) => {
  const { t } = useTranslation("preferences");
  const { disabled, onChange, setting, settings, value } = props;
  const [statuses, setStatuses] = useState<ShortcutStatus[]>([]);
  const isWindowAction = setting.path?.[1] === "windowActions";
  const action = setting.path?.[2] ?? "";

  useMount(async () => {
    try {
      setStatuses(await getShortcutStatus());
    } catch (error) {
      log.error("load shortcut status failed", error);
    }
  });
  useTauriListen<ShortcutStatus[]>(TAURI_EVENT.SHORTCUT_STATUS, (event) => {
    setStatuses(event.payload);
  });

  if (setting.control.type !== "shortcutRecorder") return null;

  const globals = [
    {
      defaultValue: "Alt+C",
      id: "openClipboard",
      nativeAction: GLOBAL_SHORTCUT_ACTION.openClipboard,
      value: settings.shortcuts.openClipboard,
    },
    {
      defaultValue: "Alt+X",
      id: "openPreference",
      nativeAction: GLOBAL_SHORTCUT_ACTION.openPreference,
      value: settings.shortcuts.openPreference,
    },
    {
      defaultValue: "Control+Shift+V",
      id: "convertToPlainText",
      nativeAction: GLOBAL_SHORTCUT_ACTION.convertToPlainText,
      value: settings.shortcuts.convertToPlainText,
    },
  ];
  const global = globals.find((item) => {
    return setting.id === `shortcuts.${item.id}`;
  });
  const conflicts = [
    ...(isWin && settings.shortcuts.winV
      ? [{ label: t("schema.settings.shortcuts.winV.title"), value: "Super+V" }]
      : []),
    ...globals
      .filter((item) => {
        return item.id !== global?.id;
      })
      .map((item) => {
        return {
          label: t(`schema.settings.shortcuts.${item.id}.title`),
          value: item.value,
        };
      }),
    ...Object.entries(settings.shortcuts.windowActions)
      .filter(([id]) => {
        return id !== action && id in WINDOW_ACTION_DEFAULTS;
      })
      .map(([id, binding]) => {
        return {
          label: t(`schema.settings.windowShortcuts.${id}.title`),
          value: binding,
        };
      }),
    ...FIXED_WINDOW_SHORTCUTS.map((binding) => {
      return { label: t("shortcutSettings.reserved"), value: binding };
    }),
  ];
  const status = statuses.find((item) => {
    return item.action === (isWindowAction ? action : global?.nativeAction);
  });
  const defaultValue = isWindowAction
    ? WINDOW_ACTION_DEFAULTS[action as keyof typeof WINDOW_ACTION_DEFAULTS]
    : global?.defaultValue;

  const handleChange = async (nextValue: string) => {
    await onChange(setting, nextValue);
  };
  const restoreDefault = async () => {
    if (!defaultValue) return;
    try {
      await handleChange(defaultValue);
    } catch (error) {
      log.error("restore shortcut default failed", error);
    }
  };
  const restoreWindowDefaults = async () => {
    try {
      await updateSettings({
        shortcuts: { windowActions: WINDOW_ACTION_DEFAULTS },
      });
    } catch (error) {
      log.error("restore window shortcut defaults failed", error);
    }
  };

  return (
    <div className="flex flex-col items-end gap-1">
      <div className="flex items-center gap-2">
        <ShortcutRecorder
          conflicts={conflicts}
          disabled={disabled}
          onChange={handleChange}
          placeholder={translatePreferencePlaceholder(t, setting)}
          scope={isWindowAction ? "window" : "global"}
          value={value}
        />
        <Button
          disabled={disabled || value === defaultValue}
          onClick={restoreDefault}
          size="small"
          type="text"
        >
          {t("shortcutSettings.restore")}
        </Button>
      </div>
      {status && (status.error || (!isWindowAction && value)) && (
        <span className="max-w-72 text-ant-secondary text-xs" role="status">
          {status.error
            ? t("shortcutSettings.failed", { reason: status.error })
            : status.active
              ? t("shortcutSettings.active")
              : t("shortcutSettings.paused")}
        </span>
      )}
      {isWindowAction && action === "paste" && (
        <Button
          disabled={disabled}
          onClick={restoreWindowDefaults}
          size="small"
          type="link"
        >
          {t("shortcutSettings.restoreWindow")}
        </Button>
      )}
    </div>
  );
};

export default ShortcutRecorderControl;
