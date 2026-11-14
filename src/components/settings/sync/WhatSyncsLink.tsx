import { useState } from 'react';
import { useTranslation } from 'react-i18next';
import { Button } from '@/components/ui/button';
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from '@/components/ui/dialog';

/** Settings → Sync, both states: "What syncs", the answer in a dialog. */
export function WhatSyncsLink() {
  const { t } = useTranslation('deviceSync');
  const { t: tc } = useTranslation('common');
  const [open, setOpen] = useState(false);
  return (
    <>
      <div>
        <Button type="button" variant="ghost" onClick={() => setOpen(true)}>
          {t('whatSyncs.title')}
        </Button>
      </div>
      <Dialog open={open} onOpenChange={setOpen}>
        <DialogContent className="max-w-[480px]">
          <DialogHeader>
            <DialogTitle>{t('whatSyncs.title')}</DialogTitle>
            <DialogDescription>{t('whatSyncs.body')}</DialogDescription>
          </DialogHeader>
          <DialogFooter>
            <Button type="button" onClick={() => setOpen(false)}>
              {tc('buttons.close')}
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>
    </>
  );
}
