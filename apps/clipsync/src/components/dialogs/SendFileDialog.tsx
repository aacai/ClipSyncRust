import { useEffect, useState } from 'react';
import { Button, Dialog, Text, toast } from 'oink-ui';
import { open as openDialog } from '@tauri-apps/plugin-dialog';
import type { DragDropEvent } from '@tauri-apps/api/webview';
import { claimDrops, releaseDrops } from '../../lib/dropBus';

interface SendFileDialogProps {
  open: boolean;
  onClose: () => void;
  onSend: (paths: string[]) => Promise<void>;
}

export function SendFileDialog({ open, onClose, onSend }: SendFileDialogProps) {
  const [paths, setPaths] = useState<string[]>([]);
  const [busy, setBusy] = useState(false);
  const [hovering, setHovering] = useState(false);

  useEffect(() => {
    if (!open) return;
    claimDrops();
    return () => releaseDrops();
  }, [open]);

  useEffect(() => {
    if (!open) return;
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
              setPaths((prev) => Array.from(new Set([...prev, ...payload.paths])));
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
  }, [open]);

  const pick = async () => {
    const picked = await openDialog({ multiple: true });
    if (!picked) return;
    setPaths(Array.isArray(picked) ? picked : [picked]);
  };

  const send = async () => {
    if (paths.length === 0) return;
    setBusy(true);
    try {
      await onSend(paths);
      setPaths([]);
      onClose();
    } catch (error) {
      toast.error(String(error));
    } finally {
      setBusy(false);
    }
  };

  return (
    <Dialog
      open={open}
      onOpenChange={(next) => !next && onClose()}
      size="md"
      closable
      title="发送文件"
      description="文件会加密后上传，房间里的设备凭直链下载"
      actions={
        <>
          <Button variant="ghost" onClick={onClose}>取消</Button>
          <Button variant="accent" onClick={() => void send()} disabled={busy || paths.length === 0}>
            {busy ? '发送中…' : `发送${paths.length ? ` (${paths.length})` : ''}`}
          </Button>
        </>
      }
    >
      <div className="send-file">
        <div className={`send-file__drop${hovering ? ' send-file__drop--over' : ''}`}>
          <Text variant="caption" tone="subtle">把文件拖到这里，或</Text>
          <Button variant="soft" leading={<span>＋</span>} onClick={() => void pick()}>
            从电脑选择
          </Button>
        </div>
        {paths.length === 0 ? (
          <Text variant="caption" tone="subtle">还没选文件</Text>
        ) : (
          <ul className="send-file__list">
            {paths.map((path) => (
              <li key={path} className="send-file__item">
                <Text variant="caption" truncate>{path.split('/').pop()}</Text>
                <button
                  type="button"
                  className="send-file__remove"
                  aria-label="移除"
                  onClick={() => setPaths((prev) => prev.filter((item) => item !== path))}
                >
                  ×
                </button>
              </li>
            ))}
          </ul>
        )}
      </div>
    </Dialog>
  );
}
