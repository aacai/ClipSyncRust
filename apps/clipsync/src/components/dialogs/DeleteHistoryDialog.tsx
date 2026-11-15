import { useEffect, useState } from 'react';
import { Button, Dialog, Text } from 'oink-ui';
import { call } from '../../lib/api';

interface DeleteHistoryDialogProps {
  open: boolean;
  onClose: () => void;
  onDeleted: () => void;
}

interface Span {
  key: string;
  label: string;
  after: number;
}

const SPANS: Span[] = [
  { key: '1h', label: '1 小时前', after: 60 * 60 * 1000 },
  { key: '1d', label: '1 天前', after: 24 * 60 * 60 * 1000 },
  { key: '7d', label: '7 天前', after: 7 * 24 * 60 * 60 * 1000 },
  { key: 'all', label: '全部', after: 0 },
];

export function DeleteHistoryDialog({ open, onClose, onDeleted }: DeleteHistoryDialogProps) {
  const [counts, setCounts] = useState<Record<string, number>>({});

  useEffect(() => {
    if (!open) return;
    void (async () => {
      const next: Record<string, number> = {};
      for (const span of SPANS) {
        next[span.key] = await call<number>('history_count_since', {
          after: Date.now() - span.after,
        });
      }
      setCounts(next);
    })();
  }, [open]);

  return (
    <Dialog
      open={open}
      onOpenChange={(next) => !next && onClose()}
      size="md"
      closable
      title="删除历史"
      description="选择要删除的范围，本机记录会一并清掉"
      actions={<Button variant="ghost" onClick={onClose}>取消</Button>}
    >
      <div className="delete-spans">
        {SPANS.map((span) => (
          <Button
            key={span.key}
            variant="soft"
            onClick={() => {
              void call<number>('history_remove_since', { after: Date.now() - span.after })
                .then((removed) => {
                  onDeleted();
                  onClose();
                  return removed;
                })
                .catch(() => onClose());
            }}
          >
            {span.label}（{counts[span.key] ?? 0} 条）
          </Button>
        ))}
      </div>
      <Text variant="micro" tone="subtle">
        置顶的条目不会被时间清理带走，但这里的删除会一并清掉。
      </Text>
    </Dialog>
  );
}
