import { useEffect, useState } from 'react';
import { Button, Dialog, Text } from 'oink-ui';
import { call } from '../../lib/api';
import type { ClipRecord } from '../../lib/types';

interface ImagePreviewDialogProps {
  item: ClipRecord | null;
  onClose: () => void;
}

export function ImagePreviewDialog({ item, onClose }: ImagePreviewDialogProps) {
  const [src, setSrc] = useState('');
  const [error, setError] = useState('');

  useEffect(() => {
    if (!item) return;
    const path = item.file?.local_path;
    if (!path) {
      setError('图片还没下载到本机');
      return;
    }
    void call<string>('image_preview', { path, maxEdge: 2048 })
      .then(setSrc)
      .catch((e) => setError(String(e)));
  }, [item]);

  useEffect(() => {
    if (!item) return;
    const onKey = (event: KeyboardEvent) => {
      if (event.key === 'Escape') onClose();
    };
    window.addEventListener('keydown', onKey);
    return () => window.removeEventListener('keydown', onKey);
  }, [item, onClose]);

  if (!item) return null;

  return (
    <Dialog
      open
      onOpenChange={(next) => !next && onClose()}
      size="lg"
      closable
      title={item.file?.name ?? '图片'}
      description="Esc 关闭"
      actions={<Button variant="accent" onClick={onClose}>关闭</Button>}
    >
      {src ? (
        <img src={src} alt={item.file?.name ?? '图片'} className="preview-image" />
      ) : error ? (
        <Text variant="caption" tone="danger">{error}</Text>
      ) : (
        <Text variant="caption" tone="subtle">载入中…</Text>
      )}
    </Dialog>
  );
}
