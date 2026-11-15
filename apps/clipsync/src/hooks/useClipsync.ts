import { useCallback, useEffect, useState } from 'react';
import { listen } from '@tauri-apps/api/event';
import { toast } from 'oink-ui';
import { call, isTauri } from '../lib/api';
import type {
  ClipRecord,
  EngineEvent,
  EngineStatus,
  HistorySnapshot,
  Peer,
  PreferencesSnapshot,
  ThemeMode,
} from '../lib/types';

export interface TransferProgress {
  upload: boolean;
  name: string;
  done: number;
  total: number;
}

export interface UpdateStatus {
  current: string;
  latest: string;
  behind: boolean;
  assetName: string | null;
  assetUrl: string | null;
  assetSize: number;
}

export interface InviteLink {
  room: string;
  password: string | null;
}

export interface RecordStatus {
  active: boolean;
  elapsedMs: number;
  path: string | null;
  name: string | null;
}

export interface RecordResult {
  path: string;
  name: string;
  size: number;
  finished: boolean;
}

export interface MicInput {
  id: string;
  name: string;
}

export interface ClipsyncState {
  ready: boolean;
  themePref: ThemeMode;
  items: ClipRecord[];
  undoDepth: number;
  prefs: PreferencesSnapshot | null;
  status: EngineStatus;
  peers: Peer[];
  room: string | null;
  keyMismatch: string[];
  refresh: () => Promise<void>;
  togglePin: (id: string) => Promise<void>;
  remove: (ids: string[]) => Promise<void>;
  undo: () => Promise<void>;
  updateText: (id: string, text: string) => Promise<void>;
  updateNote: (id: string, note: string) => Promise<void>;
  duplicate: (id: string) => Promise<void>;
  moveItem: (id: string, delta: number) => Promise<void>;
  search: (query: string, regex: boolean, caseSensitive: boolean) => Promise<number[]>;
  setPref: <K extends keyof PreferencesSnapshot>(key: K, value: PreferencesSnapshot[K]) => Promise<void>;
  join: (room: string, password: string) => Promise<void>;
  leave: () => Promise<void>;
  retry: () => Promise<void>;
  setAutostart: (enabled: boolean) => Promise<void>;
  sendClipboard: () => Promise<void>;
  exportJson: (ids: string[]) => Promise<void>;
  removeExpired: () => Promise<number>;
  uploadFile: (path: string) => Promise<void>;
  downloadFile: (item: ClipRecord) => Promise<void>;
  checkUpdate: () => Promise<UpdateStatus>;
  downloadUpdate: (url: string, name: string) => Promise<string>;
  quit: () => Promise<void>;
  randomRoom: () => Promise<string>;
  parseInvite: (link: string) => Promise<InviteLink | null>;
  setBackground: (path: string) => Promise<string>;
  clearBackground: () => Promise<void>;
  backgroundDataUrl: () => Promise<string | null>;
  memoryUsage: () => Promise<number>;
  alwaysOnTop: (on: boolean) => Promise<void>;
  clickThrough: (on: boolean) => Promise<void>;
  activateApp: () => Promise<void>;
  recordStart: () => Promise<void>;
  recordStop: () => Promise<RecordResult>;
  recordStatus: () => Promise<RecordStatus>;
  recordInputs: () => Promise<MicInput[]>;
  transferProgress: TransferProgress | null;
}

export function useClipsync(): ClipsyncState {
  const [ready, setReady] = useState(false);
  const [items, setItems] = useState<ClipRecord[]>([]);
  const [undoDepth, setUndoDepth] = useState(0);
  const [prefs, setPrefs] = useState<PreferencesSnapshot | null>(null);
  const [status, setStatus] = useState<EngineStatus>({ Idle: null });
  const [peers, setPeers] = useState<Peer[]>([]);
  const [room, setRoom] = useState<string | null>(null);
  const [keyMismatch, setKeyMismatch] = useState<string[]>([]);
  const [transferProgress, setTransferProgress] = useState<TransferProgress | null>(null);
  const [themePref, setThemePref] = useState<ThemeMode>('System');

  const refresh = useCallback(async () => {
    if (!isTauri()) return;
    const snapshot = await call<HistorySnapshot>('history_list');
    setItems(snapshot.items);
    setUndoDepth(snapshot.undoDepth);
  }, []);

  useEffect(() => {
    if (!isTauri()) {
      setReady(true);
      return;
    }
    void (async () => {
      const [loaded, snapshot] = await Promise.all([
        call<PreferencesSnapshot>('prefs_get'),
        call<HistorySnapshot>('history_list'),
      ]);
      setPrefs(loaded);
      setThemePref(loaded.theme);
      setItems(snapshot.items);
      setUndoDepth(snapshot.undoDepth);
      setRoom(loaded.lastRoomCode ?? null);
      setReady(true);
    })();
  }, []);

  useEffect(() => {
    if (!isTauri()) return;
    const unlisten = listen<TransferProgress>('clipsync://transfer', (event) => {
      setTransferProgress(event.payload);
    });
    return () => {
      void unlisten.then((stop) => stop());
    };
  }, []);

  useEffect(() => {
    if (!isTauri()) return;
    const unlisten = listen<EngineEvent>('clipsync://engine', (event) => {
      const payload = event.payload;
      if (payload.kind === 'status' && payload.status) {
        setStatus(payload.status);
      } else if (payload.kind === 'peers' && payload.peers) {
        setPeers(payload.peers);
      } else if (payload.kind === 'keyMismatch' && payload.keyMismatch) {
        setKeyMismatch(payload.keyMismatch);
      } else if (payload.kind === 'clip') {
        void refresh();
      } else if (payload.kind === 'fileTooLarge' && payload.fileTooLarge) {
        toast.error(`文件太大，已跳过：${payload.fileTooLarge}`);
      }
    });
    return () => {
      void unlisten.then((stop) => stop());
    };
  }, [refresh]);

  const togglePin = useCallback(async (id: string) => {
    await call('history_toggle_pin', { id });
    await refresh();
  }, [refresh]);

  const remove = useCallback(async (ids: string[]) => {
    await call('history_remove', { ids });
    await refresh();
  }, [refresh]);

  const undo = useCallback(async () => {
    await call('history_undo');
    await refresh();
  }, [refresh]);

  const updateText = useCallback(async (id: string, text: string) => {
    await call('history_update_text', { id, text });
    await refresh();
  }, [refresh]);

  const updateNote = useCallback(async (id: string, note: string) => {
    await call('history_update_note', { id, note });
    await refresh();
  }, [refresh]);

  const duplicate = useCallback(async (id: string) => {
    await call('history_duplicate', { id, newId: `local-${Date.now()}-${Math.random().toString(36).slice(2, 8)}` });
    await refresh();
  }, [refresh]);

  const moveItem = useCallback(async (id: string, delta: number) => {
    await call('history_move_item', { id, delta });
    await refresh();
  }, [refresh]);

  const search = useCallback(async (query: string, regex: boolean, caseSensitive: boolean) => {
    if (!isTauri()) return [];
    return call<number[]>('history_search', {
      options: { query, regex, caseSensitive },
    });
  }, []);

  const setPref = useCallback(async <K extends keyof PreferencesSnapshot>(
    key: K,
    value: PreferencesSnapshot[K],
  ) => {
    const snake = key.replace(/[A-Z]/g, (c) => `_${c.toLowerCase()}`);
    await call(`prefs_set_${snake}`, { [key]: value });
    const next = await call<PreferencesSnapshot>('prefs_get');
    setPrefs(next);
    setThemePref(next.theme);
    if (snake === 'screenshot_hotkey' || snake === 'record_hotkey') {
      await call('hotkey_reload');
    }
  }, []);

  const join = useCallback(async (roomCode: string, password: string) => {
    await call('engine_join', { request: { room: roomCode, password } });
    setRoom(roomCode);
  }, []);

  const leave = useCallback(async () => {
    await call('engine_leave');
    setRoom(null);
    setPeers([]);
  }, []);

  const retry = useCallback(async () => {
    await call('engine_retry');
  }, []);

  const setAutostart = useCallback(async (enabled: boolean) => {
    await call('set_enabled', { enabled });
    setPrefs(await call<PreferencesSnapshot>('prefs_get'));
  }, []);

  const sendClipboard = useCallback(async () => {
    const read = await call<{ text: string | null }>('clipboard_read_text');
    const text = read.text?.trim();
    if (!text) {
      throw new Error('剪贴板里没有可发送的内容');
    }
    await call('engine_publish_text', { text });
  }, []);

  const exportJson = useCallback(async (ids: string[]) => {
    const json = await call<string>('history_export_json', { ids });
    await navigator.clipboard?.writeText(json);
  }, []);

  const removeExpired = useCallback(async () => {
    const days = prefs?.retentionDays ?? 30;
    const before = Date.now() - days * 24 * 60 * 60 * 1000;
    const removed = await call<number>('history_remove_expired', { before });
    if (removed > 0) await refresh();
    return removed;
  }, [prefs, refresh]);

  const uploadFile = useCallback(async (path: string) => {
    await call('upload_and_publish', { path });
    await refresh();
  }, [refresh]);

  const downloadFile = useCallback(async (item: ClipRecord) => {
    const file = item.file;
    if (!file?.link) {
      throw new Error('还没有下载链接');
    }
    await call('download_to_cache', {
      id: item.id,
      name: file.name,
      mime: file.mime,
      size: file.size,
      sha256: file.sha256,
      link: file.link,
    });
    await refresh();
  }, [refresh]);

  const checkUpdate = useCallback(async () => {
    return call<UpdateStatus>('update_check');
  }, []);

  const downloadUpdate = useCallback(async (url: string, name: string) => {
    return call<string>('update_download', { url, name });
  }, []);

  const quit = useCallback(async () => {
    await call('app_quit');
  }, []);

  const randomRoom = useCallback(async () => {
    if (!isTauri()) {
      const alphabet = 'ABCDEFGHJKLMNPQRSTUVWXYZ23456789';
      const bytes = new Uint8Array(6);
      crypto.getRandomValues(bytes);
      return Array.from(bytes, (byte) => alphabet[byte % alphabet.length]).join('');
    }
    return call<string>('random_room_code');
  }, []);

  const parseInvite = useCallback(async (link: string) => {
    if (!isTauri()) return null;
    return call<InviteLink | null>('parse_invite_link', { link });
  }, []);

  const setBackground = useCallback(async (path: string) => {
    const stored = await call<string>('background_set', { path });
    const prefs = await call<PreferencesSnapshot>('prefs_get');
    setPrefs(prefs);
    return stored;
  }, []);

  const clearBackground = useCallback(async () => {
    await call('background_clear');
    setPrefs(await call<PreferencesSnapshot>('prefs_get'));
  }, []);

  const backgroundDataUrl = useCallback(async () => {
    if (!isTauri()) return null;
    return call<string | null>('background_data_url');
  }, []);

  const memoryUsage = useCallback(async () => {
    if (!isTauri()) return 0;
    return call<number>('memory_usage');
  }, []);

  const alwaysOnTop = useCallback(async (on: boolean) => {
    await call('window_always_on_top', { on });
  }, []);

  const clickThrough = useCallback(async (on: boolean) => {
    await call('window_click_through', { on });
  }, []);

  const activateApp = useCallback(async () => {
    await call('app_activate');
  }, []);

  const recordStart = useCallback(async () => {
    await call('record_start');
  }, []);

  const recordStop = useCallback(async () => {
    return call<RecordResult>('record_stop');
  }, []);

  const recordStatus = useCallback(async () => {
    return call<RecordStatus>('record_status');
  }, []);

  const recordInputs = useCallback(async () => {
    return call<MicInput[]>('record_inputs');
  }, []);

  return {
    ready,
    themePref,
    items,
    undoDepth,
    prefs,
    status,
    peers,
    room,
    keyMismatch,
    refresh,
    togglePin,
    remove,
    undo,
    updateText,
    updateNote,
    duplicate,
    moveItem,
    search,
    setPref,
    join,
    leave,
    retry,
    setAutostart,
    sendClipboard,
    exportJson,
    removeExpired,
    uploadFile,
    downloadFile,
    checkUpdate,
    downloadUpdate,
    quit,
    randomRoom,
    parseInvite,
    setBackground,
    clearBackground,
    backgroundDataUrl,
    memoryUsage,
    alwaysOnTop,
    clickThrough,
    activateApp,
    recordStart,
    recordStop,
    recordStatus,
    recordInputs,
    transferProgress,
  };
}
