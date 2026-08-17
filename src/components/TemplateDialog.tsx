import { useEffect, useState } from "react";
import { CopyPlus, FilePlus2, X } from "lucide-react";

interface Props {
  mode: "create" | "saveAs";
  onClose: () => void;
  onConfirm: (name: string) => void;
}

export function TemplateDialog({ mode, onClose, onConfirm }: Props) {
  const [name, setName] = useState("");
  useEffect(() => document.getElementById("template-name")?.focus(), []);
  const title = mode === "create" ? "新建模板" : "模板另存为";

  return (
    <div className="modal-backdrop" onMouseDown={(event) => event.target === event.currentTarget && onClose()}>
      <section className="dialog" role="dialog" aria-modal="true" aria-labelledby="dialog-title">
        <header>
          <span className="dialog-icon">{mode === "create" ? <FilePlus2 size={18} /> : <CopyPlus size={18} />}</span>
          <h2 id="dialog-title">{title}</h2>
          <button className="icon-button" onClick={onClose} title="关闭"><X size={18} /></button>
        </header>
        <div className="dialog-body">
          <label htmlFor="template-name">模板名称</label>
          <input
            id="template-name"
            value={name}
            onChange={(event) => setName(event.target.value)}
            onKeyDown={(event) => event.key === "Enter" && name.trim() && onConfirm(name.trim())}
          />
        </div>
        <footer>
          <button className="button secondary" onClick={onClose}>取消</button>
          <button className="button primary" disabled={!name.trim()} onClick={() => onConfirm(name.trim())}>保存</button>
        </footer>
      </section>
    </div>
  );
}
