import { useState, useRef, useEffect } from 'react';
import { useQueryClient } from '@tanstack/react-query';
import { useTranslation } from 'react-i18next';
import { listen } from '@tauri-apps/api/event';
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogHeader,
  DialogTitle,
  DialogTrigger,
  DialogFooter,
} from '@/components/ui/dialog';
import {
  AlertDialog,
  AlertDialogCancel,
  AlertDialogContent,
  AlertDialogDescription,
  AlertDialogFooter,
  AlertDialogHeader,
  AlertDialogTitle,
} from '@/components/ui/alert-dialog';
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
import { Collapsible, CollapsibleContent, CollapsibleTrigger } from '@/components/ui/collapsible';
import { Button } from '@/components/ui/button';
import { Input } from '@/components/ui/input';
import { Badge } from '@/components/ui/badge';
import { ScrollArea } from '@/components/ui/scroll-area';
import { Alert, AlertDescription, AlertTitle } from '@/components/ui/alert';
import { Progress } from '@/components/ui/progress';
import {
  Upload,
  FileText,
  AlertCircle,
  CheckCircle,
  Loader2,
  Download,
  Search,
  ChevronDown,
  Eye,
  EyeOff,
  TriangleAlert,
} from 'lucide-react';
import { investmentsApi, priceApi } from '@/lib/tauri-api';
import { exchangeName } from '@/utils/exchange-names';

// ─── Types ────────────────────────────────────────────────────────────────────

type Step = 'select' | 'mapping' | 'ticker-review' | 'importing' | 'done';

type ColumnMap = {
  date: string;
  type: string;
  ticker: string;
  quantity: string;
  price: string;
  currency: string; // empty string = not mapped, use defaultCurrency
};

type TickerStatus = 'pending' | 'searching' | 'found' | 'not_found' | 'failed';

type TickerEntry = {
  originalTicker: string;
  resolvedTicker: string;
  name: string;
  status: TickerStatus;
};

type TickerMap = Record<string, TickerEntry>;

type SearchResult = { symbol: string; shortname: string; exchange: string };

/** Payload of the backend's `stock-import-progress` event (commands/investments.rs) */
type ImportProgress = { phase: 'lookup' | 'import'; current: number; total: number };

// ─── Constants ────────────────────────────────────────────────────────────────

const YAHOO_EXCHANGE_SUFFIXES = [
  { exchangeKey: 'us', suffix: '—', example: 'AAPL' },
  { exchangeKey: 'germany', suffix: '.DE', example: 'EUNL.DE' },
  { exchangeKey: 'london', suffix: '.L', example: 'EWG.L' },
  { exchangeKey: 'paris', suffix: '.PA', example: 'AIR.PA' },
  { exchangeKey: 'amsterdam', suffix: '.AS', example: 'ASML.AS' },
  { exchangeKey: 'milan', suffix: '.MI', example: 'ENI.MI' },
  { exchangeKey: 'swiss', suffix: '.SW', example: 'NESN.SW' },
  { exchangeKey: 'prague', suffix: '.PR', example: 'CEZ.PR' },
  { exchangeKey: 'hongKong', suffix: '.HK', example: '0700.HK' },
  { exchangeKey: 'tokyo', suffix: '.T', example: '7203.T' },
  { exchangeKey: 'toronto', suffix: '.TO', example: 'RY.TO' },
  { exchangeKey: 'australia', suffix: '.AX', example: 'CBA.AX' },
];

const DATE_FORMATS = [
  { pattern: 'YYYY-MM-DD', example: '2024-01-15', value: '%Y-%m-%d' },
  { pattern: 'DD.MM.YYYY', example: '15.01.2024', value: '%d.%m.%Y' },
  { pattern: 'DD/MM/YYYY', example: '15/01/2024', value: '%d/%m/%Y' },
  { pattern: 'MM/DD/YYYY', example: '01/15/2024', value: '%m/%d/%Y' },
];

const CSV_COLUMNS = [
  { key: 'Date', required: true, examples: '2024-01-15, 15.01.2024, 15/01/2024' },
  { key: 'Type', required: true, examples: 'buy, sell' },
  { key: 'Ticker', required: true, examples: 'AAPL, EUNL.DE, EWG.L' },
  { key: 'Quantity', required: true, examples: '10, 0.5' },
  { key: 'Price', required: true, examples: '180.50' },
  { key: 'Currency', required: true, examples: 'USD, EUR, CZK, GBP' },
];

const EMPTY_COLUMN_MAP: ColumnMap = {
  date: '',
  type: '',
  ticker: '',
  quantity: '',
  price: '',
  currency: '',
};
const ZERO_CONFIDENCE: Record<keyof ColumnMap, number> = {
  date: 0,
  type: 0,
  ticker: 0,
  quantity: 0,
  price: 0,
  currency: 0,
};

// ─── Pure helpers ─────────────────────────────────────────────────────────────

function delay(ms: number): Promise<void> {
  return new Promise((resolve) => setTimeout(resolve, ms));
}

function splitCsvLine(line: string, delimiter: string): string[] {
  const result: string[] = [];
  let current = '';
  let inQuote = false;
  for (let i = 0; i < line.length; i++) {
    const ch = line[i];
    if (ch === '"') {
      inQuote = !inQuote;
    } else if (ch === delimiter && !inQuote) {
      result.push(current.trim().replace(/^"|"$/g, ''));
      current = '';
    } else {
      current += ch;
    }
  }
  result.push(current.trim().replace(/^"|"$/g, ''));
  return result;
}

/** Thrown by parseCSV; `i18nKey` is a key in the `stocks` namespace. */
class CsvParseError extends Error {
  constructor(readonly i18nKey: string) {
    super(i18nKey);
    this.name = 'CsvParseError';
  }
}

function parseCSV(text: string): { headers: string[]; rows: Record<string, string>[] } {
  const lines = text.split(/\r?\n/).filter((l) => l.trim());
  if (lines.length === 0) throw new CsvParseError('import.csvEmpty');

  const delimiter = lines[0].includes(';') ? ';' : ',';
  const headers = splitCsvLine(lines[0], delimiter);

  const rows: Record<string, string>[] = [];
  for (let i = 1; i < lines.length; i++) {
    const values = splitCsvLine(lines[i], delimiter);
    if (values.length === 0) continue;
    const row: Record<string, string> = {};
    headers.forEach((h, idx) => {
      row[h] = idx < values.length ? values[idx] : '';
    });
    rows.push(row);
  }

  if (rows.length === 0) throw new CsvParseError('import.noDataRows');
  return { headers, rows };
}

function suggestColumnMap(headers: string[]): {
  map: ColumnMap;
  confidence: Record<keyof ColumnMap, number>;
} {
  const lower = headers.map((h) => h.toLowerCase().trim());

  const findBest = (candidates: string[]): { col: string; conf: number } => {
    for (const c of candidates) {
      const idx = lower.findIndex((h) => h === c);
      if (idx !== -1) return { col: headers[idx], conf: 0.95 };
    }
    for (const c of candidates) {
      const idx = lower.findIndex((h) => h.includes(c) || c.includes(h));
      if (idx !== -1) return { col: headers[idx], conf: 0.7 };
    }
    return { col: '', conf: 0 };
  };

  const date = findBest(['date', 'datum', 'trade date', 'transaction date', 'settlement date']);
  const type = findBest(['type', 'typ', 'transaction type', 'action', 'side', 'operation']);
  const ticker = findBest(['ticker', 'symbol', 'isin', 'instrument']);
  const quantity = findBest(['quantity', 'množství', 'qty', 'shares', 'units', 'počet', 'amount']);
  const price = findBest(['price', 'cena', 'unit price', 'price per unit', 'rate', 'cost']);
  const currency = findBest(['currency', 'měna', 'mena', 'cur', 'ccy']);

  return {
    map: {
      date: date.col,
      type: type.col,
      ticker: ticker.col,
      quantity: quantity.col,
      price: price.col,
      currency: currency.col,
    },
    confidence: {
      date: date.conf,
      type: type.conf,
      ticker: ticker.conf,
      quantity: quantity.conf,
      price: price.conf,
      currency: currency.conf,
    },
  };
}

// ─── Component ────────────────────────────────────────────────────────────────

interface ImportInvestmentsModalProps {
  /**
   * Controlled mode: when `open` is given the parent owns the state and the built-in
   * trigger button is not rendered. Lets the page keep ONE instance mounted while its
   * layout changes (empty state -> table after the import), so the result screen is
   * not torn down.
   */
  open?: boolean;
  onOpenChange?: (open: boolean) => void;
}

export function ImportInvestmentsModal({
  open: controlledOpen,
  onOpenChange,
}: ImportInvestmentsModalProps = {}) {
  const { t } = useTranslation('stocks');
  const { t: tc } = useTranslation('common');
  const [internalOpen, setInternalOpen] = useState(false);
  const isControlled = controlledOpen !== undefined;
  const open = isControlled ? controlledOpen : internalOpen;
  const setOpen = (value: boolean) => {
    if (!isControlled) setInternalOpen(value);
    onOpenChange?.(value);
  };
  const [step, setStep] = useState<Step>('select');

  // File + raw CSV
  const [file, setFile] = useState<File | null>(null);
  const [headers, setHeaders] = useState<string[]>([]);
  const [rawRows, setRawRows] = useState<Record<string, string>[]>([]);
  const [parseError, setParseError] = useState<string | null>(null);

  // Column mapping
  const [columnMap, setColumnMap] = useState<ColumnMap>(EMPTY_COLUMN_MAP);
  const [columnConfidence, setColumnConfidence] =
    useState<Record<keyof ColumnMap, number>>(ZERO_CONFIDENCE);
  const [defaultCurrency, setDefaultCurrency] = useState('USD');
  const [dateFormat, setDateFormat] = useState('%Y-%m-%d');
  const [showCsvPreview, setShowCsvPreview] = useState(false);

  // Ticker review
  const [tickerMap, setTickerMap] = useState<TickerMap>({});
  const [verifyProgress, setVerifyProgress] = useState({ done: 0, total: 0 });
  const verifyAbortRef = useRef(false);
  const [searchResults, setSearchResults] = useState<SearchResult[]>([]);
  const [showPickerDialog, setShowPickerDialog] = useState(false);
  const [activeSearchKey, setActiveSearchKey] = useState<string | null>(null);

  // Import result
  const [importResult, setImportResult] = useState<{
    success: number;
    errors: string[];
    imported: string[];
  } | null>(null);
  const [showDelayedSubtitle, setShowDelayedSubtitle] = useState(false);
  // Live progress while the backend processes the rows (null until the first event)
  const [importProgress, setImportProgress] = useState<ImportProgress | null>(null);
  // Rows as sent to the backend, so the event's row number maps to a ticker
  const importRowsRef = useRef<Record<string, string>[]>([]);

  const fileInputRef = useRef<HTMLInputElement>(null);
  const queryClient = useQueryClient();

  // Delayed subtitle during importing (the flag is reset in handleImport)
  useEffect(() => {
    if (step !== 'importing') return;
    const timer = setTimeout(() => setShowDelayedSubtitle(true), 5000);
    return () => clearTimeout(timer);
  }, [step]);

  // Progress events from the backend while importing
  useEffect(() => {
    if (step !== 'importing') return;
    const unlisten = listen<ImportProgress>('stock-import-progress', (event) => {
      setImportProgress(event.payload);
    });
    return () => {
      unlisten.then((f) => f());
    };
  }, [step]);

  // ── Example CSV download ───────────────────────────────────────────────────

  const exampleCSVContent = `Date,Type,Ticker,Quantity,Price,Currency
2024-01-15,buy,AAPL,10,180.50,USD
2024-02-20,buy,MSFT,5,395.00,USD
2024-03-10,sell,AAPL,3,185.25,USD
2024-04-05,buy,EUNL.DE,2,95.50,EUR`;

  const downloadExampleCSV = async () => {
    try {
      const { save } = await import('@tauri-apps/plugin-dialog');
      const { writeTextFile } = await import('@tauri-apps/plugin-fs');
      const filePath = await save({
        defaultPath: 'example-transactions.csv',
        filters: [{ name: 'CSV', extensions: ['csv'] }],
      });
      if (filePath) await writeTextFile(filePath, exampleCSVContent);
    } catch (err) {
      console.error('Failed to save CSV:', err);
    }
  };

  // ── File parsing ───────────────────────────────────────────────────────────

  const handleFileChange = (e: React.ChangeEvent<HTMLInputElement>) => {
    const selected = e.target.files?.[0];
    if (!selected) return;
    setFile(selected);
    setParseError(null);
    const reader = new FileReader();
    reader.onload = (evt) => {
      try {
        const { headers: h, rows } = parseCSV(evt.target?.result as string);
        setHeaders(h);
        setRawRows(rows);
        const { map, confidence } = suggestColumnMap(h);
        setColumnMap(map);
        setColumnConfidence(confidence);
        setStep('mapping');
      } catch (err: unknown) {
        const detail =
          err instanceof CsvParseError
            ? t(err.i18nKey)
            : err instanceof Error
              ? err.message
              : tc('errors.unknown');
        setParseError(t('import.parseFailed', { message: detail }));
      }
    };
    reader.readAsText(selected);
  };

  const handleReset = () => {
    verifyAbortRef.current = true;
    setFile(null);
    setHeaders([]);
    setRawRows([]);
    setParseError(null);
    setColumnMap(EMPTY_COLUMN_MAP);
    setColumnConfidence(ZERO_CONFIDENCE);
    setDefaultCurrency('USD');
    setDateFormat('%Y-%m-%d');
    setShowCsvPreview(false);
    setTickerMap({});
    setVerifyProgress({ done: 0, total: 0 });
    setImportResult(null);
    setStep('select');
    if (fileInputRef.current) fileInputRef.current.value = '';
  };

  const handleClose = () => {
    handleReset();
    setOpen(false);
  };

  // ── Confidence badge ───────────────────────────────────────────────────────

  const getConfidenceBadge = (field: keyof ColumnMap) => {
    const conf = columnConfidence[field];
    if (conf >= 0.9)
      return (
        <Badge variant="default" className="ml-1 bg-green-600 text-xs px-1">
          ✓ {Math.round(conf * 100)}%
        </Badge>
      );
    if (conf >= 0.7)
      return (
        <Badge variant="secondary" className="ml-1 text-xs px-1">
          ~{Math.round(conf * 100)}%
        </Badge>
      );
    return null;
  };

  const isMappingValid = !!(
    columnMap.date &&
    columnMap.type &&
    columnMap.ticker &&
    columnMap.quantity &&
    columnMap.price &&
    columnMap.currency
  );

  // ── Ticker verification ────────────────────────────────────────────────────

  const startVerification = async (tickers: string[], currentMap: TickerMap) => {
    for (let i = 0; i < tickers.length; i++) {
      if (verifyAbortRef.current) break;
      const origKey = tickers[i];
      setTickerMap((prev) => ({ ...prev, [origKey]: { ...prev[origKey], status: 'searching' } }));
      setVerifyProgress({ done: i, total: tickers.length });

      try {
        const searchQuery = currentMap[origKey]?.resolvedTicker || origKey;
        const results = await priceApi.searchStockTickers(searchQuery);
        if (results.length > 0) {
          const best =
            results.find((r) => r.symbol.toUpperCase() === searchQuery.toUpperCase()) ?? results[0];
          setTickerMap((prev) => ({
            ...prev,
            [origKey]: {
              ...prev[origKey],
              resolvedTicker: best.symbol,
              name: best.shortname || '',
              status: 'found',
            },
          }));
        } else {
          setTickerMap((prev) => ({
            ...prev,
            [origKey]: { ...prev[origKey], status: 'not_found' },
          }));
        }
      } catch {
        setTickerMap((prev) => ({ ...prev, [origKey]: { ...prev[origKey], status: 'failed' } }));
      }

      if (i < tickers.length - 1) await delay(300);
    }
    setVerifyProgress((prev) => ({ ...prev, done: tickers.length }));
  };

  const enterTickerReview = () => {
    const uniqueTickers = [
      ...new Set(rawRows.map((row) => row[columnMap.ticker] || '').filter(Boolean)),
    ];
    const initial: TickerMap = {};
    uniqueTickers.forEach((ticker) => {
      initial[ticker] = {
        originalTicker: ticker,
        resolvedTicker: ticker,
        name: '',
        status: 'pending',
      };
    });
    setTickerMap(initial);
    setVerifyProgress({ done: 0, total: uniqueTickers.length });
    verifyAbortRef.current = false;
    setStep('ticker-review');
    startVerification(uniqueTickers, initial);
  };

  const retryFailed = () => {
    const failedKeys = Object.keys(tickerMap).filter((k) => tickerMap[k].status === 'failed');
    if (failedKeys.length === 0) return;
    verifyAbortRef.current = false;
    setVerifyProgress({ done: 0, total: failedKeys.length });
    startVerification(failedKeys, tickerMap);
  };

  const searchForTicker = async (originalKey: string) => {
    const entry = tickerMap[originalKey];
    if (!entry || entry.status === 'searching') return;
    setActiveSearchKey(originalKey);
    setTickerMap((prev) => ({
      ...prev,
      [originalKey]: { ...prev[originalKey], status: 'searching' },
    }));
    try {
      const results = await priceApi.searchStockTickers(entry.resolvedTicker);
      if (results.length === 0) {
        setTickerMap((prev) => ({
          ...prev,
          [originalKey]: { ...prev[originalKey], status: 'not_found' },
        }));
        setActiveSearchKey(null);
      } else if (results.length === 1) {
        setTickerMap((prev) => ({
          ...prev,
          [originalKey]: {
            ...prev[originalKey],
            resolvedTicker: results[0].symbol,
            name: results[0].shortname || '',
            status: 'found',
          },
        }));
        setActiveSearchKey(null);
      } else {
        setSearchResults(results);
        setShowPickerDialog(true);
        // status stays "searching" until user picks from dialog
      }
    } catch {
      setTickerMap((prev) => ({
        ...prev,
        [originalKey]: { ...prev[originalKey], status: 'failed' },
      }));
      setActiveSearchKey(null);
    }
  };

  const selectFromPicker = (result: SearchResult) => {
    if (!activeSearchKey) return;
    setTickerMap((prev) => ({
      ...prev,
      [activeSearchKey]: {
        ...prev[activeSearchKey],
        resolvedTicker: result.symbol,
        name: result.shortname || '',
        status: 'found',
      },
    }));
    setShowPickerDialog(false);
    setSearchResults([]);
    setActiveSearchKey(null);
  };

  const cancelPicker = () => {
    if (activeSearchKey) {
      setTickerMap((prev) => ({
        ...prev,
        [activeSearchKey]: { ...prev[activeSearchKey], status: 'not_found' },
      }));
    }
    setShowPickerDialog(false);
    setSearchResults([]);
    setActiveSearchKey(null);
  };

  const isTickerReviewValid =
    Object.values(tickerMap).length > 0 &&
    Object.values(tickerMap).every(
      (e) =>
        e.status !== 'pending' &&
        e.status !== 'searching' &&
        (e.status === 'found' || e.name.trim().length > 0)
    );

  // ── Row transform + import ────────────────────────────────────────────────

  const transformRows = (): Record<string, string>[] => {
    const mapped = rawRows.map((row) => {
      const origTicker = row[columnMap.ticker] || '';
      const entry = tickerMap[origTicker];
      return {
        Date: row[columnMap.date] || '',
        Type: row[columnMap.type] || '',
        Ticker: entry?.resolvedTicker || origTicker,
        Name: entry?.name || '',
        Quantity: row[columnMap.quantity] || '',
        Price: row[columnMap.price] || '',
        Currency: row[columnMap.currency] || '',
      };
    });
    // Buys before sells: the backend requires a position to exist before a sell can be recorded.
    // A stable sort ensures sells that arrive before their buy in the CSV don't fail.
    return mapped.sort((a, b) => {
      const aIsBuy = a.Type.toLowerCase() === 'buy' ? 0 : 1;
      const bIsBuy = b.Type.toLowerCase() === 'buy' ? 0 : 1;
      return aIsBuy - bIsBuy;
    });
  };

  const handleImport = async () => {
    const transformed = transformRows();
    importRowsRef.current = transformed;
    setImportProgress(null);
    setShowDelayedSubtitle(false);
    setStep('importing');
    try {
      const result = await investmentsApi.importTransactions(transformed, defaultCurrency);
      queryClient.invalidateQueries({ queryKey: ['investments'] });
      queryClient.invalidateQueries({ queryKey: ['transactions'] });
      queryClient.invalidateQueries({ queryKey: ['portfolio-metrics'] });
      queryClient.invalidateQueries({ queryKey: ['all-stock-transactions'] });
      setImportResult({
        success: result.success,
        errors: result.errors || [],
        imported: result.imported || [],
      });
      setStep('done');

      // Background: refresh prices + dividends after import (fire-and-forget)
      priceApi
        .refreshStockPrices()
        .then(() => {
          queryClient.invalidateQueries({ queryKey: ['investments'] });
          queryClient.invalidateQueries({ queryKey: ['portfolio-metrics'] });
          return priceApi.refreshDividends();
        })
        .then(() => {
          queryClient.invalidateQueries({ queryKey: ['investments'] });
          queryClient.invalidateQueries({ queryKey: ['dividend-summary'] });
        })
        .catch((err: Error) => {
          console.error('Background refresh error:', err);
        });
    } catch (err: unknown) {
      console.error('Import error:', err);
      setImportResult({
        success: 0,
        errors: [err instanceof Error ? err.message : t('import.failed')],
        imported: [],
      });
      setStep('done');
    }
  };

  // ═══════════════════════════════════════════════════════════════════════════
  //  RENDER
  // ═══════════════════════════════════════════════════════════════════════════

  return (
    <Dialog
      open={open}
      onOpenChange={(val) => {
        // The import cannot be cancelled once it started: closing now would hide
        // the result and invite a duplicate import
        if (!val && step === 'importing') return;
        setOpen(val);
        if (!val) handleReset();
      }}
    >
      {!isControlled && (
        <DialogTrigger asChild>
          <Button variant="outline" size="icon" title={t('importCSV')}>
            <Upload className="h-4 w-4" />
          </Button>
        </DialogTrigger>
      )}
      <DialogContent
        className="max-w-3xl max-h-[90vh] overflow-y-auto"
        hideCloseButton={step === 'importing'}
        onEscapeKeyDown={(e) => {
          if (step === 'importing') e.preventDefault();
        }}
        onPointerDownOutside={(e) => {
          if (step === 'importing') e.preventDefault();
        }}
        onInteractOutside={(e) => {
          if (step === 'importing') e.preventDefault();
        }}
      >
        <DialogHeader>
          <DialogTitle className="flex items-center gap-2">
            <FileText className="h-5 w-5" />
            {t('import.title')}
          </DialogTitle>
          <DialogDescription>
            {step === 'select' && t('import.description')}
            {step === 'mapping' && t('import.mapping.description')}
            {step === 'ticker-review' && t('import.tickerReview.description')}
            {step === 'importing' && t('import.status.processing')}
            {step === 'done' && t('import.complete')}
          </DialogDescription>
        </DialogHeader>

        {/* ── Step 1: Select File ───────────────────────────────────── */}
        {step === 'select' && (
          <div className="space-y-4">
            {/* CSV format table */}
            <div className="border rounded-r2 overflow-hidden">
              <div className="bg-well px-3 py-2 text-sm font-medium">
                {t('import.csvFormatTitle')}
              </div>
              <Table>
                <TableHeader>
                  <TableRow>
                    <TableHead className="text-xs w-28">{t('import.csvColumnHeader')}</TableHead>
                    <TableHead className="text-xs w-24">{t('import.csvRequiredHeader')}</TableHead>
                    <TableHead className="text-xs">{t('import.csvExamplesHeader')}</TableHead>
                  </TableRow>
                </TableHeader>
                <TableBody>
                  {CSV_COLUMNS.map((col) => (
                    <TableRow key={col.key}>
                      <TableCell className="font-mono text-xs font-medium">{col.key}</TableCell>
                      <TableCell className="text-xs">
                        {col.required ? '✓' : t('import.csvOptional')}
                      </TableCell>
                      <TableCell className="text-xs text-ink-3">{col.examples}</TableCell>
                    </TableRow>
                  ))}
                </TableBody>
              </Table>
            </div>
            <p className="text-xs text-ink-3">{t('import.columnMappingHint')}</p>
            <p className="text-xs text-ink-3">{t('import.nameAutoLoaded')}</p>

            {/* Ticker format collapsible */}
            <Collapsible>
              <CollapsibleTrigger className="flex items-center gap-1 text-sm text-ink hover:underline">
                <ChevronDown className="h-3 w-3" />
                {t('import.tickerFormatHint')}
              </CollapsibleTrigger>
              <CollapsibleContent className="mt-2">
                <div className="border rounded-r2 overflow-hidden">
                  <Table>
                    <TableHeader>
                      <TableRow>
                        <TableHead className="text-xs">{t('import.exchangeHeader')}</TableHead>
                        <TableHead className="text-xs w-20">{t('import.suffixHeader')}</TableHead>
                        <TableHead className="text-xs w-28">{t('import.exampleHeader')}</TableHead>
                      </TableRow>
                    </TableHeader>
                    <TableBody>
                      {YAHOO_EXCHANGE_SUFFIXES.map((row) => (
                        <TableRow key={row.example}>
                          <TableCell className="text-xs">
                            {t(`import.exchanges.${row.exchangeKey}`)}
                          </TableCell>
                          <TableCell className="font-mono text-xs">{row.suffix}</TableCell>
                          <TableCell className="font-mono text-xs">{row.example}</TableCell>
                        </TableRow>
                      ))}
                    </TableBody>
                  </Table>
                </div>
              </CollapsibleContent>
            </Collapsible>

            {parseError && (
              <Alert variant="destructive">
                <AlertCircle className="h-4 w-4" />
                <AlertTitle>{t('import.error')}</AlertTitle>
                <AlertDescription>{parseError}</AlertDescription>
              </Alert>
            )}

            {/* Drop zone */}
            <div
              className="border-2 border-dashed rounded-r2 p-10 text-center cursor-pointer hover:border-line-hover transition-colors"
              onClick={() => fileInputRef.current?.click()}
            >
              <Upload className="h-10 w-10 mx-auto text-ink-3 mb-3" />
              <p className="text-base font-medium">{t('import.clickToUpload')}</p>
              <p className="text-sm text-ink-3 mt-1">{t('import.dragAndDrop')}</p>
              <Input
                ref={fileInputRef}
                type="file"
                accept=".csv"
                className="hidden"
                onChange={handleFileChange}
              />
            </div>

            <button
              type="button"
              onClick={downloadExampleCSV}
              className="inline-flex items-center gap-1 text-xs text-ink hover:underline"
            >
              <Download className="h-3 w-3" />
              {t('import.downloadTemplate')}
            </button>
          </div>
        )}

        {/* ── Step 2: Column Mapping ───────────────────────────────────────── */}
        {step === 'mapping' && (
          <div className="space-y-6">
            {/* File info bar */}
            <div className="flex items-center gap-2 p-3 bg-well rounded-r2">
              <FileText className="h-5 w-5 shrink-0" />
              <span className="font-medium truncate">{file?.name}</span>
              <span className="text-ink-3 shrink-0">
                ({rawRows.length} {t('import.mapping.rows')})
              </span>
              <Button
                variant="ghost"
                size="sm"
                onClick={() => setStep('select')}
                className="ml-auto shrink-0"
              >
                {t('import.mapping.changeFile')}
              </Button>
            </div>

            {/* Required columns */}
            <div className="space-y-3">
              <h4 className="text-sm font-medium">{t('import.mapping.requiredSection')}</h4>
              <div className="grid grid-cols-2 gap-4">
                {/* Date + format selector */}
                <div className="space-y-2">
                  <div className="flex items-center text-sm font-medium">
                    {t('import.mapping.dateColumn')} *{getConfidenceBadge('date')}
                  </div>
                  <Select
                    value={columnMap.date}
                    onValueChange={(v) => setColumnMap((prev) => ({ ...prev, date: v }))}
                  >
                    <SelectTrigger>
                      <SelectValue placeholder={t('import.mapping.selectColumn')} />
                    </SelectTrigger>
                    <SelectContent>
                      {headers.map((h) => (
                        <SelectItem key={h} value={h}>
                          {h}
                        </SelectItem>
                      ))}
                    </SelectContent>
                  </Select>
                  <Select value={dateFormat} onValueChange={setDateFormat}>
                    <SelectTrigger className="text-xs h-8">
                      <SelectValue />
                    </SelectTrigger>
                    <SelectContent>
                      {DATE_FORMATS.map((f) => (
                        <SelectItem key={f.value} value={f.value}>
                          {t('import.dateFormatOption', { pattern: f.pattern, example: f.example })}
                        </SelectItem>
                      ))}
                    </SelectContent>
                  </Select>
                </div>

                {/* Type */}
                <div className="space-y-2">
                  <div className="flex items-center text-sm font-medium">
                    {t('import.mapping.typeColumn')} *{getConfidenceBadge('type')}
                  </div>
                  <Select
                    value={columnMap.type}
                    onValueChange={(v) => setColumnMap((prev) => ({ ...prev, type: v }))}
                  >
                    <SelectTrigger>
                      <SelectValue placeholder={t('import.mapping.selectColumn')} />
                    </SelectTrigger>
                    <SelectContent>
                      {headers.map((h) => (
                        <SelectItem key={h} value={h}>
                          {h}
                        </SelectItem>
                      ))}
                    </SelectContent>
                  </Select>
                </div>

                {/* Ticker */}
                <div className="space-y-2">
                  <div className="flex items-center text-sm font-medium">
                    {t('import.mapping.tickerColumn')} *{getConfidenceBadge('ticker')}
                  </div>
                  <Select
                    value={columnMap.ticker}
                    onValueChange={(v) => setColumnMap((prev) => ({ ...prev, ticker: v }))}
                  >
                    <SelectTrigger>
                      <SelectValue placeholder={t('import.mapping.selectColumn')} />
                    </SelectTrigger>
                    <SelectContent>
                      {headers.map((h) => (
                        <SelectItem key={h} value={h}>
                          {h}
                        </SelectItem>
                      ))}
                    </SelectContent>
                  </Select>
                </div>

                {/* Quantity */}
                <div className="space-y-2">
                  <div className="flex items-center text-sm font-medium">
                    {t('import.mapping.quantityColumn')} *{getConfidenceBadge('quantity')}
                  </div>
                  <Select
                    value={columnMap.quantity}
                    onValueChange={(v) => setColumnMap((prev) => ({ ...prev, quantity: v }))}
                  >
                    <SelectTrigger>
                      <SelectValue placeholder={t('import.mapping.selectColumn')} />
                    </SelectTrigger>
                    <SelectContent>
                      {headers.map((h) => (
                        <SelectItem key={h} value={h}>
                          {h}
                        </SelectItem>
                      ))}
                    </SelectContent>
                  </Select>
                </div>

                {/* Price */}
                <div className="space-y-2">
                  <div className="flex items-center text-sm font-medium">
                    {t('import.mapping.priceColumn')} *{getConfidenceBadge('price')}
                  </div>
                  <Select
                    value={columnMap.price}
                    onValueChange={(v) => setColumnMap((prev) => ({ ...prev, price: v }))}
                  >
                    <SelectTrigger>
                      <SelectValue placeholder={t('import.mapping.selectColumn')} />
                    </SelectTrigger>
                    <SelectContent>
                      {headers.map((h) => (
                        <SelectItem key={h} value={h}>
                          {h}
                        </SelectItem>
                      ))}
                    </SelectContent>
                  </Select>
                </div>

                {/* Currency */}
                <div className="space-y-2">
                  <div className="flex items-center text-sm font-medium">
                    {t('import.mapping.currencyColumn')} *{getConfidenceBadge('currency')}
                  </div>
                  <Select
                    value={columnMap.currency}
                    onValueChange={(v) => setColumnMap((prev) => ({ ...prev, currency: v }))}
                  >
                    <SelectTrigger>
                      <SelectValue placeholder={t('import.mapping.selectColumn')} />
                    </SelectTrigger>
                    <SelectContent>
                      {headers.map((h) => (
                        <SelectItem key={h} value={h}>
                          {h}
                        </SelectItem>
                      ))}
                    </SelectContent>
                  </Select>
                </div>
              </div>
            </div>

            {/* CSV preview toggle */}
            <div className="border rounded-r2">
              <Button
                variant="ghost"
                className="w-full flex items-center justify-between p-3"
                onClick={() => setShowCsvPreview((p) => !p)}
              >
                <span className="flex items-center gap-2">
                  {showCsvPreview ? <EyeOff className="h-4 w-4" /> : <Eye className="h-4 w-4" />}
                  {t('import.mapping.previewData')}
                </span>
                <span className="text-ink-3 text-sm">
                  {headers.length} {t('import.mapping.columns')}, {rawRows.length}{' '}
                  {t('import.mapping.rows')}
                </span>
              </Button>
              {showCsvPreview && (
                <div className="overflow-x-auto max-h-48 border-t">
                  <Table>
                    <TableHeader>
                      <TableRow>
                        {headers.map((h) => (
                          <TableHead key={h} className="whitespace-nowrap text-xs">
                            {h}
                          </TableHead>
                        ))}
                      </TableRow>
                    </TableHeader>
                    <TableBody>
                      {rawRows.slice(0, 5).map((row, i) => (
                        <TableRow key={i}>
                          {headers.map((h) => (
                            <TableCell
                              key={h}
                              className="whitespace-nowrap text-xs max-w-[150px] truncate"
                            >
                              {row[h]}
                            </TableCell>
                          ))}
                        </TableRow>
                      ))}
                    </TableBody>
                  </Table>
                </div>
              )}
            </div>
          </div>
        )}

        {/* ── Step 3: Ticker Review ────────────────────────────────────────── */}
        {step === 'ticker-review' && (
          <div className="space-y-4">
            {/* Progress */}
            {verifyProgress.done < verifyProgress.total && (
              <div className="space-y-1">
                <p className="text-sm text-ink-3">
                  {t('import.tickerReview.verifying', {
                    done: verifyProgress.done,
                    total: verifyProgress.total,
                  })}
                </p>
                <div className="w-full bg-well rounded-full h-2">
                  <div
                    className="bg-dark h-2 rounded-full transition-all"
                    style={{
                      width: `${verifyProgress.total > 0 ? (verifyProgress.done / verifyProgress.total) * 100 : 0}%`,
                    }}
                  />
                </div>
              </div>
            )}
            {verifyProgress.done > 0 && verifyProgress.done === verifyProgress.total && (
              <p className="text-sm text-ink-3">{t('import.tickerReview.allVerified')}</p>
            )}

            {/* Retry failed */}
            {Object.values(tickerMap).some((e) => e.status === 'failed') && (
              <Button variant="outline" size="sm" onClick={retryFailed}>
                {t('import.tickerReview.retryFailed')}
              </Button>
            )}

            {/* Ticker rows */}
            <div className="space-y-2">
              {Object.entries(tickerMap).map(([origKey, entry]) => (
                <div key={origKey} className="flex items-start gap-3 p-3 border rounded-r2">
                  {/* Status icon */}
                  <div className="mt-6 w-5 shrink-0">
                    {entry.status === 'pending' && <div className="h-4 w-4 rounded-full bg-well" />}
                    {entry.status === 'searching' && (
                      <Loader2 className="h-4 w-4 animate-spin text-ink-3" />
                    )}
                    {entry.status === 'found' && <CheckCircle className="h-4 w-4 text-green-500" />}
                    {entry.status === 'not_found' && (
                      <AlertCircle className="h-4 w-4 text-red-500" />
                    )}
                    {entry.status === 'failed' && (
                      <TriangleAlert className="h-4 w-4 text-yellow-500" />
                    )}
                  </div>

                  {/* Ticker input + search button */}
                  <div className="flex-1 space-y-1 min-w-0">
                    <p className="text-xs text-ink-3">{t('import.tickerReview.tickerLabel')}</p>
                    <div className="flex gap-1">
                      <Input
                        value={entry.resolvedTicker}
                        onChange={(e) =>
                          setTickerMap((prev) => ({
                            ...prev,
                            [origKey]: { ...prev[origKey], resolvedTicker: e.target.value },
                          }))
                        }
                        className="h-8 text-sm font-mono"
                      />
                      <Button
                        type="button"
                        variant="outline"
                        size="icon-sm"
                        className="shrink-0"
                        aria-label={tc('a11y.searchTicker')}
                        onClick={() => searchForTicker(origKey)}
                        loading={entry.status === 'searching'}
                        disabled={entry.status === 'searching' || !entry.resolvedTicker.trim()}
                      >
                        <Search />
                      </Button>
                    </div>
                  </div>

                  {/* Name input */}
                  <div className="flex-1 space-y-1 min-w-0">
                    <p className="text-xs text-ink-3">{t('import.tickerReview.nameLabel')}</p>
                    <Input
                      value={entry.name}
                      onChange={(e) =>
                        setTickerMap((prev) => ({
                          ...prev,
                          [origKey]: { ...prev[origKey], name: e.target.value },
                        }))
                      }
                      className={`h-8 text-sm ${
                        (entry.status === 'not_found' || entry.status === 'failed') &&
                        !entry.name.trim()
                          ? entry.status === 'not_found'
                            ? 'border-red-500'
                            : 'border-yellow-500'
                          : ''
                      }`}
                      placeholder={
                        entry.status === 'not_found' || entry.status === 'failed'
                          ? t('import.tickerReview.nameRequired')
                          : ''
                      }
                    />
                  </div>
                </div>
              ))}
            </div>
          </div>
        )}

        {/* ── Step 4: Importing ────────────────────────────────────────────── */}
        {step === 'importing' && (
          <div className="py-12 text-center">
            <Loader2 className="h-12 w-12 mx-auto animate-spin text-ink mb-4" />
            <p className="text-lg font-medium">{t('import.status.processing')}</p>
            <div className="mx-auto mt-4 max-w-sm space-y-2">
              {/* Indeterminate (pulsing, full) until the first progress event arrives */}
              <Progress
                value={
                  importProgress && importProgress.total > 0
                    ? (importProgress.current / importProgress.total) * 100
                    : 100
                }
                className={importProgress ? 'h-2' : 'h-2 animate-pulse'}
              />
              <p className="text-sm text-ink-3" aria-live="polite">
                {!importProgress
                  ? t('import.status.starting', { total: importRowsRef.current.length })
                  : importProgress.phase === 'lookup'
                    ? t('import.status.lookup', {
                        current: importProgress.current,
                        total: importProgress.total,
                      })
                    : t('import.status.progress', {
                        current: importProgress.current,
                        total: importProgress.total,
                        ticker: importRowsRef.current[importProgress.current - 1]?.Ticker ?? '',
                      })}
              </p>
            </div>
            <p className="text-xs text-ink-3 mt-4">{t('import.status.keepOpen')}</p>
            {showDelayedSubtitle && (
              <p className="text-sm text-ink-3 mt-2">{t('import.status.loadingDetails')}</p>
            )}
          </div>
        )}

        {/* ── Step 5: Done ─────────────────────────────────────────────────── */}
        {step === 'done' && importResult && (
          <div className="py-8 space-y-6">
            <div className="text-center">
              <CheckCircle className="h-16 w-16 mx-auto text-green-500 mb-4" />
              <p className="text-2xl font-bold">{t('import.complete')}</p>
              <p className="text-ink-3 mt-2">
                {t('import.processed', {
                  count: importResult.success + importResult.errors.length,
                })}
              </p>
            </div>
            <div
              className={`grid gap-4 ${importResult.imported.length > 0 && importResult.errors.length > 0 ? 'md:grid-cols-2' : 'grid-cols-1'}`}
            >
              {importResult.imported.length > 0 && (
                <div className="p-4 bg-green-50 rounded-r2">
                  <div className="flex items-center gap-2 mb-3">
                    <CheckCircle className="h-5 w-5 text-green-600" />
                    <span className="font-medium text-green-800">
                      {t('import.successful', { count: importResult.success })}
                    </span>
                  </div>
                  <ScrollArea className="max-h-40">
                    <ul className="list-disc pl-4 text-xs space-y-1 text-green-700">
                      {importResult.imported.map((item, i) => (
                        <li key={i}>{item}</li>
                      ))}
                    </ul>
                  </ScrollArea>
                </div>
              )}
              {importResult.errors.length > 0 && (
                <div className="p-4 bg-red-50 rounded-r2">
                  <div className="flex items-center gap-2 mb-3">
                    <AlertCircle className="h-5 w-5 text-red-600" />
                    <span className="font-medium text-red-800">
                      {t('import.skipped', { count: importResult.errors.length })}
                    </span>
                  </div>
                  <ScrollArea className="max-h-40">
                    <ul className="list-disc pl-4 text-xs space-y-1 text-red-700">
                      {importResult.errors.map((err, i) => (
                        <li key={i}>{err}</li>
                      ))}
                    </ul>
                  </ScrollArea>
                </div>
              )}
            </div>
          </div>
        )}

        {/* Ticker picker dialog */}
        <AlertDialog
          open={showPickerDialog}
          onOpenChange={(v) => {
            if (!v) cancelPicker();
          }}
        >
          <AlertDialogContent>
            <AlertDialogHeader>
              <AlertDialogTitle>{t('modal.add.selectTicker')}</AlertDialogTitle>
              <AlertDialogDescription>{t('modal.add.multipleResults')}</AlertDialogDescription>
            </AlertDialogHeader>
            <div className="max-h-60 overflow-y-auto space-y-2">
              {searchResults.map((result) => (
                <button
                  key={result.symbol}
                  onClick={() => selectFromPicker(result)}
                  className="w-full rounded-r2 border border-line p-3 text-left transition-colors hover:bg-well"
                >
                  <div className="font-600">{result.symbol}</div>
                  <div className="text-sm text-ink-3">
                    {result.shortname} • {exchangeName(result.exchange)}
                  </div>
                </button>
              ))}
            </div>
            <AlertDialogFooter>
              <AlertDialogCancel onClick={cancelPicker}>{t('import.cancel')}</AlertDialogCancel>
            </AlertDialogFooter>
          </AlertDialogContent>
        </AlertDialog>

        <DialogFooter>
          {step === 'select' && (
            <Button variant="ghost" onClick={handleClose}>
              {t('import.cancel')}
            </Button>
          )}
          {step === 'mapping' && (
            <>
              <Button variant="outline" onClick={() => setStep('select')}>
                {t('import.back')}
              </Button>
              <Button onClick={enterTickerReview} disabled={!isMappingValid}>
                {t('import.mapping.next')}
              </Button>
            </>
          )}
          {step === 'ticker-review' && (
            <>
              <Button
                variant="outline"
                onClick={() => {
                  verifyAbortRef.current = true;
                  setStep('mapping');
                }}
              >
                {t('import.back')}
              </Button>
              <Button onClick={handleImport} disabled={!isTickerReviewValid}>
                {t('import.importRecords', { count: rawRows.length })}
              </Button>
            </>
          )}
          {step === 'importing' && <Button disabled>{t('import.close')}</Button>}
          {step === 'done' && <Button onClick={handleClose}>{t('import.close')}</Button>}
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
