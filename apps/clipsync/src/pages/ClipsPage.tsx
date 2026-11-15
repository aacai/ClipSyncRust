import { useCallback, useEffect, useMemo, useRef, useState, type PointerEvent } from 'react';
import { Button, Card, EmptyState, Icon, SearchInput, Segmented, Text, toast } from 'oink-ui';
import { call, isTauri } from '../lib/api';
import type { ClipRecord } from '../lib/types';
import type { RecordStatus } from '../hooks/useClipsync';
import type { ClipsyncState } from '../hooks/useClipsync';
import { ItemMenu, type ItemMenuActions } from '../components/ItemMenu';
import { EditDialog } from '../components/dialogs/EditDialog';
import { NoteDialog } from '../components/dialogs/NoteDialog';
import { DetailDialog } from '../components/dialogs/DetailDialog';
import { TransformDialog } from '../components/dialogs/TransformDialog';
import { useShortcuts } from '../hooks/useShortcuts';
import { TranslateDialog } from '../components/dialogs/TranslateDialog';
import { QrCodeDialog } from '../components/dialogs/QrCodeDialog';
import { ImagePreviewDialog } from '../components/dialogs/ImagePreviewDialog';
import { ClipIcon } from '../components/ClipIcon';
import { Thumb } from '../components/Thumb';
import { MoveToDialog } from '../components/dialogs/MoveToDialog';
import { SendFileDialog } from '../components/dialogs/SendFileDialog';
import { ScreenPickDialog } from '../components/dialogs/ScreenPickDialog';
import { DeleteHistoryDialog } from '../components/dialogs/DeleteHistoryDialog';

type Filter = 'all' | 'text' | 'file';
type SortMode = 'time' | 'pinned';
type Scope = 'all' | 'pinned' | 'today' | 'week';
type DeviceScope = 'all' | string;

interface ClipsPageProps {
  app: ClipsyncState;
  onOpenRoomDialog: () => void;
  onNewEntry: () => void;
  searchRef: React.RefObject<HTMLInputElement | null>;
}

export function ClipsPage({ app, onOpenRoomDialog, onNewEntry, searchRef }: ClipsPageProps) {
  const [query, setQuery] = useState('');
  const [filter, setFilter] = useState<Filter>('all');
  const [scope, setScope] = useState<Scope>('all');
  const [device, setDevice] = useState<DeviceScope>('all');
  const [sort, setSort] = useState<SortMode>('time');
  const [editing, setEditing] = useState<ClipRecord | null>(null);
  const [noting, setNoting] = useState<ClipRecord | null>(null);
  const [detailing, setDetailing] = useState<ClipRecord | null>(null);
  const [transforming, setTransforming] = useState<ClipRecord | null>(null);
  const [translating, setTranslating] = useState<ClipRecord | null>(null);
  const [qrCoding, setQrCoding] = useState<ClipRecord | null>(null);
  const [previewing, setPreviewing] = useState<ClipRecord | null>(null);
  const [movingTo, setMovingTo] = useState<ClipRecord | null>(null);
  const [sendingFile, setSendingFile] = useState(false);
  const [deletingHistory, setDeletingHistory] = useState(false);
  const [recording, setRecording] = useState<RecordStatus | null>(null);
  const [picking, setPicking] = useState<string | null>(null);

  const startPick = useCallback(() => {
    void call('picker_start')
      .then(() => undefined)
      .catch(() => {
        void call<string>('capture_frame')
          .then((frame) => setPicking(frame))
          .catch((error) => toast.error(String(error)));
      });
  }, []);

  useEffect(() => {
    if (!isTauri()) return;
    let stop: (() => void) | undefined;
    void import('@tauri-apps/api/event')
      .then(({ listen }) => listen('clipsync://screenshot', () => startPick()))
      .then((unlisten) => {
        stop = unlisten;
      })
      .catch(() => {});
    return () => stop?.();
  }, [startPick]);

  useEffect(() => {
    let timer = 0;
    const poll = () => {
      void app
        .recordStatus()
        .then(setRecording)
        .catch(() => setRecording(null));
    };
    poll();
    timer = window.setInterval(poll, 1000);
    return () => window.clearInterval(timer);
  }, [app]);

  const toggleRecord = () => {
    if (recording?.active) {
      void app
        .recordStop()
        .then((result) => {
          setRecording(null);
          if (result.finished) toast.success(`录屏已保存：${result.name}`);
          else toast.error('录像没收尾，文件可能不完整');
          void call('preview_open', { path: result.path, name: result.name }).catch(() => {});
          void app.refresh();
        })
        .catch((error) => toast.error(String(error)));
      return;
    }
    void app
      .recordStart()
      .then(() => toast.success('开始录屏'))
      .catch((error) => toast.error(String(error)));
  };
  const dragRef = useRef<{ id: string; x: number; y: number; fired: boolean } | null>(null);
  const [cursorId, setCursorId] = useState<string | null>(null);
  const [selection, setSelection] = useState<string[]>([]);
  const [regex, setRegex] = useState(false);
  const [caseSensitive, setCaseSensitive] = useState(false);
  const [hits, setHits] = useState<number[] | null>(null);

  const badRegex = useMemo(() => {
    if (!regex || !query.trim()) return false;
    try {
      new RegExp(query);
      return false;
    } catch {
      return true;
    }
  }, [regex, query]);

  useEffect(() => {
    if (!query.trim()) {
      setHits(null);
      return;
    }
    let cancelled = false;
    void app
      .search(query, regex, caseSensitive)
      .then((ids) => {
        if (!cancelled) setHits(ids);
      })
      .catch(() => {
        if (!cancelled) setHits(null);
      });
    return () => {
      cancelled = true;
    };
  }, [query, regex, caseSensitive, app]);

  const devices = useMemo(() => {
    const names = new Set<string>();
    for (const item of app.items) {
      if (item.sender_name) names.add(item.sender_name);
    }
    return Array.from(names);
  }, [app.items]);

  const visible = useMemo(() => {
    const now = Date.now();
    const day = 24 * 60 * 60 * 1000;
    const matched = app.items.filter((item, index) => {
      if (filter === 'text' && item.kind !== 'text') return false;
      if (filter === 'file' && item.kind !== 'file') return false;
      if (scope === 'pinned' && !item.pinned) return false;
      if (scope === 'today' && now - item.ts > day) return false;
      if (scope === 'week' && now - item.ts > day * 7) return false;
      if (device !== 'all' && item.sender_name !== device) return false;
      if (hits && !hits.includes(index)) return false;
      return true;
    });
    return matched.sort((a, b) => {
      if (sort === 'pinned') {
        if (a.pinned !== b.pinned) return a.pinned ? -1 : 1;
      }
      return b.ts - a.ts;
    });
  }, [app.items, query, filter, scope, device, sort]);

  const copy = (item: ClipRecord) => {
    if (item.kind === 'text') {
      void navigator.clipboard?.writeText(item.text);
      toast.success('已复制到剪贴板');
      return;
    }
    const path = item.file?.local_path;
    if (!path) {
      toast('文件还没下载到本机');
      return;
    }
    void call('clipboard_write_files', { paths: [path] }).then((ok) => {
      if (ok) toast.success('已复制到剪贴板');
      else toast.error('复制失败');
    });
  };

  const toggleSelect = (id: string) => {
    setSelection((prev) => (prev.includes(id) ? prev.filter((item) => item !== id) : [...prev, id]));
  };

  const sendRecord = (item: ClipRecord) => {
    if (!app.room) {
      toast.error('还没进房间：先进房间再发送');
      return;
    }
    if (item.kind === 'text') {
      void call('engine_publish_text', { text: item.text })
        .then(() => toast.success('已同步到房间'))
        .catch((error) => toast.error(String(error)));
      return;
    }
    const path = item.file?.local_path;
    if (!path) {
      toast.error('文件还没落到本机，先下载再发');
      return;
    }
    void app
      .uploadFile(path)
      .then(() => toast.success('已发到房间'))
      .catch((error) => toast.error(String(error)));
  };

  const selectedItems = useMemo(
    () => app.items.filter((item) => selection.includes(item.id)),
    [app.items, selection],
  );

  const copySelection = () => {
    if (selection.length === 0) return;
    const payload = selectedItems
      .map((item) => (item.file ? `[${item.file.name}]` : item.text))
      .join('\n\n');
    if (!payload) return;
    void navigator.clipboard?.writeText(payload);
    toast.success(`已复制 ${selection.length} 项`);
  };

  const exportSelection = () => {
    if (selection.length === 0) return;
    void app
      .exportJson(selection)
      .then(() => toast.success(`已把 ${selection.length} 条导出到剪贴板`))
      .catch((error) => toast.error(String(error)));
  };

  const deleteSelection = () => {
    if (selection.length === 0) return;
    void app.remove(selection).then(() => {
      setSelection([]);
      toast({
        title: `已删除 ${selection.length} 项`,
        duration: 9000,
        action: {
          label: '撤销',
          onClick: () => {
            void app.undo();
          },
        },
      });
    });
  };

  const onDragDown = (event: PointerEvent<HTMLElement>, item: ClipRecord) => {
    if (event.button !== 0 || (event.target as HTMLElement).closest('button')) return;
    dragRef.current = { id: item.id, x: event.clientX, y: event.clientY, fired: false };
  };

  const onDragMove = (event: PointerEvent<HTMLElement>, item: ClipRecord) => {
    const state = dragRef.current;
    if (!state || state.id !== item.id || state.fired) return;
    if (Math.hypot(event.clientX - state.x, event.clientY - state.y) < 8) return;
    state.fired = true;
    if (!isTauri()) return;
    const batch = selection.length > 1 ? selectedItems : [item];
    const sources = batch.map((entry) =>
      entry.file?.local_path ? { filePath: entry.file.local_path } : { text: entry.text },
    );
    void call('begin_drag', { sources }).catch(() => toast.error('拖出失败：文件不可用'));
  };

  const onDragEnd = () => {
    dragRef.current = null;
  };

  const actions: ItemMenuActions = {
    onEdit: (item) => setEditing(item),
    onNote: (item) => setNoting(item),
    onTransform: (item) => setTransforming(item),
    onTranslate: (item) => setTranslating(item),
    onDuplicate: (item) => void app.duplicate(item.id),
    onSaveText: (item) => {
      void call<string>('save_text_to_downloads', { text: item.text })
        .then((path) => toast.success(`已保存：${path}`))
        .catch(() => toast.error('保存失败'));
    },
    onSaveFile: (item) => {
      const path = item.file?.local_path;
      if (!path) {
        toast('文件还没下载到本机');
        return;
      }
      void call<string>('save_file_to_downloads', { path })
        .then((saved) => toast.success(`已保存：${saved}`))
        .catch(() => toast.error('保存失败'));
    },
    onResend: (item) => {
      const path = item.file?.local_path;
      if (!path) {
        toast('文件还没下载到本机，无法补传');
        return;
      }
      void app
        .uploadFile(path)
        .then(() => toast.success('已补传到房间'))
        .catch((error) => toast.error(String(error)));
    },
    onDownload: (item) => {
      if (!item.file?.link) {
        toast('还没有下载链接');
        return;
      }
      void app
        .downloadFile(item)
        .then(() => toast.success('已下载到本机缓存'))
        .catch((e) => toast.error(String(e)));
    },
    onSend: (item) => sendRecord(item),
    onOpen: (item) => {
      const path = item.file?.local_path;
      if (!path) {
        toast('文件还没下载到本机');
        return;
      }
      void call('open_local_path', { path })
        .then(() => toast.success('已交给系统打开'))
        .catch((error) => toast.error(String(error)));
    },
    onCopyImage: (item) => {
      const path = item.file?.local_path;
      if (!path) {
        toast('这张图还没下载到本机');
        return;
      }
      void call<boolean>('clipboard_write_image', { path })
        .then((ok) => {
          if (ok) toast.success('图片已复制到剪贴板');
          else toast.error('复制失败');
        })
        .catch((error) => toast.error(String(error)));
    },
    onDetail: (item) => setDetailing(item),
    onQrCode: (item) => setQrCoding(item),
    onExport: (item) => {
      void app
        .exportJson([item.id])
        .then(() => toast.success('已复制到剪贴板'))
        .catch((error) => toast.error(String(error)));
    },
    onMoveUp: (item) => void app.moveItem(item.id, -1),
    onMoveDown: (item) => void app.moveItem(item.id, 1),
    onMoveTop: (item) => {
      void call('history_move_to_edge', { id: item.id, top: true }).then(() => void app.refresh());
    },
    onMoveBottom: (item) => {
      void call('history_move_to_edge', { id: item.id, top: false }).then(() => void app.refresh());
    },
    onMoveTo: (item) => setMovingTo(item),
    onDelete: (item) => {
      void app.remove([item.id]).then(() => {
        toast({
          title: '已删除',
          duration: 9000,
          action: {
            label: '撤销',
            onClick: () => {
              void app.undo();
            },
          },
        });
      });
    },
  };

  useShortcuts({
    onSearch: () => searchRef.current?.focus(),
    onNew: onNewEntry,
    onRoom: onOpenRoomDialog,
    onUndo: () => void app.undo(),
    onDuplicate: () => {
      const target = visible.find((item) => item.id === cursorId);
      if (target) void app.duplicate(target.id);
    },
    onDelete: () => {
      if (selection.length > 0) {
        deleteSelection();
        return;
      }
      const target = visible.find((item) => item.id === cursorId);
      if (target) void app.remove([target.id]);
    },
    onEdit: () => {
      const target = visible.find((item) => item.id === cursorId);
      if (target) setEditing(target);
    },
    onCopy: () => {
      if (selection.length > 0) {
        copySelection();
        return;
      }
      const target = visible.find((item) => item.id === cursorId);
      if (target) copy(target);
    },
    onSelectAll: () => {
      setSelection(visible.map((item) => item.id));
    },
    onActivate: () => {
      if (selection.length > 0) {
        const target = visible.find((item) => item.id === cursorId);
        if (target) toggleSelect(target.id);
        return;
      }
      const target = visible.find((item) => item.id === cursorId);
      if (target) copy(target);
    },
    onMoveCursor: (delta) => {
      if (visible.length === 0) return;
      const index = visible.findIndex((item) => item.id === cursorId);
      const next = index < 0 ? 0 : Math.min(visible.length - 1, Math.max(0, index + delta));
      setCursorId(visible[next].id);
    },
    onExtendCursor: (delta) => {
      if (visible.length === 0) return;
      const index = visible.findIndex((item) => item.id === cursorId);
      const next = index < 0 ? 0 : Math.min(visible.length - 1, Math.max(0, index + delta));
      const target = visible[next];
      setCursorId(target.id);
      setSelection((prev) => (prev.includes(target.id) ? prev : [...prev, target.id]));
    },
    onMoveItem: (delta) => {
      const target = visible.find((item) => item.id === cursorId);
      if (target) void app.moveItem(target.id, delta);
    },
    onJumpEdge: (toEnd) => {
      if (visible.length === 0) return;
      setCursorId((toEnd ? visible[visible.length - 1] : visible[0]).id);
    },
    onEscape: () => {
      if (selection.length > 0) {
        setSelection([]);
        return;
      }
      if (query) {
        setQuery('');
        return;
      }
      if (cursorId) {
        setCursorId(null);
        return;
      }
      searchRef.current?.blur();
    },
  });

  if (!app.ready) {
    return (
      <div className="page">
        <Text variant="caption" tone="subtle">正在载入…</Text>
      </div>
    );
  }

  return (
    <div className="page">
      <header className="page__header">
        <div>
          <Text as="h1" variant="h2">剪贴板</Text>
          <Text variant="caption" tone="subtle">所有设备的最新内容，都会同步到这里</Text>
        </div>
        <div className="page__header-actions">
          <Button
            variant="soft"
            leading={<ClipIcon name="send" />}
            onClick={() => {
              void app
                .sendClipboard()
                .then(() => toast.success('已发送到房间'))
                .catch((error) => toast.error(String(error)));
            }}
          >
            发送剪贴板
          </Button>
          <Button variant="soft" leading={<ClipIcon name="attach" />} onClick={() => setSendingFile(true)}>
            发送文件
          </Button>
          <Button
            variant="soft"
            leading={<ClipIcon name="crop" />}
            onClick={startPick}
          >
            截屏
          </Button>
          <Button
            variant={recording?.active ? 'accent' : 'soft'}
            leading={<ClipIcon name="videocam" />}
            onClick={toggleRecord}
          >
            {recording?.active ? '停止录屏' : '录屏'}
          </Button>
          <Button variant="accent" leading={<ClipIcon name="plus" />} onClick={onNewEntry}>
            新建
          </Button>
        </div>
      </header>

      {badRegex && (
        <Text variant="caption" tone="danger">正则表达式写错了，改一下再搜</Text>
      )}

      {recording?.active && (
        <div className="record-bar">
          <span className="record-bar__dot" />
          <Text variant="label">正在录屏 {formatDuration(recording.elapsedMs)}</Text>
          <Text variant="caption" tone="subtle">{recording.name}</Text>
          <Button variant="ghost" size="sm" onClick={toggleRecord}>停止</Button>
        </div>
      )}

      <div className="page__toolbar">
        <SearchInput ref={searchRef} value={query} onChange={setQuery} placeholder="搜索内容、来源设备…" />
        <button
          type="button"
          className="chip"
          data-active={regex}
          aria-pressed={regex}
          title="按正则表达式搜索"
          onClick={() => setRegex((prev) => !prev)}
        >
          .*
        </button>
        <button
          type="button"
          className="chip"
          data-active={caseSensitive}
          aria-pressed={caseSensitive}
          title="区分大小写"
          onClick={() => setCaseSensitive((prev) => !prev)}
        >
          Aa
        </button>
        <Segmented<Filter>
          value={filter}
          onChange={setFilter}
          size="sm"
          options={[
            { value: 'all', label: '全部' },
            { value: 'text', label: '文本' },
            { value: 'file', label: '文件' },
          ]}
        />
        <Segmented<Scope>
          value={scope}
          onChange={setScope}
          size="sm"
          options={[
            { value: 'all', label: '不限' },
            { value: 'pinned', label: '置顶' },
            { value: 'today', label: '24 小时' },
            { value: 'week', label: '7 天' },
          ]}
        />
        <Segmented<SortMode>
          value={sort}
          onChange={setSort}
          size="sm"
          options={[
            { value: 'time', label: '按时间' },
            { value: 'pinned', label: '置顶优先' },
          ]}
        />
        <Button variant="ghost" size="sm" leading={<ClipIcon name="trash" />} onClick={() => setDeletingHistory(true)}>
          删除历史
        </Button>
      </div>

      {devices.length > 0 && (
        <div className="page__toolbar page__toolbar--devices">
          <Segmented<DeviceScope>
            value={device}
            onChange={setDevice}
            size="sm"
            options={[
              { value: 'all', label: '全部来源' },
              ...devices.map((name) => ({ value: name, label: name })),
            ]}
          />
        </div>
      )}

      {visible.length === 0 ? (
        <EmptyState
          icon={<ClipIcon name="clipboard" size={28} />}
          title={app.items.length === 0 ? '还没有任何内容' : '没有找到内容'}
          description={app.items.length === 0 ? '复制点东西，或者把文件拖进窗口，这里就会记下来。' : '换个关键词，或者放宽筛选条件试试。'}
          actions={<Button variant="soft" onClick={() => { setQuery(''); setFilter('all'); }}>清除筛选</Button>}
        />
      ) : (
        <div className="clip-list">
          {visible.map((item) => (
            <Card
              key={item.id}
              size="sm"
              className={`clip-card${item.id === cursorId ? ' clip-card--cursor' : ''}${selection.includes(item.id) ? ' clip-card--selected' : ''}`}
              title="拖到桌面可保存为文件"
              onPointerDown={(event) => {
                if (event.shiftKey || event.metaKey || event.ctrlKey) {
                  toggleSelect(item.id);
                  setCursorId(item.id);
                  return;
                }
                setCursorId(item.id);
                onDragDown(event, item);
              }}
              onPointerMove={(event) => onDragMove(event, item)}
              onPointerUp={onDragEnd}
              onPointerCancel={onDragEnd}
            >
              <div className="clip-card__icon">
                <Icon name={iconFor(item)} />
              </div>
              <div className="clip-card__body">
                <div className="clip-card__top">
                  <Text variant="label" truncate>{item.file?.name ?? (item.text.split('\n')[0] || '（空）')}</Text>
                  {item.pinned && <Icon name="bookmark" size={14} filled />}
                </div>
                {isPreviewable(item) && (
                  <Thumb
                    path={item.file?.local_path}
                    hash={item.file?.sha256}
                    name={item.file?.name ?? ''}
                    alt={item.file?.name ?? ''}
                    maxEdge={256}
                    onOpen={() => setPreviewing(item)}
                  />
                )}
                {isPlayable(item) && (
                  <Button
                    variant="soft"
                    size="sm"
                    leading={<ClipIcon name="videocam" />}
                    onClick={() => {
                      const path = item.file?.local_path;
                      if (!path) {
                        toast('这段录像还没下载到本机');
                        return;
                      }
                      void call('preview_open', { path, name: item.file?.name ?? '录像' }).catch((error) =>
                        toast.error(String(error)),
                      );
                    }}
                  >
                    回放
                  </Button>
                )}
                <Text variant="caption" tone="subtle" lineClamp={2} className="clip-card__preview">
                  {item.kind === 'text' ? item.text : fileMeta(item)}
                </Text>
                {item.note && <Text variant="micro" tone="subtle">备注：{item.note}</Text>}
                <div className="clip-card__meta">
                  <Text variant="micro" tone="subtle">{item.sender_name || '本机'}</Text>
                  <span className="dot" />
                  <Text variant="micro" tone="subtle">{formatTime(item.ts)}</Text>
                </div>
              </div>
              <div className="clip-card__actions">
                <Button
                  iconOnly
                  aria-label="发送到房间"
                  variant="ghost"
                  size="sm"
                  leading={<ClipIcon name="send" />}
                  onClick={() => sendRecord(item)}
                />
                <Button iconOnly aria-label="复制" variant="ghost" size="sm" leading={<ClipIcon name="copy" />} onClick={() => copy(item)} />
                {item.file?.local_path && (
                  <Button
                    iconOnly
                    aria-label="打开"
                    variant="ghost"
                    size="sm"
                    leading={<ClipIcon name="open-in-new" />}
                    onClick={() => {
                      void call('open_local_path', { path: item.file?.local_path })
                        .then(() => toast.success('已交给系统打开'))
                        .catch((error) => toast.error(String(error)));
                    }}
                  />
                )}
                {item.file?.link && (
                  <Button
                    iconOnly
                    aria-label="下载"
                    variant="ghost"
                    size="sm"
                    leading={<ClipIcon name="download" />}
                    onClick={() => actions.onDownload(item)}
                  />
                )}
                <Button iconOnly aria-label={item.pinned ? '取消置顶' : '置顶'} variant="ghost" size="sm" leading={<ClipIcon name="pin" />} onClick={() => void app.togglePin(item.id)} />
                <ItemMenu item={item} actions={actions} />
              </div>
            </Card>
          ))}
        </div>
      )}

      {selection.length > 0 && (
        <div className="selection-bar">
          <Text variant="label">已选 {selection.length} 项</Text>
          <div className="row-actions">
            <Button variant="soft" size="sm" leading={<ClipIcon name="copy" />} onClick={copySelection}>
              复制
            </Button>
            <Button variant="soft" size="sm" leading={<ClipIcon name="export" />} onClick={exportSelection}>
              导出 JSON
            </Button>
            <Button variant="ghost" size="sm" onClick={() => setSelection([])}>
              取消
            </Button>
            <Button variant="ghost" size="sm" leading={<ClipIcon name="trash" />} onClick={deleteSelection}>
              删除
            </Button>
          </div>
        </div>
      )}

      <EditDialog item={editing} onClose={() => setEditing(null)} onSubmit={(id, text) => void app.updateText(id, text)} />
      <NoteDialog item={noting} onClose={() => setNoting(null)} onSubmit={(id, note) => void app.updateNote(id, note)} />
      <DetailDialog item={detailing} onClose={() => setDetailing(null)} onCopy={copy} />
      <TransformDialog item={transforming} onClose={() => setTransforming(null)} />
      <TranslateDialog item={translating} onClose={() => setTranslating(null)} />
      <QrCodeDialog item={qrCoding} onClose={() => setQrCoding(null)} />
      <ImagePreviewDialog item={previewing} onClose={() => setPreviewing(null)} />
      <MoveToDialog
        item={movingTo}
        total={visible.length}
        onClose={() => setMovingTo(null)}
        onSubmit={(id, target) => {
          void call('history_move_to', { id, target }).then(() => void app.refresh());
        }}
      />
      <ScreenPickDialog
        frame={picking}
        onClose={() => setPicking(null)}
        onConfirm={(cropped) => {
          void call('apply_screenshot', { dataUrl: cropped })
            .then(() => {
              setPicking(null);
              toast.success('已截屏并同步');
              void app.refresh();
            })
            .catch((error) => toast.error(String(error)));
        }}
      />

      <SendFileDialog
        open={sendingFile}
        onClose={() => setSendingFile(false)}
        onSend={async (paths) => {
          if (!app.room) throw new Error('先加入房间，才能把文件发出去');
          for (const path of paths) {
            await app.uploadFile(path);
          }
          toast.success(`已发送 ${paths.length} 个文件`);
        }}
      />
      <DeleteHistoryDialog
        open={deletingHistory}
        onClose={() => setDeletingHistory(false)}
        onDeleted={() => void app.refresh()}
      />
    </div>
  );
}

function isPreviewable(item: ClipRecord): boolean {
  return item.kind === 'file' && item.file?.mime.startsWith('image/') === true;
}

function isPlayable(item: ClipRecord): boolean {
  return item.kind === 'file' && item.file?.mime.startsWith('video/') === true;
}

function iconFor(item: ClipRecord): 'file' | 'image' | 'paperclip' {
  if (item.kind !== 'file') return 'file';
  return item.file?.mime.startsWith('image/') ? 'image' : 'paperclip';
}

function fileMeta(item: ClipRecord): string {
  const file = item.file;
  if (!file) return '本机文件';
  const size = file.size >= 1024 * 1024
    ? `${(file.size / 1024 / 1024).toFixed(1)} MB`
    : `${Math.max(1, Math.round(file.size / 1024))} KB`;
  const state = file.cached ? '已缓存' : '未下载';
  return `${file.mime} · ${size} · ${state}`;
}

function formatTime(ts: number): string {
  if (!ts) return '刚刚';
  const diff = Date.now() - ts;
  if (diff < 60_000) return '刚刚';
  if (diff < 3_600_000) return `${Math.floor(diff / 60_000)} 分钟前`;
  if (diff < 86_400_000) return `${Math.floor(diff / 3_600_000)} 小时前`;
  return new Date(ts).toLocaleDateString();
}

function formatDuration(ms: number): string {
  const total = Math.floor(ms / 1000);
  const minutes = Math.floor(total / 60);
  const seconds = total % 60;
  return `${minutes}:${String(seconds).padStart(2, '0')}`;
}
