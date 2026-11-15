import { useEffect, useState } from 'react';
import { Button, Dialog, Text, toast } from 'oink-ui';
import { call } from '../../lib/api';
import type { ClipRecord } from '../../lib/types';

interface TransformDialogProps {
  item: ClipRecord | null;
  onClose: () => void;
}

interface TextOp {
  key: string;
  label: string;
}

export function TransformDialog({ item, onClose }: TransformDialogProps) {
  const [ops, setOps] = useState<TextOp[]>([]);
  const [result, setResult] = useState('');
  const [active, setActive] = useState('');

  useEffect(() => {
    void call<TextOp[]>('text_op_list').then(setOps).catch(() => setOps([]));
  }, []);

  useEffect(() => {
    if (item) {
      setResult(item.text);
      setActive('');
    }
  }, [item]);

  if (!item) return null;

  const run = async (op: string) => {
    setActive(op);
    try {
      const out = await call<string>('text_transform', { text: item.text, op });
      setResult(out);
    } catch (error) {
      setResult(String(error));
    }
  };

  return (
    <Dialog
      open
      onOpenChange={(next) => !next && onClose()}
      size="lg"
      closable
      title="文本处理"
      description="一键变换，结果可以复制"
      actions={
        <>
          <Button
            variant="ghost"
            onClick={() => {
              void navigator.clipboard?.writeText(result);
              toast.success('已复制结果');
            }}
          >
            复制结果
          </Button>
          <Button variant="accent" onClick={onClose}>关闭</Button>
        </>
      }
    >
      <div className="op-grid">
        {ops.map((op) => (
          <Button key={op.key} variant="soft" size="sm" disabled={active === op.key} onClick={() => void run(op.key)}>
            {op.label}
          </Button>
        ))}
      </div>
      {ops.length === 0 && <Text variant="caption" tone="subtle">文本操作加载中…</Text>}
      <pre className="detail-text">{result}</pre>
    </Dialog>
  );
}
