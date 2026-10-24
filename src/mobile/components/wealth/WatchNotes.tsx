import { useState, type ReactNode } from 'react';
import { useTranslation } from 'react-i18next';
import ReactMarkdown, { type Components } from 'react-markdown';
import remarkGfm from 'remark-gfm';
import { openUrl } from '@tauri-apps/plugin-opener';
import { toast } from 'sonner';
import { Button } from '@/components/ui/button';
import { cn } from '@/lib/utils';
import { FOOTER_GHOST, FOOTER_PRIMARY } from '@/mobile/components/import/styles';

/** The paper card of the notes (README §13, `.m-notes`): the empty text and the rendered markdown sit in it. */
const CARD = 'mb-3.5 rounded-r4 border border-line bg-paper px-4 py-3.5 shadow-e1';

/**
 * The notes' typography on tokens (README §13, `.m-notes`): body text 15 `ink-2`, the headings in
 * `ink` at the section and row sizes, lists, links, inline code and code blocks on `well`, a quote
 * with a hairline at its left, tables that scroll inside the card. Descendant selectors, because
 * the markdown renders elements without classes.
 */
const PROSE =
  'text-m-body text-ink-2 [overflow-wrap:anywhere] ' +
  '[&>*:first-child]:mt-0 [&>*:last-child]:mb-0 ' +
  '[&_p]:mb-2.5 [&_p]:mt-0 ' +
  '[&_h1]:mb-1.5 [&_h1]:mt-3 [&_h1]:text-m-section [&_h1]:text-ink ' +
  '[&_h2]:mb-1.5 [&_h2]:mt-3 [&_h2]:text-m-row [&_h2]:text-ink ' +
  '[&_h3]:mb-1.5 [&_h3]:mt-3 [&_h3]:text-m-body [&_h3]:font-semibold [&_h3]:text-ink ' +
  '[&_h4]:mb-1.5 [&_h4]:mt-3 [&_h4]:text-m-body [&_h4]:font-semibold [&_h4]:text-ink ' +
  '[&_ul]:mb-2.5 [&_ul]:mt-0 [&_ul]:list-disc [&_ul]:pl-5 [&_ol]:mb-2.5 [&_ol]:mt-0 [&_ol]:list-decimal [&_ol]:pl-5 ' +
  '[&_li]:mb-[3px] [&_li>ul]:mt-[3px] [&_li>ol]:mt-[3px] ' +
  '[&_strong]:font-semibold [&_strong]:text-ink [&_del]:text-ink-3 ' +
  '[&_a]:text-ink [&_a]:underline [&_a]:underline-offset-[3px] ' +
  '[&_code]:rounded-[6px] [&_code]:bg-well-2 [&_code]:px-[5px] [&_code]:py-px [&_code]:font-mono [&_code]:text-[0.9em] ' +
  '[&_pre]:mb-2.5 [&_pre]:mt-0 [&_pre]:overflow-x-auto [&_pre]:rounded-[10px] [&_pre]:bg-well [&_pre]:px-3 [&_pre]:py-2.5 ' +
  '[&_pre_code]:bg-transparent [&_pre_code]:p-0 [&_pre_code]:text-m-sub [&_pre_code]:[overflow-wrap:normal] ' +
  '[&_blockquote]:mb-2.5 [&_blockquote]:mt-0 [&_blockquote]:border-l-[3px] [&_blockquote]:border-line-strong [&_blockquote]:pl-3 [&_blockquote]:text-ink-3 ' +
  '[&_hr]:my-3 [&_hr]:border-0 [&_hr]:border-t [&_hr]:border-line ' +
  '[&_table]:mb-2.5 [&_table]:block [&_table]:max-w-full [&_table]:overflow-x-auto [&_table]:border-collapse [&_table]:text-m-sub ' +
  '[&_th]:border-b [&_th]:border-line [&_th]:px-2 [&_th]:py-1 [&_th]:text-left [&_th]:font-semibold [&_th]:text-ink ' +
  '[&_td]:border-b [&_td]:border-line-soft [&_td]:px-2 [&_td]:py-1 ' +
  '[&_img]:h-auto [&_img]:max-w-full';

/** What a link of the notes may open: the system browser, a mail or a call — never the app's own webview. */
const OPENABLE = /^(https?:|mailto:|tel:)/i;

/**
 * A link of the notes. A plain `<a>` would navigate the app's webview away from Moony, with no way
 * back, so a web, mail or phone link is handed to the system (`openUrl`, as the export guide's
 * button does) and anything else (a relative path, a fragment) is just text.
 */
function NoteLink({ href, children }: { href?: string; children?: ReactNode }) {
  const { t } = useTranslation('mobile');
  const { t: tc } = useTranslation('common');
  if (!href || !OPENABLE.test(href)) return <>{children}</>;
  return (
    <a
      href={href}
      onClick={(event) => {
        event.preventDefault();
        openUrl(href).catch(() => {
          toast.error(tc('status.error'), { description: t('import.guide.openFailed') });
        });
      }}
    >
      {children}
    </a>
  );
}

const MARKDOWN_COMPONENTS: Components = {
  a: ({ href, children }) => <NoteLink href={href}>{children}</NoteLink>,
};

interface WatchNotesProps {
  /** The notes as saved, markdown. */
  notes: string;
  /** The save is running. */
  saving: boolean;
  /** Saves the notes; a rejection (already toasted by the mutation) keeps the editor open. */
  onSave: (notes: string) => Promise<unknown>;
}

/**
 * "Poznámky" of a followed stock (spec §8.8): the notes are rendered as markdown in a paper card
 * (the desktop's `MarkdownView` rules: GitHub flavour; the empty sentence says "tap", not the desktop's "click") and
 * "Upravit" in the section head turns the card into a plain text area — body type, eight rows —
 * with Zrušit and Uložit under it. There is no toolbar and no preview on the phone: markdown is
 * typed as text and read in view mode.
 */
export function WatchNotes({ notes, saving, onSave }: WatchNotesProps) {
  const { t } = useTranslation('stockMonitor');
  const { t: tp } = useTranslation('mobile');
  const [editing, setEditing] = useState(false);
  const [draft, setDraft] = useState(notes);

  const startEditing = () => {
    setDraft(notes);
    setEditing(true);
  };
  const save = async () => {
    try {
      await onSave(draft);
      setEditing(false);
    } catch {
      // The mutation has toasted the error; the draft stays
    }
  };

  return (
    <section aria-label={t('notes.title')}>
      {/* The section head of `SectionHead`, whose action is a link: this one switches the card to editing */}
      <div className="mx-0.5 mb-2 mt-4 flex items-baseline justify-between gap-3">
        <h2 className="m-0 min-w-0 truncate text-m-section text-ink">{t('notes.title')}</h2>
        {!editing && (
          <button
            type="button"
            onClick={startEditing}
            className="relative flex-none whitespace-nowrap text-m-sub font-semibold text-ink-3 [-webkit-touch-callout:none] after:absolute after:-inset-x-2 after:-inset-y-[14px] after:content-[''] active:text-ink focus-visible:outline-none focus-visible:shadow-focus"
          >
            {t('notes.edit')}
          </button>
        )}
      </div>
      {editing ? (
        <div className="mb-3.5">
          <textarea
            value={draft}
            onChange={(event) => setDraft(event.target.value)}
            rows={8}
            aria-label={t('notes.title')}
            placeholder={t('notes.placeholder')}
            autoCapitalize="sentences"
            className="m-0 block w-full resize-none rounded-[10px] border border-line-strong bg-paper px-3.5 py-3 text-m-body text-ink transition-[border-color,box-shadow] duration-fast placeholder:text-ink-5 focus:border-focus focus:shadow-focus focus-visible:outline-none"
          />
          <div className="mt-2.5 flex items-center gap-2.5">
            <Button
              type="button"
              variant="ghost"
              className={FOOTER_GHOST}
              onClick={() => setEditing(false)}
            >
              {t('notes.cancel')}
            </Button>
            <Button
              type="button"
              className={FOOTER_PRIMARY}
              loading={saving}
              onClick={() => void save()}
            >
              {t('notes.save')}
            </Button>
          </div>
        </div>
      ) : notes.trim() === '' ? (
        <p className={cn(CARD, 'm-0 text-m-sub leading-[1.5] text-ink-3')}>
          {tp('wealth.watch.notesEmpty')}
        </p>
      ) : (
        <div className={cn(CARD, PROSE)}>
          <ReactMarkdown remarkPlugins={[remarkGfm]} components={MARKDOWN_COMPONENTS}>
            {notes}
          </ReactMarkdown>
        </div>
      )}
    </section>
  );
}
