import { useMemo, useState } from 'react';
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import { useTranslation } from 'react-i18next';
import { toast } from 'sonner';
import { Gem, Plus } from 'lucide-react';
import type { OtherAsset } from '@shared/schema';
import { exportApi, otherAssetsApi, portfolioApi } from '@/lib/tauri-api';
import { useCurrency } from '@/lib/currency';
import { useFormat } from '@/lib/use-format';
import { translateApiError } from '@/lib/translate-api-error';
import { useShellPage } from '@/components/shell/shell-context';
import { PageHead } from '@/components/shell/PageHead';
import { Button } from '@/components/ui/button';
import { EmptyState } from '@/components/common/EmptyState';
import { ExportButton } from '@/components/common/ExportButton';
import { ConfirmDeleteDialog } from '@/components/common/ConfirmDeleteDialog';
import { Stat, StatSkeleton, Stats } from '@/components/common/Stat';
import PortfolioTrendCard from '@/components/common/PortfolioTrendCard';
import { OtherAssetsTable } from '@/components/other-assets/OtherAssetsTable';
import { otherAssetRow } from '@/utils/other-assets';
import { AddOtherAssetModal } from '@/components/other-assets/AddOtherAssetModal';
import { EditOtherAssetModal } from '@/components/other-assets/EditOtherAssetModal';
import { BuyOtherAssetModal } from '@/components/other-assets/BuyOtherAssetModal';
import { SellOtherAssetModal } from '@/components/other-assets/SellOtherAssetModal';
import { OtherAssetTransactionsModal } from '@/components/other-assets/OtherAssetTransactionsModal';
import { RevalueDialog } from '@/components/valuations/RevalueDialog';

type Modal = 'add' | 'transactions' | 'buy' | 'sell' | 'edit' | 'revalue' | 'delete' | null;

/**
 * Other assets (design system §7 List, prototype other-assets.html): four
 * stats — value, gain against the purchase average (the old card showed the
 * yield under that label), yearly yield, last change — the value trend from
 * portfolio history, and the assets table with edit and revalue actions.
 */
export default function OtherAssets() {
  const { t } = useTranslation('otherAssets');
  const { t: tc } = useTranslation('common');
  const queryClient = useQueryClient();
  const { formatCurrency, formatCurrencySigned } = useCurrency();
  const fmt = useFormat();
  const [modal, setModal] = useState<Modal>(null);
  const [selected, setSelected] = useState<OtherAsset | null>(null);

  const { data: assets = [], isLoading } = useQuery<OtherAsset[]>({
    queryKey: ['other-assets'],
    queryFn: () => otherAssetsApi.getAll(),
  });

  const rows = useMemo(() => assets.map(otherAssetRow), [assets]);
  const totals = rows.reduce(
    (acc, r) => ({
      value: acc.value + r.valueCzk,
      cost: acc.cost + r.costCzk,
      yield: acc.yield + r.yieldCzk,
      yielding: acc.yielding + (r.yieldCzk > 0 ? 1 : 0),
    }),
    { value: 0, cost: 0, yield: 0, yielding: 0 }
  );
  const gain = totals.value - totals.cost;
  const latest = assets.reduce<OtherAsset | null>(
    (max, a) => (max === null || a.updatedAt > max.updatedAt ? a : max),
    null
  );

  useShellPage({
    status: {
      text: latest ? t('status.latest', { date: fmt.day(latest.updatedAt) }) : t('status.manual'),
      tone: 'neutral',
    },
  });

  const deleteMutation = useMutation({
    mutationFn: (id: string) => otherAssetsApi.delete(id),
    onSuccess: async () => {
      queryClient.invalidateQueries({ queryKey: ['other-assets'] });
      queryClient.invalidateQueries({ queryKey: ['portfolio-metrics'] });
      queryClient.invalidateQueries({ queryKey: ['cashflow-report'] });
      try {
        await portfolioApi.recordSnapshot();
        queryClient.invalidateQueries({ queryKey: ['portfolio-history'] });
      } catch (error) {
        console.error('Failed to record portfolio snapshot:', error);
      }
      toast(t('toast.deleted'));
      close();
    },
    onError: (error: Error) => {
      toast.error(tc('status.error'), { description: translateApiError(error, tc) });
    },
  });

  const open = (which: Modal) => (asset: OtherAsset) => {
    setSelected(asset);
    setModal(which);
  };
  const close = () => {
    setModal(null);
    setSelected(null);
  };
  const closeIf = (next: boolean) => {
    if (!next) close();
  };

  const isEmpty = !isLoading && assets.length === 0;

  return (
    <>
      <PageHead
        eyebrow={t('eyebrow')}
        title={t('title')}
        description={t('subtitle')}
        actions={
          !isEmpty && (
            <>
              <ExportButton exportFn={exportApi.otherAssets} label={t('export')} />
              <Button onClick={() => setModal('add')}>
                <Plus />
                {t('addAsset')}
              </Button>
            </>
          )
        }
      />

      {isLoading ? (
        <Stats>
          <StatSkeleton />
          <StatSkeleton />
          <StatSkeleton />
          <StatSkeleton />
        </Stats>
      ) : isEmpty ? (
        <EmptyState
          icon={<Gem />}
          title={t('empty.title')}
          description={t('empty.description')}
          action={
            <Button onClick={() => setModal('add')}>
              <Plus />
              {t('addAsset')}
            </Button>
          }
        />
      ) : (
        <>
          <Stats>
            <Stat
              label={t('summary.value')}
              value={formatCurrency(totals.value)}
              note={t('summary.assets', { count: assets.length })}
            />
            <Stat
              label={t('summary.gain')}
              value={formatCurrencySigned(gain)}
              tone={gain > 0 ? 'gain' : gain < 0 ? 'loss' : 'neutral'}
              note={
                totals.cost > 0
                  ? `${fmt.percent(gain / totals.cost, 1, { signed: true })} ${t('summary.vsCost', { amount: formatCurrency(totals.cost) })}`
                  : t('summary.noCost')
              }
            />
            <Stat
              label={t('summary.yearlyYield')}
              value={formatCurrencySigned(totals.yield)}
              tone={totals.yield > 0 ? 'gain' : 'neutral'}
              noteTone="neutral"
              note={
                totals.yield > 0
                  ? t('summary.yieldNote', {
                      percent: fmt.percent(totals.value > 0 ? totals.yield / totals.value : 0, 1),
                      count: totals.yielding,
                    })
                  : t('summary.noYield')
              }
            />
            <Stat
              label={t('summary.lastChange')}
              value={latest ? fmt.day(latest.updatedAt) : '—'}
              note={latest ? latest.name : undefined}
            />
          </Stats>

          <PortfolioTrendCard type="otherAssets" currentValue={totals.value} />

          <OtherAssetsTable
            rows={rows}
            onViewTransactions={open('transactions')}
            onBuy={open('buy')}
            onSell={open('sell')}
            onEdit={open('edit')}
            onRevalue={open('revalue')}
            onDelete={open('delete')}
          />
        </>
      )}

      <AddOtherAssetModal open={modal === 'add'} onOpenChange={closeIf} />
      <BuyOtherAssetModal asset={selected} open={modal === 'buy'} onOpenChange={closeIf} />
      <SellOtherAssetModal asset={selected} open={modal === 'sell'} onOpenChange={closeIf} />
      {selected && (
        <OtherAssetTransactionsModal
          asset={selected}
          open={modal === 'transactions'}
          onOpenChange={closeIf}
        />
      )}
      <EditOtherAssetModal asset={selected} open={modal === 'edit'} onOpenChange={closeIf} />
      <RevalueDialog
        open={modal === 'revalue'}
        onOpenChange={closeIf}
        kind="otherAsset"
        asset={
          selected
            ? {
                id: selected.id,
                name: selected.name,
                currentValue: Number(selected.marketPrice) || 0,
                currency: selected.currency || 'CZK',
                unit: t('table.unit'),
                quantity: Number(selected.quantity) || 0,
              }
            : null
        }
      />
      <ConfirmDeleteDialog
        open={modal === 'delete'}
        onOpenChange={closeIf}
        title={t('confirmDelete.title')}
        description={t('confirmDelete.description', { name: selected?.name ?? '' })}
        onConfirm={() => selected && deleteMutation.mutate(selected.id)}
        isPending={deleteMutation.isPending}
        confirmLabel={t('actions.delete')}
      />
    </>
  );
}
