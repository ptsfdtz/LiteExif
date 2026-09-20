import { useEffect, useRef } from "react";
import { FolderOpen, Trash2 } from "lucide-react";
import type { FileNode } from "../types";

interface Props {
  node: FileNode;
  x: number;
  y: number;
  onReveal: (node: FileNode) => void;
  onDelete: (node: FileNode) => void;
  onClose: () => void;
}

export function FileContextMenu({ node, x, y, onReveal, onDelete, onClose }: Props) {
  const menuRef = useRef<HTMLDivElement>(null);
  const isFile = !!node.is_file;

  useEffect(() => {
    const onPointerDown = (event: PointerEvent) => {
      if (!menuRef.current?.contains(event.target as HTMLElement)) onClose();
    };
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key === "Escape") onClose();
    };
    const onScroll = () => onClose();
    document.addEventListener("pointerdown", onPointerDown);
    document.addEventListener("keydown", onKeyDown);
    window.addEventListener("blur", onClose);
    // 树滚动时跟随关闭，避免错位
    document.querySelector(".tree-columns")?.addEventListener("scroll", onScroll);
    return () => {
      document.removeEventListener("pointerdown", onPointerDown);
      document.removeEventListener("keydown", onKeyDown);
      window.removeEventListener("blur", onClose);
      document.querySelector(".tree-columns")?.removeEventListener("scroll", onScroll);
    };
  }, [onClose]);

  // 防止菜单超出视口
  const style: React.CSSProperties = { left: Math.min(x, window.innerWidth - 190), top: Math.min(y, window.innerHeight - 110) };

  return (
    <div ref={menuRef} className="context-menu" style={style} role="menu">
      <div className="context-title" title={node.value ?? node.label}>{node.label}</div>
      <button
        type="button"
        className="context-item"
        onClick={() => { onReveal(node); onClose(); }}
      >
        <FolderOpen size={14} />
        <span>{isFile ? "打开所在位置" : "打开文件夹"}</span>
      </button>
      {isFile && (
        <button
          type="button"
          className="context-item is-danger"
          onClick={() => { onDelete(node); onClose(); }}
        >
          <Trash2 size={14} />
          <span>删除（移入回收站）</span>
        </button>
      )}
    </div>
  );
}
