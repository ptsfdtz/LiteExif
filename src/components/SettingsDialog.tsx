import CodeMirror from "@uiw/react-codemirror";
import { json } from "@codemirror/lang-json";
import { CopyPlus, FilePlus2, FolderOpen, Settings2, X } from "lucide-react";
import { Select } from "./Select";
import type { AppConfig } from "../types";

interface Props {
  draft: AppConfig;
  onChange: (next: AppConfig) => void;
  onClose: () => void;
  onSave: () => void;
  onChooseFolder: (field: "input_folder" | "output_folder") => void;
  onSwitchTemplate: (name: string) => void;
  onCreateTemplate: (mode: "create" | "saveAs") => void;
}

export function SettingsDialog({
  draft,
  onChange,
  onClose,
  onSave,
  onChooseFolder,
  onSwitchTemplate,
  onCreateTemplate,
}: Props) {
  return (
    <div
      className="modal-backdrop"
      onMouseDown={(event) => event.target === event.currentTarget && onClose()}
    >
      <section className="dialog settings-dialog" role="dialog" aria-modal="true" aria-labelledby="settings-title">
        <header>
          <span className="dialog-icon">
            <Settings2 size={17} />
          </span>
          <h2 id="settings-title">导出设置</h2>
          <button className="icon-button" onClick={onClose} title="关闭">
            <X size={17} />
          </button>
        </header>

        <div className="dialog-body settings-body">
          <div className="settings-grid">
            <section className="field-group">
              <label>输入目录</label>
              <div className="path-field">
                <input
                  value={draft.input_folder}
                  onChange={(event) => onChange({ ...draft, input_folder: event.target.value })}
                  placeholder="选择照片所在目录"
                />
                <button className="icon-button" onClick={() => onChooseFolder("input_folder")} title="选择输入目录">
                  <FolderOpen size={16} />
                </button>
              </div>
            </section>
            <section className="field-group">
              <label>输出目录</label>
              <div className="path-field">
                <input
                  value={draft.output_folder}
                  onChange={(event) => onChange({ ...draft, output_folder: event.target.value })}
                  placeholder="选择导出目录"
                />
                <button className="icon-button" onClick={() => onChooseFolder("output_folder")} title="选择输出目录">
                  <FolderOpen size={16} />
                </button>
              </div>
            </section>
          </div>

          <div className="settings-grid">
            <section className="setting-row">
              <span>覆盖已有文件</span>
              <button
                className={`switch ${draft.override_existed ? "is-on" : ""}`}
                onClick={() => onChange({ ...draft, override_existed: !draft.override_existed })}
                role="switch"
                aria-checked={draft.override_existed}
              >
                <span />
              </button>
            </section>
            <section className="field-group quality-field inline">
              <div className="field-label-row">
                <label>输出质量</label>
                <output>{draft.quality}%</output>
              </div>
              <input
                type="range"
                min="1"
                max="100"
                value={draft.quality}
                style={{ "--range-progress": `${draft.quality}%` } as React.CSSProperties}
                onChange={(event) => onChange({ ...draft, quality: Number(event.target.value) })}
              />
            </section>
          </div>

          <section className="field-group template-section">
            <div className="field-label-row">
              <label>
                水印模板
                <em>数字 1-{draft.templates.length} 对应预览下方快捷按钮</em>
              </label>
              <div className="compact-actions">
                <button className="icon-button" onClick={() => onCreateTemplate("create")} title="新建模板">
                  <FilePlus2 size={16} />
                </button>
                <button className="icon-button" onClick={() => onCreateTemplate("saveAs")} title="模板另存为">
                  <CopyPlus size={16} />
                </button>
              </div>
            </div>
            <Select
              value={draft.template_name}
              options={draft.templates}
              onChange={onSwitchTemplate}
              placeholder="选择模板"
              ariaLabel="水印模板"
            />
            <div className="code-editor">
              <CodeMirror
                value={draft.template}
                height="100%"
                extensions={[json()]}
                editable
                basicSetup={{ lineNumbers: true, foldGutter: true, highlightActiveLine: true }}
                onChange={(value) => onChange({ ...draft, template: value })}
              />
            </div>
          </section>
        </div>

        <footer>
          <button className="button secondary" onClick={onClose}>
            取消
          </button>
          <button className="button primary" onClick={onSave}>
            保存设置
          </button>
        </footer>
      </section>
    </div>
  );
}
