import { getCurrentWindow } from '@tauri-apps/api/window';
import { AppShell, Button, Icon, Text, useAppShell, useOink } from 'oink-ui';

type WindowAction = 'startDragging' | 'minimize' | 'toggleMaximize' | 'close';

function invokeWindow(action: WindowAction) {
  try {
    const win = getCurrentWindow();
    void (win[action]() as Promise<void>).catch(() => {});
  } catch {}
}

interface TitleBarProps {
  status: unknown;
  room: string | null;
  peers: number;
  memory: number;
}

export function TitleBar({ status, room, peers, memory }: TitleBarProps) {
  const { resolvedTheme, setTheme } = useOink();
  const { sidebarCollapsed } = useAppShell();
  const online = typeof status === 'object' && status !== null && 'Online' in status;
  const isMac = navigator.userAgent.includes('Mac');

  return (
    <div className={`title-bar${isMac ? ' title-bar--mac' : ''}`}>
      <div className="title-bar__drag" onPointerDown={() => invokeWindow('startDragging')}>
        <AppShell.Trigger
          className="title-bar__nav"
          label="切换导航"
          onPointerDown={(event) => event.stopPropagation()}
        >
          <Icon name="menu" size={17} />
        </AppShell.Trigger>
        {!isMac && <span className="title-bar__mark" aria-hidden="true">C</span>}
        {!isMac && (
          <div className="title-bar__state">
            <span className={`status-dot${online ? ' status-dot--on' : ''}`} />
            <Text variant="caption" tone="subtle" title={memory ? `内存占用 ${formatMemory(memory)}` : undefined}>
              {room ? `房间 ${room}` : '未加入房间'} · {peers} 台设备
            </Text>
          </div>
        )}
      </div>
      {isMac && (
        <div className="title-bar__title" onPointerDown={() => invokeWindow('startDragging')}>
          <Text variant="label">ClipSync</Text>
        </div>
      )}
      <div className="title-bar__actions">
        {isMac && (
          <span
            className="title-bar__state title-bar__state--inline"
            title={memory ? `内存占用 ${formatMemory(memory)}` + ` · ${peers} 台设备` : `${peers} 台设备`}
          >
            <span className={`status-dot${online ? ' status-dot--on' : ''}`} />
          </span>
        )}
        <Button
          iconOnly
          aria-label="切换主题"
          variant="ghost"
          size="sm"
          leading={<Icon name={resolvedTheme === 'dark' ? 'sun' : 'moon'} />}
          onClick={() => setTheme(resolvedTheme === 'dark' ? 'light' : 'dark')}
        />
        {!sidebarCollapsed && <span className="title-bar__sep" />}
        {!isMac && (
          <>
            <Button iconOnly aria-label="最小化" variant="ghost" size="sm" leading={<Icon name="minus" />} onClick={() => invokeWindow('minimize')} />
            <Button iconOnly aria-label="最大化" variant="ghost" size="sm" leading={<Icon name="grid" />} onClick={() => invokeWindow('toggleMaximize')} />
            <Button iconOnly aria-label="关闭" variant="ghost" size="sm" leading={<Icon name="close" />} onClick={() => invokeWindow('close')} />
          </>
        )}
      </div>
    </div>
  );
}

function formatMemory(bytes: number): string {
  const mb = bytes / 1024 / 1024;
  if (mb < 1024) return `${mb.toFixed(1)} MB`;
  return `${(mb / 1024).toFixed(2)} GB`;
}
