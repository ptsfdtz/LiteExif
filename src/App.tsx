import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import CodeMirror from "@uiw/react-codemirror";
import { json } from "@codemirror/lang-json";
import { convertFileSrc, invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { open } from "@tauri-apps/plugin-dialog";
import {
  Aperture,
  Check,
  CopyPlus,
  FileImage,
  FilePlus2,
  FolderOpen,
  ImageOff,
  Pencil,
  Play,
  RefreshCw,
  RotateCcw,
  Save,
  Settings2,
  X,
} from "lucide-react";
import { FileTree } from "./components/FileTree";
import { TemplateDialog } from "./components/TemplateDialog";
import type { AppConfig, EngineEvent, FileNode, FileTrees, ProgressState } from "./types";

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

function flattenFiles(nodes: FileNode[]): string[] {
  return nodes.flatMap((node) => node.is_file && node.value ? [node.value] : flattenFiles(node.children ?? []));
}

function errorMessage(error: unknown): string {
  return error instanceof Error ? error.message : String(error);
}

export default function App() {
  const [config, setConfig] = useState<AppConfig>(emptyConfig);
  const [savedConfig, setSavedConfig] = useState<AppConfig>(emptyConfig);
  const [trees, setTrees] = useState<FileTrees>({ input_files: [], output_files: [] });
  const [selected, setSelected] = useState<Set<string>>(new Set());
  const [preview, setPreview] = useState<{ path: string; name: string; url: string } | null>(null);
  const [previewLoading, setPreviewLoading] = useState(false);
  const [editing, setEditing] = useState(false);
  const [loading, setLoading] = useState(true);
  const [refreshing, setRefreshing] = useState(false);
  const [progress, setProgress] = useState<ProgressState>(emptyProgress);
  const [dialog, setDialog] = useState<"create" | "saveAs" | null>(null);
  const [toast, setToast] = useState<{ text: string; kind: "success" | "error" } | null>(null);
  const previewRequest = useRef(0);

  const notify = useCallback((text: string, kind: "success" | "error" = "success") => {
    setToast({ text, kind });
    window.setTimeout(() => setToast(null), 3200);
  }, []);

  const refreshFiles = useCallback(async () => {
    setRefreshing(true);
    try {
      const data = await invoke<FileTrees>("list_files");
      setTrees(data);
      const current = new Set(flattenFiles(data.input_files));
      setSelected((previous) => new Set([...previous].filter((path) => current.has(path))));
    } catch (error) {
      notify(`文件列表加载失败：${errorMessage(error)}`, "error");
    } finally {
      setRefreshing(false);
    }
  }, [notify]);

  const loadConfig = useCallback(async () => {
    const data = await invoke<AppConfig>("get_config");
    setConfig(data);
    setSavedConfig(data);
  }, []);

  useEffect(() => {
    Promise.all([loadConfig(), refreshFiles()])
      .catch((error) => notify(`初始化失败：${errorMessage(error)}`, "error"))
      .finally(() => setLoading(false));
  }, [loadConfig, notify, refreshFiles]);

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
    }).then((cleanup) => { unlisten = cleanup; });
    return () => unlisten?.();
  }, []);

  const sourceNodes = trees.input_files[0]?.children ?? [];
  const outputNodes = trees.output_files[0]?.children ?? [];
  const allFiles = useMemo(() => flattenFiles(sourceNodes), [sourceNodes]);

  const updateSelection = (paths: string[], shouldSelect: boolean) => {
    setSelected((current) => {
      const next = new Set(current);
      paths.forEach((path) => shouldSelect ? next.add(path) : next.delete(path));
      return next;
    });
  };

  const showPreview = async (node: FileNode, processed = false) => {
    if (!node.value) return;
    const request = ++previewRequest.current;
    setPreviewLoading(true);
    try {
      const result = await invoke<{ path: string }>(
        processed ? "prepare_processed_preview" : "prepare_preview",
        processed ? { path: node.value, template: config.template } : { path: node.value },
      );
      if (request === previewRequest.current) {
        setPreview({ path: node.value, name: node.label, url: convertFileSrc(result.path) });
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
    const selectedFolder = await open({ directory: true, multiple: false, defaultPath: config[field] || undefined });
    if (typeof selectedFolder === "string") setConfig((value) => ({ ...value, [field]: selectedFolder }));
  };

  const switchTemplate = async (templateName: string) => {
    try {
      const result = await invoke<{ content: string }>("get_template", { templateName });
      setConfig((value) => ({ ...value, template_name: templateName, template: result.content }));
    } catch (error) {
      notify(`模板加载失败：${errorMessage(error)}`, "error");
    }
  };

  const save = async () => {
    try {
      JSON.parse(config.template);
      await invoke("save_config", { config });
      setSavedConfig(config);
      setEditing(false);
      await refreshFiles();
      notify("配置已保存");
    } catch (error) {
      notify(`保存失败：${errorMessage(error)}`, "error");
    }
  };

  const createTemplate = async (name: string) => {
    try {
      const content = dialog === "saveAs" ? config.template : "[]";
      JSON.parse(content);
      await invoke("create_template", { templateName: name, content });
      const templates = [...config.templates, name].sort((a, b) => a.localeCompare(b, "zh-CN"));
      setConfig((value) => ({ ...value, templates, template_name: name, template: content }));
      setDialog(null);
      setEditing(true);
      notify(`已创建模板：${name}`);
    } catch (error) {
      notify(`创建模板失败：${errorMessage(error)}`, "error");
    }
  };

  const startProcessing = async () => {
    setProgress({ ...emptyProgress, active: true, total: selected.size, message: "准备处理" });
    try {
      await invoke("start_processing", { selectedItems: [...selected] });
      await refreshFiles();
    } catch (error) {
      setProgress((value) => ({ ...value, active: false }));
      notify(`处理失败：${errorMessage(error)}`, "error");
    }
  };

  if (loading) {
    return <div className="launch-screen"><Aperture size={32} /><span>LiteExif</span></div>;
  }

  return (
    <div className="app-shell">
      <header className="topbar">
        <div className="brand-mark"><Aperture size={20} strokeWidth={2.3} /></div>
        <div className="brand-name">LiteExif</div>
        <div className="topbar-meta">2.1.5</div>
        <div className="topbar-spacer" />
        <div className={`engine-state ${progress.active ? "is-busy" : ""}`}>
          <span />{progress.active ? "正在导出" : "就绪"}
        </div>
      </header>

      <main className="workspace">
        <aside className="settings-pane">
          <div className="pane-heading">
            <h1><Settings2 size={17} />导出设置</h1>
            {!editing ? (
              <button className="icon-button" onClick={() => setEditing(true)} title="编辑设置"><Pencil size={16} /></button>
            ) : (
              <button className="icon-button" onClick={() => { setConfig(savedConfig); setEditing(false); }} title="取消编辑"><X size={17} /></button>
            )}
          </div>

          <div className="settings-scroll">
            <section className="field-group">
              <label>输入目录</label>
              <div className="path-field">
                <input value={config.input_folder} disabled={!editing} onChange={(event) => setConfig({ ...config, input_folder: event.target.value })} />
                <button className="icon-button" disabled={!editing} onClick={() => chooseFolder("input_folder")} title="选择输入目录"><FolderOpen size={16} /></button>
              </div>
            </section>

            <section className="field-group">
              <label>输出目录</label>
              <div className="path-field">
                <input value={config.output_folder} disabled={!editing} onChange={(event) => setConfig({ ...config, output_folder: event.target.value })} />
                <button className="icon-button" disabled={!editing} onClick={() => chooseFolder("output_folder")} title="选择输出目录"><FolderOpen size={16} /></button>
              </div>
            </section>

            <section className="setting-row">
              <div><span>覆盖已有文件</span></div>
              <button
                className={`switch ${config.override_existed ? "is-on" : ""}`}
                disabled={!editing}
                onClick={() => setConfig({ ...config, override_existed: !config.override_existed })}
                role="switch"
                aria-checked={config.override_existed}
              ><span /></button>
            </section>

            <section className="field-group quality-field">
              <div className="field-label-row"><label>输出质量</label><output>{config.quality}%</output></div>
              <input type="range" min="1" max="100" value={config.quality} disabled={!editing} onChange={(event) => setConfig({ ...config, quality: Number(event.target.value) })} />
            </section>

            <div className="section-rule" />

            <section className="field-group template-section">
              <div className="field-label-row">
                <label>水印模板</label>
                <div className="compact-actions">
                  <button className="icon-button" disabled={!editing} onClick={() => setDialog("create")} title="新建模板"><FilePlus2 size={16} /></button>
                  <button className="icon-button" disabled={!editing} onClick={() => setDialog("saveAs")} title="模板另存为"><CopyPlus size={16} /></button>
                </div>
              </div>
              <select disabled={!editing} value={config.template_name} onChange={(event) => switchTemplate(event.target.value)}>
                {config.templates.map((name) => <option key={name} value={name}>{name}</option>)}
              </select>
              <div className={`code-editor ${!editing ? "is-readonly" : ""}`}>
                <CodeMirror
                  value={config.template}
                  height="100%"
                  extensions={[json()]}
                  editable={editing}
                  basicSetup={{ lineNumbers: true, foldGutter: true, highlightActiveLine: editing }}
                  onChange={(value) => setConfig({ ...config, template: value })}
                />
              </div>
            </section>
          </div>

          {editing && (
            <div className="settings-actions">
              <button className="button secondary" onClick={() => { setConfig(savedConfig); setEditing(false); }}><RotateCcw size={15} />撤销</button>
              <button className="button primary" onClick={save}><Save size={15} />保存设置</button>
            </div>
          )}
        </aside>

        <section className="files-pane">
          <div className="pane-heading">
            <h2><FileImage size={17} />照片</h2>
            <button className="icon-button" onClick={refreshFiles} disabled={refreshing} title="刷新目录">
              <RefreshCw size={16} className={refreshing ? "is-spinning" : ""} />
            </button>
          </div>
          <div className="tree-toolbar">
            <div className="segmented-control">
              <span className="is-active">待处理</span>
              <span>已输出</span>
            </div>
            <button className="text-button" onClick={() => setSelected(selected.size === allFiles.length ? new Set() : new Set(allFiles))}>
              {selected.size === allFiles.length && allFiles.length ? "取消全选" : "全选"}
            </button>
          </div>
          <div className="tree-columns">
            <div className="tree-section">
              <div className="tree-section-title"><span>输入</span><strong>{selected.size}/{allFiles.length}</strong></div>
              <FileTree
                nodes={sourceNodes}
                selected={selected}
                selectable
                previewPath={preview?.path}
                onSelectionChange={updateSelection}
                onPreview={(node) => showPreview(node, true)}
              />
            </div>
            <div className="tree-section output-tree">
              <div className="tree-section-title"><span>输出</span><strong>{flattenFiles(outputNodes).length}</strong></div>
              <FileTree nodes={outputNodes} previewPath={preview?.path} onPreview={(node) => showPreview(node)} />
            </div>
          </div>
        </section>

        <section className="preview-pane">
          <div className="pane-heading">
            <h2>预览</h2>
            {preview && <span className="preview-filename" title={preview.name}>{preview.name}</span>}
          </div>
          <div className="preview-stage">
            {preview ? (
              <img src={preview.url} alt={preview.name} />
            ) : (
              <div className="preview-empty"><ImageOff size={30} /><span>选择一张照片</span></div>
            )}
            {previewLoading && <div className="preview-loading"><RefreshCw size={20} className="is-spinning" /></div>}
          </div>

          <div className="process-panel">
            <div className="progress-header">
              <div>
                <span className="progress-label">{progress.message || "批量导出"}</span>
                {progress.current && <span className="progress-current">{progress.current}</span>}
              </div>
              <strong>{progress.active || progress.complete ? `${progress.percent}%` : `${selected.size} 张`}</strong>
            </div>
            <div className="progress-track"><span style={{ width: `${progress.percent}%` }} /></div>
            <div className="progress-stats">
              <span>完成 <strong>{progress.success}</strong></span>
              <span>跳过 <strong>{progress.skipped}</strong></span>
              <span className={progress.failure ? "has-error" : ""}>失败 <strong>{progress.failure}</strong></span>
            </div>
            <button className="export-button" disabled={!selected.size || progress.active || editing} onClick={startProcessing}>
              {progress.active ? <RefreshCw size={18} className="is-spinning" /> : progress.complete ? <Check size={18} /> : <Play size={18} fill="currentColor" />}
              {progress.active ? "正在导出" : "开始处理"}
            </button>
          </div>
        </section>
      </main>

      {dialog && <TemplateDialog mode={dialog} onClose={() => setDialog(null)} onConfirm={createTemplate} />}
      {toast && <div className={`toast ${toast.kind}`}><span>{toast.kind === "success" ? <Check size={16} /> : <X size={16} />}</span>{toast.text}</div>}
    </div>
  );
}
