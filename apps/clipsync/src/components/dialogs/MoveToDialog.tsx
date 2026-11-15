import { useEffect, useState } from 'react';
import { Button, Dialog, Field, Input } from 'oink-ui';
import type { ClipRecord } from '../../lib/types';

interface MoveToDialogProps {
  item: ClipRecord | null;
  total: number;
  onClose: () => void;
  onSubmit: (id: string, target: number) => void;
}

export function MoveToDialog({ item, total, onClose, onSubmit }: MoveToDialogProps) {
  const [target, setTarget] = useState('1');

  useEffect(() => {
    if (item) setTarget('1');
  }, [item]);

  if (!item) return null;

  const parsed = Number.parseInt(target, 10);
  const invalid = !Number.isFinite(parsed) || parsed < 1 || parsed > total;

  return (
    <Dialog
      open
      onOpenChange={(next) => !next && onClose()}
      size="sm"
      closable
      as="form"
      title="移动到指定位置"
      description={`当前共有 ${total} 条`}
      onSubmit={(event) => {
        event.preventDefault();
        if (invalid) return;
        onSubmit(item.id, parsed - 1);
        onClose();
      }}
      actions={
        <>
          <Button variant="ghost" onClick={onClose}>取消</Button>
          <Button variant="accent" type="submit" disabled={invalid}>移动</Button>
        </>
      }
    >
      <Field label="目标位置" hint="从 1 开始计数" error={invalid && target.length > 0 ? '位置不合法' : undefined}>
        <Input
          value={target}
          onChange={setTarget}
          inputMode="numeric"
          autoFocus
          onKeyDown={(event) => {
            if (event.key === 'Enter') {
              event.preventDefault();
            }
          }}
        />
      </Field>
    </Dialog>
  );
}
