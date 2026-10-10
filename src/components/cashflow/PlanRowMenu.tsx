import { useTranslation } from 'react-i18next';
import { useLocation } from 'wouter';
import { ArrowUpRight, Check, Ellipsis, EyeOff, Pencil, Plus, Trash2 } from 'lucide-react';
import { Button } from '@/components/ui/button';
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuSeparator,
  DropdownMenuTrigger,
} from '@/components/ui/dropdown-menu';

export type PlanRowKind = 'category' | 'auto' | 'manual';

export interface PlanModuleLink {
  label: string;
  url: string;
}

interface PlanRowMenuProps {
  name: string;
  kind: PlanRowKind;
  included: boolean;
  /** For item rows: whether the whole category is in the plan. */
  categoryIncluded?: boolean;
  module?: PlanModuleLink;
  onToggleInclusion: () => void;
  onReplace?: () => void;
  onEdit?: () => void;
  onDelete?: () => void;
  onAdd?: () => void;
}

/**
 * The "···" menu of a planning row (design system README §11, cashflow planning):
 * one affordance for every row kind. Clicks and keys inside the menu must not
 * reach the group row, which toggles on click / Enter / Space.
 */
export function PlanRowMenu({
  name,
  kind,
  included,
  categoryIncluded = true,
  module,
  onToggleInclusion,
  onReplace,
  onEdit,
  onDelete,
  onAdd,
}: PlanRowMenuProps) {
  const { t } = useTranslation('reports');
  const [, navigate] = useLocation();
  const toggleLabel =
    kind === 'category'
      ? t(included ? 'planning.menu.excludeCategory' : 'planning.menu.includeCategory')
      : t(included ? 'planning.menu.exclude' : 'planning.menu.include');
  return (
    <DropdownMenu>
      <DropdownMenuTrigger asChild>
        <Button
          variant="ghost"
          size="icon-sm"
          aria-label={t('planning.menu.label', { name })}
          title={t('planning.menu.label', { name })}
          className="shrink-0 text-ink-4 hover:text-ink data-[state=open]:text-ink"
          onClick={(e) => e.stopPropagation()}
          onKeyDown={(e) => e.stopPropagation()}
        >
          <Ellipsis />
        </Button>
      </DropdownMenuTrigger>
      <DropdownMenuContent
        align="end"
        onClick={(e) => e.stopPropagation()}
        onKeyDown={(e) => e.stopPropagation()}
      >
        {kind !== 'category' && !categoryIncluded ? (
          <DropdownMenuItem disabled>
            <EyeOff />
            {t('planning.menu.categoryExcluded')}
          </DropdownMenuItem>
        ) : (
          <DropdownMenuItem onSelect={onToggleInclusion}>
            {included ? <EyeOff /> : <Check />}
            {toggleLabel}
          </DropdownMenuItem>
        )}
        {kind === 'category' && onAdd && (
          <DropdownMenuItem onSelect={onAdd}>
            <Plus />
            {t('planning.table.addToCategory')}
          </DropdownMenuItem>
        )}
        {kind === 'auto' && onReplace && (
          <DropdownMenuItem onSelect={onReplace}>
            <Pencil />
            {t('planning.menu.replace')}
          </DropdownMenuItem>
        )}
        {kind === 'auto' && module && (
          <DropdownMenuItem onSelect={() => navigate(module.url)}>
            <ArrowUpRight />
            {t('planning.menu.openModule', { module: module.label })}
          </DropdownMenuItem>
        )}
        {kind === 'manual' && onEdit && (
          <DropdownMenuItem onSelect={onEdit}>
            <Pencil />
            {t('addItem.editTitle')}
          </DropdownMenuItem>
        )}
        {kind === 'manual' && onDelete && (
          <>
            <DropdownMenuSeparator />
            <DropdownMenuItem variant="danger" onSelect={onDelete}>
              <Trash2 />
              {t('planning.deleteItem')}
            </DropdownMenuItem>
          </>
        )}
      </DropdownMenuContent>
    </DropdownMenu>
  );
}
