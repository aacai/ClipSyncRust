import { useState } from 'react';
import { Button, Dialog, Field } from 'oink-ui';

interface NewEntryDialogProps {
  open: boolean;
  onClose: () => void;
  onSubmit: (text: string) => void;
}

export function NewEntryDialog({ open, onClose, onSubmit }: NewEntryDialogProps) {
  const [text, setText] = useState('');

  return (
    <Dialog
      open={open}
      onOpenChange={(next) => !next && onClose()}
      size="lg"
      closable
      as="form"
      title="新建一条"
      description="已加入房间时发送给对方，未加入则只存在本机"
      onSubmit={(event) => {
        event.preventDefault();
        if (!text.trim()) return;
        onSubmit(text);
        setText('');
        onClose();
      }}
      actions={
        <>
          <Button variant="ghost" onClick={onClose}>取消</Button>
          <Button variant="accent" type="submit" disabled={!text.trim()}>发送</Button>
        </>
      }
    >
      <Field label="内容">
        <textarea
          className="text-area"
          value={text}
          rows={10}
          autoFocus
          onChange={(event) => setText(event.target.value)}
          onKeyDown={(event) => {
            if ((event.metaKey || event.ctrlKey) && event.key === 'Enter') {
              event.preventDefault();
              if (!text.trim()) return;
              onSubmit(text);
              setText('');
              onClose();
            }
          }}
        />
      </Field>
    </Dialog>
  );
}
