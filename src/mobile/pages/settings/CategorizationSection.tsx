import { useMemo, useState, type ReactNode } from 'react';
import { useTranslation } from 'react-i18next';
import type { CustomRule, LearnedPayeeEntry } from '@shared/schema';
import { useBankAccounts } from '@/hooks/use-bank-accounts';
import { useCategories, useCategoryName } from '@/hooks/use-categories';
import { useLanguage } from '@/i18n/I18nProvider';
import { CustomRuleRow } from '@/mobile/components/categorization/CustomRuleRow';
import { LearnedPayeeRow } from '@/mobile/components/categorization/LearnedPayeeRow';
import { PackRow } from '@/mobile/components/categorization/PackRow';
import { EntityRows } from '@/mobile/components/EntityRow';
import { ScaledReportError } from '@/mobile/components/Scaled';
import { SearchField } from '@/mobile/components/SearchField';
import { SectionHead } from '@/mobile/components/SectionHead';
import { SectionPage } from '@/mobile/components/SectionPage';
import {
  useCategorizationMutations,
  useCategorizationRules,
  usePackLabel,
} from '@/mobile/hooks/use-categorization-rules';
import { sortPacks } from '@/mobile/lib/categorization-rows';
import { ConfirmSheet } from '@/mobile/sheets/ConfirmSheet';
import { PackRulesSheet } from '@/mobile/sheets/PackRulesSheet';
import { filterLearnedRules, ALL_CATEGORIES } from '@/utils/learned-rules';

/** From this many learned payees on, the list gets a search field. */
const SEARCH_FROM = 15;

type Pending = { kind: 'custom'; rule: CustomRule } | { kind: 'learned'; entry: LearnedPayeeEntry };

/** The page's loading state: a block in outline. */
function ListSkeleton() {
  return (
    <div aria-hidden className="mb-3.5">
      <div className="skeleton h-28 w-full" />
    </div>
  );
}

/**
 * Nastavení → Kategorizace: what the app files by itself — the rule packs of the countries (on
 * or off, one rule at a time), the own rules (on or off, deleted) and the payees it learned from
 * the corrections ("Zapomenout"). The phone creates and edits no rule; the desktop does. Same
 * queries and writes as the desktop's rules page and its Balíčky card, so both share one cache.
 */
export default function CategorizationSection() {
  const { t } = useTranslation('mobile');
  const { t: tc } = useTranslation('categorization');
  const { language } = useLanguage();
  const packLabel = usePackLabel();
  const { packs, custom, learned, hits } = useCategorizationRules();
  const { categories } = useCategories();
  const categoryName = useCategoryName();
  const { accounts } = useBankAccounts();
  const mutations = useCategorizationMutations();
  const [openPack, setOpenPack] = useState<string | null>(null);
  const [pending, setPending] = useState<Pending | null>(null);
  const [shownPending, setShownPending] = useState<Pending | null>(null);
  const [query, setQuery] = useState('');
  if (pending !== null && pending !== shownPending) setShownPending(pending);

  const categoryById = useMemo(() => new Map(categories.map((c) => [c.id, c])), [categories]);
  const accountName = (id: string) => accounts.find((a) => a.id === id)?.name ?? null;
  const sortedPacks = useMemo(
    () => sortPacks(packs.data ?? [], (p) => packLabel(p.packId, p.country), language),
    [packs.data, packLabel, language]
  );
  const learnedRules = useMemo(() => learned.data ?? [], [learned.data]);
  const shownLearned = useMemo(
    () =>
      filterLearnedRules(learnedRules, { query, categoryId: ALL_CATEGORIES }, (id) => {
        const c = categoryById.get(id);
        return c ? categoryName(c) : undefined;
      }),
    [learnedRules, query, categoryById, categoryName]
  );
  const openedPack = sortedPacks.find((p) => p.packId === openPack);

  const confirmDelete = async () => {
    if (!pending) return;
    try {
      if (pending.kind === 'custom') await mutations.deleteCustomRule.mutateAsync(pending.rule.id);
      else await mutations.deleteLearnedPayee.mutateAsync(pending.entry.id);
      setPending(null);
    } catch {
      // onError already showed a translated toast; keep the sheet open.
    }
  };

  const section = (
    title: string,
    query_: { isError: boolean; isLoading: boolean; refetch: () => unknown },
    empty: boolean,
    emptyText: string,
    list: () => ReactNode,
    above?: ReactNode
  ) => (
    <section>
      <SectionHead title={title} />
      {query_.isError ? (
        <ScaledReportError onRetry={() => void query_.refetch()} />
      ) : query_.isLoading ? (
        <ListSkeleton />
      ) : empty ? (
        <p className="mx-0.5 mb-3.5 mt-0 text-m-sub text-ink-3">{emptyText}</p>
      ) : (
        <>
          {above}
          <EntityRows>{list()}</EntityRows>
        </>
      )}
    </section>
  );

  return (
    <SectionPage title={t('settings.rows.categorization')} lead={t('settings.categorization.lead')}>
      {section(
        t('settings.categorization.packs'),
        packs,
        sortedPacks.length === 0,
        t('settings.categorization.packsEmpty'),
        () =>
          sortedPacks.map((pack) => (
            <PackRow
              key={pack.packId}
              pack={pack}
              label={packLabel(pack.packId, pack.country)}
              disabled={mutations.setPackEnabled.isPending}
              onToggle={(enabled) =>
                mutations.setPackEnabled.mutate({
                  packs: packs.data ?? [],
                  packId: pack.packId,
                  enabled,
                })
              }
              onOpen={() => setOpenPack(pack.packId)}
            />
          ))
      )}
      {section(
        t('settings.categorization.custom'),
        custom,
        (custom.data ?? []).length === 0,
        t('settings.categorization.customEmpty'),
        () =>
          (custom.data ?? []).map((rule) => (
            <CustomRuleRow
              key={rule.id}
              rule={rule}
              category={categoryById.get(rule.categoryId)}
              hit={hits.get(rule.id)}
              busy={mutations.setCustomRuleActive.isPending}
              onToggle={(isActive) => mutations.setCustomRuleActive.mutate({ rule, isActive })}
              onDelete={() => setPending({ kind: 'custom', rule })}
            />
          ))
      )}
      {section(
        t('settings.categorization.learned'),
        learned,
        learnedRules.length === 0,
        t('settings.categorization.learnedEmpty'),
        () =>
          shownLearned.length === 0 ? (
            <li className="list-none px-3.5 py-3 text-m-sub text-ink-3">
              {t('settings.categorization.noMatching')}
            </li>
          ) : (
            shownLearned.map((entry) => (
              <LearnedPayeeRow
                key={entry.id}
                entry={entry}
                category={categoryById.get(entry.categoryId)}
                accountName={accountName}
                onForget={() => setPending({ kind: 'learned', entry })}
              />
            ))
          ),
        learnedRules.length > SEARCH_FROM ? (
          <SearchField
            value={query}
            onChange={setQuery}
            placeholder={tc('learnedTab.search')}
            clearLabel={t('settings.categorization.clearSearch')}
          />
        ) : undefined
      )}

      <PackRulesSheet
        pack={
          openedPack
            ? {
                packId: openedPack.packId,
                label: packLabel(openedPack.packId, openedPack.country),
                ruleCount: openedPack.ruleCount,
              }
            : null
        }
        onClose={() => setOpenPack(null)}
      />
      <ConfirmSheet
        open={pending !== null}
        onOpenChange={(open) => {
          if (!open) setPending(null);
        }}
        title={
          shownPending?.kind === 'learned'
            ? tc('learnedTab.confirmForget.title')
            : tc('customRules.deleteConfirmTitle')
        }
        description={
          shownPending?.kind === 'learned'
            ? tc('learnedTab.confirmForget.description')
            : tc('customRules.deleteConfirmDescription')
        }
        confirmLabel={
          shownPending?.kind === 'learned' ? tc('learnedTab.forget') : tc('customRules.deleteRule')
        }
        isPending={mutations.deleteCustomRule.isPending || mutations.deleteLearnedPayee.isPending}
        onConfirm={() => void confirmDelete()}
      />
    </SectionPage>
  );
}
