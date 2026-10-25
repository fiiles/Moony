import { useTranslation } from 'react-i18next';
import { Loader2 } from 'lucide-react';
import type { CryptoSearchResult } from '@/lib/tauri-api';
import type { CryptoSearchState } from '@/hooks/use-crypto-search';
import { AssetLogo } from '@/components/common/AssetLogo';

/**
 * The suggestion box under a coin search field (design system §6, the add dialog's list): a
 * searching line, the error or no-match line, or the matches with name, ticker and Yahoo symbol
 * (the symbol tells same-name listings apart). Picking a row hands its Yahoo listing to the
 * dialog.
 */
export function CryptoSearchList({
  search,
  onPick,
}: {
  search: CryptoSearchState;
  onPick: (result: CryptoSearchResult) => void;
}) {
  const { t } = useTranslation('crypto');
  const { waiting, results, failed } = search;

  return (
    <div
      className="overflow-hidden rounded-r3 border border-line-strong bg-paper shadow-pop"
      aria-live="polite"
    >
      {waiting && results.length === 0 ? (
        <p className="flex items-center gap-2 px-3 py-2.5 text-table text-ink-3">
          <Loader2 className="size-3.5 animate-spin" />
          {t('modal.add.searching')}
        </p>
      ) : failed ? (
        <p className="px-3 py-2.5 text-table text-loss">{t('modal.add.searchError')}</p>
      ) : results.length === 0 ? (
        <p className="px-3 py-2.5 text-table text-ink-3">{t('modal.add.noResults')}</p>
      ) : (
        <ul className="m-0 max-h-56 list-none overflow-y-auto p-1">
          {results.map((result) => (
            <li key={result.symbol}>
              <button
                type="button"
                onClick={() => onPick(result)}
                className="flex w-full items-center gap-2.5 rounded-r1 px-2.5 py-2 text-left text-table text-ink transition-colors duration-fast hover:bg-well focus-visible:bg-well focus-visible:outline-none"
              >
                <AssetLogo ticker={result.ticker} type="crypto" variant="soft" />
                <span className="min-w-0 flex-1 truncate">
                  <b className="font-650">{result.name}</b>{' '}
                  <span className="text-ink-4">{result.ticker}</span>{' '}
                  <span className="text-ink-4">{result.symbol}</span>
                </span>
              </button>
            </li>
          ))}
        </ul>
      )}
    </div>
  );
}
