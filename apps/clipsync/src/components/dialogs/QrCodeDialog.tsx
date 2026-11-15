import { useEffect, useState } from 'react';
import { Button, Dialog, Text, toast } from 'oink-ui';
import { call } from '../../lib/api';
import type { ClipRecord } from '../../lib/types';

interface QrCodeDialogProps {
  item: ClipRecord | null;
  onClose: () => void;
}

export function QrCodeDialog({ item, onClose }: QrCodeDialogProps) {
  const [image, setImage] = useState('');
  const [error, setError] = useState('');

  useEffect(() => {
    if (!item) return;
    setImage('');
    setError('');
    void call<string>('qr_code_png_for', { text: item.text, fileLink: item.file?.link ?? null })
      .then((dataUrl) => setImage(dataUrl))
      .catch((e) => setError(String(e)));
  }, [item]);

  if (!item) return null;

  return (
    <Dialog
      open
      onOpenChange={(next) => !next && onClose()}
      size="md"
      closable
      title="二维码"
      description={item.file ? '扫码即可下载这个文件' : '扫码即可拿到这段文字'}
      actions={
        <>
          <Button
            variant="ghost"
            onClick={() => {
              void navigator.clipboard?.writeText(item.kind === 'text' ? item.text : item.file?.link ?? '');
              toast.success('已复制内容');
            }}
          >
            复制内容
          </Button>
          <Button variant="accent" onClick={onClose}>关闭</Button>
        </>
      }
    >
      {image ? (
        <div className="qr-wrap">
          <img src={image} alt="二维码" className="qr-image" />
          <Text variant="caption" tone="subtle">用手机相机扫一下</Text>
        </div>
      ) : error ? (
        <Text variant="caption" tone="danger">{error}</Text>
      ) : (
        <Text variant="caption" tone="subtle">生成中…</Text>
      )}
    </Dialog>
  );
}
