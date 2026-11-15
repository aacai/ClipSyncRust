import { Button, Card, Text } from 'oink-ui';
import { ClipIcon } from '../components/ClipIcon';
import type { ClipsyncState } from '../hooks/useClipsync';

interface DevicesPageProps {
  app: ClipsyncState;
  onOpenRoomDialog: () => void;
}

export function DevicesPage({ app, onOpenRoomDialog }: DevicesPageProps) {
  const peers = app.peers;
  const safety = typeof app.status === 'object' && app.status !== null && 'Online' in app.status
    ? (app.status as { Online: { safety: string } }).Online.safety
    : '';

  return (
    <div className="page">
      <header className="page__header">
        <div>
          <Text as="h1" variant="h2">设备</Text>
          <Text variant="caption" tone="subtle">同一房间内的设备会自动发现并同步</Text>
        </div>
        <Button variant="accent" leading={<ClipIcon name="plus" />} onClick={onOpenRoomDialog}>
          {app.room ? '切换房间' : '加入房间'}
        </Button>
      </header>

      {safety && (
        <Card size="sm" className="safety-card">
          <Text variant="micro" tone="subtle">安全码</Text>
          <Text as="p" className="safety-code">{safety}</Text>
          <Text variant="caption" tone="subtle">
            安全码是房间密钥的指纹。在同一房间的设备上核对一致，才能确认内容没被中间人替换。
          </Text>
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
        </Card>
      )}

      <Card size="sm" className="device-card">
        {peers.length === 0 ? (
          <div className="device-row">
            <Text variant="caption" tone="subtle">还没有同屋设备。加入房间后，这里会列出它们。</Text>
          </div>
        ) : (
          peers.map((peer) => (
            <div className="device-row" key={peer.id}>
              <div className="device-row__icon"><ClipIcon name="devices" /></div>
              <div className="device-row__info">
                <Text variant="label">{peer.name}</Text>
                <Text variant="caption" tone="subtle">{peer.os}</Text>
              </div>
              <div className="device-row__status">
                <span className="status-dot status-dot--on" />
                <Text variant="caption" tone="subtle">在线</Text>
              </div>
            </div>
          ))
        )}
      </Card>

      {app.room && (
        <Button variant="ghost" onClick={onOpenRoomDialog}>房间设置与退出</Button>
      )}
    </div>
  );
}
