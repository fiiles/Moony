import { useRef, useState } from 'react';
import { useTranslation } from 'react-i18next';
import ReactMarkdown from 'react-markdown';
import remarkGfm from 'remark-gfm';
import { Bold, Heading2, Italic, Link as LinkIcon, List, ListOrdered, Pencil } from 'lucide-react';
import { Button } from '@/components/ui/button';
import { Card, CardDescription, CardHeader, CardTitle } from '@/components/ui/card';
import { Tabs, TabsContent, TabsList, TabsTrigger } from '@/components/ui/tabs';
import { Textarea } from '@/components/ui/textarea';

interface StockNotesCardProps {
  notes: string;
  onSave: (notes: string) => Promise<unknown>;
  saving: boolean;
}

/** Wrap the current selection of the textarea with prefix/suffix markdown */
function wrapSelection(
  textarea: HTMLTextAreaElement,
  prefix: string,
  suffix: string,
  setDraft: (v: string) => void
) {
  const { selectionStart, selectionEnd, value } = textarea;
  const selected = value.slice(selectionStart, selectionEnd);
  const next =
    value.slice(0, selectionStart) + prefix + selected + suffix + value.slice(selectionEnd);
  setDraft(next);
  requestAnimationFrame(() => {
    textarea.focus();
    textarea.setSelectionRange(selectionStart + prefix.length, selectionEnd + prefix.length);
  });
}

/** Prefix each selected line (headings, lists) */
function prefixLines(
  textarea: HTMLTextAreaElement,
  linePrefix: string,
  setDraft: (v: string) => void
) {
  const { selectionStart, selectionEnd, value } = textarea;
  const lineStart = value.lastIndexOf('\n', selectionStart - 1) + 1;
  const block = value.slice(lineStart, selectionEnd);
  const prefixed = block
    .split('\n')
    .map((line) => linePrefix + line)
    .join('\n');
  setDraft(value.slice(0, lineStart) + prefixed + value.slice(selectionEnd));
  requestAnimationFrame(() => textarea.focus());
}

function MarkdownView({ markdown, emptyText }: { markdown: string; emptyText: string }) {
  if (!markdown.trim()) {
    return <p className="text-table text-ink-3">{emptyText}</p>;
  }
  // Design-system prose (moony.css `.prose`): 13 px, 1.55, headings 650 in ink
  return (
    <div className="max-w-none text-body leading-[1.55] text-ink-2 [&_a]:text-ink [&_a]:underline [&_a]:underline-offset-[3px] [&_h1]:mb-1.5 [&_h1]:text-[15px] [&_h1]:font-650 [&_h1]:text-ink [&_h2]:mb-1.5 [&_h2]:text-[14px] [&_h2]:font-650 [&_h2]:text-ink [&_h3]:mb-1.5 [&_h3]:text-[13px] [&_h3]:font-650 [&_h3]:text-ink [&_h4]:mb-1.5 [&_h4]:text-[13px] [&_h4]:font-650 [&_h4]:text-ink [&_li]:mb-[3px] [&_ol]:my-1.5 [&_ol]:mb-2.5 [&_ol]:pl-[18px] [&_p]:mb-2.5 [&_strong]:font-650 [&_strong]:text-ink [&_ul]:my-1.5 [&_ul]:mb-2.5 [&_ul]:list-disc [&_ul]:pl-[18px]">
      <ReactMarkdown remarkPlugins={[remarkGfm]}>{markdown}</ReactMarkdown>
    </div>
  );
}

export function StockNotesCard({ notes, onSave, saving }: StockNotesCardProps) {
  const { t } = useTranslation('stockMonitor');
  const [editing, setEditing] = useState(false);
  const [draft, setDraft] = useState(notes);
  const [tab, setTab] = useState<'write' | 'preview'>('write');
  const textareaRef = useRef<HTMLTextAreaElement>(null);

  const startEditing = () => {
    setDraft(notes);
    setTab('write');
    setEditing(true);
  };

  const toolbar = [
    { icon: Bold, label: t('notes.toolbar.bold'), action: () => wrap('**', '**') },
    { icon: Italic, label: t('notes.toolbar.italic'), action: () => wrap('*', '*') },
    { icon: Heading2, label: t('notes.toolbar.heading'), action: () => lines('## ') },
    { icon: List, label: t('notes.toolbar.list'), action: () => lines('- ') },
    { icon: ListOrdered, label: t('notes.toolbar.orderedList'), action: () => lines('1. ') },
    { icon: LinkIcon, label: t('notes.toolbar.link'), action: () => wrap('[', '](url)') },
  ];

  function wrap(prefix: string, suffix: string) {
    if (textareaRef.current) wrapSelection(textareaRef.current, prefix, suffix, setDraft);
  }
  function lines(prefix: string) {
    if (textareaRef.current) prefixLines(textareaRef.current, prefix, setDraft);
  }

  const paragraphs = notes.split(/\n\s*\n/).filter((p) => p.trim().length > 0).length;

  return (
    <Card data-testid="stock-notes-card">
      <CardHeader>
        <div>
          <CardTitle>{t('notes.title')}</CardTitle>
          <CardDescription>{t('notes.sub')}</CardDescription>
        </div>
        {!editing && (
          <Button variant="outline" size="sm" onClick={startEditing} data-testid="edit-notes">
            <Pencil />
            {t('notes.edit')}
          </Button>
        )}
      </CardHeader>
      <div className="px-[21px] pb-5">
        {!editing ? (
          <>
            <MarkdownView markdown={notes} emptyText={t('notes.empty')} />
            {notes.trim().length > 0 && (
              <div className="mt-[14px] flex items-center justify-between border-t border-line-soft pt-3 text-micro font-500 text-ink-4">
                <span>{t('notes.paragraphs', { count: paragraphs })}</span>
                <span>{t('notes.markdownHint')}</span>
              </div>
            )}
          </>
        ) : (
          <Tabs value={tab} onValueChange={(v) => setTab(v as 'write' | 'preview')}>
            <div className="flex flex-wrap items-center justify-between gap-2">
              <TabsList>
                <TabsTrigger value="write">{t('notes.write')}</TabsTrigger>
                <TabsTrigger value="preview">{t('notes.preview')}</TabsTrigger>
              </TabsList>
              <div className="flex gap-0.5">
                {toolbar.map(({ icon: Icon, label, action }) => (
                  <Button
                    key={label}
                    variant="ghost"
                    size="icon-sm"
                    aria-label={label}
                    title={label}
                    onClick={action}
                    disabled={tab !== 'write'}
                  >
                    <Icon />
                  </Button>
                ))}
              </div>
            </div>
            <TabsContent value="write" className="mt-3">
              <Textarea
                ref={textareaRef}
                value={draft}
                onChange={(e) => setDraft(e.target.value)}
                placeholder={t('notes.placeholder')}
                className="min-h-48 font-mono text-caption leading-[1.55]"
                data-testid="notes-textarea"
              />
            </TabsContent>
            <TabsContent value="preview" className="mt-3">
              <div className="min-h-48 rounded-r2 border border-line bg-well px-3 py-2.5">
                <MarkdownView markdown={draft} emptyText={t('notes.empty')} />
              </div>
            </TabsContent>
            <div className="mt-3 flex justify-end gap-2">
              <Button variant="ghost" size="sm" onClick={() => setEditing(false)}>
                {t('notes.cancel')}
              </Button>
              <Button
                size="sm"
                loading={saving}
                onClick={async () => {
                  try {
                    await onSave(draft);
                    setEditing(false);
                  } catch {
                    // keep editing; error already toasted by the mutation hook
                  }
                }}
                data-testid="save-notes"
              >
                {t('notes.save')}
              </Button>
            </div>
          </Tabs>
        )}
      </div>
    </Card>
  );
}
