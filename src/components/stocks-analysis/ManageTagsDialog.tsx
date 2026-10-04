import { useState } from 'react';
import { useMutation, useQueryClient } from '@tanstack/react-query';
import { useTranslation } from 'react-i18next';
import { toast } from 'sonner';
import { ChevronDown, Trash2 } from 'lucide-react';
import type { StockTag, StockTagGroup } from '@shared/schema';
import { stockTagsApi } from '@/lib/tauri-api';
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from '@/components/ui/dialog';
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuLabel,
  DropdownMenuSeparator,
  DropdownMenuTrigger,
} from '@/components/ui/dropdown-menu';
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from '@/components/ui/select';
import { Input } from '@/components/ui/input';
import { Label } from '@/components/ui/label';
import { Button } from '@/components/ui/button';
import { Chip } from '@/components/ui/chip';
import { ConfirmDeleteDialog } from '@/components/common/ConfirmDeleteDialog';

const NO_GROUP = '__none__';
const NEW_GROUP = '__new__';

interface ManageTagsDialogProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  tags: StockTag[];
  groups: StockTagGroup[];
}

/**
 * "Spravovat štítky" (prototype `stocks-analysis.html`): tags listed by
 * group as chips — each chip opens a menu to move it to another group or
 * delete it — and a form that creates a tag in an existing or a new group.
 */
export function ManageTagsDialog({ open, onOpenChange, tags, groups }: ManageTagsDialogProps) {
  const { t } = useTranslation('reports');
  const { t: tc } = useTranslation('common');
  const queryClient = useQueryClient();
  const [name, setName] = useState('');
  const [groupChoice, setGroupChoice] = useState<string>(groups[0]?.id ?? NO_GROUP);
  // The stored choice may point at a group that is gone (or at nothing while the
  // list refetches); fall back to the first group so the select never goes blank.
  const choice =
    groupChoice === NO_GROUP ||
    groupChoice === NEW_GROUP ||
    groups.some((g) => g.id === groupChoice)
      ? groupChoice
      : (groups[0]?.id ?? NO_GROUP);
  const [newGroupName, setNewGroupName] = useState('');
  const [pendingDelete, setPendingDelete] = useState<
    { kind: 'tag'; tag: StockTag } | { kind: 'group'; group: StockTagGroup } | null
  >(null);

  const invalidate = () => {
    queryClient.invalidateQueries({ queryKey: ['stock-tags'] });
    queryClient.invalidateQueries({ queryKey: ['stock-tag-groups'] });
    queryClient.invalidateQueries({ queryKey: ['stocks-analysis'] });
    queryClient.invalidateQueries({ queryKey: ['tag-metrics'] });
    queryClient.invalidateQueries({ queryKey: ['stock-twr'] });
  };
  const fail = (error: Error) => toast.error(tc('status.error'), { description: error.message });

  const createMutation = useMutation({
    mutationFn: async () => {
      let groupId: string | null = choice === NO_GROUP ? null : choice;
      if (choice === NEW_GROUP) {
        const group = await stockTagsApi.createGroup({ name: newGroupName.trim() });
        groupId = group.id;
      }
      // Colors come from the series order on the page; nothing to store
      return stockTagsApi.create({ name: name.trim(), color: null, groupId });
    },
    onSuccess: (tag) => {
      invalidate();
      setName('');
      setNewGroupName('');
      if (tag.groupId) setGroupChoice(tag.groupId);
      toast.success(t('stocksAnalysis.manage.created', { name: tag.name }));
    },
    onError: fail,
  });

  const moveMutation = useMutation({
    mutationFn: ({ tag, groupId }: { tag: StockTag; groupId: string | null }) =>
      stockTagsApi.update(tag.id, { name: tag.name, color: tag.color, groupId }),
    onSuccess: () => {
      invalidate();
      toast.success(t('stocksAnalysis.manage.moved'));
    },
    onError: fail,
  });

  const deleteMutation = useMutation({
    mutationFn: async (target: NonNullable<typeof pendingDelete>) => {
      if (target.kind === 'tag') await stockTagsApi.delete(target.tag.id);
      else await stockTagsApi.deleteGroup(target.group.id);
    },
    onSuccess: (_, target) => {
      invalidate();
      setPendingDelete(null);
      toast.success(
        target.kind === 'tag'
          ? t('stocksAnalysis.manage.tagDeleted')
          : t('stocksAnalysis.manage.groupDeleted')
      );
    },
    onError: fail,
  });

  const canCreate =
    name.trim().length > 0 && (choice !== NEW_GROUP || newGroupName.trim().length > 0);

  const sections: { group: StockTagGroup | null; tags: StockTag[] }[] = [
    ...groups.map((group) => ({ group, tags: tags.filter((x) => x.groupId === group.id) })),
  ];
  const ungrouped = tags.filter((x) => !x.groupId || !groups.some((g) => g.id === x.groupId));
  if (ungrouped.length > 0) sections.push({ group: null, tags: ungrouped });

  const tagChip = (tag: StockTag) => (
    <DropdownMenu key={tag.id}>
      <DropdownMenuTrigger asChild>
        <Chip aria-label={`${tag.name} · ${tc('labels.moreActions')}`}>
          {tag.name}
          <ChevronDown className="text-ink-4" />
        </Chip>
      </DropdownMenuTrigger>
      <DropdownMenuContent align="start">
        <DropdownMenuLabel>{t('stocksAnalysis.manage.moveTo')}</DropdownMenuLabel>
        {groups
          .filter((g) => g.id !== tag.groupId)
          .map((g) => (
            <DropdownMenuItem
              key={g.id}
              onSelect={() => moveMutation.mutate({ tag, groupId: g.id })}
            >
              {g.name}
            </DropdownMenuItem>
          ))}
        {tag.groupId && (
          <DropdownMenuItem onSelect={() => moveMutation.mutate({ tag, groupId: null })}>
            {t('stocksAnalysis.manage.noGroup')}
          </DropdownMenuItem>
        )}
        <DropdownMenuSeparator />
        <DropdownMenuItem variant="danger" onSelect={() => setPendingDelete({ kind: 'tag', tag })}>
          {t('stocksAnalysis.manage.deleteTag')}
        </DropdownMenuItem>
      </DropdownMenuContent>
    </DropdownMenu>
  );

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent>
        <DialogHeader>
          <DialogTitle>{t('stocksAnalysis.manage.title')}</DialogTitle>
          <DialogDescription>{t('stocksAnalysis.manage.description')}</DialogDescription>
        </DialogHeader>

        <div>
          {sections.length === 0 && (
            <p className="py-3 text-table text-ink-3">{t('stocksAnalysis.manage.empty')}</p>
          )}
          {sections.map(({ group, tags: groupTags }) => (
            <div
              key={group?.id ?? NO_GROUP}
              className="border-b border-line-soft py-3 first:pt-0 last:border-b-0"
            >
              <h4 className="mb-2 flex items-center justify-between text-caption font-650 text-ink">
                <span>
                  {group ? group.name : t('stocksAnalysis.manage.noGroup')}{' '}
                  <small className="font-500 text-ink-4">
                    {t('stocksAnalysis.manage.tagCount', { count: groupTags.length })}
                  </small>
                </span>
                {group && (
                  <Button
                    variant="ghost"
                    size="icon-sm"
                    aria-label={t('stocksAnalysis.manage.deleteGroup')}
                    onClick={() => setPendingDelete({ kind: 'group', group })}
                  >
                    <Trash2 />
                  </Button>
                )}
              </h4>
              <div className="flex flex-wrap gap-1.5">
                {groupTags.length > 0 ? (
                  groupTags.map(tagChip)
                ) : (
                  <span className="text-micro font-500 text-ink-4">
                    {t('stocksAnalysis.manage.noTagsInGroup')}
                  </span>
                )}
              </div>
            </div>
          ))}
        </div>

        <form
          className="mt-2 grid grid-cols-2 gap-3"
          onSubmit={(e) => {
            e.preventDefault();
            if (canCreate) createMutation.mutate();
          }}
        >
          <div className="grid gap-1.5">
            <Label htmlFor="new-tag-name">{t('stocksAnalysis.manage.newTag')}</Label>
            <Input
              id="new-tag-name"
              value={name}
              onChange={(e) => setName(e.target.value)}
              placeholder={t('stocksAnalysis.manage.newTagPlaceholder')}
            />
          </div>
          <div className="grid gap-1.5">
            <Label htmlFor="new-tag-group">{t('stocksAnalysis.manage.group')}</Label>
            <Select value={choice} onValueChange={(v) => v && setGroupChoice(v)}>
              <SelectTrigger id="new-tag-group">
                <SelectValue />
              </SelectTrigger>
              <SelectContent>
                {groups.map((g) => (
                  <SelectItem key={g.id} value={g.id}>
                    {g.name}
                  </SelectItem>
                ))}
                <SelectItem value={NO_GROUP}>{t('stocksAnalysis.manage.noGroup')}</SelectItem>
                <SelectItem value={NEW_GROUP}>{t('stocksAnalysis.manage.newGroup')}</SelectItem>
              </SelectContent>
            </Select>
          </div>
          {choice === NEW_GROUP && (
            <div className="col-span-2 grid gap-1.5">
              <Label htmlFor="new-group-name">{t('stocksAnalysis.manage.newGroupName')}</Label>
              <Input
                id="new-group-name"
                value={newGroupName}
                onChange={(e) => setNewGroupName(e.target.value)}
                placeholder={t('stocksAnalysis.manage.newGroupPlaceholder')}
              />
            </div>
          )}
          <DialogFooter className="col-span-2 mt-3">
            <Button type="button" variant="ghost" onClick={() => onOpenChange(false)}>
              {tc('buttons.close')}
            </Button>
            <Button type="submit" disabled={!canCreate} loading={createMutation.isPending}>
              {t('stocksAnalysis.manage.create')}
            </Button>
          </DialogFooter>
        </form>

        <ConfirmDeleteDialog
          open={pendingDelete !== null}
          onOpenChange={(o) => !o && setPendingDelete(null)}
          title={
            pendingDelete?.kind === 'group'
              ? t('stocksAnalysis.manage.deleteGroupTitle', { name: pendingDelete.group.name })
              : t('stocksAnalysis.manage.deleteTagTitle', { name: pendingDelete?.tag.name ?? '' })
          }
          description={
            pendingDelete?.kind === 'group'
              ? t('stocksAnalysis.manage.deleteGroupDescription')
              : t('stocksAnalysis.manage.deleteTagDescription')
          }
          confirmLabel={tc('buttons.delete')}
          isPending={deleteMutation.isPending}
          onConfirm={() => pendingDelete && deleteMutation.mutate(pendingDelete)}
        />
      </DialogContent>
    </Dialog>
  );
}
