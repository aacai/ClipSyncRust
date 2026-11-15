import { Button, Dialog } from 'oink-ui';

interface LicenseDialogProps {
  open: boolean;
  onClose: () => void;
}

const LICENSE = `HarmonyOS Sans 字体授权

本应用界面使用的 HarmonyOS Sans 字体，由华为字体团队设计并开源，
遵循 SIL Open Font License, Version 1.1 授权发布。

你可以自由使用、复制、修改与分发该字体，也可以将其嵌入到
商业应用中，但必须保留版权声明与授权条款。

SIL Open Font License, Version 1.1 — 2022 年 2 月 14 日发布
https://scripts.sil.org/OFL`;

export function LicenseDialog({ open, onClose }: LicenseDialogProps) {
  return (
    <Dialog
      open={open}
      onOpenChange={(next) => !next && onClose()}
      size="lg"
      closable
      title="字体授权"
      actions={<Button variant="accent" onClick={onClose}>关闭</Button>}
    >
      <pre className="detail-text">{LICENSE}</pre>
    </Dialog>
  );
}
