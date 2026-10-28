import { useState } from 'react';
import { useTranslation } from 'react-i18next';
import { Plus } from 'lucide-react';
import { Button } from '@/components/ui/button';
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from '@/components/ui/dialog';
import { CategoryManager } from '@/components/categories/CategoryManager';

/** "Správa kategorií" of the Cashflow screen: the Settings category manager in a modal. */
export function CategoryManagerDialog({
  open,
  onOpenChange,
}: {
  open: boolean;
  onOpenChange: (open: boolean) => void;
}) {
  const { t } = useTranslation('bank_accounts');
  const { t: tc } = useTranslation('common');
  const [creating, setCreating] = useState(false);
  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="max-w-[560px]">
        <DialogHeader>
          <DialogTitle>{t('categoryManagement.title')}</DialogTitle>
          <DialogDescription>{t('categoryManagement.description')}</DialogDescription>
        </DialogHeader>
        <CategoryManager
          creating={creating}
          onCreatingChange={setCreating}
          className="max-h-[min(440px,55vh)]"
        />
        <DialogFooter>
          <Button variant="ghost" onClick={() => onOpenChange(false)}>
            {tc('buttons.close')}
          </Button>
          <Button onClick={() => setCreating(true)}>
            <Plus />
            {t('categoryManagement.add')}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
