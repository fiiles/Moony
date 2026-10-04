import { useState } from 'react';
import { useQuery } from '@tanstack/react-query';
import { useTranslation } from 'react-i18next';
import { Loader2, Search } from 'lucide-react';
import { Button } from '@/components/ui/button';
import { Input, InputWrap } from '@/components/ui/input';
import { Label } from '@/components/ui/label';
import { CurrencyCombobox } from '@/components/common/CurrencyCombobox';
import { useDebouncedValue } from '@/hooks/use-debounced-value';
import { priceApi, type StockSearchResult } from '@/lib/tauri-api';
import { exchangeName, resolveTickerCurrency } from '@/utils/exchange-names';
import type { StockImportInstrument, StockInstrumentCandidate } from '@shared/schema';
import { isCurrencyCode, isValidTicker, normalizeTicker } from './import-config';

/** What the user decided in the editor of one instrument. */
export interface InstrumentEdit {
  ticker: string;
  name: string;
  /** Only with a currency taken from the listing; null leaves it as it is. */
  currency: string | null;
  /** The listing the user picked from the results (already confirmed on Yahoo Finance). */
  picked: StockInstrumentCandidate | null;
}

/** Pause after the last keystroke before the ticker search starts. */
const SEARCH_DEBOUNCE_MS = 400;
/** Typing shorter than this does not search. */
const MIN_SEARCH_CHARS = 2;
/** The result of a search is kept for the session so retyping a name is instant. */
const SEARCH_STALE_MS = 5 * 60 * 1000;

interface InstrumentEditorProps {
  instrument: StockImportInstrument;
  /** The name now shown for the instrument (the user's, the file's or Yahoo Finance's). */
  name: string;
  /** Listings Yahoo Finance offered for this instrument, to pick from without typing. */
  candidates: readonly StockInstrumentCandidate[];
  onApply: (edit: InstrumentEdit) => void;
  onCancel: () => void;
}

/**
 * Inline editor of one instrument (spec §3, step 3): the symbol, with the
 * Yahoo Finance search the Add investment dialog uses, the name and — when the
 * currency comes from the listing — the currency. A change applies to every
 * trade of the instrument.
 */
export function InstrumentEditor({
  instrument,
  name,
  candidates,
  onApply,
  onCancel,
}: InstrumentEditorProps) {
  const { t } = useTranslation('stocks');
  const { t: tc } = useTranslation('common');
  const [ticker, setTicker] = useState(instrument.ticker ?? instrument.symbol ?? '');
  const [typedName, setTypedName] = useState(name);
  const [currency, setCurrency] = useState<string | null>(instrument.currency);
  const [picked, setPicked] = useState<StockInstrumentCandidate | null>(null);
  // Whatever was typed last in either field is what the search looks for.
  const [searchText, setSearchText] = useState('');
  // The list gives way to the choice made in it until the user types again.
  const [listOpen, setListOpen] = useState(true);
  const debounced = useDebouncedValue(searchText, SEARCH_DEBOUNCE_MS);
  const query = debounced.trim();
  const typingPending = debounced !== searchText;

  const {
    data: found = [],
    isFetching,
    isError,
  } = useQuery<StockSearchResult[]>({
    queryKey: ['stock-ticker-search', query],
    queryFn: () => priceApi.searchStockTickers(query),
    enabled: query.length >= MIN_SEARCH_CHARS && !typingPending,
    staleTime: SEARCH_STALE_MS,
  });

  const searching = searchText.trim().length >= MIN_SEARCH_CHARS;
  const waiting = searching && (typingPending || isFetching);
  const results: StockInstrumentCandidate[] = searching
    ? found.map((result) => ({
        symbol: result.symbol,
        name: result.shortname || result.symbol,
        exchange: result.exchange,
        currency: resolveTickerCurrency(result.exchange).currency,
      }))
    : [...candidates];

  const tickerValid = isValidTicker(ticker);
  // An empty field is not an error yet, only a reason the button waits.
  const tickerInvalid = ticker.trim() !== '' && !tickerValid;
  const currencyValid = currency == null || isCurrencyCode(currency);
  const canApply = tickerValid && currencyValid;

  const typeSearch = (value: string) => {
    setSearchText(value);
    setPicked(null);
    setListOpen(true);
  };

  const pick = (candidate: StockInstrumentCandidate) => {
    setTicker(candidate.symbol);
    setTypedName(candidate.name);
    setCurrency(candidate.currency);
    setPicked(candidate);
    setSearchText('');
    setListOpen(false);
  };

  const apply = () => {
    if (!canApply) return;
    onApply({
      ticker: normalizeTicker(ticker),
      name: typedName,
      // Only a change of the currency is an override: the file's own stays as it is.
      currency: currency && currency !== instrument.currency ? currency : null,
      picked,
    });
  };

  const showList = listOpen && (searching || results.length > 0);

  return (
    <div className="rounded-r3 border border-line bg-paper px-4 py-3.5">
      <div className="grid grid-cols-[1fr_1.4fr_1fr] gap-3">
        <div className="grid gap-1.5">
          <Label htmlFor={`edit-symbol-${instrument.key}`}>{t('importWizard.roles.symbol')}</Label>
          <InputWrap
            icon={searching && waiting ? <Loader2 className="animate-spin" /> : <Search />}
          >
            <Input
              id={`edit-symbol-${instrument.key}`}
              value={ticker}
              autoFocus
              autoComplete="off"
              spellCheck={false}
              aria-invalid={tickerInvalid || undefined}
              onChange={(e) => {
                setTicker(e.target.value);
                typeSearch(e.target.value);
              }}
              onKeyDown={(e) => {
                if (e.key === 'Enter') {
                  e.preventDefault();
                  apply();
                }
              }}
            />
          </InputWrap>
          {tickerInvalid && (
            <p className="m-0 text-micro font-600 text-loss">{tc('validation.tickerInvalid')}</p>
          )}
        </div>
        <div className="grid gap-1.5">
          <Label htmlFor={`edit-name-${instrument.key}`}>{t('importWizard.roles.name')}</Label>
          <Input
            id={`edit-name-${instrument.key}`}
            value={typedName}
            autoComplete="off"
            onChange={(e) => {
              setTypedName(e.target.value);
              typeSearch(e.target.value);
            }}
            onKeyDown={(e) => {
              if (e.key === 'Enter') {
                e.preventDefault();
                apply();
              }
            }}
          />
        </div>
        <div className="grid gap-1.5">
          <Label>{t('importWizard.roles.currency')}</Label>
          <CurrencyCombobox
            value={currency && isCurrencyCode(currency) ? currency : ''}
            onChange={setCurrency}
            showName={false}
            aria-label={t('importWizard.roles.currency')}
          />
        </div>
      </div>

      {showList && (
        <div
          className="mt-3 overflow-hidden rounded-r3 border border-line-strong bg-paper"
          aria-live="polite"
        >
          <p className="m-0 border-b border-line-soft bg-well px-3 py-1.5 text-micro font-600 text-ink-4">
            {searching ? t('importWizard.editor.searchResults') : t('importWizard.editor.found')}
          </p>
          {waiting && results.length === 0 ? (
            <p className="m-0 flex items-center gap-2 px-3 py-2.5 text-table text-ink-3">
              <Loader2 className="size-3.5 animate-spin" aria-hidden />
              {t('importWizard.editor.searching')}
            </p>
          ) : isError && searching ? (
            <p className="m-0 px-3 py-2.5 text-table text-loss">
              {t('importWizard.editor.searchError')}
            </p>
          ) : results.length === 0 ? (
            <p className="m-0 px-3 py-2.5 text-table text-ink-3">
              {t('importWizard.editor.noResults')}
            </p>
          ) : (
            <ul className="m-0 max-h-48 list-none overflow-y-auto p-1">
              {results.map((candidate) => (
                <li key={`${candidate.symbol}-${candidate.exchange}`}>
                  <button
                    type="button"
                    onClick={() => pick(candidate)}
                    className="flex w-full items-baseline gap-2.5 rounded-r1 px-2.5 py-2 text-left text-table text-ink transition-colors duration-fast hover:bg-well focus-visible:bg-well focus-visible:outline-none"
                  >
                    <b className="w-20 shrink-0 font-650 num">{candidate.symbol}</b>
                    <span className="min-w-0 flex-1 truncate text-ink-2">{candidate.name}</span>
                    <span className="shrink-0 text-micro font-500 text-ink-4">
                      {exchangeName(candidate.exchange)} · {candidate.currency}
                    </span>
                  </button>
                </li>
              ))}
            </ul>
          )}
        </div>
      )}

      <div className="mt-3.5 flex justify-end gap-2">
        <Button type="button" variant="ghost" size="sm" onClick={onCancel}>
          {tc('buttons.cancel')}
        </Button>
        <Button type="button" size="sm" disabled={!canApply} onClick={apply}>
          {t('importWizard.editor.apply')}
        </Button>
      </div>
    </div>
  );
}
