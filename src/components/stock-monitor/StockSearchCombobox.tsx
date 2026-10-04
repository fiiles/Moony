import { useEffect, useRef, useState } from 'react';
import { useLocation } from 'wouter';
import { useTranslation } from 'react-i18next';
import { Search } from 'lucide-react';
import { Input, InputWrap } from '@/components/ui/input';
import { cn } from '@/lib/utils';
import { priceApi } from '@/lib/tauri-api';

type SearchResult = Awaited<ReturnType<typeof priceApi.searchStockTickers>>[number];

interface StockSearchComboboxProps {
  className?: string;
}

/**
 * Ticker search (prototype `stock-monitor.html`): a wide search field whose
 * results drop down as a pop surface; picking one opens the ticker's detail,
 * where it can be followed.
 */
export function StockSearchCombobox({ className }: StockSearchComboboxProps) {
  const { t } = useTranslation('stockMonitor');
  const [, setLocation] = useLocation();
  const [query, setQuery] = useState('');
  const [results, setResults] = useState<SearchResult[]>([]);
  const [searching, setSearching] = useState(false);
  const [open, setOpen] = useState(false);
  const containerRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    if (query.trim().length < 2) {
      // Already reset synchronously by handleQueryChange below.
      return;
    }
    let cancelled = false;
    const handle = setTimeout(async () => {
      setSearching(true);
      try {
        const found = await priceApi.searchStockTickers(query.trim());
        if (!cancelled) {
          setResults(found);
          setOpen(true);
        }
      } catch {
        if (!cancelled) {
          setResults([]);
          setOpen(true);
        }
      } finally {
        if (!cancelled) setSearching(false);
      }
    }, 300);
    return () => {
      cancelled = true;
      clearTimeout(handle);
    };
  }, [query]);

  // Close the dropdown on outside click
  useEffect(() => {
    const onClick = (e: MouseEvent) => {
      if (!containerRef.current?.contains(e.target as Node)) setOpen(false);
    };
    document.addEventListener('mousedown', onClick);
    return () => document.removeEventListener('mousedown', onClick);
  }, []);

  const select = (symbol: string) => {
    setOpen(false);
    setQuery('');
    setLocation(`/stock-monitor/${encodeURIComponent(symbol)}`);
  };

  const handleQueryChange = (value: string) => {
    setQuery(value);
    if (value.trim().length < 2) {
      // Close immediately instead of waiting for the debounced effect.
      setResults([]);
      setOpen(false);
    }
  };

  return (
    <div ref={containerRef} className={cn('relative w-[320px]', className)}>
      <InputWrap icon={<Search />}>
        <Input
          value={query}
          onChange={(e) => handleQueryChange(e.target.value)}
          onKeyDown={(e) => {
            if (e.key === 'Escape') setOpen(false);
          }}
          placeholder={t('search.placeholder')}
          aria-label={t('search.placeholder')}
          data-testid="stock-monitor-search"
        />
      </InputWrap>
      {open && (
        <div className="absolute z-50 mt-1.5 w-full overflow-hidden rounded-r3 border border-line-strong bg-paper shadow-pop">
          {searching ? (
            <div className="px-3 py-2.5 text-table text-ink-3">{t('search.searching')}</div>
          ) : results.length === 0 ? (
            <div className="px-3 py-2.5 text-table text-ink-3">{t('search.noResults')}</div>
          ) : (
            <ul className="m-0 max-h-72 list-none overflow-auto p-1">
              {results.map((r) => (
                <li key={r.symbol}>
                  <button
                    type="button"
                    onClick={() => select(r.symbol)}
                    className="flex w-full items-center gap-2.5 rounded-r1 px-2.5 py-2 text-left text-table text-ink transition-colors duration-fast hover:bg-well focus-visible:bg-well focus-visible:outline-none"
                  >
                    <b className="w-[64px] shrink-0 font-650 num">{r.symbol}</b>
                    <span className="flex-1 truncate text-ink-2">{r.shortname}</span>
                    <span className="text-micro font-500 text-ink-4">{r.exchange}</span>
                  </button>
                </li>
              ))}
            </ul>
          )}
        </div>
      )}
    </div>
  );
}
