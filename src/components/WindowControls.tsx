import { useEffect, useState } from "react";
import { Copy, Minus, Square, X } from "lucide-react";
import { getCurrentWindow, type Window } from "@tauri-apps/api/window";

let cachedWindow: Window | null | undefined;

export function appWindow(): Window | null {
  if (cachedWindow !== undefined) return cachedWindow;
  try {
    cachedWindow = getCurrentWindow();
  } catch {
    cachedWindow = null;
  }
  return cachedWindow;
}

function windowErrorMessage(error: unknown): string {
  return error instanceof Error ? error.message : String(error);
}

export async function toggleMaximize() {
  const win = appWindow();
  if (!win) throw new Error("当前不在 Tauri 窗口中");
  await win.toggleMaximize();
}

interface Props {
  onError?: (message: string) => void;
}

export function WindowControls({ onError }: Props) {
  const [supported, setSupported] = useState(true);
  const [maximized, setMaximized] = useState(false);

  useEffect(() => {
    let unlisten: (() => void) | undefined;
    let cancelled = false;
    (async () => {
      const win = appWindow();
      if (!win) {
        if (!cancelled) setSupported(false);
        return;
      }
      try {
        if (!cancelled) setMaximized(await win.isMaximized());
        unlisten = await win.onResized(async () => {
          if (!cancelled) {
            try {
              setMaximized(await win.isMaximized());
            } catch (error) {
              onError?.(`窗口状态同步失败：${windowErrorMessage(error)}`);
            }
          }
        });
      } catch (error) {
        if (!cancelled) {
          // isMaximized 不可用不代表按钮不可用，照常显示，报错可见即可
          onError?.(`窗口初始化失败：${windowErrorMessage(error)}`);
        }
      }
    })();
    return () => {
      cancelled = true;
      unlisten?.();
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  // 浏览器里直接跑 vite 时没有窗口可管，隐藏按钮
  if (!supported) return null;

  const run = (label: string, action: (win: Window) => Promise<void>) => async () => {
    const win = appWindow();
    if (!win) {
      onError?.(`${label}失败：当前不在 Tauri 窗口中`);
      return;
    }
    try {
      await action(win);
    } catch (error) {
      onError?.(`${label}失败：${windowErrorMessage(error)}`);
    }
  };

  const toggle = async () => {
    try {
      await toggleMaximize();
      const win = appWindow();
      if (win) setMaximized(await win.isMaximized());
    } catch (error) {
      onError?.(`最大化失败：${windowErrorMessage(error)}`);
    }
  };

  return (
    <div
      className="window-controls"
      aria-label="窗口控制"
      onPointerDown={(event) => event.stopPropagation()}
      onMouseDown={(event) => event.stopPropagation()}
    >
      <button
        type="button"
        className="window-button"
        title="最小化"
        onClick={run("最小化", (win) => win.minimize())}
        onDoubleClick={(event) => event.stopPropagation()}
      >
        <Minus size={15} />
      </button>
      <button
        type="button"
        className="window-button"
        title={maximized ? "还原" : "最大化"}
        onClick={toggle}
        onDoubleClick={(event) => event.stopPropagation()}
      >
        {maximized ? <Copy size={13} /> : <Square size={13} />}
      </button>
      <button
        type="button"
        className="window-button is-close"
        title="关闭"
        onClick={run("关闭", (win) => win.close())}
        onDoubleClick={(event) => event.stopPropagation()}
      >
        <X size={16} />
      </button>
    </div>
  );
}
