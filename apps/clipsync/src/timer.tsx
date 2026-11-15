import { useEffect, useState } from 'react';
import { createRoot } from 'react-dom/client';
import { Button, OinkProvider, Text } from 'oink-ui';
import { call } from './lib/api';
import 'oink-ui/styles.css';
import './app.css';

interface RecordStatus {
  active: boolean;
  elapsedMs: number;
}

function Timer() {
  const [elapsed, setElapsed] = useState(0);

  useEffect(() => {
    const timer = window.setInterval(() => {
      void call<RecordStatus>('record_status')
        .then((status) => {
          if (!status.active) {
            void call('record_stop').catch(() => {});
            return;
          }
          setElapsed(status.elapsedMs);
        })
        .catch(() => {});
    }, 500);
    return () => window.clearInterval(timer);
  }, []);

  return (
    <div className="record-timer">
      <span className="record-bar__dot" />
      <Text variant="label">{format(elapsed)}</Text>
      <Button
        variant="soft"
        size="sm"
        onClick={() => {
          void call('record_stop').catch(() => {});
        }}
      >
        结束
      </Button>
    </div>
  );
}

function format(ms: number): string {
  const total = Math.floor(ms / 1000);
  const minutes = Math.floor(total / 60);
  const seconds = total % 60;
  return `${minutes}:${String(seconds).padStart(2, '0')}`;
}

createRoot(document.getElementById('root') as HTMLElement).render(
  <OinkProvider defaultTheme="system" defaultAccent="pink" locale="zh-CN" storageKey="clipsync-ui">
    <Timer />
  </OinkProvider>,
);
