import { Button, Dialog, Text } from 'oink-ui';

export interface ChoiceOption<T extends string> {
  value: T;
  label: string;
  description?: string;
}

interface ChoiceDialogProps<T extends string> {
  open: boolean;
  title: string;
  options: ChoiceOption<T>[];
  value: T;
  onSelect: (value: T) => void;
  onClose: () => void;
}

export function ChoiceDialog<T extends string>({
  open,
  title,
  options,
  value,
  onSelect,
  onClose,
}: ChoiceDialogProps<T>) {
  return (
    <Dialog
      open={open}
      onOpenChange={(next) => !next && onClose()}
      size="sm"
      closable
      title={title}
      description="选一个就定下来了"
      actions={
        <Button variant="ghost" onClick={onClose}>取消</Button>
      }
    >
      <div className="choice">
        {options.map((option) => (
          <button
            key={option.value}
            type="button"
            className="choice__item"
            data-active={option.value === value}
            onClick={() => {
              onSelect(option.value);
              onClose();
            }}
          >
            <span className="choice__mark" aria-hidden="true" />
            <span className="choice__text">
              <Text variant="label">{option.label}</Text>
              {option.description && (
                <Text variant="micro" tone="subtle">{option.description}</Text>
              )}
            </span>
          </button>
        ))}
      </div>
    </Dialog>
  );
}
