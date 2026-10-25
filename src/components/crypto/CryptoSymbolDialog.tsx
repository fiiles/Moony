import { useState } from 'react';
import { useMutation, useQueryClient } from '@tanstack/react-query';
import { useTranslation } from 'react-i18next';
import { toast } from 'sonner';
import { Search } from 'lucide-react';
import type { CryptoRow } from '@/utils/positions';
import { cryptoApi, type CryptoSearchResult } from '@/lib/tauri-api';
import { invalidateAfterPriceRefresh } from '@/lib/price-refresh';
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogHeader,
  DialogTitle,
} from '@/components/ui/dialog';
import { Input, InputWrap } from '@/components/ui/input';
import { CryptoSearchList } from '@/components/crypto/CryptoSearchList';
import { useCryptoSearch } from '@/hooks/use-crypto-search';

interface CryptoSymbolDialogProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  investment: CryptoRow | null;
}

/**
 * "Zdroj kurzu" (design system §6 Modal): says which Yahoo Finance listing a coin follows and
 * lets the user search for another (the search opens on the coin's name); picking a row stores it, and the rate and the history are
 * rebuilt from it. The search list is the add dialog's.
 */
export function CryptoSymbolDialog({ open, onOpenChange, investment }: CryptoSymbolDialogProps) {
  const { t } = useTranslation('crypto');
  const { t: tc } = useTranslation('common');
  const queryClient = useQueryClient();
  // `null` until the user types: the field then shows (and searches) the coin's own name
  const [typedQuery, setTypedQuery] = useState<string | null>(null);
  const query = typedQuery ?? investment?.name ?? '';
  const search = useCryptoSearch({ typed: query, enabled: open });

  const handleOpenChange = (next: boolean) => {
    if (!next) setTypedQuery(null);
    onOpenChange(next);
  };

  const setSymbol = useMutation({
    mutationFn: (result: CryptoSearchResult) => {
      if (!investment) throw new Error('no coin selected');
      return cryptoApi.setSymbol(investment.id, result.symbol);
    },
    onSuccess: async () => {
      queryClient.invalidateQueries({ queryKey: ['crypto'] });
      queryClient.invalidateQueries({ queryKey: ['all-crypto-transactions'] });
      void invalidateAfterPriceRefresh(queryClient);
      toast(t('modal.symbol.saved'));
      handleOpenChange(false);
    },
    onError: (error: Error) => {
      toast.error(tc('status.error'), { description: error.message });
    },
  });

  if (!investment) return null;

  return (
    <Dialog open={open} onOpenChange={handleOpenChange}>
      <DialogContent className="max-w-[440px]">
        <DialogHeader>
          <DialogTitle>{t('modal.symbol.title', { name: investment.name })}</DialogTitle>
          <DialogDescription>{t('modal.symbol.description')}</DialogDescription>
        </DialogHeader>
        <div className="space-y-3">
          <p className="text-caption text-ink-3">
            {investment.yahooSymbol
              ? t('modal.symbol.current', { symbol: investment.yahooSymbol })
              : t('modal.symbol.none')}
          </p>
          <InputWrap icon={<Search />}>
            <Input
              value={query}
              onChange={(e) => setTypedQuery(e.target.value)}
              placeholder={investment.name}
              aria-label={t('modal.add.search')}
              autoComplete="off"
              autoFocus
            />
          </InputWrap>
          {search.show ? (
            <CryptoSearchList
              search={search}
              onPick={(result) => !setSymbol.isPending && setSymbol.mutate(result)}
            />
          ) : (
            <p className="text-caption text-ink-3">{t('modal.add.searchHint')}</p>
          )}
        </div>
      </DialogContent>
    </Dialog>
  );
}
