import { useEffect, useRef, useState } from 'react';
import { Button, Dialog, Text } from 'oink-ui';

interface ScreenPickDialogProps {
  frame: string | null;
  onClose: () => void;
  onConfirm: (cropped: string) => void;
}

interface Rect {
  x: number;
  y: number;
  width: number;
  height: number;
}

export function ScreenPickDialog({ frame, onClose, onConfirm }: ScreenPickDialogProps) {
  const imageRef = useRef<HTMLImageElement | null>(null);
  const [start, setStart] = useState<{ x: number; y: number } | null>(null);
  const [rect, setRect] = useState<Rect | null>(null);

  useEffect(() => {
    if (!frame) return;
    setStart(null);
    setRect(null);
  }, [frame]);

  useEffect(() => {
    if (!frame) return;
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key === 'Enter') {
        event.preventDefault();
        void crop();
      }
    };
    window.addEventListener('keydown', onKeyDown);
    return () => window.removeEventListener('keydown', onKeyDown);
  });

  const pick = (event: React.PointerEvent<HTMLDivElement>) => {
    if (!frame) return;
    const bounds = event.currentTarget.getBoundingClientRect();
    setStart({ x: event.clientX - bounds.left, y: event.clientY - bounds.top });
    setRect(null);
  };

  const drag = (event: React.PointerEvent<HTMLDivElement>) => {
    if (!start || !frame) return;
    const bounds = event.currentTarget.getBoundingClientRect();
    const current = { x: event.clientX - bounds.left, y: event.clientY - bounds.top };
    setRect({
      x: Math.min(start.x, current.x),
      y: Math.min(start.y, current.y),
      width: Math.abs(current.x - start.x),
      height: Math.abs(current.y - start.y),
    });
  };

  const crop = async () => {
    const image = imageRef.current;
    if (!image || !rect || rect.width < 2 || rect.height < 2) return;
    const scale = image.naturalWidth / image.clientWidth;
    const canvas = document.createElement('canvas');
    canvas.width = Math.round(rect.width * scale);
    canvas.height = Math.round(rect.height * scale);
    const context = canvas.getContext('2d');
    if (!context) return;
    context.drawImage(
      image,
      Math.round(rect.x * scale),
      Math.round(rect.y * scale),
      canvas.width,
      canvas.height,
      0,
      0,
      canvas.width,
      canvas.height,
    );
    onConfirm(canvas.toDataURL('image/png'));
  };

  return (
    <Dialog
      open={Boolean(frame)}
      onOpenChange={(next) => !next && onClose()}
      size="lg"
      closable
      title="框选要截的区域"
      description="拖动鼠标框一块区域，回车确认，Esc 取消"
      actions={
        <>
          <Button variant="ghost" onClick={onClose}>取消</Button>
          <Button
            variant="accent"
            disabled={!rect || rect.width < 2 || rect.height < 2}
            onClick={() => void crop()}
          >
            截这一块
          </Button>
        </>
      }
    >
      <div
        className="screen-pick"
        onPointerDown={pick}
        onPointerMove={drag}
        onPointerUp={() => setStart(null)}
        onDoubleClick={() => void crop()}
        onContextMenu={(event) => {
          event.preventDefault();
          onClose();
        }}
      >
        {frame && <img ref={imageRef} className="screen-pick__frame" src={frame} alt="屏幕画面" />}
        {rect && (
          <div
            className="screen-pick__rect"
            style={{ left: rect.x, top: rect.y, width: rect.width, height: rect.height }}
          />
        )}
      </div>
      <Text variant="caption" tone="subtle">
        {rect ? `${Math.round(rect.width)} × ${Math.round(rect.height)}` : '按住鼠标拖动来选区域'}
      </Text>
    </Dialog>
  );
}
