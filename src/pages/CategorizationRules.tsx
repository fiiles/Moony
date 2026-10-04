/**
 * Categorization rules (design system prototype categorization-rules.html):
 * what happens to every new transaction, and which rules actually work —
 * the evaluation order, the automation rate, hit counts per rule in all
 * three tabs, and the editor with a live match preview.
 */
import { useEffect, useMemo, useState, type ReactNode } from 'react';
import { Link, useLocation, useSearch } from 'wouter';
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import { useTranslation } from 'react-i18next';
import { toast } from 'sonner';
import { Pencil, Plus, Search, X } from 'lucide-react';
import type {
  CustomRule,
  LearnedPayeeEntry,
  RuleHitCount,
  TransactionCategory,
} from '@shared/schema';
import { categorizationApi } from '@/lib/tauri-api';
import { useFormat } from '@/lib/use-format';
import { useLanguage } from '@/i18n/I18nProvider';
import { translateApiError } from '@/lib/translate-api-error';
import { useCategories, useCategoryName } from '@/hooks/use-categories';
import { useShellPage } from '@/components/shell/shell-context';
import { PageHead } from '@/components/shell/PageHead';
import { Stat, Stats } from '@/components/common/Stat';
import { ConfirmDeleteDialog } from '@/components/common/ConfirmDeleteDialog';
import { CategoryChip, CategoryPick } from '@/components/common/CategoryPick';
import { Badge } from '@/components/ui/badge';
import { Button } from '@/components/ui/button';
import { Card, CardDescription, CardHeader, CardTitle } from '@/components/ui/card';
import { Checkbox } from '@/components/ui/checkbox';
import { Input, InputWrap } from '@/components/ui/input';
import { Progress } from '@/components/ui/progress';
import { Switch } from '@/components/ui/switch';
import { Tabs, TabsContent, TabsList, TabsTrigger } from '@/components/ui/tabs';
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from '@/components/ui/select';
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from '@/components/ui/table';
import RuleEditDialog, { type RuleEditPrefill } from '@/components/categorization/RuleEditDialog';
import { RulesPager } from '@/components/categorization/RulesPager';
import {
  ALL_CATEGORIES,
  DEFAULT_PAGE_SIZE,
  countRulesByCategory,
  filterLearnedRules,
  paginate,
  selectionState,
} from '@/utils/learned-rules';
import {
  HIT_DAYS,
  formatIbanDisplay,
  hitMap,
  idleCount,
  maxHits,
  packCountryName,
  ruleTypeKey,
  topRule,
} from '@/utils/rule-hits';
import { RULES_PAGE_PATH, parseNewRuleSearch } from '@/utils/rules-link';
import { cn } from '@/lib/utils';

type Tab = 'custom' | 'learned' | 'packs';
const AUTOMATIC_SOURCES = new Set(['rule', 'exact_match', 'own_account']);

export default function CategorizationRules() {
  const { t } = useTranslation('categorization');
  const { t: tc } = useTranslation('common');
  const fmt = useFormat();
  const { getLocale } = useLanguage();
  const queryClient = useQueryClient();
  const { categories } = useCategories();
  const categoryName = useCategoryName();

  // A link like `?new=1&payee=…&category=…` (a transaction row's "Create a rule" button) opens
  // the new-rule dialog prefilled (RUL-03); read once on arrival, then cleaned from the URL.
  const search = useSearch();
  const [, setLocation] = useLocation();
  const [linkedNewRule] = useState(() => parseNewRuleSearch(search));
  useEffect(() => {
    if (linkedNewRule) setLocation(RULES_PAGE_PATH, { replace: true });
  }, [linkedNewRule, setLocation]);

  const [tab, setTab] = useState<Tab>('custom');
  const [selectedPackId, setSelectedPackId] = useState('');
  const [query, setQuery] = useState('');
  const [categoryFilter, setCategoryFilter] = useState(ALL_CATEGORIES);
  const [page, setPage] = useState(0);
  const [pageSize, setPageSize] = useState<number>(DEFAULT_PAGE_SIZE);
  const [selectedIds, setSelectedIds] = useState<Set<string>>(new Set());
  const [pendingDelete, setPendingDelete] = useState<
    | { kind: 'learned'; id: string }
    | { kind: 'learnedBulk' }
    | { kind: 'custom'; rule: CustomRule }
    | null
  >(null);
  const [ruleDialogOpen, setRuleDialogOpen] = useState(linkedNewRule !== null);
  const [editingRule, setEditingRule] = useState<CustomRule | null>(null);
  const [rulePrefill, setRulePrefill] = useState<RuleEditPrefill | null>(() =>
    linkedNewRule
      ? {
          name: linkedNewRule.payee,
          pattern: linkedNewRule.payee,
          categoryId: linkedNewRule.categoryId ?? undefined,
        }
      : null
  );

  // ---- queries ----
  const { data: learnedRules = [] } = useQuery({
    queryKey: ['learnedPayees'],
    queryFn: () => categorizationApi.getLearnedPayees(),
  });
  const { data: customRules = [] } = useQuery({
    queryKey: ['customRules'],
    queryFn: () => categorizationApi.getCustomRules(),
  });
  const { data: rulePacks = [] } = useQuery({
    queryKey: ['rulePacks'],
    queryFn: () => categorizationApi.getRulePacks(),
  });
  const enabledPacks = rulePacks.filter((p) => p.enabled);
  const activePackId = selectedPackId || enabledPacks[0]?.packId || rulePacks[0]?.packId || '';
  const activePack = rulePacks.find((p) => p.packId === activePackId);
  const { data: packRules = [] } = useQuery({
    queryKey: ['packRules', activePackId],
    queryFn: () => categorizationApi.getPackRules(activePackId),
    enabled: activePackId.length > 0,
  });
  const { data: hitCounts = [] } = useQuery({
    queryKey: ['ruleHits', HIT_DAYS],
    queryFn: () => categorizationApi.getRuleHitCounts(HIT_DAYS),
  });
  const { data: overview } = useQuery({
    queryKey: ['categorizationOverview', HIT_DAYS],
    queryFn: () => categorizationApi.getCategorizationOverview(HIT_DAYS),
  });
  const hits = useMemo(() => hitMap(hitCounts), [hitCounts]);

  // ---- derived ----
  const automatic =
    overview?.bySource
      .filter((s) => AUTOMATIC_SOURCES.has(s.source))
      .reduce((sum, s) => sum + s.count, 0) ?? 0;
  const total = overview?.total ?? 0;
  const uncategorized = total - (overview?.categorized ?? 0);
  const automationRate = total > 0 ? automatic / total : 0;
  const packRuleTotal = enabledPacks.reduce((s, p) => s + p.ruleCount, 0);
  const top = useMemo(
    () => topRule(hitCounts, customRules, learnedRules, packRules),
    [hitCounts, customRules, learnedRules, packRules]
  );
  const countryOf = (pack: { packId: string; country: string }) =>
    packCountryName(pack.packId, pack.country, getLocale());
  const packCountries = enabledPacks.filter((p) => p.packId !== 'global').map(countryOf);
  const activeCountry = activePack ? countryOf(activePack) : '';

  useShellPage({
    crumb: t('crumb'),
    status:
      overview && total > 0
        ? {
            text: t('status', { percent: fmt.percent(automationRate, 0), days: HIT_DAYS }),
            tone: 'fresh',
          }
        : undefined,
  });

  const invalidateAll = () => {
    queryClient.invalidateQueries({ queryKey: ['learnedPayees'] });
    queryClient.invalidateQueries({ queryKey: ['customRules'] });
    queryClient.invalidateQueries({ queryKey: ['packRules'] });
    queryClient.invalidateQueries({ queryKey: ['ruleHits'] });
    queryClient.invalidateQueries({ queryKey: ['categorizationOverview'] });
  };
  const onError = (error: Error) => {
    toast.error(tc('status.error'), { description: translateApiError(error, tc) });
  };

  // ---- mutations ----
  const togglePackRule = useMutation({
    mutationFn: ({ ruleId, disabled }: { ruleId: string; disabled: boolean }) =>
      categorizationApi.setPackRuleDisabled(ruleId, disabled),
    onSuccess: (_r, v) => {
      invalidateAll();
      toast(t(v.disabled ? 'packTab.ruleOff' : 'packTab.ruleOn'));
    },
    onError,
  });
  const toggleCustomRule = useMutation({
    mutationFn: ({ rule, isActive }: { rule: CustomRule; isActive: boolean }) =>
      categorizationApi.updateCustomRule(rule.id, {
        name: rule.name,
        ruleType: rule.ruleType,
        pattern: rule.pattern,
        categoryId: rule.categoryId,
        priority: rule.priority,
        isActive,
        stopProcessing: rule.stopProcessing,
        ibanPattern: rule.ibanPattern,
      }),
    onSuccess: (_r, v) => {
      invalidateAll();
      toast(t(v.isActive ? 'customTab.activated' : 'customTab.deactivated'));
    },
    onError,
  });
  const deleteLearned = useMutation({
    mutationFn: (id: string) => categorizationApi.deleteLearnedPayee(id),
    onSuccess: () => {
      invalidateAll();
      setPendingDelete(null);
      toast(t('learnedRules.deleteSuccess'));
    },
    onError,
  });
  const bulkDeleteLearned = useMutation({
    mutationFn: (ids: string[]) => categorizationApi.deleteLearnedPayeesBulk(ids),
    onSuccess: (_result, ids) => {
      invalidateAll();
      setSelectedIds(new Set());
      setPendingDelete(null);
      toast(t('learnedRules.bulkDeleteSuccess', { count: ids.length }));
    },
    onError,
  });
  const updateLearnedCategory = useMutation({
    mutationFn: ({ id, categoryId }: { id: string; categoryId: string }) =>
      categorizationApi.updateLearnedPayeeCategory(id, categoryId),
    onSuccess: () => {
      invalidateAll();
      toast(t('learnedRules.updateCategorySuccess'));
    },
    onError,
  });
  const deleteCustom = useMutation({
    mutationFn: (id: string) => categorizationApi.deleteCustomRule(id),
    onSuccess: () => {
      invalidateAll();
      setPendingDelete(null);
      toast(t('customRules.deleteSuccess'));
    },
    onError,
  });

  // ---- learned filtering + paging ----
  const categoriesWithRules = countRulesByCategory(learnedRules);
  const activeCategoryFilter = categoriesWithRules.has(categoryFilter)
    ? categoryFilter
    : ALL_CATEGORIES;
  const categoryById = useMemo(() => new Map(categories.map((c) => [c.id, c])), [categories]);
  const nameOf = (categoryId: string) => {
    const c = categoryById.get(categoryId);
    return c ? categoryName(c) : categoryId;
  };
  const filteredLearned = filterLearnedRules(
    learnedRules,
    { query, categoryId: activeCategoryFilter },
    nameOf
  );
  const learnedPage = paginate(filteredLearned, page, pageSize);
  const pageIds = learnedPage.items.map((r) => r.id);
  const pageSelection = selectionState(pageIds, selectedIds);
  const selectedFilteredIds = filteredLearned.filter((r) => selectedIds.has(r.id)).map((r) => r.id);
  const filterCategories = categories
    .filter((c) => categoriesWithRules.has(c.id))
    .sort((a, b) => categoryName(a).localeCompare(categoryName(b)));

  const q = query.trim().toLowerCase();
  const filteredCustom = customRules.filter(
    (rule) =>
      !q ||
      rule.name.toLowerCase().includes(q) ||
      rule.pattern.toLowerCase().includes(q) ||
      rule.ibanPattern?.toLowerCase().includes(q) ||
      nameOf(rule.categoryId).toLowerCase().includes(q)
  );
  const filteredPack = packRules.filter(
    (rule) =>
      !q ||
      rule.name.toLowerCase().includes(q) ||
      rule.pattern.toLowerCase().includes(q) ||
      nameOf(rule.categoryId).toLowerCase().includes(q)
  );

  const customMax = maxHits(
    hits,
    customRules.map((r) => r.id)
  );
  const learnedMax = maxHits(
    hits,
    learnedRules.map((r) => r.id)
  );
  const packMax = maxHits(
    hits,
    packRules.map((r) => r.id)
  );

  const openEditor = (rule: CustomRule | null, prefill: RuleEditPrefill | null = null) => {
    setEditingRule(rule);
    setRulePrefill(prefill);
    setRuleDialogOpen(true);
  };

  // ---- shared cells ----
  const sub = (content: ReactNode, mono = false) => (
    <small
      className={cn(
        'mt-[3px] block truncate text-micro font-500 text-ink-4',
        mono && 'font-mono text-[11px]'
      )}
    >
      {content}
    </small>
  );
  const hitsCell = (id: string, max: number, withLast = true) => {
    const h = hits.get(id);
    const count = h?.hits ?? 0;
    return (
      <>
        <span className="flex items-center justify-end gap-2">
          <Progress value={(count / max) * 100} className="w-11" />
          <span className="min-w-[22px] text-right num">{count}</span>
        </span>
        {withLast &&
          sub(h?.lastHit != null ? t('hits.last', { date: fmt.day(h.lastHit) }) : t('hits.none'))}
      </>
    );
  };
  const categoryCell = (categoryId: string) => {
    const c = categoryById.get(categoryId);
    return c ? <CategoryChip category={c} /> : <span className="text-ink-4">{categoryId}</span>;
  };
  const searchBox = (placeholder: string) => (
    <InputWrap icon={<Search />} className="w-60">
      <Input
        className="h-[35px] text-table"
        placeholder={placeholder}
        value={query}
        onChange={(e) => {
          setQuery(e.target.value);
          setPage(0);
        }}
      />
    </InputWrap>
  );

  return (
    <>
      <PageHead
        eyebrow={t('eyebrow')}
        title={t('title')}
        description={t('lead')}
        actions={
          <Button onClick={() => openEditor(null)}>
            <Plus />
            {t('customRules.addRule')}
          </Button>
        }
      />

      <div className="-mt-2 mb-6 flex flex-wrap items-center gap-2 text-caption text-ink-3">
        <span>{t('flow.label')}</span>
        <Badge variant="dark" className="h-6 text-caption">
          {t('flow.custom')}
        </Badge>
        <span className="text-ink-5">→</span>
        <Badge className="h-6 text-caption">{t('flow.learned')}</Badge>
        <span className="text-ink-5">→</span>
        <Badge variant="outline" className="h-6 text-caption">
          {t('flow.ownAccounts')}
        </Badge>
        <span className="text-ink-5">→</span>
        <Badge variant="outline" className="h-6 text-caption">
          {packCountries.length === 1
            ? t('flow.pack', { country: packCountries[0] })
            : t('flow.packs', { countries: packCountries.join(', ') || 'global' })}
        </Badge>
        <span className="text-ink-5">{t('flow.none')}</span>
      </div>

      <Stats>
        <Stat
          label={t('stats.automated')}
          value={total > 0 ? fmt.percent(automationRate, 0) : '—'}
          note={t('stats.automatedNote', { auto: automatic, total, days: HIT_DAYS })}
        />
        <Stat
          label={t('stats.uncategorized')}
          value={fmt.number(uncategorized)}
          tone={uncategorized > 0 ? 'loss' : 'neutral'}
          noteTone="neutral"
          note={
            <>
              <Link
                href="/bank-accounts"
                className="font-600 text-ink-3 underline-offset-[3px] hover:text-ink hover:underline"
              >
                {t('stats.uncategorizedLink')}
              </Link>{' '}
              · {t('stats.uncategorizedNote')}
            </>
          }
        />
        <Stat
          label={t('stats.rules')}
          value={fmt.number(learnedRules.length + customRules.length + packRuleTotal)}
          note={t('stats.rulesNote', {
            learned: learnedRules.length,
            custom: customRules.length,
            pack: packRuleTotal,
          })}
        />
        <Stat
          label={t('stats.top')}
          value={
            top ? (
              <span className="text-[18px] leading-tight">
                {top.name} → {nameOf(top.categoryId)}
              </span>
            ) : (
              '—'
            )
          }
          note={
            top
              ? t('stats.topNote', {
                  hits: t('hitsCount', { count: top.hits }),
                  days: HIT_DAYS,
                  source:
                    top.source === 'custom'
                      ? t('stats.sourceCustom')
                      : top.source === 'learned'
                        ? t('stats.sourceLearned')
                        : t('stats.sourcePack', { country: activeCountry }),
                })
              : t('stats.topNone')
          }
        />
      </Stats>

      <Tabs value={tab} onValueChange={(v) => setTab(v as Tab)}>
        <TabsList variant="underline">
          <TabsTrigger value="custom">
            {t('tabs.custom')} <span className="text-ink-4">{customRules.length}</span>
          </TabsTrigger>
          <TabsTrigger value="learned">
            {t('tabs.learned')} <span className="text-ink-4">{learnedRules.length}</span>
          </TabsTrigger>
          <TabsTrigger value="packs">
            {activePack ? t('tabs.pack', { country: activeCountry }) : t('tabs.packGeneric')}{' '}
            <span className="text-ink-4">{packRules.length}</span>
          </TabsTrigger>
        </TabsList>

        {/* ---------------- Custom rules ---------------- */}
        <TabsContent value="custom" className="mt-0">
          <Card variant="table">
            <CardHeader>
              <div>
                <CardTitle>{t('customRules.title')}</CardTitle>
                <CardDescription>{t('customTab.subtitle', { days: HIT_DAYS })}</CardDescription>
              </div>
              {searchBox(t('customTab.search'))}
            </CardHeader>
            <Table>
              <TableHeader>
                <TableRow>
                  <TableHead className="w-[30%]">{t('customTab.rule')}</TableHead>
                  <TableHead>{t('customTab.type')}</TableHead>
                  <TableHead>{t('customTab.category')}</TableHead>
                  <TableHead className="text-right">{t('customTab.priority')}</TableHead>
                  <TableHead className="text-right">
                    {t('hits.column', { days: HIT_DAYS })}
                  </TableHead>
                  <TableHead className="text-right">{t('customTab.active')}</TableHead>
                  <TableHead className="w-[72px]">
                    <span className="sr-only">{tc('labels.actions')}</span>
                  </TableHead>
                </TableRow>
              </TableHeader>
              <TableBody>
                {filteredCustom.length === 0 ? (
                  <TableRow className="hover:bg-transparent">
                    <TableCell colSpan={7} className="py-12 text-center text-table text-ink-3">
                      {customRules.length === 0
                        ? t('customRules.noRules')
                        : t('customTab.noMatching')}
                    </TableCell>
                  </TableRow>
                ) : (
                  filteredCustom.map((rule) => {
                    const ibanRule = rule.pattern === '*' && !!rule.ibanPattern;
                    return (
                      <TableRow
                        key={rule.id}
                        className={cn('h-14', !rule.isActive && 'text-ink-4')}
                      >
                        <TableCell>
                          <b className={cn('font-600', rule.isActive ? 'text-ink' : 'text-ink-3')}>
                            {rule.name}
                            {rule.isSystem && (
                              <Badge variant="outline" className="ml-1.5 align-[1px]">
                                {t('customTab.system')}
                              </Badge>
                            )}
                          </b>
                          {sub(ibanRule ? formatIbanDisplay(rule.ibanPattern) : rule.pattern, true)}
                        </TableCell>
                        <TableCell>
                          <Badge>
                            {ibanRule
                              ? t('ruleTypes.iban')
                              : t(`ruleTypes.${ruleTypeKey(rule.ruleType)}`, {
                                  defaultValue: rule.ruleType,
                                })}
                          </Badge>
                        </TableCell>
                        <TableCell>{categoryCell(rule.categoryId)}</TableCell>
                        <TableCell className="text-right num">{rule.priority}</TableCell>
                        <TableCell className="text-right">{hitsCell(rule.id, customMax)}</TableCell>
                        <TableCell className="text-right">
                          <Switch
                            checked={rule.isActive}
                            disabled={rule.isSystem || toggleCustomRule.isPending}
                            onCheckedChange={(checked) =>
                              toggleCustomRule.mutate({ rule, isActive: checked })
                            }
                            aria-label={t('customTab.active')}
                          />
                        </TableCell>
                        <TableCell className="text-right">
                          {!rule.isSystem && (
                            <div data-row-actions className="inline-flex gap-0.5">
                              <Button
                                variant="ghost"
                                size="icon-sm"
                                aria-label={t('customRules.editRule')}
                                onClick={() => openEditor(rule)}
                              >
                                <Pencil />
                              </Button>
                              <Button
                                variant="ghost"
                                size="icon-sm"
                                aria-label={t('customRules.deleteRule')}
                                onClick={() => setPendingDelete({ kind: 'custom', rule })}
                              >
                                <X />
                              </Button>
                            </div>
                          )}
                        </TableCell>
                      </TableRow>
                    );
                  })
                )}
              </TableBody>
            </Table>
            <div className="flex items-center justify-between gap-6 border-t border-line px-[14px] py-3 text-micro font-500 text-ink-4">
              <span>
                {t('customTab.footer', { count: customRules.length })} ·{' '}
                {t('customTab.footerIdle', {
                  count: idleCount(
                    hits,
                    customRules.map((r) => r.id)
                  ),
                  days: HIT_DAYS,
                })}
              </span>
              <span>{t('customTab.footerNote')}</span>
            </div>
          </Card>
        </TabsContent>

        {/* ---------------- Learned rules ---------------- */}
        <TabsContent value="learned" className="mt-0">
          <Card variant="table">
            <CardHeader>
              <div>
                <CardTitle>{t('learnedRules.title')}</CardTitle>
                <CardDescription>{t('learnedTab.subtitle')}</CardDescription>
              </div>
              <div className="flex items-center gap-3">
                {selectedFilteredIds.length > 0 && (
                  <Button
                    variant="danger"
                    size="sm"
                    onClick={() => setPendingDelete({ kind: 'learnedBulk' })}
                  >
                    {t('learnedTab.forgetSelected', { count: selectedFilteredIds.length })}
                  </Button>
                )}
                {filterCategories.length > 1 && (
                  <Select
                    value={activeCategoryFilter}
                    onValueChange={(v) => {
                      setCategoryFilter(v);
                      setPage(0);
                    }}
                  >
                    <SelectTrigger
                      className="h-[35px] w-[180px] text-table"
                      aria-label={t('learnedRules.filterByCategory')}
                    >
                      <SelectValue />
                    </SelectTrigger>
                    <SelectContent>
                      <SelectItem value={ALL_CATEGORIES}>
                        {t('learnedRules.allCategories')}
                      </SelectItem>
                      {filterCategories.map((category) => (
                        <SelectItem key={category.id} value={category.id}>
                          {categoryName(category)} ({categoriesWithRules.get(category.id)})
                        </SelectItem>
                      ))}
                    </SelectContent>
                  </Select>
                )}
                {searchBox(t('learnedTab.search'))}
              </div>
            </CardHeader>
            <Table>
              <TableHeader>
                <TableRow>
                  <TableHead className="w-10">
                    <Checkbox
                      checked={pageSelection === 'all'}
                      onCheckedChange={() => {
                        const next = new Set(selectedIds);
                        for (const id of pageIds) {
                          if (pageSelection === 'all') next.delete(id);
                          else next.add(id);
                        }
                        setSelectedIds(next);
                      }}
                      aria-label={t('learnedRules.selectPage')}
                    />
                  </TableHead>
                  <TableHead className="w-[30%]">{t('learnedTab.counterparty')}</TableHead>
                  <TableHead>{t('learnedTab.iban')}</TableHead>
                  <TableHead>{t('learnedTab.category')}</TableHead>
                  <TableHead className="text-right">
                    {t('hits.column', { days: HIT_DAYS })}
                  </TableHead>
                  <TableHead className="text-right">{t('learnedTab.lastHit')}</TableHead>
                  <TableHead className="w-10">
                    <span className="sr-only">{tc('labels.actions')}</span>
                  </TableHead>
                </TableRow>
              </TableHeader>
              <TableBody>
                {learnedPage.items.length === 0 ? (
                  <TableRow className="hover:bg-transparent">
                    <TableCell colSpan={7} className="py-12 text-center text-table text-ink-3">
                      {learnedRules.length === 0
                        ? t('learnedRules.noRulesDescription')
                        : t('learnedRules.noMatching')}
                    </TableCell>
                  </TableRow>
                ) : (
                  learnedPage.items.map((rule) => (
                    <LearnedRow
                      key={rule.id}
                      rule={rule}
                      selected={selectedIds.has(rule.id)}
                      onToggle={() => {
                        const next = new Set(selectedIds);
                        if (next.has(rule.id)) next.delete(rule.id);
                        else next.add(rule.id);
                        setSelectedIds(next);
                      }}
                      categories={categories}
                      hit={hits.get(rule.id)}
                      max={learnedMax}
                      onCategory={(categoryId) =>
                        updateLearnedCategory.mutate({ id: rule.id, categoryId })
                      }
                      onDelete={() => setPendingDelete({ kind: 'learned', id: rule.id })}
                    />
                  ))
                )}
              </TableBody>
            </Table>
            <RulesPager
              page={learnedPage.page}
              pageCount={learnedPage.pageCount}
              pageSize={pageSize}
              total={learnedPage.total}
              from={learnedPage.from}
              to={learnedPage.to}
              onPageChange={setPage}
              onPageSizeChange={(size) => {
                setPageSize(size);
                setPage(0);
              }}
            />
          </Card>
        </TabsContent>

        {/* ---------------- Pack rules ---------------- */}
        <TabsContent value="packs" className="mt-0">
          <Card variant="table">
            <CardHeader>
              <div>
                <CardTitle>
                  {activePack
                    ? t('packTab.title', { country: activeCountry })
                    : t('packRules.title')}
                </CardTitle>
                {activePack && (
                  <CardDescription>
                    {t('packTab.subtitle', {
                      count: activePack.ruleCount,
                      version: activePack.version,
                    })}
                  </CardDescription>
                )}
              </div>
              <div className="flex items-center gap-3">
                {activePack && (
                  <Badge variant={activePack.enabled ? 'gain' : 'outline'}>
                    {t(activePack.enabled ? 'packTab.enabled' : 'packTab.disabled')}
                  </Badge>
                )}
                {rulePacks.length > 1 && (
                  <Select value={activePackId} onValueChange={setSelectedPackId}>
                    <SelectTrigger
                      className="h-[35px] w-[180px] text-table"
                      aria-label={t('packRules.selectPack')}
                    >
                      <SelectValue />
                    </SelectTrigger>
                    <SelectContent>
                      {rulePacks.map((pack) => (
                        <SelectItem key={pack.packId} value={pack.packId}>
                          {countryOf(pack)}
                          {pack.enabled ? '' : ` — ${t('packRules.packDisabled')}`}
                        </SelectItem>
                      ))}
                    </SelectContent>
                  </Select>
                )}
                {searchBox(t('packTab.search'))}
              </div>
            </CardHeader>
            <Table>
              <TableHeader>
                <TableRow>
                  <TableHead className="w-[26%]">{t('packTab.rule')}</TableHead>
                  <TableHead>{t('packTab.type')}</TableHead>
                  <TableHead>{t('packTab.pattern')}</TableHead>
                  <TableHead>{t('packTab.category')}</TableHead>
                  <TableHead className="text-right">
                    {t('hits.column', { days: HIT_DAYS })}
                  </TableHead>
                  <TableHead className="text-right">{t('packTab.on')}</TableHead>
                </TableRow>
              </TableHeader>
              <TableBody>
                {filteredPack.map((rule) => (
                  <TableRow key={rule.id} className={cn('h-14', rule.disabled && 'text-ink-4')}>
                    <TableCell>
                      <b className={cn('font-600', rule.disabled ? 'text-ink-3' : 'text-ink')}>
                        {rule.name}
                      </b>
                      {rule.disabled && sub(t('packTab.disabledNote'))}
                    </TableCell>
                    <TableCell>
                      <Badge>
                        {t(`ruleTypes.${ruleTypeKey(rule.ruleType)}`, {
                          defaultValue: rule.ruleType,
                        })}
                      </Badge>
                    </TableCell>
                    <TableCell className="max-w-[200px] truncate font-mono text-[11px] text-ink-3">
                      {rule.pattern}
                    </TableCell>
                    <TableCell>{categoryCell(rule.categoryId)}</TableCell>
                    <TableCell className="text-right">
                      {hitsCell(rule.id, packMax, false)}
                    </TableCell>
                    <TableCell className="text-right">
                      <Switch
                        checked={!rule.disabled}
                        disabled={togglePackRule.isPending}
                        onCheckedChange={(checked) =>
                          togglePackRule.mutate({ ruleId: rule.id, disabled: !checked })
                        }
                        aria-label={t('packTab.on')}
                      />
                    </TableCell>
                  </TableRow>
                ))}
              </TableBody>
            </Table>
            <div className="flex items-center justify-between gap-6 border-t border-line px-[14px] py-3 text-micro font-500 text-ink-4">
              <span>
                {t('packTab.footer', {
                  shown: filteredPack.length,
                  total: packRules.length,
                  idle: idleCount(
                    hits,
                    packRules.map((r) => r.id)
                  ),
                })}
              </span>
              <Link
                href="/settings"
                className="font-600 text-ink-3 underline-offset-[3px] hover:text-ink hover:underline"
              >
                {t('packTab.settingsLink')}
              </Link>
            </div>
          </Card>
        </TabsContent>
      </Tabs>

      <ConfirmDeleteDialog
        open={pendingDelete !== null}
        onOpenChange={(next) => {
          if (!next) setPendingDelete(null);
        }}
        title={
          pendingDelete?.kind === 'learnedBulk'
            ? t('learnedTab.confirmForgetBulk.title')
            : pendingDelete?.kind === 'learned'
              ? t('learnedTab.confirmForget.title')
              : t('customRules.deleteConfirmTitle')
        }
        description={
          pendingDelete?.kind === 'learnedBulk'
            ? t('learnedTab.confirmForgetBulk.description', { count: selectedFilteredIds.length })
            : pendingDelete?.kind === 'learned'
              ? t('learnedTab.confirmForget.description')
              : t('customRules.deleteConfirmDescription')
        }
        onConfirm={() => {
          if (!pendingDelete) return;
          if (pendingDelete.kind === 'learnedBulk') bulkDeleteLearned.mutate(selectedFilteredIds);
          else if (pendingDelete.kind === 'learned') deleteLearned.mutate(pendingDelete.id);
          else deleteCustom.mutate(pendingDelete.rule.id);
        }}
        isPending={deleteLearned.isPending || bulkDeleteLearned.isPending || deleteCustom.isPending}
        confirmLabel={
          pendingDelete?.kind === 'custom' ? t('customRules.deleteRule') : t('learnedTab.forget')
        }
      />

      {/* Keyed by rule id so the form remounts with fresh state */}
      <RuleEditDialog
        key={editingRule?.id ?? (rulePrefill ? 'prefilled' : 'new')}
        open={ruleDialogOpen}
        onOpenChange={(open) => {
          setRuleDialogOpen(open);
          if (!open) {
            setEditingRule(null);
            setRulePrefill(null);
          }
        }}
        rule={editingRule}
        prefill={rulePrefill}
        categories={categories}
      />
    </>
  );
}

function LearnedRow({
  rule,
  selected,
  onToggle,
  categories,
  hit,
  max,
  onCategory,
  onDelete,
}: {
  rule: LearnedPayeeEntry;
  selected: boolean;
  onToggle: () => void;
  categories: TransactionCategory[];
  hit: RuleHitCount | undefined;
  max: number;
  onCategory: (categoryId: string) => void;
  onDelete: () => void;
}) {
  const { t } = useTranslation('categorization');
  const fmt = useFormat();
  const category = categories.find((c) => c.id === rule.categoryId);
  const payee = rule.normalizedPayee || rule.originalPayee;
  const count = hit?.hits ?? 0;
  return (
    <TableRow className="h-14">
      <TableCell>
        <Checkbox
          checked={selected}
          onCheckedChange={onToggle}
          aria-label={t('learnedRules.selectRule', {
            payee: payee || formatIbanDisplay(rule.counterpartyIban),
          })}
        />
      </TableCell>
      <TableCell>
        {payee ? (
          <>
            <b className="block truncate font-600 text-ink" title={rule.originalPayee ?? undefined}>
              {payee}
            </b>
            {rule.originalPayee && rule.originalPayee !== payee && (
              <small className="mt-[3px] block truncate text-micro font-500 text-ink-4">
                {rule.originalPayee}
              </small>
            )}
          </>
        ) : (
          <span className="text-ink-4">{t('learnedTab.ibanOnly')}</span>
        )}
      </TableCell>
      <TableCell className="font-mono text-[11px] text-ink-3">
        {formatIbanDisplay(rule.counterpartyIban) || '—'}
      </TableCell>
      <TableCell>
        <CategoryPick
          categories={categories}
          selectedId={rule.categoryId}
          onPick={(id) => {
            if (id) onCategory(id);
          }}
        >
          <button
            type="button"
            className="inline-flex max-w-full items-center rounded-[6px] border border-dashed border-line-strong bg-paper px-[7px] py-0.5 text-micro hover:border-ink-3"
          >
            {category ? (
              <CategoryChip category={category} className="text-micro" />
            ) : (
              <span className="text-ink-3">{rule.categoryId}</span>
            )}
          </button>
        </CategoryPick>
      </TableCell>
      <TableCell className="text-right">
        <span className="flex items-center justify-end gap-2">
          <Progress value={(count / max) * 100} className="w-11" />
          <span className="min-w-[22px] text-right num">{count}</span>
        </span>
      </TableCell>
      <TableCell className="text-right num text-ink-3">
        {hit?.lastHit != null ? fmt.day(hit.lastHit) : '—'}
      </TableCell>
      <TableCell className="text-right">
        <div data-row-actions>
          <Button
            variant="ghost"
            size="icon-sm"
            aria-label={t('learnedTab.forget')}
            title={t('learnedTab.forget')}
            onClick={onDelete}
          >
            <X />
          </Button>
        </div>
      </TableCell>
    </TableRow>
  );
}
