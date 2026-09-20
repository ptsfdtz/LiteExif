import { useMemo, useState } from "react";
import {
  Check,
  ChevronRight,
  FileImage,
  Folder,
  FolderOpen,
  RefreshCw,
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
  onLoadChildren?: (node: FileNode) => void | Promise<void>;
  onLoadMore?: (node: FileNode) => void | Promise<void>;
  rootHasMore?: boolean;
  onLoadMoreRoot?: () => void | Promise<void>;
}

type SharedProps = Omit<Props, "nodes" | "rootHasMore" | "onLoadMoreRoot">;

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
  onLoadChildren,
  onLoadMore,
}: SharedProps & { node: FileNode; depth: number }) {
  const [expanded, setExpanded] = useState(false);
  const [loading, setLoading] = useState(false);
  const paths = useMemo(() => collectFiles(node), [node]);
  const checkedCount = paths.filter((path) => selected?.has(path)).length;
  const checked = paths.length > 0 && checkedCount === paths.length;
  const partial = checkedCount > 0 && !checked;
  const isFolder = !node.is_file;
  const children = node.children;

  const expand = async () => {
    if (expanded) {
      setExpanded(false);
      return;
    }
    setExpanded(true);
    if (isFolder && children === undefined && !loading) {
      setLoading(true);
      try {
        await onLoadChildren?.(node);
      } finally {
        setLoading(false);
      }
    }
  };

  const choose = () => {
    if (node.is_file) onPreview(node);
    else void expand();
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
            onClick={() => void expand()}
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
      {isFolder && expanded && (
        <div className="tree-children">
          {loading && (
            <div className="tree-status" style={{ paddingInlineStart: `${26 + depth * 16}px` }}>
              <RefreshCw size={12} className="is-spinning" />
              <span>加载中…</span>
            </div>
          )}
          {!loading &&
            children?.map((child) => (
              <TreeRow
                key={child.value ?? `${node.label}-${child.label}`}
                node={child}
                depth={depth + 1}
                selected={selected}
                selectable={selectable}
                previewPath={previewPath}
                onSelectionChange={onSelectionChange}
                onPreview={onPreview}
                onContextMenu={onContextMenu}
                onLoadChildren={onLoadChildren}
                onLoadMore={onLoadMore}
              />
            ))}
          {!loading && children && children.length === 0 && (
            <div className="tree-status" style={{ paddingInlineStart: `${26 + depth * 16}px` }}>
              无图片
            </div>
          )}
          {!loading && node.has_more && (
            <button
              className="tree-more"
              style={{ marginInlineStart: `${26 + depth * 16}px` }}
              onClick={() => void onLoadMore?.(node)}
            >
              显示更多…
            </button>
          )}
        </div>
      )}
    </div>
  );
}

export function FileTree({
  nodes,
  rootHasMore,
  onLoadMoreRoot,
  ...rest
}: Props) {
  return (
    <div className="file-tree">
      {!nodes.length && <div className="empty-tree">目录中没有支持的图片</div>}
      {nodes.map((node) => (
        <TreeRow
          key={node.value ?? node.label}
          node={node}
          depth={0}
          {...rest}
        />
      ))}
      {rootHasMore && (
        <button className="tree-more" onClick={() => void onLoadMoreRoot?.()}>
          显示更多…
        </button>
      )}
    </div>
  );
}
