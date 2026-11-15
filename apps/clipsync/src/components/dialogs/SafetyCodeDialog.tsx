import { Button, Dialog, Text } from 'oink-ui';
import type { ClipsyncState } from '../../hooks/useClipsync';

interface SafetyCodeDialogProps {
  app: ClipsyncState;
  open: boolean;
  onClose: () => void;
}

export function SafetyCodeDialog({ app, open, onClose }: SafetyCodeDialogProps) {
  const safety =
    typeof app.status === 'object' && app.status !== null && 'Online' in app.status
      ? (app.status as { Online: { safety: string } }).Online.safety
      : '';

  return (
    <Dialog
      open={open}
      onOpenChange={(next) => !next && onClose()}
      size="md"
      closable
      title="端到端加密"
      description="安全码是房间密钥的指纹，两端对上才说明内容没被换过"
      actions={
        <Button variant="accent" onClick={onClose}>知道了</Button>
      }
    >
      <div className="safety-card safety-card--dialog">
        <Text variant="micro" tone="subtle">安全码</Text>
        <Text as="p" className="safety-code">{safety || '还没连上房间'}</Text>
        <ol className="safety-notes">
          <li>把两台设备上的安全码念一遍，一字不差才算对上。</li>
          <li>只要有一位对不上，就说明两台设备不在同一个房间，或者密码不一样。</li>
          <li>对不上时别急着传文件，先确认房间码和密码。</li>
        </ol>
        {app.peers.length > 0 && (
          <div className="safety-peers">
            {app.peers.map((peer) => {
              const mismatch = app.keyMismatch.includes(peer.id) || app.keyMismatch.includes(peer.name);
              return (
                <div className="safety-peer" key={peer.id}>
                  <span className={`status-dot${mismatch ? ' status-dot--bad' : ' status-dot--on'}`} />
                  <Text variant="caption">{peer.name}</Text>
                  <Text variant="micro" tone="subtle">{mismatch ? '解不开' : '一致'}</Text>
                </div>
              );
            })}
          </div>
        )}
      </div>
    </Dialog>
  );
}
