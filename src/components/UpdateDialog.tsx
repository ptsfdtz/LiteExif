import { useEffect, useRef, useState } from "react";
import { Check, Download, RefreshCw, Sparkles, X } from "lucide-react";
import { checkForAppUpdate, restartAfterUpdate, type AppUpdate } from "../appUpdater";

type Phase = "checking" | "ready" | "latest" | "downloading" | "installing" | "failed" | "error";

interface Props {
  manual?: boolean;
  onClose: () => void;
}

export function UpdateDialog({ manual = false, onClose }: Props) {
  const [phase, setPhase] = useState<Phase>("checking");
  const [update, setUpdate] = useState<AppUpdate | null>(null);
  const [downloaded, setDownloaded] = useState(0);
  const [total, setTotal] = useState<number | undefined>();
  const started = useRef(false);

  useEffect(() => {
    if (started.current) return;
    started.current = true;
    checkForAppUpdate()
      .then((result) => {
        if (!result) {
          if (manual) setPhase("latest");
          else onClose();
          return;
        }
        setUpdate(result);
        setPhase("ready");
      })
      .catch((error) => {
        if (manual) setPhase("error");
        else {
          console.warn("自动检查更新失败：", error);
          onClose();
        }
      });
  }, [manual, onClose]);

  useEffect(() => {
    if (phase !== "ready") return;
    const onKey = (event: KeyboardEvent) => {
      if (event.key === "Escape") {
        void update?.close();
        onClose();
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [onClose, phase, update]);

  // 自动检查时先静默进行，只有发现新版本才弹出提示。
  if (!manual && phase === "checking") return null;

  const busy = phase === "downloading" || phase === "installing";
  const progress = total ? Math.min(100, Math.round((downloaded / total) * 100)) : undefined;

  const install = async () => {
    if (!update) return;
    setPhase("downloading");
    setDownloaded(0);
    setTotal(undefined);
    try {
      await update.downloadAndInstall((event) => {
        if (event.event === "Started") setTotal(event.data.contentLength);
        else if (event.event === "Progress") setDownloaded((value) => value + event.data.chunkLength);
        else if (event.event === "Finished") setPhase("installing");
      });
      await restartAfterUpdate();
    } catch (error) {
      console.error("安装更新失败：", error);
      setPhase("failed");
    }
  };

  const dismiss = () => {
    void update?.close();
    onClose();
  };

  const icon = phase === "failed" ? <RefreshCw size={17} /> : phase === "latest" ? <Check size={17} /> : <Download size={17} />;
  const title =
    phase === "latest"
      ? "已是最新版本"
      : phase === "failed"
        ? "更新失败"
        : phase === "error"
          ? "检查更新失败"
          : "发现新版本";

  return (
    <div className="modal-backdrop" onMouseDown={(event) => !busy && event.target === event.currentTarget && dismiss()}>
      <section className="dialog update-dialog" role="alertdialog" aria-modal="true" aria-labelledby="update-title">
        <header>
          <span className="dialog-icon">{icon}</span>
          <h2 id="update-title">{title}</h2>
          {!busy && (
            <button type="button" className="icon-button" onClick={dismiss} title="关闭">
              <X size={16} />
            </button>
          )}
        </header>
        <div className="dialog-body">
          {phase === "checking" && (
            <p className="update-hint"><RefreshCw size={14} className="is-spinning" /> 正在检查更新…</p>
          )}

          {phase === "latest" && (
            <p className="update-hint">当前版本 v{__APP_VERSION__} 已是最新版本。</p>
          )}

          {phase === "error" && (
            <p className="update-hint">无法连接到更新服务器，请检查网络后重试。</p>
          )}

          {phase === "ready" && update && (
            <>
              <p className="update-version">新版本 <strong>v{update.version}</strong>（当前 v{__APP_VERSION__}）</p>
              {update.notes && (
                <section className="update-notes" aria-label="更新内容">{update.notes}</section>
              )}
              <p className="update-hint"><Sparkles size={14} /> 更新后应用会自动重启，请先保存正在编辑的内容。</p>
            </>
          )}

          {phase === "failed" && (
            <p className="update-hint">更新安装失败，请稍后重试，或前往 GitHub Releases 手动下载安装包。</p>
          )}

          {busy && (
            <div aria-live="polite">
              <div className="update-progress-meta">
                <span>{phase === "installing" ? "正在安装…" : "正在下载更新…"}</span>
                {progress !== undefined && phase === "downloading" && <span>{progress}%</span>}
              </div>
              <div className={`progress-track ${progress === undefined ? "is-indeterminate" : ""}`}>
                <span style={progress === undefined ? { width: "40%" } : { width: `${progress}%` }} />
              </div>
            </div>
          )}
        </div>
        <footer>
          {phase === "ready" || phase === "failed" ? (
            <>
              <button type="button" className="button secondary" onClick={dismiss} disabled={busy}>稍后</button>
              <button type="button" className="button primary" onClick={() => void install()} disabled={busy} autoFocus>
                {phase === "failed" ? "重试" : "立即更新"}
              </button>
            </>
          ) : (
            <button type="button" className="button primary" onClick={dismiss} disabled={busy} autoFocus>知道了</button>
          )}
        </footer>
      </section>
    </div>
  );
}
