import { useEffect, useRef, useState } from 'react';
import { createRoot } from 'react-dom/client';
import { convertFileSrc } from '@tauri-apps/api/core';
import { Button, Text } from 'oink-ui';
import { call } from './lib/api';
import 'oink-ui/styles.css';
import './preview.css';

function Preview() {
  const [file, setFile] = useState<{ path: string; name: string } | null>(null);
  const [playing, setPlaying] = useState(false);
  const [progress, setProgress] = useState(0);
  const [duration, setDuration] = useState(0);
  const videoRef = useRef<HTMLVideoElement | null>(null);

  useEffect(() => {
    void call<[string | null, string | null]>('preview_file')
      .then(([path, name]) => {
        if (path) setFile({ path, name: name ?? '录像' });
      })
      .catch(() => {});
  }, []);

  const toggle = () => {
    const video = videoRef.current;
    if (!video) return;
    if (video.paused) {
      void video.play();
      setPlaying(true);
    } else {
      video.pause();
      setPlaying(false);
    }
  };

  const seek = (value: number) => {
    const video = videoRef.current;
    if (!video || !duration) return;
    video.currentTime = value * duration;
  };

  return (
    <div className="preview">
      <div className="preview__stage">
        {file ? (
          <video
            ref={videoRef}
            className="preview__video"
            src={convertFileSrc(file.path)}
            onTimeUpdate={(event) => {
              const video = event.currentTarget;
              if (video.duration) setProgress(video.currentTime / video.duration);
            }}
            onLoadedMetadata={(event) => setDuration(event.currentTarget.duration)}
            onEnded={() => setPlaying(false)}
            onClick={toggle}
          />
        ) : (
          <Text variant="caption" tone="subtle">没有可回放的录像</Text>
        )}
      </div>
      <div className="preview__bar">
        <Button variant="soft" size="sm" onClick={toggle}>
          {playing ? '暂停' : '播放'}
        </Button>
        <input
          className="preview__seek"
          type="range"
          min={0}
          max={1}
          step={0.001}
          value={progress}
          onChange={(event) => seek(Number(event.target.value))}
        />
        <Text variant="micro" tone="subtle">{format(duration * progress)} / {format(duration)}</Text>
        <Button
          variant="ghost"
          size="sm"
          onClick={() => {
            void call('save_file_to_downloads', { path: file?.path }).catch(() => {});
          }}
        >
          存到下载
        </Button>
        <Button variant="ghost" size="sm" onClick={() => void call('preview_close').catch(() => {})}>
          关闭
        </Button>
      </div>
      <Text variant="caption" tone="subtle" className="preview__name">{file?.name ?? ''}</Text>
    </div>
  );
}

function format(seconds: number): string {
  if (!Number.isFinite(seconds) || seconds <= 0) return '0:00';
  const total = Math.floor(seconds);
  return `${Math.floor(total / 60)}:${String(total % 60).padStart(2, '0')}`;
}

createRoot(document.getElementById('root') as HTMLElement).render(<Preview />);
