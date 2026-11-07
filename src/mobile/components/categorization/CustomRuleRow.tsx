import { useTranslation } from 'react-i18next';
import { Power, Trash2 } from 'lucide-react';
import type { CustomRule, RuleHitCount, TransactionCategory } from '@shared/schema';
import { Switch } from '@/components/ui/switch';
import { useCategoryName } from '@/hooks/use-categories';
import { CategoryChip } from '@/mobile/components/cashflow/CategoryChip';
import { EntityRow, RowChip } from '@/mobile/components/EntityRow';
import { RowBadge } from '@/mobile/components/RowBadge';
import { RowMenuItem } from '@/mobile/components/RowMenu';
import { customRuleActions } from '@/mobile/lib/categorization-rows';
import { HIT_DAYS, formatIbanDisplay } from '@/utils/rule-hits';

interface CustomRuleRowProps {
  rule: CustomRule;
  category: TransactionCategory | undefined;
  hit: RuleHitCount | undefined;
  /** A rule write is in flight: the switch waits. */
  busy: boolean;
  onToggle: (isActive: boolean) => void;
  onDelete: () => void;
}

/**
 * One own rule: its name (or pattern), "→ category · N× za 90 dní" and an `isActive` switch.
 * Swipe left and the long press's menu switch and delete; a system rule has the badge, a switch
 * that cannot be moved and a menu that only says why it cannot go. The row has no tap action of
 * its own (the switch is a button, which cannot sit in the row's).
 */
export function CustomRuleRow({
  rule,
  category,
  hit,
  busy,
  onToggle,
  onDelete,
}: CustomRuleRowProps) {
  const { t } = useTranslation('categorization');
  const { t: tm } = useTranslation('mobile');
  const { t: tc } = useTranslation('common');
  const categoryName = useCategoryName();
  const actions = customRuleActions(rule);
  const title =
    rule.name || (rule.ibanPattern ? formatIbanDisplay(rule.ibanPattern) : rule.pattern);
  const line = tm('settings.categorization.ruleLine', {
    category: category ? categoryName(category) : rule.categoryId,
    n: hit?.hits ?? 0,
    days: HIT_DAYS,
  });

  return (
    <EntityRow
      chip={
        category ? (
          <CategoryChip category={category} />
        ) : (
          <RowChip>
            <Power aria-hidden strokeWidth={1.75} />
          </RowChip>
        )
      }
      title={
        <>
          {title}
          {rule.isSystem && <RowBadge>{t('customTab.system')}</RowBadge>}
        </>
      }
      sub={line}
      muted={!rule.isActive}
      value={
        <span className="-my-3 -mr-1.5 inline-flex size-11 items-center justify-center align-middle">
          <Switch
            checked={rule.isActive}
            disabled={!actions.toggle || busy}
            onCheckedChange={onToggle}
            aria-label={tm('settings.categorization.ruleSwitch', { name: title })}
          />
        </span>
      }
      onSwipeLeft={actions.delete ? onDelete : undefined}
      swipeLeftLabel={tc('buttons.delete')}
      swipeLeftIcon={Trash2}
      swipeLeftDestructive
      menu={{
        label: tm('cashflow.rowMenu', { title }),
        // Smazat is nowhere else on the row
        shown: true,
        children: (
          <>
            {actions.toggle && (
              <RowMenuItem disabled={busy} onSelect={() => onToggle(!rule.isActive)}>
                <Power aria-hidden strokeWidth={1.75} />
                {rule.isActive
                  ? tm('settings.categorization.turnOff')
                  : tm('settings.categorization.turnOn')}
              </RowMenuItem>
            )}
            {actions.delete ? (
              <RowMenuItem variant="danger" onSelect={onDelete}>
                <Trash2 aria-hidden strokeWidth={1.75} />
                {t('customRules.deleteRule')}
              </RowMenuItem>
            ) : (
              <RowMenuItem disabled>
                <Trash2 aria-hidden strokeWidth={1.75} />
                {t('customTab.footerNote')}
              </RowMenuItem>
            )}
          </>
        ),
      }}
    />
  );
}
