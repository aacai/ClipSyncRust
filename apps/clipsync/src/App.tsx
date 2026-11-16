import { useCallback, useEffect, useRef, useState } from 'react';
import { AppShell, Group, Icon, Nav, NavGroup, NavItem, Row, Surface, Text, toast, useAppShell, useOink } from 'oink-ui';
import { listen } from '@tauri-apps/api/event';
import { ClipIcon } from './components/ClipIcon';
import { TitleBar } from './components/TitleBar';
import { useClipsync } from './hooks/useClipsync';
import { useFileDrop } from './hooks/useFileDrop';
import { call, isTauri } from './lib/api';
import { ClipsPage } from './pages/ClipsPage';
import { JoinRoomDialog } from './components/JoinRoomDialog';
import { NewEntryDialog } from './components/dialogs/NewEntryDialog';
import { AboutDialog } from './components/dialogs/AboutDialog';
import { LicenseDialog } from './components/dialogs/LicenseDialog';
import { SafetyCodeDialog } from './components/dialogs/SafetyCodeDialog';
import { DevicesPage } from './pages/DevicesPage';
import { SettingsPage } from './pages/SettingsPage';
import { dropsClaimed } from './lib/dropBus';
import type { Screen } from './lib/types';

function toHex(value: string): string | null {
  if (!value) return null;
  const hex = value.trim();
  if (/^#[0-9a-f]{6}$/i.test(hex)) return hex;
  const rgb = hex.match(/^rgba?\(\s*(\d+)[, ]\s*(\d+)[, ]\s*(\d+)/i);
  if (!rgb) return null;
  const part = (raw: string) => Number(raw).toString(16).padStart(2, '0');
  return `#${part(rgb[1])}${part(rgb[2])}${part(rgb[3])}`;
}

export function App() {
  const [screen, setScreen] = useState<Screen>('clips');
  const { theme, setTheme, resolvedTheme } = useOink();
  const [roomDialogOpen, setRoomDialogOpen] = useState(false);
  const [newEntryOpen, setNewEntryOpen] = useState(false);
  const [aboutOpen, setAboutOpen] = useState(false);
  const [licenseOpen, setLicenseOpen] = useState(false);
  const [safetyOpen, setSafetyOpen] = useState(false);
  const [memory, setMemory] = useState(0);
  const searchRef = useRef<HTMLInputElement | null>(null);
  const app = useClipsync();

  useEffect(() => {
    if (!isTauri()) return;
    const unlisten = listen('clipsync://about', () => setAboutOpen(true));
    return () => {
      void unlisten.then((stop) => stop());
    };
  }, []);

  useEffect(() => {
    if (!isTauri()) return;
    const unlisten = listen<string>('clipsync://notify', (event) => {
      toast(String(event.payload));
    });
    return () => {
      void unlisten.then((stop) => stop());
    };
  }, []);

  useEffect(() => {
    if (!isTauri()) return;
    const timer = window.setInterval(() => {
      void app.memoryUsage().then(setMemory).catch(() => {});
    }, 5000);
    void app.memoryUsage().then(setMemory).catch(() => {});
    return () => window.clearInterval(timer);
  }, [app]);

  useEffect(() => {
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key !== 'Escape' || screen !== 'settings') return;
      const target = event.target as HTMLElement | null;
      if (target && (target.tagName === 'INPUT' || target.tagName === 'TEXTAREA' || target.isContentEditable)) {
        return;
      }
      setScreen('clips');
    };
    window.addEventListener('keydown', onKeyDown);
    return () => window.removeEventListener('keydown', onKeyDown);
  }, [screen]);

  useEffect(() => {
    if (!app.ready) return;
    const wanted =
      app.themePref === 'System'
        ? 'system'
        : app.themePref === 'Dark' || app.themePref === 'Glass'
          ? 'dark'
          : 'light';
    if (theme !== wanted) {
      setTheme(wanted);
    }
  }, [app.ready, app.themePref, theme, setTheme]);

  useEffect(() => {
    document.documentElement.classList.toggle('theme-glass', app.themePref === 'Glass');
  }, [app.themePref]);

  useEffect(() => {
    if (!app.ready || !isTauri()) return;
    const raw = getComputedStyle(document.documentElement)
      .getPropertyValue('--ok-bg')
      .trim();
    const color = toHex(raw);
    if (!color) return;
    void call('window_tint', { color, dark: resolvedTheme === 'dark' }).catch(() => {});
  }, [app.ready, resolvedTheme]);

  useEffect(() => {
    if (!app.ready) return;
    let cancelled = false;
    void app
      .backgroundDataUrl()
      .then((dataUrl) => {
        if (cancelled) return;
        const root = document.documentElement;
        root.style.setProperty('--app-background', dataUrl ? `url("${dataUrl}")` : 'none');
        root.style.setProperty('--app-background-dim', String(app.prefs?.backgroundDim ?? 0));
      })
      .catch(() => {});
    return () => {
      cancelled = true;
    };
  }, [app.ready, app.prefs?.backgroundPath, app.prefs?.backgroundDim]);

  const onDrop = useCallback(
    (paths: string[]) => {
      if (dropsClaimed()) return;
      void call<Array<{ path: string; name: string; size: number; mime: string; tooLarge: boolean }>>(
        'inspect_dropped_paths',
        { paths },
      )
        .then(async (files) => {
          if (files.length === 0) return;
          const tooLarge = files.filter((file) => file.tooLarge).length;
          const usable = files.filter((file) => !file.tooLarge);
          if (tooLarge > 0) {
            toast.error(`${tooLarge} 个文件超过大小上限`);
          }
          if (usable.length === 0) return;
          if (!app.room) {
            toast.error('先加入房间，才能把文件发出去');
            return;
          }
          for (const file of usable) {
            try {
              await app.uploadFile(file.path);
            } catch (error) {
              toast.error(`${file.name} 上传失败：${error}`);
              break;
            }
          }
          toast.success(`已发送 ${usable.length} 个文件`);
        })
        .catch(() => toast.error('读取拖入文件失败'));
    },
    [app],
  );

  const hovering = useFileDrop(onDrop);

  return (
    <AppShell contained collapsed="auto" collapseBelow="md" className="app-shell">
      <AppShell.Header className="title-bar-host">
        <TitleBar status={app.status} room={app.room} peers={app.peers.length} memory={memory} />
      </AppShell.Header>

      <AppShell.Sidebar className="sidebar" width={224} drawerWidth={300}>
        <SidebarContent
          screen={screen}
          onScreen={setScreen}
          status={app.status}
          room={app.room}
          itemCount={app.items.length}
          peerCount={app.peers.length}
          onRoom={() => setRoomDialogOpen(true)}
          onSafety={() => (app.room ? setSafetyOpen(true) : setRoomDialogOpen(true))}
        />
      </AppShell.Sidebar>

      <AppShell.Main className="main">
        {screen === 'clips' ? (
          <ClipsPage
            app={app}
            onOpenRoomDialog={() => setRoomDialogOpen(true)}
            onNewEntry={() => setNewEntryOpen(true)}
            searchRef={searchRef}
          />
        ) : screen === 'devices' ? (
          <DevicesPage app={app} onOpenRoomDialog={() => setRoomDialogOpen(true)} />
        ) : (
          <SettingsPage app={app} onOpenAbout={() => setAboutOpen(true)} onBack={() => setScreen('clips')} />
        )}
      </AppShell.Main>

      {hovering && (
        <div className="drop-overlay">
          <div className="drop-overlay__card">
            <ClipIcon name="clipboard" size={32} />
            <Text variant="label">松开以导入文件</Text>
            <Text variant="caption" tone="subtle">文件会出现在剪贴板列表顶部</Text>
          </div>
        </div>
      )}

      <JoinRoomDialog app={app} open={roomDialogOpen} onClose={() => setRoomDialogOpen(false)} />
      <NewEntryDialog open={newEntryOpen} onClose={() => setNewEntryOpen(false)} onSubmit={(text) => { void call('engine_publish_text', { text }); }} />
      <AboutDialog
        open={aboutOpen}
        onClose={() => setAboutOpen(false)}
        onOpenLicense={() => {
          setAboutOpen(false);
          setLicenseOpen(true);
        }}
      />
      <LicenseDialog open={licenseOpen} onClose={() => setLicenseOpen(false)} />
      <SafetyCodeDialog app={app} open={safetyOpen} onClose={() => setSafetyOpen(false)} />

      {app.transferProgress && (
        <div className="transfer-bar">
          <span className="transfer-bar__label">
            {app.transferProgress.upload ? '上传' : '下载'} {app.transferProgress.name}
          </span>
          <span className="transfer-bar__track">
            <span
              className="transfer-bar__fill"
              style={{
                width: `${app.transferProgress.total > 0 ? Math.min(100, (app.transferProgress.done / app.transferProgress.total) * 100) : 0}%`,
              }}
            />
          </span>
        </div>
      )}

      {isTauri() && app.keyMismatch.length > 0 && (
        <div className="banner banner--error">
          同屋的设备发来的内容本机解不开：多半是某一端的房间密码打错了
        </div>
      )}

      {app.status && 'Offline' in app.status && (
        <div className="banner banner--warning">
          <span>连接已断开{app.status.Offline.reason ? `：${app.status.Offline.reason}` : ''}</span>
          <button type="button" className="banner__action" onClick={() => void app.retry()}>
            重连
          </button>
        </div>
      )}
    </AppShell>
  );
}

interface SidebarContentProps {
  screen: Screen;
  onScreen: (screen: Screen) => void;
  status: unknown;
  room: string | null;
  itemCount: number;
  peerCount: number;
  onRoom: () => void;
  onSafety: () => void;
}

function SidebarContent({ screen, onScreen, status, room, itemCount, peerCount, onRoom, onSafety }: SidebarContentProps) {
  const { rail, sidebarCollapsed, toggleRail } = useAppShell();

  return (
    <>
      <div className="brand">
        <span className="brand__mark" aria-hidden="true">C</span>
        <div className="brand__text">
          <Text variant="label">ClipSync</Text>
          <Text variant="micro" tone="subtle">剪贴板同步</Text>
        </div>
        {!sidebarCollapsed && (
          <button type="button" className="brand__rail" aria-label="收起侧栏" onClick={toggleRail}>
            <Icon name="chevron-left" size={15} />
          </button>
        )}
      </div>

      <Nav aria-label="主导航" className="nav">
        <NavGroup label="工作区">
          <NavItem
            current={screen === 'clips'}
            icon={<ClipIcon name="clipboard" />}
            badge={String(itemCount)}
            label="剪贴板"
            onClick={() => onScreen('clips')}
          >
            剪贴板
          </NavItem>
          <NavItem
            current={screen === 'devices'}
            icon={<ClipIcon name="devices" />}
            badge={String(peerCount)}
            label="设备"
            onClick={() => onScreen('devices')}
          >
            设备
          </NavItem>
        </NavGroup>
        <NavGroup label="偏好">
          <NavItem
            current={screen === 'settings'}
            icon={<ClipIcon name="settings" />}
            label="设置"
            onClick={() => onScreen('settings')}
          >
            设置
          </NavItem>
        </NavGroup>
      </Nav>

      <div className="sidebar__footer">
        <Surface size="sm">
          <Group compact>
            <Row
              label={statusLabel(status)}
              description={room ?? '点击加入房间'}
              leading={<span className={`status-dot${isOnline(status) ? ' status-dot--on' : ''}`} />}
              onClick={onRoom}
              interactive
            />
            <Row
              label="端到端加密"
              description={room ? '点击查看安全码' : '点击加入房间'}
              onClick={onSafety}
              interactive
            />
            <Row label="本机记录" description={`${itemCount} 条`} />
          </Group>
        </Surface>
        {rail && !sidebarCollapsed && (
          <button type="button" className="brand__rail brand__rail--foot" aria-label="展开侧栏" onClick={toggleRail}>
            <Icon name="chevron-right" size={15} />
          </button>
        )}
      </div>
    </>
  );
}

export function isOnline(status: unknown): boolean {
  return typeof status === 'object' && status !== null && 'Online' in status;
}

export function statusLabel(status: unknown): string {
  if (typeof status !== 'object' || status === null) return '未连接';
  if ('Idle' in status) return '未加入房间';
  if ('Connecting' in status) return '连接中';
  if ('Online' in status) return '房间已连接';
  return '连接已断开';
}
