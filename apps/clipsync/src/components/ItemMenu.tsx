import { Button, DropdownMenu, MenuItem, MenuSeparator } from 'oink-ui';
import { ClipIcon } from './ClipIcon';
import type { ClipRecord } from '../lib/types';

export interface ItemMenuActions {
  onEdit: (item: ClipRecord) => void;
  onNote: (item: ClipRecord) => void;
  onTransform: (item: ClipRecord) => void;
  onTranslate: (item: ClipRecord) => void;
  onDuplicate: (item: ClipRecord) => void;
  onSaveText: (item: ClipRecord) => void;
  onSaveFile: (item: ClipRecord) => void;
  onResend: (item: ClipRecord) => void;
  onDownload: (item: ClipRecord) => void;
  onCopyImage: (item: ClipRecord) => void;
  onOpen: (item: ClipRecord) => void;
  onSend: (item: ClipRecord) => void;
  onExport: (item: ClipRecord) => void;
  onDetail: (item: ClipRecord) => void;
  onQrCode: (item: ClipRecord) => void;
  onMoveUp: (item: ClipRecord) => void;
  onMoveDown: (item: ClipRecord) => void;
  onMoveTop: (item: ClipRecord) => void;
  onMoveBottom: (item: ClipRecord) => void;
  onMoveTo: (item: ClipRecord) => void;
  onDelete: (item: ClipRecord) => void;
}

interface ItemMenuProps {
  item: ClipRecord;
  actions: ItemMenuActions;
}

export function ItemMenu({ item, actions }: ItemMenuProps) {
  const isText = item.kind === 'text';
  const hasFile = item.kind === 'file' && Boolean(item.file);
  const hasLink = hasFile && Boolean(item.file?.link);
  const isImage = hasFile && (item.file?.mime.startsWith('image/') ?? false);
  const isCached = hasFile && Boolean(item.file?.local_path);

  return (
    <DropdownMenu
      trigger={
        <Button iconOnly aria-label="更多操作" variant="ghost" size="sm" leading={<ClipIcon name="more" />} />
      }
      placement="bottom-end"
    >
      <MenuItem icon={<ClipIcon name="edit" />} disabled={!isText} onSelect={() => actions.onEdit(item)}>编辑内容</MenuItem>
      <MenuItem icon={<ClipIcon name="note" />} onSelect={() => actions.onNote(item)}>写备注</MenuItem>
      <MenuItem icon="sparkle" disabled={!isText} onSelect={() => actions.onTransform(item)}>文本处理</MenuItem>
      <MenuItem icon={<ClipIcon name="translate" />} disabled={!isText} onSelect={() => actions.onTranslate(item)}>中英互译</MenuItem>
      <MenuItem icon={<ClipIcon name="duplicate" />} onSelect={() => actions.onDuplicate(item)}>生成副本</MenuItem>
      <MenuItem icon={<ClipIcon name="send" />} onSelect={() => actions.onSend(item)}>发送到房间</MenuItem>
      <MenuSeparator />
      <MenuItem icon={<ClipIcon name="download" />} disabled={!isText} onSelect={() => actions.onSaveText(item)}>存为 txt</MenuItem>
      <MenuItem icon={<ClipIcon name="export" />} disabled={!hasFile} onSelect={() => actions.onSaveFile(item)}>存到下载目录</MenuItem>
      <MenuItem icon={<ClipIcon name="export" />} disabled={!hasFile || hasLink} onSelect={() => actions.onResend(item)}>补传到房间</MenuItem>
      <MenuItem icon={<ClipIcon name="download" />} disabled={!hasLink} onSelect={() => actions.onDownload(item)}>下载文件</MenuItem>
      <MenuItem icon={<ClipIcon name="open-in-new" />} disabled={!isCached} onSelect={() => actions.onOpen(item)}>打开文件</MenuItem>
      <MenuItem icon="image" disabled={!isImage || !isCached} onSelect={() => actions.onCopyImage(item)}>复制图片</MenuItem>
      <MenuSeparator />
      <MenuItem icon={<ClipIcon name="info" />} onSelect={() => actions.onDetail(item)}>查看详情</MenuItem>
      <MenuItem icon={<ClipIcon name="export" />} onSelect={() => actions.onExport(item)}>导出为 JSON</MenuItem>
      <MenuItem icon="grid" onSelect={() => actions.onQrCode(item)}>二维码</MenuItem>
      <MenuSeparator />
      <MenuItem icon={<ClipIcon name="arrow-up" />} onSelect={() => actions.onMoveUp(item)}>向上移</MenuItem>
      <MenuItem icon={<ClipIcon name="arrow-down" />} onSelect={() => actions.onMoveDown(item)}>向下移</MenuItem>
      <MenuItem icon={<ClipIcon name="top" />} onSelect={() => actions.onMoveTop(item)}>移到最顶部</MenuItem>
      <MenuItem icon={<ClipIcon name="bottom" />} onSelect={() => actions.onMoveBottom(item)}>移到最底部</MenuItem>
      <MenuItem icon="command" onSelect={() => actions.onMoveTo(item)}>移动到指定位置…</MenuItem>
      <MenuSeparator />
      <MenuItem icon={<ClipIcon name="trash" />} tone="danger" onSelect={() => actions.onDelete(item)}>删除</MenuItem>
    </DropdownMenu>
  );
}
