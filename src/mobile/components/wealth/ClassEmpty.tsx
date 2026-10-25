import type { ReactNode } from 'react';
import { Plus } from 'lucide-react';
import { Button } from '@/components/ui/button';
import { ScaledEmptyState } from '@/mobile/components/Scaled';

interface ClassEmptyProps {
  icon: ReactNode;
  title: string;
  description: string;
  /** The button's words ("Přidat investici"). */
  addLabel: string;
  onAdd: () => void;
}

/** Nothing owned: the empty state of a class, with the button that adds the first position. */
export function ClassEmpty({ icon, title, description, addLabel, onAdd }: ClassEmptyProps) {
  return (
    <ScaledEmptyState
      icon={icon}
      title={title}
      description={description}
      className="px-5 py-10"
      action={
        <Button
          className="h-auto min-h-12 w-full whitespace-normal rounded-[10px] py-3 text-m-row"
          onClick={onAdd}
        >
          <Plus aria-hidden />
          {addLabel}
        </Button>
      }
    />
  );
}
