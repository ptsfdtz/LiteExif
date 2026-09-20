import { TriangleAlert } from "lucide-react";

interface Props {
  title: string;
  message: string;
  confirmText?: string;
  onClose: () => void;
  onConfirm: () => void;
}

export function ConfirmDialog({ title, message, confirmText = "删除", onClose, onConfirm }: Props) {
  return (
    <div className="modal-backdrop" onMouseDown={(event) => event.target === event.currentTarget && onClose()}>
      <section className="dialog confirm-dialog" role="alertdialog" aria-modal="true" aria-labelledby="confirm-title">
        <header>
          <span className="dialog-icon is-danger"><TriangleAlert size={17} /></span>
          <h2 id="confirm-title">{title}</h2>
        </header>
        <div className="dialog-body">
          <p>{message}</p>
        </div>
        <footer>
          <button type="button" className="button secondary" onClick={onClose}>取消</button>
          <button type="button" className="button danger" onClick={onConfirm}>{confirmText}</button>
        </footer>
      </section>
    </div>
  );
}
