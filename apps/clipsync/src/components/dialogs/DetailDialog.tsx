import { Button, Dialog, Text } from 'oink-ui';
import type { ClipRecord } from '../../lib/types';

interface DetailDialogProps {
  item: ClipRecord | null;
  onClose: () => void;
  onCopy: (item: ClipRecord) => void;
}

export function DetailDialog({ item, onClose, onCopy }: DetailDialogProps) {
  if (!item) return null;
  const file = item.file;

  return (
    <Dialog
      open
      onOpenChange={(next) => !next && onClose()}
      size="lg"
      closable
      title="详情"
      description={item.sender_name ? `来自 ${item.sender_name}` : '本机记录'}
      actions={
        <>
          <Button variant="ghost" onClick={onClose}>关闭</Button>
          <Button variant="accent" onClick={() => { onCopy(item); onClose(); }}>复制</Button>
        </>
      }
    >
      {item.kind === 'text' ? (
        <pre className="detail-text">{item.text}</pre>
      ) : (
        <div className="detail-file">
          <Text variant="label">{file?.name}</Text>
          <Text variant="caption" tone="subtle">{file?.mime} · {file?.size ?? 0} 字节</Text>
          <Text variant="caption" tone="subtle">
            {file?.local_path ? `本机路径：${file.local_path}` : '尚未下载到本机'}
          </Text>
          {file?.link && <Text variant="caption" tone="subtle">下载链接：{file.link}</Text>}
        </div>
      )}
      {item.note && (
        <div className="detail-note">
          <Text variant="micro" tone="subtle">备注</Text>
          <Text variant="caption">{item.note}</Text>
        </div>
      )}
    </Dialog>
  );
}
