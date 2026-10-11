import { useDebounceFn, useMount, useUnmount } from "ahooks";
import type { MenuProps } from "antd";
import type { ChangeEvent, FC } from "react";
import { useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { useSnapshot } from "valtio";
import {
  type ClipboardWindowPinState,
  clearClipboardItems,
  getClipboardWindowPinState,
  setClipboardWindowPinned,
  showWindow,
} from "@/commands";
import CustomIconButton from "@/components/CustomIconButton";
import Dropdown, {
  type AppDropdownProps,
  type DropdownMenuItems,
} from "@/components/Dropdown";
import KeyHint from "@/components/KeyHint";
import Tooltip from "@/components/Tooltip";
import { TAURI_EVENT } from "@/constants/events";
import { WINDOW_LABEL } from "@/constants/windows";
import { useTauriListen } from "@/hooks/useTauriListen";
import { clipboardViewState } from "@/stores/clipboardView";
import { settingsState } from "@/stores/settings";
import { formatShortcutDisplay } from "@/utils/shortcut";
import SearchInput from "./SearchInput";

interface WindowVisibilityPayload {
  label: string;
  visible: boolean;
}

type HeaderMoreMenuKey = "clear" | "preference";

const MORE_ACTION_TRIGGER: AppDropdownProps["trigger"] = ["click"];
const PREFERENCE_SHORTCUT = formatShortcutDisplay("CmdOrCtrl+,", " ");

/**
 * 剪贴板窗口顶部条：logo、搜索框（⌘F / Ctrl+F 聚焦）、固定窗口与更多操作入口。
 */
const Header: FC = () => {
  const { t } = useTranslation("clipboard");
  const settings = useSnapshot(settingsState);
  const [pinned, setPinned] = useState(false);
  const [pinPending, setPinPending] = useState(true);
  const pinRevisionRef = useRef(-1);
  const pinBusyRef = useRef(false);
  const mountedRef = useRef(true);
  const [searchBlurToken, setSearchBlurToken] = useState(0);
  const [searchClearToken, setSearchClearToken] = useState(0);
  const [searchFocusToken, setSearchFocusToken] = useState(0);

  /**
   * 统一处理偏好设置入口（按钮点击/快捷键）。
   */
  const handleOpenPreference = () => {
    return showWindow(WINDOW_LABEL.PREFERENCE);
  };

  /**
   * 清空剪贴板历史；确认、toast 与后端调用统一收口在命令包装内。
   */
  const handleClearClipboardItems = async () => {
    await clearClipboardItems();
  };

  /**
   * 更多操作菜单分发：危险操作走确认弹窗，偏好设置打开独立窗口。
   */
  const handleMoreMenuClick: MenuProps["onClick"] = async (info) => {
    const key = info.key as HeaderMoreMenuKey;

    if (key === "clear") {
      await handleClearClipboardItems();

      return;
    }

    await handleOpenPreference();
  };

  /**
   * 切换剪贴板窗口固定态：Rust 侧立即生效（resign_key / 外部点击钩子读取），本地态仅用于按钮渲染。
   */
  const handleTogglePinned = async () => {
    if (pinPending || pinBusyRef.current) return;

    pinBusyRef.current = true;
    setPinPending(true);

    try {
      applyPinState(await setClipboardWindowPinned(!pinned));
    } catch {
      // command 包装已经展示错误；按钮继续显示最后确认的 Rust 状态。
    } finally {
      pinBusyRef.current = false;
      if (mountedRef.current) setPinPending(false);
    }
  };

  /**
   * 保留最新版本，避免初始化查询或保存响应迟到时回退按钮状态。
   */
  const applyPinState = (state: ClipboardWindowPinState) => {
    if (!mountedRef.current || state.revision < pinRevisionRef.current) return;

    pinRevisionRef.current = state.revision;
    setPinned(state.pinned);
  };

  const refreshPinState = async () => {
    try {
      applyPinState(await getClipboardWindowPinState());
    } catch {
      return;
    } finally {
      if (mountedRef.current && !pinBusyRef.current) setPinPending(false);
    }
  };

  useMount(refreshPinState);
  useUnmount(() => {
    mountedRef.current = false;
  });

  useTauriListen<ClipboardWindowPinState>(
    TAURI_EVENT.WINDOW_PINNED,
    (event) => {
      applyPinState(event.payload);
    },
  );

  /**
   * 防抖写入共享 store：连续打字时仅保留最后一次值，下游 List 直接消费 store 触发查询。
   * 搜索框自身不受 store 控制（非受控），避免 IME composition 期回灌导致重复字符。
   */
  const { cancel: cancelKeywordChange, run: handleKeywordChange } =
    useDebounceFn(
      (event: ChangeEvent<HTMLInputElement>) => {
        clipboardViewState.keyword = event.target.value.trim();
      },
      { wait: 200 },
    );

  /**
   * 递增 token 触发搜索框清空，同时同步查询状态回到完整列表。
   */
  const clearSearch = () => {
    cancelKeywordChange();
    clipboardViewState.keyword = "";

    setSearchClearToken((current) => {
      return current + 1;
    });
  };

  /**
   * 递增 token 让搜索框失焦，避免窗口重新打开时保留上一次 activeElement。
   */
  const blurSearch = () => {
    setSearchBlurToken((current) => {
      return current + 1;
    });
  };

  /**
   * 递增 token 触发搜索框在窗口完成显示后的下一帧聚焦。
   */
  const focusSearch = () => {
    setSearchFocusToken((current) => {
      return current + 1;
    });
  };

  /**
   * 剪贴板窗口显隐变化时执行搜索框偏好：下次显示时清空关键词，显示后按设置自动聚焦。
   */
  const handleWindowVisibility = (event: {
    payload: WindowVisibilityPayload;
  }) => {
    const { label, visible } = event.payload;
    if (label !== WINDOW_LABEL.CLIPBOARD) return;
    void refreshPinState();

    if (!visible) {
      blurSearch();

      if (settings.clipboard.search.clearOnHide) {
        clearSearch();
      }

      return;
    }

    if (settings.clipboard.search.clearOnHide) {
      clearSearch();
    }

    if (!settings.clipboard.search.defaultFocus) {
      blurSearch();

      return;
    }

    focusSearch();
  };

  useTauriListen<WindowVisibilityPayload>(
    TAURI_EVENT.WINDOW_VISIBILITY,
    handleWindowVisibility,
  );

  const moreMenuItems: DropdownMenuItems = [
    {
      extra: PREFERENCE_SHORTCUT,
      icon: "i-lucide:settings",
      key: "preference",
      label: t("header.openPreference"),
    },
    {
      danger: true,
      icon: "i-lucide:trash-2",
      key: "clear",
      label: t("header.clearRecords"),
    },
  ];

  return (
    <div
      className="flex items-center justify-between p-3 pb-2"
      data-tauri-drag-region
    >
      <img alt={t("header.logoAlt")} className="size-5" src="/logo.png" />

      <div className="flex items-center gap-1">
        <SearchInput
          allowClear
          blurToken={searchBlurToken}
          className="w-40"
          clearToken={searchClearToken}
          focusToken={searchFocusToken}
          onChange={handleKeywordChange}
          placeholder={t("header.searchPlaceholder")}
          size="small"
        />

        <Tooltip title={t(pinned ? "header.unpin" : "header.pin")}>
          <CustomIconButton
            aria-label={t(pinned ? "header.unpin" : "header.pin")}
            aria-pressed={pinned}
            disabled={pinPending}
            icon={
              <KeyHint
                hintKey="P"
                iconName="i-lets-icons:pin"
                onKeyPress={handleTogglePinned}
              />
            }
            onClick={handleTogglePinned}
            size="small"
            type={pinned ? "primary" : "text"}
          />
        </Tooltip>

        <Dropdown
          menu={{ items: moreMenuItems, onClick: handleMoreMenuClick }}
          tooltip={t("header.moreActions")}
          trigger={MORE_ACTION_TRIGGER}
        >
          <CustomIconButton
            icon={
              <KeyHint
                hintKey=","
                iconName="i-lets-icons:meatballs-menu"
                onKeyPress={handleOpenPreference}
              />
            }
            size="small"
            type="text"
          />
        </Dropdown>
      </div>
    </div>
  );
};

export default Header;
