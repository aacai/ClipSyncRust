import { useEffect, useState } from 'react';
import { Button, Group, Hotkey, Icon, Row, Slider, Surface, Switch, Text, toast, useOink, type Accent } from 'oink-ui';
import { ClipIcon } from '../components/ClipIcon';
import { open as openDialog } from '@tauri-apps/plugin-dialog';
import { isTauri } from '../lib/api';
import { ChoiceDialog } from '../components/dialogs/ChoiceDialog';
import type { ClipsyncState, MicInput, UpdateStatus } from '../hooks/useClipsync';

const swatches: Array<{ name: Accent; label: string; color: string }> = [
  { name: 'pink', label: '粉', color: '#e4779c' },
  { name: 'orange', label: '橙', color: '#e08b4c' },
  { name: 'amber', label: '琥珀', color: '#d9a532' },
  { name: 'blue', label: '蓝', color: '#5a8fd6' },
  { name: 'violet', label: '紫', color: '#8b6fd4' },
];

type ChoiceKey = 'theme' | 'language' | 'retention' | 'region' | 'edge' | 'fps';

interface SettingsPageProps {
  app: ClipsyncState;
  onOpenAbout: () => void;
  onBack: () => void;
}

export function SettingsPage({ app, onOpenAbout, onBack }: SettingsPageProps) {
  const { theme, setTheme, accent, setAccent, motion, setMotion } = useOink();
  const prefs = app.prefs;
  const [update, setUpdate] = useState<UpdateStatus | null>(null);
  const [checking, setChecking] = useState(false);
  const [memory, setMemory] = useState(0);
  const [alwaysOnTop, setAlwaysOnTop] = useState(false);
  const [clickThrough, setClickThrough] = useState(false);
  const [mics, setMics] = useState<MicInput[]>([]);
  const [choice, setChoice] = useState<ChoiceKey | null>(null);

  useEffect(() => {
    void app.memoryUsage().then(setMemory).catch(() => setMemory(0));
    void app.recordInputs().then(setMics).catch(() => setMics([]));
  }, [app]);

  if (!prefs) {
    return (
      <div className="page">
        <Text variant="caption" tone="subtle">正在载入…</Text>
      </div>
    );
  }


  const choiceTitle = () => {
    switch (choice) {
      case 'theme':
        return '主题';
      case 'language':
        return '语言';
      case 'retention':
        return '保留时长';
      case 'region':
        return '录制范围';
      case 'edge':
        return '画面大小';
      case 'fps':
        return '帧率';
      default:
        return '';
    }
  };

  const choiceOptions = (): Record<ChoiceKey, Array<{ value: string; label: string; description?: string }>> => ({
    theme: [
      { value: 'light', label: '浅色', description: '白底黑字，白天看着清楚' },
      { value: 'dark', label: '深色', description: '黑底浅字，晚上不刺眼' },
      { value: 'glass', label: '毛玻璃', description: '半透明面板，背景透出来' },
      { value: 'system', label: '跟随系统', description: '系统换主题，这里跟着换' },
    ],
    language: [
      { value: 'auto', label: '跟随系统' },
      { value: 'zh', label: '简体中文' },
      { value: 'en', label: 'English' },
    ],
    retention: [
      { value: '7', label: '7 天' },
      { value: '30', label: '30 天' },
      { value: '90', label: '90 天' },
      { value: '365', label: '1 年' },
    ],
    region: [
      { value: 'region', label: '区域', description: '只录框出来的那一块' },
      { value: 'screen', label: '整屏', description: '录整块屏幕' },
    ],
    edge: [
      { value: '0', label: '原始', description: '保持屏幕本身的分辨率' },
      { value: '1920', label: '1080p', description: '长边 1920' },
      { value: '1280', label: '720p', description: '长边 1280' },
    ],
    fps: [
      { value: '60', label: '60 帧', description: '更流畅，也更吃带宽' },
      { value: '30', label: '30 帧', description: '日常够用' },
      { value: '15', label: '15 帧', description: '最省空间' },
    ],
  });

  const choiceValue = () => {
    switch (choice) {
      case 'theme':
        return prefs?.theme === 'Glass' ? 'glass' : theme === 'dark' ? 'dark' : theme === 'light' ? 'light' : 'system';
      case 'language':
        return prefs?.language === 'Chinese' ? 'zh' : prefs?.language === 'English' ? 'en' : 'auto';
      case 'retention':
        return String(prefs?.retentionDays ?? 30);
      case 'region':
        return prefs?.recordRegion ? 'region' : 'screen';
      case 'edge':
        return String(prefs?.recordLongEdge ?? 0);
      case 'fps':
        return String(prefs?.recordFps ?? 30);
      default:
        return '';
    }
  };

  const themeLabel = () => {
    if (prefs?.theme === 'Glass') return '毛玻璃';
    if (theme === 'dark') return '深色';
    if (theme === 'light') return '浅色';
    return '跟随系统';
  };

  const languageLabel = () => {
    if (prefs?.language === 'Chinese') return '简体中文';
    if (prefs?.language === 'English') return 'English';
    return '跟随系统';
  };

  const retentionLabel = () => {
    const days = prefs?.retentionDays ?? 30;
    if (days === 365) return '1 年';
    return `${days} 天`;
  };

  const edgeLabel = () => {
    const edge = prefs?.recordLongEdge ?? 0;
    if (edge === 0) return '原始';
    if (edge >= 1920) return '1080p';
    return '720p';
  };

  const flag = async (key: keyof typeof prefs, value: boolean) => {
    await app.setPref(key, value as never);
  };

  const checkUpdate = async () => {
    setChecking(true);
    try {
      const status = await app.checkUpdate();
      setUpdate(status);
      if (!status.behind) toast.success(`已经是最新版本（${status.latest}）`);
      else if (!status.assetUrl) toast('有新版本，但这台机器没有对应的安装包');
      else toast(`发现新版本 ${status.latest}`);
    } catch (error) {
      toast.error(String(error));
    } finally {
      setChecking(false);
    }
  };

  const downloadUpdate = async () => {
    if (!update?.assetUrl || !update.assetName) return;
    setChecking(true);
    try {
      const saved = await app.downloadUpdate(update.assetUrl, update.assetName);
      toast.success(`安装包已下载：${saved}`);
    } catch (error) {
      toast.error(String(error));
    } finally {
      setChecking(false);
    }
  };

  const pickBackground = async () => {
    try {
      const picked = await openDialog({
        multiple: false,
        filters: [{ name: '图片', extensions: ['png', 'jpg', 'jpeg', 'gif', 'webp', 'bmp'] }],
      });
      if (!picked || Array.isArray(picked)) return;
      await app.setBackground(picked);
      toast.success('背景图已换好');
    } catch (error) {
      toast.error(String(error));
    }
  };

  return (
    <div className="page">
      <header className="page__header">
        <div className="settings__heading">
          <Button variant="ghost" size="sm" leading={<ClipIcon name="arrow-back" />} onClick={onBack}>
            返回
          </Button>
          <div>
            <Text as="h1" variant="h2">设置</Text>
            <Text variant="caption" tone="subtle">外观、同步与剪贴板行为</Text>
          </div>
        </div>
      </header>

      <div className="settings">
        <section>
          <Text as="h2" variant="h3" className="settings__title">外观</Text>
          <Surface size="sm">
            <Group>
              <Row label="主题" description="跟随系统会随系统外观自动切换" stack="auto">
                <button type="button" className="choice-trigger" onClick={() => setChoice('theme')}>
                  {themeLabel()}
                </button>
              </Row>
              <Row label="强调色" description="用于按钮、选中态与重点提示" stack="auto">
                <div className="swatches">
                  {swatches.map((swatch) => (
                    <button
                      key={swatch.name}
                      type="button"
                      className="swatch"
                      style={{ background: swatch.color }}
                      data-active={accent === swatch.name}
                      aria-label={swatch.label}
                      onClick={() => setAccent(swatch.name)}
                    />
                  ))}
                </div>
              </Row>
              <Row label="减少动效" description="关闭界面过渡与揭开动画">
                <Switch checked={motion === 'reduced'} onChange={(value) => setMotion(value ? 'reduced' : 'auto')} aria-label="减少动效" />
              </Row>
              <Row label="语言" description="跟随系统按系统语言决定" stack="auto">
                <button type="button" className="choice-trigger" onClick={() => setChoice('language')}>
                  {languageLabel()}
                </button>
              </Row>
              <Row
                label="背景图"
                description={prefs.backgroundPath ? '已设置' : '未设置'}
                stack="auto"
              >
                <div className="row-actions">
                  <Button variant="soft" leading={<Icon name="image" />} onClick={() => void pickBackground()}>
                    选择
                  </Button>
                  {prefs.backgroundPath && (
                    <Button
                      variant="ghost"
                      onClick={() => {
                        void app.clearBackground().then(() => toast.success('已移除背景图'));
                      }}
                    >
                      移除
                    </Button>
                  )}
                </div>
              </Row>
              {prefs.backgroundPath && (
                <Row label="压暗" description="背景太亮时压一压" stack="auto">
                  <Slider
                    value={prefs.backgroundDim}
                    min={0}
                    max={0.9}
                    step={0.05}
                    digits={2}
                    format={(value) => `${Math.round(value * 100)}%`}
                    onChangeEnd={(value) => void app.setPref('backgroundDim', value)}
                  />
                </Row>
              )}
            </Group>
          </Surface>
        </section>

        <section>
          <Text as="h2" variant="h3" className="settings__title">剪贴板</Text>
          <Surface size="sm">
            <Group>
              <Row label="记录剪贴板" description="捕获其他设备复制的内容">
                <Switch checked={prefs.watchClipboard} onChange={(value) => void flag('watchClipboard', value)} aria-label="记录剪贴板" />
              </Row>
              <Row label="记录截图" description="同步复制的图片">
                <Switch checked={prefs.watchImages} onChange={(value) => void flag('watchImages', value)} aria-label="记录截图" />
              </Row>
              <Row label="记录文件" description="同步复制的文件，单个最大 50 MB">
                <Switch checked={prefs.watchFiles} onChange={(value) => void flag('watchFiles', value)} aria-label="记录文件" />
              </Row>
              <Row label="收到即进剪贴板" description="收到文本自动写入本机剪贴板">
                <Switch checked={prefs.autoCopy} onChange={(value) => void flag('autoCopy', value)} aria-label="收到即进剪贴板" />
              </Row>
              <Row label="收到文件即进剪贴板" description="文件下载后直接放进剪贴板">
                <Switch checked={prefs.autoPasteFile} onChange={(value) => void flag('autoPasteFile', value)} aria-label="收到文件即进剪贴板" />
              </Row>
            </Group>
          </Surface>
        </section>

        <section>
          <Text as="h2" variant="h3" className="settings__title">同步</Text>
          <Surface size="sm">
            <Group>
              <Row label="房间" description={app.room ?? '未加入房间'}>
                <Text variant="caption" tone="subtle">{app.room ?? '未加入'}</Text>
              </Row>
              <Row label="记住房间" description="下次启动自动回到上次的房间">
                <Switch checked={prefs.rememberRoom} onChange={(value) => void flag('rememberRoom', value)} aria-label="记住房间" />
              </Row>
              <Row label="开机自动启动" description="登录系统后自动运行">
                <Switch
                  checked={prefs.launchAtLogin}
                  onChange={(value) => {
                    void app.setAutostart(value).catch((error) => {
                      toast.error(String(error));
                    });
                  }}
                  aria-label="开机自动启动"
                />
              </Row>
              <Row label="删除过期条目" description="按保留时长自动清理">
                <Switch checked={prefs.deleteExpired} onChange={(value) => void flag('deleteExpired', value)} aria-label="删除过期条目" />
              </Row>
              <Row label="保留时长" description="超过期限的记录会被清理" stack="auto">
                <button type="button" className="choice-trigger" onClick={() => setChoice('retention')}>
                  {retentionLabel()}
                </button>
              </Row>
              <Row label="昵称" description="本机在房间里显示的名字" stack="auto">
                <input
                  className="text-input"
                  value={prefs.nickname}
                  maxLength={24}
                  placeholder="本机用户名"
                  onChange={(event) => void app.setPref('nickname', event.target.value)}
                  onKeyDown={(event) => {
                    if ((event.metaKey || event.ctrlKey) && event.key === 'Enter') {
                      event.preventDefault();
                      event.currentTarget.blur();
                      toast.success('昵称已保存');
                    }
                  }}
                />
              </Row>
            </Group>
          </Surface>
        </section>

        <section>
          <Text as="h2" variant="h3" className="settings__title">窗口</Text>
          <Surface size="sm">
            <Group>
              <Row label="窗口置顶" description="让窗口压在其它窗口上面">
                <Switch
                  checked={alwaysOnTop}
                  onChange={(value) => {
                    setAlwaysOnTop(value);
                    void app.alwaysOnTop(value).catch((error) => toast.error(String(error)));
                  }}
                  aria-label="窗口置顶"
                />
              </Row>
              <Row label="鼠标穿透" description="窗口不吃鼠标点击，键盘仍然可用">
                <Switch
                  checked={clickThrough}
                  onChange={(value) => {
                    setClickThrough(value);
                    void app.clickThrough(value).catch((error) => toast.error(String(error)));
                  }}
                  aria-label="鼠标穿透"
                />
              </Row>
            </Group>
          </Surface>
        </section>

        <section>
          <Text as="h2" variant="h3" className="settings__title">录屏与截屏</Text>
          <Surface size="sm">
            <Group>
              <Row label="截屏快捷键" description="点一下录制新的组合键" stack="auto">
                <HotkeyField
                  value={prefs.screenshotHotkey}
                  onSave={(value) => void app.setPref('screenshotHotkey', value)}
                />
              </Row>
              <Row label="截图时保留本窗口" description="截屏时不把这扇窗也拍进去">
                <Switch checked={prefs.screenshotKeepWindow} onChange={(value) => void flag('screenshotKeepWindow', value)} aria-label="截图时保留本窗口" />
              </Row>
              <Row label="录屏快捷键" description="点一下录制新的组合键" stack="auto">
                <HotkeyField
                  value={prefs.recordHotkey}
                  onSave={(value) => void app.setPref('recordHotkey', value)}
                />
              </Row>
              <Row label="录制范围" description="框选一块区域，或者录整屏" stack="auto">
                <button type="button" className="choice-trigger" onClick={() => setChoice('region')}>
                  {prefs.recordRegion ? '区域' : '整屏'}
                </button>
              </Row>
              <Row label="画面大小" description="录屏分辨率的长边" stack="auto">
                <button type="button" className="choice-trigger" onClick={() => setChoice('edge')}>
                  {edgeLabel()}
                </button>
              </Row>
              <Row label="帧率" description="每秒录多少帧" stack="auto">
                <button type="button" className="choice-trigger" onClick={() => setChoice('fps')}>
                  {prefs.recordFps} 帧
                </button>
              </Row>
              <Row label="录麦克风" description="录制时收进麦克风声音">
                <Switch checked={prefs.recordMic} onChange={(value) => void flag('recordMic', value)} aria-label="录麦克风" />
              </Row>
              <Row label="麦克风设备" description="留空表示用系统默认输入" stack="auto">
                <select
                  className="text-input"
                  value={prefs.recordMicDevice}
                  onChange={(event) => void app.setPref('recordMicDevice', event.target.value)}
                >
                  <option value="">系统默认</option>
                  {mics.map((mic) => (
                    <option key={mic.id} value={mic.id}>{mic.name}</option>
                  ))}
                </select>
              </Row>
              <Row label="录系统声音" description="录制时收进电脑的声音">
                <Switch checked={prefs.recordSystemAudio} onChange={(value) => void flag('recordSystemAudio', value)} aria-label="录系统声音" />
              </Row>
            </Group>
          </Surface>
        </section>

        <section>
          <Text as="h2" variant="h3" className="settings__title">关于</Text>
          <Surface size="sm">
            <Group>
              <Row label="版本" description={isTauri() ? 'ClipSync Tauri' : '浏览器预览'}>
                <Text variant="caption" tone="subtle">0.0.10</Text>
              </Row>
              <Row
                label="检查更新"
                description={
                  update
                    ? update.behind
                      ? `发现新版本 ${update.latest}`
                      : `已是最新（${update.latest}）`
                    : '从 GitHub Releases 查最新版本'
                }
                stack="auto"
              >
                <div className="row-actions">
                  {update?.behind && update.assetUrl && (
                    <Button variant="soft" leading={<ClipIcon name="download" />} onClick={() => void downloadUpdate()}>
                      下载
                    </Button>
                  )}
                  <Button variant="soft" leading={<ClipIcon name="refresh" />} disabled={checking} onClick={() => void checkUpdate()}>
                    {checking ? '检查中…' : '检查'}
                  </Button>
                </div>
              </Row>
              <Row label="本地记录" description="保存在本机的剪贴板条数">
                <Text variant="caption" tone="subtle">{app.items.length}</Text>
              </Row>
              <Row label="所属系统" description="房间里的其它设备会看到它">
                <Text variant="caption" tone="subtle">{systemName()}</Text>
              </Row>
              <Row label="内存占用" description="本进程当前占用的物理内存">
                <Text variant="caption" tone="subtle">{memory ? formatMemory(memory) : '—'}</Text>
              </Row>
              <Row label="关于" description="版本、字体授权与说明" stack="auto">
                <Button variant="soft" onClick={onOpenAbout}>打开关于</Button>
              </Row>
              <Row label="退出" description="彻底关掉 ClipSync" stack="auto">
                <Button
                  variant="ghost"
                  onClick={() => {
                    void app.quit();
                  }}
                >
                  退出
                </Button>
              </Row>
            </Group>
          </Surface>
        </section>
      </div>

      <ChoiceDialog
        open={choice !== null}
        title={choiceTitle()}
        options={(choiceOptions()[choice ?? 'theme'] ?? []) as never}
        value={choiceValue() as never}
        onSelect={(value) => {
          switch (choice) {
            case 'theme':
              if (value === 'glass') {
                setTheme('dark');
                void app.setPref('theme', 'Glass');
              } else {
                setTheme(value as 'light' | 'dark' | 'system');
                void app.setPref('theme', value === 'dark' ? 'Dark' : value === 'light' ? 'Light' : 'System');
              }
              break;
            case 'language':
              void app.setPref('language', value === 'zh' ? 'Chinese' : value === 'en' ? 'English' : 'System');
              break;
            case 'retention':
              void app.setPref('retentionDays', Number(value));
              break;
            case 'region':
              void app.setPref('recordRegion', value === 'region');
              break;
            case 'edge':
              void app.setPref('recordLongEdge', Number(value));
              break;
            case 'fps':
              void app.setPref('recordFps', Number(value));
              break;
            default:
              break;
          }
        }}
        onClose={() => setChoice(null)}
      />
    </div>
  );
}

function HotkeyField({ value, onSave }: { value: string; onSave: (value: string) => void }) {
  const [current, setCurrent] = useState(value);

  useEffect(() => {
    setCurrent(value);
  }, [value]);

  return (
    <div className="row-actions">
      <Hotkey
        value={current}
        onChange={(next) => {
          setCurrent(next);
          onSave(next);
        }}
      />
    </div>
  );
}

function formatMemory(bytes: number): string {
  const mb = bytes / 1024 / 1024;
  if (mb < 1024) return `${mb.toFixed(1)} MB`;
  return `${(mb / 1024).toFixed(2)} GB`;
}

function systemName(): string {
  const agent = navigator.userAgent;
  if (agent.includes('Mac')) return 'macOS';
  if (agent.includes('Windows')) return 'Windows';
  if (agent.includes('Linux')) return 'Linux';
  return '未知';
}
