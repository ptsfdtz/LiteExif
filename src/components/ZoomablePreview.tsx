import { useCallback, useEffect, useRef, useState } from "react";
import { ImageOff, Maximize, RefreshCw, ZoomIn, ZoomOut } from "lucide-react";

interface Props {
  src: string | null;
  name: string;
  loading: boolean;
}

const MIN_SCALE = 0.2;
const MAX_SCALE = 8;

export function ZoomablePreview({ src, name, loading }: Props) {
  const stageRef = useRef<HTMLDivElement>(null);
  const [scale, setScale] = useState(1);
  const [offset, setOffset] = useState({ x: 0, y: 0 });
  const dragRef = useRef<{ startX: number; startY: number; baseX: number; baseY: number; active: boolean }>({
    startX: 0,
    startY: 0,
    baseX: 0,
    baseY: 0,
    active: false,
  });

  // 切换照片时复位视角
  useEffect(() => {
    setScale(1);
    setOffset({ x: 0, y: 0 });
  }, [src]);

  const zoomAt = useCallback((factor: number) => {
    setScale((value) => Math.min(MAX_SCALE, Math.max(MIN_SCALE, value * factor)));
  }, []);

  const reset = useCallback(() => {
    setScale(1);
    setOffset({ x: 0, y: 0 });
  }, []);

  // 滚轮缩放（非 passive，避免页面滚动）
  useEffect(() => {
    const stage = stageRef.current;
    if (!stage) return;
    const onWheel = (event: WheelEvent) => {
      if (!src) return;
      event.preventDefault();
      const factor = event.deltaY < 0 ? 1.12 : 1 / 1.12;
      setScale((value) => Math.min(MAX_SCALE, Math.max(MIN_SCALE, value * factor)));
    };
    stage.addEventListener("wheel", onWheel, { passive: false });
    return () => stage.removeEventListener("wheel", onWheel);
  }, [src]);

  const onPointerDown = (event: React.PointerEvent) => {
    if (!src) return;
    (event.target as HTMLElement).setPointerCapture?.(event.pointerId);
    dragRef.current = {
      startX: event.clientX,
      startY: event.clientY,
      baseX: offset.x,
      baseY: offset.y,
      active: true,
    };
  };

  const onPointerMove = (event: React.PointerEvent) => {
    if (!dragRef.current.active) return;
    const dx = event.clientX - dragRef.current.startX;
    const dy = event.clientY - dragRef.current.startY;
    setOffset({ x: dragRef.current.baseX + dx, y: dragRef.current.baseY + dy });
  };

  const endDrag = () => {
    dragRef.current.active = false;
  };

  return (
    <div
      ref={stageRef}
      className="preview-stage zoomable"
      onPointerDown={onPointerDown}
      onPointerMove={onPointerMove}
      onPointerUp={endDrag}
      onPointerLeave={endDrag}
      onDoubleClick={reset}
      title={src ? "拖动平移 · 滚轮缩放 · 双击复位" : undefined}
    >
      {src ? (
        <img
          src={src}
          alt={name}
          draggable={false}
          style={{
            transform: `translate(${offset.x}px, ${offset.y}px) scale(${scale})`,
            cursor: dragRef.current.active ? "grabbing" : "grab",
          }}
        />
      ) : (
        <div className="preview-empty">
          <ImageOff size={30} />
          <span>选择一张照片</span>
          <small>点击左侧文件即可预览处理效果</small>
        </div>
      )}
      {loading && (
        <div className="preview-loading">
          <RefreshCw size={20} className="is-spinning" />
        </div>
      )}
      {src && (
        <div className="zoom-toolbar" onPointerDown={(event) => event.stopPropagation()}>
          <button className="icon-button" onClick={() => zoomAt(1 / 1.25)} title="缩小">
            <ZoomOut size={15} />
          </button>
          <button className="zoom-percent" onClick={reset} title="点击复位">
            {Math.round(scale * 100)}%
          </button>
          <button className="icon-button" onClick={() => zoomAt(1.25)} title="放大">
            <ZoomIn size={15} />
          </button>
          <span className="zoom-divider" />
          <button className="icon-button" onClick={reset} title="适应窗口">
            <Maximize size={14} />
          </button>
        </div>
      )}
    </div>
  );
}
