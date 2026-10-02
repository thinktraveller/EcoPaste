import { useMount, useUnmount } from "ahooks";
import { type FC, useEffect, useRef, useState } from "react";
import {
  type ConversionFeedback,
  getConversionFeedbackState,
  presentConversionFeedback,
} from "@/commands";
import { TAURI_EVENT } from "@/constants/events";
import { useTauriListen } from "@/hooks/useTauriListen";

/**
 * 只展示Rust确认的转换成功；窗口显示、焦点和1秒计时均由Rust管理。
 */
const ConversionFeedbackPage: FC = () => {
  const [feedback, setFeedback] = useState<ConversionFeedback | null>(null);
  const latestIdRef = useRef(0);
  const mountedRef = useRef(true);

  const applyFeedback = (state: ConversionFeedback | null) => {
    if (!mountedRef.current || !state || state.requestId <= latestIdRef.current)
      return;

    latestIdRef.current = state.requestId;
    setFeedback(state);
  };

  useTauriListen<ConversionFeedback>(
    TAURI_EVENT.CLIPBOARD_CONVERSION_FEEDBACK,
    (event) => {
      applyFeedback(event.payload);
    },
  );

  useMount(async () => {
    try {
      applyFeedback(await getConversionFeedbackState());
    } catch {
      return;
    }
  });

  useUnmount(() => {
    mountedRef.current = false;
  });

  useEffect(() => {
    if (!feedback) return;

    const present = async () => {
      try {
        await presentConversionFeedback(feedback.requestId);
      } catch {
        return;
      }
    };
    // 隐藏WebView的rAF可能被暂停；effect运行时DOM已经提交，可直接确认显示。
    void present();
  }, [feedback]);

  return (
    <div className="pointer-events-none flex size-screen items-center justify-center">
      <div
        aria-live="polite"
        className="flex items-center gap-2 rounded-2.5 border border-ant-border bg-ant-container px-4 py-3 text-sm shadow-lg"
        role="status"
      >
        <i
          aria-hidden="true"
          className="i-lucide:check size-4 text-ant-success"
        />
        {feedback?.message}
      </div>
    </div>
  );
};

export default ConversionFeedbackPage;
