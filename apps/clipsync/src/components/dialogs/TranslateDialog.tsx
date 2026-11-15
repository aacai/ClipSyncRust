import { useEffect, useState } from 'react';
import { Button, Dialog, Segmented, Text, toast } from 'oink-ui';
import { call } from '../../lib/api';
import type { ClipRecord } from '../../lib/types';

interface TranslateDialogProps {
  item: ClipRecord | null;
  onClose: () => void;
}

export function TranslateDialog({ item, onClose }: TranslateDialogProps) {
  const [direction, setDirection] = useState('en2zh');
  const [result, setResult] = useState('');
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState('');

  useEffect(() => {
    if (!item) return;
    void call<string>('translate_detect_target', { text: item.text })
      .then((detected) => {
        setDirection(detected);
        return run(detected, item.text);
      })
      .catch(() => setError('无法识别语言方向'));
  }, [item]);

  if (!item) return null;

  async function run(target: string, source: string) {
    setBusy(true);
    setError('');
    try {
      const out = await call<string>('translate_text', { text: source, target });
      setResult(out);
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(false);
    }
  }

  return (
    <Dialog
      open
      onOpenChange={(next) => !next && onClose()}
      size="lg"
      closable
      title="中英互译"
      description="自动识别原文语言，翻译结果可复制或替换原文"
      actions={
        <>
          <Button
            variant="ghost"
            onClick={() => {
              void navigator.clipboard?.writeText(result);
              toast.success('已复制译文');
            }}
          >
            复制译文
          </Button>
          <Button variant="accent" onClick={onClose}>关闭</Button>
        </>
      }
    >
      <div className="translate-bar">
        <Segmented
          value={direction}
          onChange={(value) => {
            setDirection(value);
            void run(value, item.text);
          }}
          size="sm"
          options={[
            { value: 'en2zh', label: '英文 → 中文' },
            { value: 'zh2en', label: '中文 → 英文' },
          ]}
        />
      </div>
      <pre className="detail-text">{item.text}</pre>
      {busy && <Text variant="caption" tone="subtle">翻译中…</Text>}
      {error && <Text variant="caption" tone="danger">{error}</Text>}
      {!busy && !error && result && <pre className="detail-text detail-text--result">{result}</pre>}
    </Dialog>
  );
}
