import { useEffect, useRef, useState } from 'react';
import { call } from '../lib/api';
import { thumbCached, thumbStore } from '../lib/thumbs';

interface ThumbProps {
  path?: string | null;
  hash?: string | null;
  name: string;
  alt: string;
  maxEdge?: number;
  onOpen?: () => void;
}

export function Thumb({ path, hash, name, alt, maxEdge = 256, onOpen }: ThumbProps) {
  const [src, setSrc] = useState('');
  const [failed, setFailed] = useState(false);
  const holderRef = useRef<HTMLDivElement | null>(null);
  const key = path ?? hash ?? '';

  useEffect(() => {
    const cached = key ? thumbCached(key) : undefined;
    if (cached) {
      setSrc(cached);
      setFailed(false);
      return;
    }
    setSrc('');
    setFailed(false);

    const holder = holderRef.current;
    if (!holder) return;

    let cancelled = false;
    const load = async () => {
      try {
        if (path) {
          const dataUrl = await call<string>('image_thumbnail', { path, maxEdge });
          if (cancelled) return;
          thumbStore(key, dataUrl);
          setSrc(dataUrl);
          return;
        }
        if (hash) {
          const dataUrl = await call<string>('image_thumbnail_by_hash', { hash, name, maxEdge });
          if (cancelled) return;
          thumbStore(key, dataUrl);
          setSrc(dataUrl);
          return;
        }
        if (!cancelled) setFailed(true);
      } catch {
        if (!cancelled) setFailed(true);
      }
    };

    const observer = new IntersectionObserver(
      (entries) => {
        if (entries.some((entry) => entry.isIntersecting)) {
          observer.disconnect();
          void load();
        }
      },
      { rootMargin: '640px' },
    );
    observer.observe(holder);

    return () => {
      cancelled = true;
      observer.disconnect();
    };
  }, [path, hash, name, maxEdge, key]);

  if (failed) {
    return <div className="thumb thumb--failed" aria-label={alt} />;
  }

  if (!src) {
    return <div ref={holderRef} className="thumb thumb--loading" aria-label={alt} />;
  }

  return (
    <button
      type="button"
      className="thumb"
      aria-label={onOpen ? '放大查看' : alt}
      onClick={onOpen}
      disabled={!onOpen}
    >
      <img src={src} alt={alt} />
    </button>
  );
}
