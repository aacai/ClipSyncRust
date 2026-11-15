import { useEffect, useRef, useState } from 'react';
import type { DragDropEvent } from '@tauri-apps/api/webview';

export function useFileDrop(onDrop: (paths: string[]) => void): boolean {
  const [hovering, setHovering] = useState(false);
  const handler = useRef(onDrop);
  handler.current = onDrop;

  useEffect(() => {
    let unlisten: (() => void) | undefined;
    let disposed = false;

    void (async () => {
      try {
        const { getCurrentWebview } = await import('@tauri-apps/api/webview');
        const stop = await getCurrentWebview().onDragDropEvent((event) => {
          const payload = event.payload as DragDropEvent;
          if (payload.type === 'enter' || payload.type === 'over') {
            setHovering(true);
          } else if (payload.type === 'leave') {
            setHovering(false);
          } else if (payload.type === 'drop') {
            setHovering(false);
            if (payload.paths.length > 0) {
              handler.current(payload.paths);
            }
          }
        });
        if (disposed) stop();
        else unlisten = stop;
      } catch {}
    })();

    return () => {
      disposed = true;
      unlisten?.();
    };
  }, []);

  return hovering;
}
