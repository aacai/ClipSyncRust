import { useEffect, useRef, useState } from 'react';
import { createRoot } from 'react-dom/client';
import { call } from './lib/api';
import 'oink-ui/styles.css';
import './picker.css';

interface Rect {
  x: number;
  y: number;
  width: number;
  height: number;
}

interface WindowRect {
  x: number;
  y: number;
  w: number;
  h: number;
}

function Picker() {
  const [frame, setFrame] = useState<string | null>(null);
  const [windows, setWindows] = useState<WindowRect[]>([]);
  const [rect, setRect] = useState<Rect | null>(null);
  const [snap, setSnap] = useState<WindowRect | null>(null);
  const stageRef = useRef<HTMLDivElement | null>(null);
  const imageRef = useRef<HTMLImageElement | null>(null);
  const startRef = useRef<{ x: number; y: number } | null>(null);

  useEffect(() => {
    void call<string | null>('picker_frame').then(setFrame).catch(() => {});
    void call<WindowRect[]>('snap_windows').then(setWindows).catch(() => {});
  }, []);

  const stageOrigin = () => {
    const stage = stageRef.current;
    if (!stage) return { left: 0, top: 0 };
    const bounds = stage.getBoundingClientRect();
    return { left: bounds.left, top: bounds.top };
  };

  const onDown = (event: React.PointerEvent) => {
    const origin = stageOrigin();
    startRef.current = { x: event.clientX - origin.left, y: event.clientY - origin.top };
    setRect(null);
    setSnap(null);
  };

  const onMove = (event: React.PointerEvent) => {
    const start = startRef.current;
    if (!start) return;
    const origin = stageOrigin();
    const current = { x: event.clientX - origin.left, y: event.clientY - origin.top };
    const next: Rect = {
      x: Math.min(start.x, current.x),
      y: Math.min(start.y, current.y),
      width: Math.abs(current.x - start.x),
      height: Math.abs(current.y - start.y),
    };
    const hit = windows.find(
      (item) =>
        current.x >= item.x &&
        current.x <= item.x + item.w &&
        current.y >= item.y &&
        current.y <= item.y + item.h,
    );
    setSnap(hit ?? null);
    setRect(next);
  };

  const onUp = () => {
    startRef.current = null;
  };

  const crop = async () => {
    const image = imageRef.current;
    let target = rect;
    if (snap) {
      target = { x: snap.x, y: snap.y, width: snap.w, height: snap.h };
    }
    if (!image || !target || target.width < 2 || target.height < 2) return;
    const scale = image.naturalWidth / Math.max(1, image.clientWidth);
    const canvas = document.createElement('canvas');
    canvas.width = Math.round(target.width * scale);
    canvas.height = Math.round(target.height * scale);
    const context = canvas.getContext('2d');
    if (!context) return;
    context.drawImage(
      image,
      Math.round(target.x * scale),
      Math.round(target.y * scale),
      canvas.width,
      canvas.height,
      0,
      0,
      canvas.width,
      canvas.height,
    );
    void call('picker_result', { dataUrl: canvas.toDataURL('image/png') }).catch(() => {});
  };

  useEffect(() => {
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key === 'Escape') {
        event.preventDefault();
        void call('picker_cancel').catch(() => {});
      }
      if (event.key === 'Enter') {
        event.preventDefault();
        void crop();
      }
    };
    window.addEventListener('keydown', onKeyDown);
    return () => window.removeEventListener('keydown', onKeyDown);
  });

  useEffect(() => {
    const onContextMenu = (event: MouseEvent) => {
      event.preventDefault();
      void call('picker_cancel').catch(() => {});
    };
    window.addEventListener('contextmenu', onContextMenu);
    return () => window.removeEventListener('contextmenu', onContextMenu);
  }, []);

  const shown: Rect | null = snap
    ? { x: snap.x, y: snap.y, width: snap.w, height: snap.h }
    : rect;

  return (
    <div className="picker" onPointerDown={onDown} onPointerMove={onMove} onPointerUp={onUp}>
      {frame ? (
        <div className="picker__stage" ref={stageRef}>
          <img ref={imageRef} className="picker__frame" src={frame} alt="屏幕画面" draggable={false} />
          {shown && (
            <div
              className="picker__rect"
              style={{ left: shown.x, top: shown.y, width: shown.width, height: shown.height }}
            />
          )}
        </div>
      ) : (
        <div className="picker__empty">正在取画面…</div>
      )}
      <div className="picker__hint">
        {shown ? `${Math.round(shown.width)} × ${Math.round(shown.height)}` : '按住鼠标拖动来选区域'}
        {'　回车确认　Esc 取消　双击确认'}
      </div>
    </div>
  );
}

createRoot(document.getElementById('root') as HTMLElement).render(<Picker />);
