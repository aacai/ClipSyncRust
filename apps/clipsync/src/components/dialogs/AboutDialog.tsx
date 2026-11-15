import { Button, Dialog, Text } from 'oink-ui';

interface AboutDialogProps {
  open: boolean;
  onClose: () => void;
  onOpenLicense: () => void;
}

export function AboutDialog({ open, onClose, onOpenLicense }: AboutDialogProps) {
  return (
    <Dialog
      open={open}
      onOpenChange={(next) => !next && onClose()}
      size="md"
      closable
      title="关于 ClipSync"
      description="跨设备的剪贴板同步，端到端加密"
      actions={
        <>
          <Button variant="ghost" onClick={onOpenLicense}>字体授权</Button>
          <Button variant="accent" onClick={onClose}>关闭</Button>
        </>
      }
    >
      <div className="about">
        <span className="about__mark" aria-hidden="true">C</span>
        <Text variant="caption" tone="subtle">版本 0.0.10</Text>
        <Text variant="caption" tone="subtle">
          房间里的内容只在设备之间直传，第三方只能看到密文。
        </Text>
      </div>
    </Dialog>
  );
}
