import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { convertFileSrc, invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { open } from "@tauri-apps/plugin-dialog";
import {
  Aperture,
  Check,
  Download,
  FileImage,
  Gauge,
  Play,
  RefreshCw,
  Settings2,
  X,
} from "lucide-react";
import { ConfirmDialog } from "./components/ConfirmDialog";
import { FileContextMenu } from "./components/FileContextMenu";
import { FileTree } from "./components/FileTree";
import { SettingsDialog } from "./components/SettingsDialog";
import { TemplateDialog } from "./components/TemplateDialog";
import { UpdateDialog } from "./components/UpdateDialog";
import { WindowControls, toggleMaximize } from "./components/WindowControls";
import { ZoomablePreview } from "./components/ZoomablePreview";
import type {
  AppConfig,
  EngineEvent,
  FileNode,
  FileTrees,
  ProgressState,
} from "./types";

const emptyConfig: AppConfig = {
  input_folder: "",
  output_folder: "",
  override_existed: false,
  template_name: "",
  template: "[]",
  quality: 60,
  templates: [],
};

const emptyProgress: ProgressState = {
  active: false,
  complete: false,
  total: 0,
  processed: 0,
  success: 0,
  failure: 0,
  skipped: 0,
  percent: 0,
  current: "",
  message: "",
};

type AccelerationStatus = {
  backend: string;
  state: "pending" | "validated" | "disabled";
  adapter: string | null;
  pixel_exact: boolean;
  scope: string;
};

function flattenFiles(nodes: FileNode[]): string[] {
  return nodes.flatMap((node) =>
    node.is_file && node.value
      ? [node.value]
      : flattenFiles(node.children ?? []),
  );
}

function updateNode(
  nodes: FileNode[],
  path: string,
  updater: (node: FileNode) => FileNode,
): FileNode[] {
  return nodes.map((node) => {
    if (node.value === path) return updater(node);
    if (node.children?.length)
      return { ...node, children: updateNode(node.children, path, updater) };
    return node;
  });
}

function errorMessage(error: unknown): string {
  return error instanceof Error ? error.message : String(error);
}

export default function App() {
  const [config, setConfig] = useState<AppConfig>(emptyConfig);
  const [draft, setDraft] = useState<AppConfig>(emptyConfig);
  const [settingsOpen, setSettingsOpen] = useState(false);
  const [trees, setTrees] = useState<FileTrees>({
    input_files: [],
    output_files: [],
  });
  const [selected, setSelected] = useState<Set<string>>(new Set());
  const [preview, setPreview] = useState<{
    path: string;
    name: string;
    url: string;
  } | null>(null);
  const [previewLoading, setPreviewLoading] = useState(false);
  const [loading, setLoading] = useState(true);
  const [refreshing, setRefreshing] = useState(false);
  const [progress, setProgress] = useState<ProgressState>(emptyProgress);
  const [dialog, setDialog] = useState<"create" | "saveAs" | null>(null);
  const [contextMenu, setContextMenu] = useState<{
    node: FileNode;
    x: number;
    y: number;
  } | null>(null);
  const [pendingDelete, setPendingDelete] = useState<FileNode | null>(null);
  const [deleting, setDeleting] = useState(false);
  const [toast, setToast] = useState<{
    text: string;
    kind: "success" | "error";
  } | null>(null);
  const [acceleration, setAcceleration] = useState<AccelerationStatus | null>(
    null,
  );
  const [previewCacheHit, setPreviewCacheHit] = useState(false);
  const [previewOriginal, setPreviewOriginal] = useState(false);
  const [updateOpen, setUpdateOpen] = useState(false);
  const [updateManual, setUpdateManual] = useState(false);
  const previewRequest = useRef(0);

  const notify = useCallback(
    (text: string, kind: "success" | "error" = "success") => {
      setToast({ text, kind });
      window.setTimeout(() => setToast(null), 3200);
    },
    [],
  );

  const refreshFiles = useCallback(async () => {
    setRefreshing(true);
    try {
      const data = await invoke<FileTrees>("list_files");
      setTrees(data);
      const current = new Set(flattenFiles(data.input_files));
      setSelected(
        (previous) =>
          new Set([...previous].filter((path) => current.has(path))),
      );
    } catch (error) {
      notify(`文件列表加载失败：${errorMessage(error)}`, "error");
    } finally {
      setRefreshing(false);
    }
  }, [notify]);

  // Directories load one level at a time. Appending a page keeps every request
  // bounded so huge network folders never arrive (or render) all at once.
  const applyChildren = useCallback(
    (path: string, children: FileNode[], hasMore: boolean, append: boolean) => {
      setTrees((previous) => {
        const patch = (node: FileNode): FileNode => ({
          ...node,
          children: append ? [...(node.children ?? []), ...children] : children,
          has_more: hasMore,
        });
        return {
          input_files: updateNode(previous.input_files, path, patch),
          output_files: updateNode(previous.output_files, path, patch),
        };
      });
    },
    [],
  );

  const loadChildren = useCallback(
    async (node: FileNode) => {
      if (!node.value) return;
      try {
        const result = await invoke<{
          children: FileNode[];
          has_more: boolean;
        }>("list_children", {
          path: node.value,
          offset: 0,
        });
        applyChildren(node.value, result.children, result.has_more, false);
      } catch (error) {
        notify(`无法读取目录：${errorMessage(error)}`, "error");
      }
    },
    [applyChildren, notify],
  );

  const loadMoreChildren = useCallback(
    async (node: FileNode) => {
      if (!node.value) return;
      try {
        const result = await invoke<{
          children: FileNode[];
          has_more: boolean;
        }>("list_children", {
          path: node.value,
          offset: node.children?.length ?? 0,
        });
        applyChildren(node.value, result.children, result.has_more, true);
      } catch (error) {
        notify(`无法读取目录：${errorMessage(error)}`, "error");
      }
    },
    [applyChildren, notify],
  );

  const loadConfig = useCallback(async () => {
    const data = await invoke<AppConfig>("get_config");
    setConfig(data);
    setDraft(data);
  }, []);

  const refreshAcceleration = useCallback(async () => {
    setAcceleration(
      await invoke<AccelerationStatus>("get_acceleration_status"),
    );
  }, []);

  useEffect(() => {
    Promise.all([loadConfig(), refreshFiles(), refreshAcceleration()])
      .catch((error) => notify(`初始化失败：${errorMessage(error)}`, "error"))
      .finally(() => setLoading(false));
  }, [loadConfig, notify, refreshAcceleration, refreshFiles]);

  useEffect(() => {
    let unlisten: (() => void) | undefined;
    listen<EngineEvent>("processing-progress", ({ payload }) => {
      const data = payload.data;
      setProgress((previous) => ({
        ...previous,
        ...data,
        active: payload.event !== "complete" && payload.event !== "error",
        complete: payload.event === "complete",
      }));
    }).then((cleanup) => {
      unlisten = cleanup;
    });
    return () => unlisten?.();
  }, []);

  // 启动后自动检查更新，发现新版本时提醒用户升级到最新版。
  useEffect(() => {
    const timer = window.setTimeout(() => {
      setUpdateManual(false);
      setUpdateOpen(true);
    }, 2500);
    return () => window.clearTimeout(timer);
  }, []);

  const checkUpdates = () => {
    setUpdateManual(true);
    setUpdateOpen(true);
  };

  const inputRoot = trees.input_files[0] ?? null;
  const outputRoot = trees.output_files[0] ?? null;
  const sourceNodes = inputRoot?.children ?? [];
  const outputNodes = outputRoot?.children ?? [];
  const allFiles = useMemo(() => flattenFiles(sourceNodes), [sourceNodes]);

  const updateSelection = (paths: string[], shouldSelect: boolean) => {
    setSelected((current) => {
      const next = new Set(current);
      paths.forEach((path) =>
        shouldSelect ? next.add(path) : next.delete(path),
      );
      return next;
    });
  };

  const showPreview = async (
    node: FileNode,
    processed = false,
    template = config.template,
  ) => {
    if (!node.value) return;
    const request = ++previewRequest.current;
    setPreviewLoading(true);
    setPreviewCacheHit(false);
    try {
      const isHeic = /\.hei[cf]$/i.test(node.value);
      if (!isHeic) {
        setPreview({
          path: node.value,
          name: node.label,
          url: convertFileSrc(node.value),
        });
      } else {
        const source = await invoke<{ path: string }>("prepare_preview", {
          path: node.value,
        });
        if (request === previewRequest.current) {
          setPreview({
            path: node.value,
            name: node.label,
            url: convertFileSrc(source.path),
          });
        }
      }
      if (!processed || request !== previewRequest.current) return;

      const result = await invoke<{
        path: string;
        cache_hit: boolean;
        acceleration: AccelerationStatus;
      }>("prepare_processed_preview", {
        path: node.value,
        template,
      });
      if (request === previewRequest.current) {
        setPreview({
          path: node.value,
          name: node.label,
          url: convertFileSrc(result.path),
        });
        setPreviewCacheHit(result.cache_hit);
        setAcceleration(result.acceleration);
      }
    } catch (error) {
      if (request === previewRequest.current) {
        notify(`无法预览图片：${errorMessage(error)}`, "error");
      }
    } finally {
      if (request === previewRequest.current) setPreviewLoading(false);
    }
  };

  const chooseFolder = async (field: "input_folder" | "output_folder") => {
    const selectedFolder = await open({
      directory: true,
      multiple: false,
      defaultPath: draft[field] || undefined,
    });
    if (typeof selectedFolder === "string")
      setDraft((value) => ({ ...value, [field]: selectedFolder }));
  };

  const applyTemplateContent = useCallback(
    (templateName: string, content: string) => {
      setConfig((value) => ({
        ...value,
        template_name: templateName,
        template: content,
      }));
      setDraft((value) => ({
        ...value,
        template_name: templateName,
        template: content,
      }));
    },
    [],
  );

  const switchTemplate = async (templateName: string) => {
    try {
      const result = await invoke<{ content: string }>("get_template", {
        templateName,
      });
      applyTemplateContent(templateName, result.content);
      setPreviewOriginal(false);
      try {
        await invoke("set_active_template", { templateName });
      } catch (error) {
        notify(
          `模板已切换但自动保存失败，导出仍将使用旧模板：${errorMessage(error)}`,
          "error",
        );
      }
      if (preview) {
        void showPreview(
          { label: preview.name, value: preview.path, is_file: true },
          true,
          result.content,
        );
      }
    } catch (error) {
      notify(`模板加载失败：${errorMessage(error)}`, "error");
    }
  };

  // 快捷按钮切换：settings 弹窗里改的草稿也要同步
  const switchTemplateFromBar = (templateName: string) => {
    setPreviewOriginal(false);
    if (templateName === config.template_name) {
      if (preview)
        void showPreview(
          { label: preview.name, value: preview.path, is_file: true },
          true,
          config.template,
        );
      return;
    }
    void switchTemplate(templateName);
  };

  const toggleWindowMaximize = () => {
    toggleMaximize().catch((error) =>
      notify(`最大化失败：${errorMessage(error)}`, "error"),
    );
  };

  const openSettings = () => {
    setDraft(config);
    setSettingsOpen(true);
  };

  const saveSettings = async () => {
    try {
      JSON.parse(draft.template);
      await invoke("save_config", { config: draft });
      setConfig(draft);
      setSettingsOpen(false);
      await refreshFiles();
      if (preview && !previewOriginal) {
        void showPreview(
          { label: preview.name, value: preview.path, is_file: true },
          true,
          draft.template,
        );
      }
      notify("配置已保存");
    } catch (error) {
      notify(`保存失败：${errorMessage(error)}`, "error");
    }
  };

  const createTemplate = async (name: string) => {
    try {
      const content = dialog === "saveAs" ? draft.template : "[]";
      JSON.parse(content);
      await invoke("create_template", { templateName: name, content });
      const templates = [...draft.templates, name].sort((a, b) =>
        a.localeCompare(b, "zh-CN"),
      );
      const next = {
        ...draft,
        templates,
        template_name: name,
        template: content,
      };
      setDraft(next);
      setConfig(next);
      setDialog(null);
      try {
        await invoke("set_active_template", { templateName: name });
      } catch (error) {
        notify(
          `模板已创建但自动保存失败，导出仍将使用旧模板：${errorMessage(error)}`,
          "error",
        );
        return;
      }
      notify(`已创建模板：${name}`);
    } catch (error) {
      notify(`创建模板失败：${errorMessage(error)}`, "error");
    }
  };

  const startProcessing = async () => {
    setProgress({
      ...emptyProgress,
      active: true,
      total: selected.size,
      message: "准备处理",
    });
    try {
      await invoke("start_processing", { selectedItems: [...selected] });
      await refreshAcceleration();
      await refreshFiles();
    } catch (error) {
      setProgress((value) => ({ ...value, active: false }));
      notify(`处理失败：${errorMessage(error)}`, "error");
    }
  };

  const openContextMenu = (node: FileNode, x: number, y: number) => {
    setContextMenu({ node, x, y });
  };

  const revealNode = async (node: FileNode) => {
    if (!node.value) return;
    try {
      await invoke("reveal_in_folder", { path: node.value });
    } catch (error) {
      notify(`打开所在位置失败：${errorMessage(error)}`, "error");
    }
  };

  const executeDelete = async () => {
    const node = pendingDelete;
    if (!node?.value || deleting) return;
    const target = node.value;
    setDeleting(true);
    try {
      await invoke("delete_file", { path: target });
      setSelected((current) => {
        const next = new Set(current);
        next.delete(target);
        return next;
      });
      setPreview((current) => (current?.path === target ? null : current));
      setPendingDelete(null);
      await refreshFiles();
      notify(`已删除：${node.label}`);
    } catch (error) {
      notify(`删除失败：${errorMessage(error)}`, "error");
    } finally {
      setDeleting(false);
    }
  };

  if (loading) {
    return (
      <div className="launch-screen">
        <Aperture size={32} />
        <span>LiteExif</span>
      </div>
    );
  }

  const exportDisabled = !selected.size || progress.active;
  const exportLabel = progress.active
    ? `正在导出 ${progress.percent}%`
    : selected.size
      ? `开始处理 · ${selected.size} 张`
      : "开始处理";

  return (
    <div className="app-shell">
      <header className="topbar">
        <div
          className="titlebar-drag"
          data-tauri-drag-region
          onDoubleClick={toggleWindowMaximize}
        >
          <div className="brand-mark">
            <Aperture size={20} strokeWidth={2.3} />
          </div>
          <div className="brand-name">LiteExif</div>
          <div className="topbar-meta">{__APP_VERSION__}</div>
        </div>
        <div
          className="topbar-spacer"
          data-tauri-drag-region
          onDoubleClick={toggleWindowMaximize}
        />
        <div
          className={`engine-state ${progress.active ? "is-busy" : ""} ${acceleration?.state === "disabled" ? "has-error" : ""}`}
          title={acceleration?.adapter ?? "GPU 将在需要模糊计算时初始化"}
        >
          <Gauge size={14} />
          {progress.active
            ? "正在导出"
            : acceleration?.state === "validated"
              ? `DX12 · ${acceleration.adapter?.replace("NVIDIA GeForce ", "") ?? "GPU"}${previewCacheHit ? " · 缓存" : ""}`
              : acceleration?.state === "disabled"
                ? "GPU 不可用 · CPU"
                : `GPU 待触发${previewCacheHit ? " · 缓存" : ""}`}
        </div>
        <button className="icon-button" onClick={checkUpdates} title="检查更新">
          <Download size={17} />
        </button>
        <button
          className="icon-button settings-trigger"
          onClick={openSettings}
          title="导出设置"
        >
          <Settings2 size={17} />
        </button>
        <button
          className="topbar-export"
          disabled={exportDisabled}
          onClick={startProcessing}
          title={
            selected.size
              ? `导出选中的 ${selected.size} 张`
              : "先在左侧选择照片"
          }
        >
          {progress.active ? (
            <RefreshCw size={15} className="is-spinning" />
          ) : (
            <Play size={15} fill="currentColor" />
          )}
          <span>{exportLabel}</span>
        </button>
        <WindowControls onError={(message) => notify(message, "error")} />
      </header>

      <main className="workspace workspace-v2">
        <section className="files-pane">
          <div className="pane-heading">
            <h2>
              <FileImage size={16} />
              照片
            </h2>
            <span className="pane-count">
              {selected.size}/{allFiles.length}
            </span>
            <button
              className="icon-button"
              onClick={refreshFiles}
              disabled={refreshing}
              title="刷新目录"
            >
              <RefreshCw
                size={15}
                className={refreshing ? "is-spinning" : ""}
              />
            </button>
          </div>
          <div className="tree-toolbar">
            <button
              className="text-button"
              onClick={() =>
                setSelected(
                  selected.size === allFiles.length
                    ? new Set()
                    : new Set(allFiles),
                )
              }
            >
              {selected.size === allFiles.length && allFiles.length
                ? "取消全选"
                : "全选"}
            </button>
          </div>
          <div className="tree-columns">
            <div className="tree-section">
              <div className="tree-section-title">
                <span>待处理</span>
                <strong>
                  {selected.size}/{allFiles.length}
                </strong>
              </div>
              <FileTree
                nodes={sourceNodes}
                selected={selected}
                selectable
                previewPath={preview?.path}
                onSelectionChange={updateSelection}
                onPreview={(node) => showPreview(node, !previewOriginal)}
                onContextMenu={openContextMenu}
                onLoadChildren={loadChildren}
                onLoadMore={loadMoreChildren}
                rootHasMore={inputRoot?.has_more}
                onLoadMoreRoot={() => inputRoot && loadMoreChildren(inputRoot)}
              />
            </div>
            <div className="tree-section output-tree">
              <div className="tree-section-title">
                <span>已输出</span>
                <strong>{flattenFiles(outputNodes).length}</strong>
              </div>
              <FileTree
                nodes={outputNodes}
                previewPath={preview?.path}
                onPreview={(node) => showPreview(node)}
                onContextMenu={openContextMenu}
                onLoadChildren={loadChildren}
                onLoadMore={loadMoreChildren}
                rootHasMore={outputRoot?.has_more}
                onLoadMoreRoot={() =>
                  outputRoot && loadMoreChildren(outputRoot)
                }
              />
            </div>
          </div>
        </section>

        <section className="preview-pane">
          <div className="pane-heading">
            <h2>预览</h2>
            {preview && (
              <span className="preview-filename" title={preview.name}>
                {preview.name}
              </span>
            )}
            {previewCacheHit && <span className="cache-badge">缓存</span>}
          </div>

          <ZoomablePreview
            src={preview?.url ?? null}
            name={preview?.name ?? ""}
            loading={previewLoading}
          />

          <div className="effect-bar" role="tablist" aria-label="效果切换">
            <button
              role="tab"
              aria-selected={previewOriginal}
              className={`effect-dot ${previewOriginal ? "is-active" : ""}`}
              title="原图（不应用预设）"
              onClick={() => {
                setPreviewOriginal(true);
                if (preview)
                  void showPreview({
                    label: preview.name,
                    value: preview.path,
                    is_file: true,
                  });
              }}
            >
              0
            </button>
            {config.templates.map((name, index) => (
              <button
                key={name}
                role="tab"
                aria-selected={
                  !previewOriginal && config.template_name === name
                }
                className={`effect-dot ${!previewOriginal && config.template_name === name ? "is-active" : ""}`}
                title={name}
                onClick={() => switchTemplateFromBar(name)}
              >
                {index + 1}
              </button>
            ))}
            {!config.templates.length && (
              <span className="effect-empty">暂无模板，可在设置中新建</span>
            )}
          </div>
          {(previewOriginal || config.template_name) && (
            <div
              className="effect-name"
              title={previewOriginal ? "原图" : config.template_name}
            >
              {previewOriginal
                ? "原图 · 不应用预设"
                : `效果 ${config.templates.indexOf(config.template_name) + 1} · ${config.template_name}`}
            </div>
          )}

          <div className="process-strip">
            <div className="progress-track">
              <span style={{ width: `${progress.percent}%` }} />
            </div>
            <div className="progress-meta">
              <span className="progress-label">
                {progress.message ||
                  (selected.size
                    ? `已选 ${selected.size} 张，右上角开始导出`
                    : "在左侧选择照片后导出")}
              </span>
              <span className="progress-stats">
                完成 <strong>{progress.success}</strong> · 跳过{" "}
                <strong>{progress.skipped}</strong> ·{" "}
                <span className={progress.failure ? "has-error" : ""}>
                  失败 <strong>{progress.failure}</strong>
                </span>
              </span>
            </div>
          </div>
        </section>
      </main>

      {settingsOpen && (
        <SettingsDialog
          draft={draft}
          onChange={setDraft}
          onClose={() => setSettingsOpen(false)}
          onSave={saveSettings}
          onChooseFolder={chooseFolder}
          onSwitchTemplate={switchTemplate}
          onCreateTemplate={(mode) => setDialog(mode)}
        />
      )}
      {dialog && (
        <TemplateDialog
          mode={dialog}
          onClose={() => setDialog(null)}
          onConfirm={createTemplate}
        />
      )}
      {contextMenu && (
        <FileContextMenu
          node={contextMenu.node}
          x={contextMenu.x}
          y={contextMenu.y}
          onReveal={revealNode}
          onDelete={(node) => setPendingDelete(node)}
          onClose={() => setContextMenu(null)}
        />
      )}
      {pendingDelete && (
        <ConfirmDialog
          title="删除文件"
          message={`确定删除「${pendingDelete.label}」吗？文件将移入系统回收站，可在回收站还原。`}
          confirmText={deleting ? "删除中…" : "删除"}
          onClose={() => !deleting && setPendingDelete(null)}
          onConfirm={executeDelete}
        />
      )}
      {toast && (
        <div className={`toast ${toast.kind}`}>
          <span>
            {toast.kind === "success" ? <Check size={16} /> : <X size={16} />}
          </span>
          {toast.text}
        </div>
      )}
      {updateOpen && (
        <UpdateDialog
          manual={updateManual}
          onClose={() => setUpdateOpen(false)}
        />
      )}
    </div>
  );
}
