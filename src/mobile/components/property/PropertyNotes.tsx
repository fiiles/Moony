import { useState } from 'react';
import { useTranslation } from 'react-i18next';
import type { RealEstate } from '@shared/schema';
import { Button } from '@/components/ui/button';
import { useRealEstateMutations } from '@/hooks/use-real-estate-mutations';
import { FOOTER_GHOST, FOOTER_PRIMARY } from '@/mobile/components/import/styles';

/**
 * "Poznámky" of a property (the desktop's `RealEstateNotes`, RED-06): the text in a paper card as
 * plain text (a property's notes are not markdown), "Upravit" in the section head turns the card
 * into a text area with Zrušit and Uložit under it. The save is the desktop's `updateNotes`
 * (the property form is not needed for a note); a failed save keeps the draft open.
 */
export function PropertyNotes({ property }: { property: RealEstate }) {
  const { t } = useTranslation('realEstate');
  const { t: tc } = useTranslation('common');
  const { updateNotes } = useRealEstateMutations();
  const [editing, setEditing] = useState(false);
  const [draft, setDraft] = useState('');

  const startEditing = () => {
    setDraft(property.notes ?? '');
    setEditing(true);
  };
  const save = () =>
    updateNotes.mutate(
      { realEstate: property, notes: draft },
      { onSuccess: () => setEditing(false) }
    );

  return (
    <section aria-label={t('detail.tabs.notes')}>
      <div className="mx-0.5 mb-2 mt-4 flex items-baseline justify-between gap-3">
        <h2 className="m-0 min-w-0 truncate text-m-section text-ink">{t('detail.tabs.notes')}</h2>
        {!editing && (
          <button
            type="button"
            onClick={startEditing}
            className="relative flex-none whitespace-nowrap text-m-sub font-semibold text-ink-3 [-webkit-touch-callout:none] after:absolute after:-inset-x-2 after:-inset-y-[14px] after:content-[''] active:text-ink focus-visible:outline-none focus-visible:shadow-focus"
          >
            {tc('buttons.edit')}
          </button>
        )}
      </div>
      {editing ? (
        <div className="mb-3.5">
          <textarea
            value={draft}
            onChange={(event) => setDraft(event.target.value)}
            rows={8}
            aria-label={t('detail.notes')}
            placeholder={t('detail.notesPlaceholder')}
            autoCapitalize="sentences"
            className="m-0 block w-full resize-none rounded-[10px] border border-line-strong bg-paper px-3.5 py-3 text-m-field text-ink transition-[border-color,box-shadow] duration-fast placeholder:text-ink-5 focus:border-focus focus:shadow-focus focus-visible:outline-none"
          />
          <div className="mt-2.5 flex items-center gap-2.5">
            <Button
              type="button"
              variant="ghost"
              className={FOOTER_GHOST}
              disabled={updateNotes.isPending}
              onClick={() => setEditing(false)}
            >
              {tc('buttons.cancel')}
            </Button>
            <Button
              type="button"
              className={FOOTER_PRIMARY}
              loading={updateNotes.isPending}
              onClick={save}
            >
              {tc('buttons.save')}
            </Button>
          </div>
        </div>
      ) : property.notes ? (
        <div className="mb-3.5 rounded-r4 border border-line bg-[linear-gradient(180deg,var(--paper),var(--paper-2))] px-4 py-3.5 shadow-e1">
          <p className="m-0 whitespace-pre-wrap break-words text-m-body text-ink-2">
            {property.notes}
          </p>
        </div>
      ) : (
        <p className="mx-0.5 mb-3.5 text-m-sub text-ink-3">{t('detail.noNotes')}</p>
      )}
    </section>
  );
}
