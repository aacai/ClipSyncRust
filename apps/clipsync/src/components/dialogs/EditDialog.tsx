import { useEffect, useState } from 'react';
import { Button, Dialog, Field } from 'oink-ui';
import type { ClipRecord } from '../../lib/types';

interface EditDialogProps {
  item: ClipRecord | null;
  onClose: () => void;
  onSubmit: (id: string, text: string) => void;
}

export function EditDialog({ item, onClose, onSubmit }: EditDialogProps) {
  const [text, setText] = useState('');

  useEffect(() => {
    if (item) setText(item.text);
  }, [item]);

  if (!item) return null;

  return (
    <Dialog
      open
      onOpenChange={(next) => !next && onClose()}
      size="lg"
      closable
      as="form"
      title="编辑内容"
      description="修改后只保存在本机"
      onSubmit={(event) => {
        event.preventDefault();
        onSubmit(item.id, text);
        onClose();
      }}
      actions={
        <>
          <Button variant="ghost" onClick={onClose}>取消</Button>
          <Button variant="accent" type="submit">保存</Button>
        </>
      }
    >
      <Field label="内容">
        <textarea
          className="text-area"
          value={text}
          rows={12}
          autoFocus
          onChange={(event) => setText(event.target.value)}
        />
      </Field>
    </Dialog>
  );
}
