import { useMemo, useState } from "react";
import {
  Check,
  ChevronRight,
  FileImage,
  Folder,
  FolderOpen,
} from "lucide-react";
import type { FileNode } from "../types";

interface Props {
  nodes: FileNode[];
  selected?: Set<string>;
  selectable?: boolean;
  previewPath?: string;
  onSelectionChange?: (paths: string[], selected: boolean) => void;
  onPreview: (node: FileNode) => void;
  onContextMenu?: (node: FileNode, x: number, y: number) => void;
}

function collectFiles(node: FileNode): string[] {
  if (node.is_file && node.value) return [node.value];
  return (node.children ?? []).flatMap(collectFiles);
}

function TreeRow({
  node,
  depth,
  selected,
  selectable,
  previewPath,
  onSelectionChange,
  onPreview,
  onContextMenu,
}: Props & { node: FileNode; depth: number }) {
  const [expanded, setExpanded] = useState(depth < 1);
  const paths = useMemo(() => collectFiles(node), [node]);
  const checkedCount = paths.filter((path) => selected?.has(path)).length;
  const checked = paths.length > 0 && checkedCount === paths.length;
  const partial = checkedCount > 0 && !checked;
  const isFolder = !node.is_file;

  const choose = () => {
    if (node.is_file) onPreview(node);
    else setExpanded((value) => !value);
  };

  return (
    <div className="tree-branch">
      <div
        className={`tree-row ${previewPath === node.value ? "is-previewing" : ""}`}
        style={{ paddingInlineStart: `${10 + depth * 16}px` }}
        onContextMenu={(event) => {
          if (!node.value || !onContextMenu) return;
          event.preventDefault();
          event.stopPropagation();
          onContextMenu(node, event.clientX, event.clientY);
        }}
      >
        {isFolder ? (
          <button
            className="icon-button tree-toggle"
            onClick={() => setExpanded((value) => !value)}
            title={expanded ? "折叠" : "展开"}
          >
            <ChevronRight size={14} className={expanded ? "is-rotated" : ""} />
          </button>
        ) : (
          <span className="tree-spacer" />
        )}

        {selectable && paths.length > 0 && (
          <button
            className={`check-control ${checked ? "is-checked" : ""} ${partial ? "is-partial" : ""}`}
            onClick={() => onSelectionChange?.(paths, !checked)}
            aria-label={`${checked ? "取消选择" : "选择"} ${node.label}`}
          >
            {checked && <Check size={12} strokeWidth={3} />}
            {partial && <span />}
          </button>
        )}

        <button
          className="tree-label"
          onClick={choose}
          title={node.value ?? node.label}
        >
          {isFolder ? (
            expanded ? (
              <FolderOpen size={16} />
            ) : (
              <Folder size={16} />
            )
          ) : (
            <FileImage size={16} />
          )}
          <span>{node.label}</span>
        </button>
      </div>
      {isFolder &&
        expanded &&
        node.children?.map((child) => (
          <TreeRow
            key={child.value ?? `${node.label}-${child.label}`}
            node={child}
            depth={depth + 1}
            nodes={[]}
            selected={selected}
            selectable={selectable}
            previewPath={previewPath}
            onSelectionChange={onSelectionChange}
            onPreview={onPreview}
            onContextMenu={onContextMenu}
          />
        ))}
    </div>
  );
}

export function FileTree(props: Props) {
  if (!props.nodes.length) {
    return <div className="empty-tree">目录中没有支持的图片</div>;
  }
  return (
    <div className="file-tree">
      {props.nodes.map((node) => (
        <TreeRow
          key={node.value ?? node.label}
          {...props}
          node={node}
          depth={0}
        />
      ))}
    </div>
  );
}
