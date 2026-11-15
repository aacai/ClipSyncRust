const CACHE_LIMIT = 16;

const store = new Map<string, string>();

export function thumbCached(key: string): string | undefined {
  const value = store.get(key);
  if (value === undefined) return undefined;
  store.delete(key);
  store.set(key, value);
  return value;
}

export function thumbStore(key: string, value: string): void {
  store.delete(key);
  store.set(key, value);
  while (store.size > CACHE_LIMIT) {
    const oldest = store.keys().next();
    if (oldest.done) break;
    store.delete(oldest.value);
  }
}
