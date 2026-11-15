export type ClipKind = 'text' | 'file';

export interface FileInfo {
  name: string;
  mime: string;
  size: number;
  sha256: string;
  link: string;
  expires_at: number;
  cached: boolean;
  local_path: string | null;
}

export interface ClipRecord {
  id: string;
  kind: string;
  ts: number;
  sender_id: string;
  sender_name: string;
  text: string;
  file: FileInfo | null;
  pinned: boolean;
  order: number;
  note: string;
}

export interface HistorySnapshot {
  items: ClipRecord[];
  undoDepth: number;
}

export type ThemeMode = 'System' | 'Light' | 'Dark' | 'Glass';
export type Language = 'System' | 'Chinese' | 'English';

export interface PreferencesSnapshot {
  theme: ThemeMode;
  language: Language;
  rememberRoom: boolean;
  nickname: string;
  watchClipboard: boolean;
  autoCopy: boolean;
  autoPasteFile: boolean;
  watchImages: boolean;
  watchFiles: boolean;
  launchAtLogin: boolean;
  deleteExpired: boolean;
  retentionDays: number;
  screenshotHotkey: string;
  screenshotKeepWindow: boolean;
  recordHotkey: string;
  recordRegion: boolean;
  recordMic: boolean;
  recordMicDevice: string;
  recordSystemAudio: boolean;
  recordLongEdge: number;
  recordFps: number;
  lastRoomCode: string | null;
  backgroundPath: string | null;
  backgroundDim: number;
}

export type EngineStatus =
  | { Idle: null }
  | { Connecting: null }
  | { Online: { safety: string } }
  | { Offline: { reason: string | null } };

export interface Peer {
  id: string;
  name: string;
  os: string;
}

export interface EngineEvent {
  kind: 'status' | 'clip' | 'peers' | 'keyMismatch' | 'fileTooLarge';
  status: EngineStatus | null;
  clip: ClipRecord | null;
  peers: Peer[] | null;
  keyMismatch: string[] | null;
  fileTooLarge: string | null;
}
export type Screen = 'clips' | 'devices' | 'settings';
