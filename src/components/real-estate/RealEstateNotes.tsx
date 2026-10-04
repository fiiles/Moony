import { useState } from 'react';
import { useTranslation } from 'react-i18next';
import { Pencil } from 'lucide-react';
import { Button } from '@/components/ui/button';
import { Card, CardContent, CardHeader, CardTitle } from '@/components/ui/card';
import { Textarea } from '@/components/ui/textarea';
import { useRealEstateMutations } from '@/hooks/use-real-estate-mutations';
import type { RealEstate } from '@shared/schema';

/**
 * Notes tab of the property detail with an inline editor (RED-06): Edit swaps the text for a
 * textarea with Save / Cancel, so notes no longer need the full property dialog.
 */
export function RealEstateNotes({ realEstate }: { realEstate: RealEstate }) {
  const { t } = useTranslation('realEstate');
  const { t: tc } = useTranslation('common');
  const { updateNotes } = useRealEstateMutations();
  const [editing, setEditing] = useState(false);
  const [draft, setDraft] = useState('');

  const startEditing = () => {
    setDraft(realEstate.notes ?? '');
    setEditing(true);
  };

  const save = () => {
    updateNotes.mutate({ realEstate, notes: draft }, { onSuccess: () => setEditing(false) });
  };

  return (
    <Card>
      <CardHeader className="flex flex-row items-center justify-between space-y-0">
        <CardTitle>{t('detail.notes')}</CardTitle>
        {!editing && (
          <Button variant="outline" size="sm" onClick={startEditing}>
            <Pencil className="mr-2 h-4 w-4" />
            {tc('buttons.edit')}
          </Button>
        )}
      </CardHeader>
      <CardContent>
        {editing ? (
          <div className="space-y-3">
            <Textarea
              value={draft}
              onChange={(e) => setDraft(e.target.value)}
              placeholder={t('detail.notesPlaceholder')}
              aria-label={t('detail.notes')}
              rows={8}
              autoFocus
            />
            <div className="flex justify-end gap-2">
              <Button
                variant="outline"
                onClick={() => setEditing(false)}
                disabled={updateNotes.isPending}
              >
                {tc('buttons.cancel')}
              </Button>
              <Button onClick={save} disabled={updateNotes.isPending}>
                {updateNotes.isPending ? tc('status.saving') : tc('buttons.save')}
              </Button>
            </div>
          </div>
        ) : (
          <div className="whitespace-pre-wrap text-sm">
            {realEstate.notes || t('detail.noNotes')}
          </div>
        )}
      </CardContent>
    </Card>
  );
}
