import { useState } from 'react';
import { Button, Dialog, Field, Input, Text, toast } from 'oink-ui';
import type { ClipsyncState } from '../hooks/useClipsync';

interface JoinRoomDialogProps {
  app: ClipsyncState;
  open: boolean;
  onClose: () => void;
}

function sanitizeRoom(value: string): string {
  return value
    .toUpperCase()
    .replace(/[^A-Z0-9_-]/g, '')
    .slice(0, 24);
}

export function JoinRoomDialog({ app, open, onClose }: JoinRoomDialogProps) {
  const [room, setRoom] = useState(app.room ?? '');
  const [password, setPassword] = useState('');
  const [invite, setInvite] = useState('');
  const [busy, setBusy] = useState(false);

  const submit = async () => {
    const code = sanitizeRoom(room);
    if (!code) return;
    setBusy(true);
    try {
      await app.join(code, password);
      toast.success('已加入房间');
      onClose();
    } catch (error) {
      toast.error(String(error));
    } finally {
      setBusy(false);
    }
  };

  const leave = async () => {
    await app.leave();
    toast.success('已退出房间，本地历史保留');
    onClose();
  };

  const random = async () => {
    const code = await app.randomRoom();
    setRoom(code);
  };

  const applyInvite = async () => {
    const parsed = await app.parseInvite(invite);
    if (!parsed) {
      toast.error('这串邀请链接读不出来');
      return;
    }
    setRoom(parsed.room);
    if (parsed.password) setPassword(parsed.password);
    setInvite('');
    toast.success(`已填入房间 ${parsed.room}`);
  };

  return (
    <Dialog
      open={open}
      onOpenChange={(next) => !next && onClose()}
      size="md"
      closable
      as="form"
      title={app.room ? '房间' : '加入房间'}
      description="同一房间码的设备会自动发现并同步"
      onSubmit={(event) => {
        event.preventDefault();
        void submit();
      }}
      actions={
        <>
          {app.room && (
            <Button variant="ghost" onClick={() => void leave()}>退出房间</Button>
          )}
          <Button variant="ghost" onClick={onClose}>取消</Button>
          <Button variant="accent" type="submit" disabled={busy || !room.trim()}>
            {app.room ? '切换房间' : '加入'}
          </Button>
        </>
      }
    >
      <div className="form">
        <Field label="邀请链接" hint="粘贴 oink://room/xxxx?key=… 会自动填好房间和密码">
          <div className="room-code-row">
            <Input
              value={invite}
              onChange={setInvite}
              placeholder="oink://room/7KQ29WXM?key=…"
            />
            <Button variant="soft" disabled={!invite.trim()} onClick={() => void applyInvite()}>
              填入
            </Button>
          </div>
        </Field>

        <Field label="房间码" hint="字母、数字、- 和 _，最多 24 位">
          <div className="room-code-row">
            <Input
              value={room}
              onChange={(value) => setRoom(sanitizeRoom(value))}
              placeholder="例如 7KQ29WXM"
              className="room-code-input"
            />
            <Button variant="soft" onClick={() => void random()}>随机</Button>
          </div>
        </Field>

        <Field label="房间密码" hint="留空表示不设密码，两端都要一致才能互通">
          <Input
            type="password"
            value={password}
            onChange={setPassword}
            placeholder="可留空"
          />
        </Field>

        <Text variant="micro" tone="subtle">
          在其他设备上输入同一个房间码即可互联；随机生成适合新开一间。
        </Text>
      </div>
    </Dialog>
  );
}
