import { useEffect, useState } from 'react';
import { Button, Dialog, Field, Input } from 'oink-ui';
import type { ClipRecord } from '../../lib/types';

interface NoteDialogProps {
  item: ClipRecord | null;
  onClose: () => void;
  onSubmit: (id: string, note: string) => void;
}

export function NoteDialog({ item, onClose, onSubmit }: NoteDialogProps) {
  const [note, setNote] = useState('');

  useEffect(() => {
    if (item) setNote(item.note);
  }, [item]);

  if (!item) return null;

  return (
    <Dialog
      open
      onOpenChange={(next) => !next && onClose()}
      size="md"
      closable
      as="form"
      title="写备注"
      description="备注只保存在本机，不会同步给别人"
      onSubmit={(event) => {
        event.preventDefault();
        onSubmit(item.id, note);
        onClose();
      }}
      actions={
        <>
          <Button variant="ghost" onClick={onClose}>取消</Button>
          <Button variant="accent" type="submit">保存</Button>
        </>
      }
    >
      <Field label="备注">
        <Input
          value={note}
          onChange={setNote}
          placeholder="这条内容对你意味着什么？"
          autoFocus
        />
      </Field>
    </Dialog>
  );
}
