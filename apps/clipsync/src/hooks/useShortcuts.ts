import { useEffect } from 'react';

export interface ShortcutHandlers {
  onSearch: () => void;
  onNew: () => void;
  onRoom: () => void;
  onUndo: () => void;
  onDuplicate: () => void;
  onDelete: () => void;
  onEdit: () => void;
  onActivate: () => void;
  onCopy: () => void;
  onSelectAll: () => void;
  onMoveCursor: (delta: number) => void;
  onExtendCursor: (delta: number) => void;
  onMoveItem: (delta: number) => void;
  onJumpEdge: (toEnd: boolean) => void;
  onEscape: () => void;
}

function isTypingTarget(target: EventTarget | null): boolean {
  const element = target as HTMLElement | null;
  if (!element) return false;
  const tag = element.tagName;
  return tag === 'INPUT' || tag === 'TEXTAREA' || element.isContentEditable;
}

export function useShortcuts(handlers: ShortcutHandlers) {
  useEffect(() => {
    const onKeyDown = (event: KeyboardEvent) => {
      const mod = event.metaKey || event.ctrlKey;
      const key = event.key;

      if (mod && key.toLowerCase() === 'f') {
        event.preventDefault();
        handlers.onSearch();
        return;
      }

      if (isTypingTarget(event.target)) {
        if (key === 'Escape') handlers.onEscape();
        return;
      }

      if (mod && event.shiftKey && (key === 'ArrowUp' || key === 'ArrowDown')) {
        event.preventDefault();
        handlers.onMoveItem(key === 'ArrowUp' ? -1 : 1);
        return;
      }

      if (key === 'Escape') {
        event.preventDefault();
        handlers.onEscape();
        return;
      }

      if (mod) {
        const lower = key.toLowerCase();
        if (lower === 'z') {
          event.preventDefault();
          handlers.onUndo();
          return;
        }
        if (lower === 'c') {
          event.preventDefault();
          handlers.onCopy();
          return;
        }
        if (lower === 'd') {
          event.preventDefault();
          handlers.onDuplicate();
          return;
        }
        if (lower === 'n') {
          event.preventDefault();
          handlers.onNew();
          return;
        }
        if (lower === 'a') {
          event.preventDefault();
          handlers.onSelectAll();
          return;
        }
      }

      if (key === 'F2') {
        event.preventDefault();
        handlers.onEdit();
        return;
      }

      if (key === 'Delete' || key === 'Backspace') {
        event.preventDefault();
        handlers.onDelete();
        return;
      }

      if (key === 'Enter') {
        event.preventDefault();
        handlers.onActivate();
        return;
      }

      if (event.shiftKey && (key === 'ArrowUp' || key === 'ArrowDown')) {
        event.preventDefault();
        handlers.onExtendCursor(key === 'ArrowUp' ? -1 : 1);
        return;
      }

      if (key === 'ArrowUp' || key === 'ArrowDown') {
        event.preventDefault();
        handlers.onMoveCursor(key === 'ArrowUp' ? -1 : 1);
        return;
      }

      if (key === 'Home' || key === 'End') {
        event.preventDefault();
        handlers.onJumpEdge(key === 'End');
      }
    };
    window.addEventListener('keydown', onKeyDown);
    return () => window.removeEventListener('keydown', onKeyDown);
  }, [handlers]);
}
