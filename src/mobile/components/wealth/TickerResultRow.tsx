import type { StockSearchResult } from '@/lib/tauri-api';
import { exchangeName } from '@/utils/exchange-names';
import { EntityRow, RowChip } from '@/mobile/components/EntityRow';

/**
 * A hit of the ticker search under Sledované (spec §8.8): the symbol with the company and its
 * exchange in the sub-line, a chevron because it opens something. A tap opens the watch detail of
 * the symbol (`onPick`); following happens there, as on the desktop.
 */
export function TickerResultRow({
  result,
  onPick,
}: {
  result: StockSearchResult;
  onPick: (symbol: string) => void;
}) {
  return (
    <EntityRow
      chip={<RowChip>{result.symbol.slice(0, 2).toUpperCase()}</RowChip>}
      title={result.symbol}
      sub={[result.shortname, exchangeName(result.exchange)].filter(Boolean).join(' · ')}
      chevron
      onClick={() => onPick(result.symbol)}
    />
  );
}
