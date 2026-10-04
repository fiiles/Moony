/**
 * The one place that turns numbers, money and dates into text
 *
 * Every formatter is driven by the UI language's locale, never by the
 * currency's home locale or the OS locale: an English UI shows `CZK 1,234`
 * style separators, a Czech UI `1 234 Kč`. Day-granular timestamps are
 * calendar days stored as UTC midnight (ADR 0008), so `day()` formats in UTC.
 */
import { currencyDecimals } from '@shared/currencies';

export interface MoneyOptions {
  /** Override the currency's own decimals (CZK/JPY 0, most others 2). */
  decimals?: number;
  /** Explicit "+" for positive values (change badges). */
  signed?: boolean;
  /** 1.2M / 350k style for cards and axes; `decimals` caps the fraction digits (default 1). */
  compact?: boolean;
}

export interface Formatters {
  locale: string;
  /** "€1,234.56" (en) / "1 234,56 €" (cs); `-0` is shown as `0`. */
  money: (amount: number, currency: string, opts?: MoneyOptions) => string;
  /** Plain number with the locale's separators. */
  number: (value: number, opts?: Intl.NumberFormatOptions) => string;
  /** 0.1234 → "12.3 %" style; `digits` = fraction digits (default 1), `signed` adds "+". */
  percent: (ratio: number, digits?: number, opts?: { signed?: boolean }) => string;
  /** Calendar day of a UTC-midnight unix-seconds timestamp, formatted in UTC. */
  day: (unixSeconds: number, opts?: Intl.DateTimeFormatOptions) => string;
  /** Real instant (created_at, fetched_at): local time zone. */
  dateTime: (unixSeconds: number, opts?: Intl.DateTimeFormatOptions) => string;
  /** Time of day of a real instant ("14:05" / "2:05 PM"), local time zone. */
  time: (unixSeconds: number, opts?: Intl.DateTimeFormatOptions) => string;
  /**
   * A `Date` object (or ISO string) in the local time zone, for values that are
   * real instants or UI-computed dates (period ranges, "today"). Never use it
   * for booking dates stored as UTC midnight: use `day()` so the calendar day
   * cannot shift in western time zones. Invalid input gives "".
   */
  date: (date: Date | string, opts?: Intl.DateTimeFormatOptions) => string;
  /** "September 2026" / "září 2026" for a UTC date (period labels). */
  month: (date: Date, opts?: Intl.DateTimeFormatOptions) => string;
}

/** `-0` and NaN never reach the user. */
function clean(value: number): number {
  if (!Number.isFinite(value)) return 0;
  return Object.is(value, -0) ? 0 : value;
}

const numberFormatCache = new Map<string, Intl.NumberFormat>();
function numberFormat(locale: string, options: Intl.NumberFormatOptions): Intl.NumberFormat {
  const key = `${locale}|${JSON.stringify(options)}`;
  let nf = numberFormatCache.get(key);
  if (!nf) {
    try {
      nf = new Intl.NumberFormat(locale, options);
    } catch {
      // Unknown currency code or option: fall back to a plain number format
      nf = new Intl.NumberFormat(locale, {
        minimumFractionDigits: options.minimumFractionDigits,
        maximumFractionDigits: options.maximumFractionDigits,
      });
    }
    numberFormatCache.set(key, nf);
  }
  return nf;
}

const dateFormatCache = new Map<string, Intl.DateTimeFormat>();
function dateFormat(locale: string, options: Intl.DateTimeFormatOptions): Intl.DateTimeFormat {
  const key = `${locale}|${JSON.stringify(options)}`;
  let df = dateFormatCache.get(key);
  if (!df) {
    df = new Intl.DateTimeFormat(locale, options);
    dateFormatCache.set(key, df);
  }
  return df;
}

const ISO_CURRENCY = /^[A-Z]{3}$/;

/**
 * Signed changes per the design system (§10): "+ 60 110 Kč", "− 15 200 Kč" — a true
 * minus (U+2212) and a no-break space between the sign and the number, whatever
 * Intl produced ("-15 200 Kč", "+€1,500.00", "-CZK 15,200").
 */
function spaceSign(text: string): string {
  const i = text.search(/[+\-\u2212]/);
  if (i < 0) return text;
  const sign = text[i] === '+' ? '+' : '\u2212';
  return `${text.slice(0, i)}${sign}\u00a0${text.slice(i + 1).replace(/^\s+/, '')}`;
}

export function createFormatters(locale: string): Formatters {
  const money: Formatters['money'] = (amount, currency, opts = {}) => {
    const label = (currency || '').trim();
    const code = label.toUpperCase();
    const decimals = opts.decimals ?? currencyDecimals(code);
    // Round first so that -0.001 CZK becomes 0, not "-0".
    const factor = 10 ** decimals;
    const value = opts.compact ? clean(amount) : clean(Math.round(clean(amount) * factor) / factor);
    const base: Intl.NumberFormatOptions = {
      minimumFractionDigits: opts.compact ? 0 : decimals,
      maximumFractionDigits: opts.compact ? (opts.decimals ?? 1) : decimals,
      ...(opts.signed ? { signDisplay: 'exceptZero' as const } : {}),
      ...(opts.compact ? { notation: 'compact' as const } : {}),
    };
    if (ISO_CURRENCY.test(code)) {
      const nf = numberFormat(locale, {
        ...base,
        style: 'currency',
        currency: code,
        currencyDisplay: 'narrowSymbol',
      });
      // numberFormat() falls back to a plain format for codes Intl rejects;
      // detect that and append the code ourselves.
      const text = nf.format(value);
      const out = nf.resolvedOptions().style === 'currency' ? text : `${text} ${label}`;
      return opts.signed ? spaceSign(out) : out;
    }
    const text = numberFormat(locale, base).format(value);
    const out = label ? `${text} ${label}` : text;
    return opts.signed ? spaceSign(out) : out;
  };

  const number: Formatters['number'] = (value, opts = {}) => {
    const min = opts.minimumFractionDigits ?? 0;
    return numberFormat(locale, {
      ...opts,
      minimumFractionDigits: min,
      maximumFractionDigits: opts.maximumFractionDigits ?? Math.max(min, 2),
    }).format(clean(value));
  };

  const percent: Formatters['percent'] = (ratio, digits = 1, opts = {}) => {
    const text = numberFormat(locale, {
      style: 'percent',
      minimumFractionDigits: digits,
      maximumFractionDigits: digits,
      ...(opts.signed ? { signDisplay: 'exceptZero' as const } : {}),
    }).format(clean(ratio));
    return opts.signed ? spaceSign(text) : text;
  };

  const day: Formatters['day'] = (unixSeconds, opts = {}) => {
    // Explicit options replace the default numeric date (a caller asking for
    // "Sep 30" must not get the year appended).
    const base: Intl.DateTimeFormatOptions = Object.keys(opts).length
      ? opts
      : { year: 'numeric', month: 'numeric', day: 'numeric' };
    return dateFormat(locale, { ...base, timeZone: 'UTC' }).format(new Date(unixSeconds * 1000));
  };

  const dateTime: Formatters['dateTime'] = (unixSeconds, opts = {}) =>
    dateFormat(locale, {
      year: 'numeric',
      month: 'numeric',
      day: 'numeric',
      hour: '2-digit',
      minute: '2-digit',
      ...opts,
    }).format(new Date(unixSeconds * 1000));

  const time: Formatters['time'] = (unixSeconds, opts = {}) =>
    dateFormat(locale, { hour: '2-digit', minute: '2-digit', ...opts }).format(
      new Date(unixSeconds * 1000)
    );

  const date: Formatters['date'] = (value, opts = {}) => {
    const d = typeof value === 'string' ? new Date(value) : value;
    if (!(d instanceof Date) || Number.isNaN(d.getTime())) return '';
    // Same defaulting as Date#toLocaleDateString: numeric y/m/d unless the
    // caller names a date component or style.
    const named = ['weekday', 'year', 'month', 'day', 'dateStyle', 'timeStyle'] as const;
    const hasDatePart = named.some((k) => opts[k] !== undefined);
    const base: Intl.DateTimeFormatOptions = hasDatePart
      ? opts
      : { year: 'numeric', month: 'numeric', day: 'numeric', ...opts };
    return dateFormat(locale, base).format(d);
  };

  const month: Formatters['month'] = (value, opts = {}) => {
    const text = dateFormat(locale, {
      month: 'long',
      year: 'numeric',
      ...opts,
      timeZone: 'UTC',
    }).format(value);
    // Czech month names are lower-case; period labels start a line.
    return text.charAt(0).toUpperCase() + text.slice(1);
  };

  return { locale, money, number, percent, day, dateTime, time, date, month };
}

const formatterCache = new Map<string, Formatters>();
/** Memoized per locale (formatters are pure). */
export function getFormatters(locale: string): Formatters {
  let f = formatterCache.get(locale);
  if (!f) {
    f = createFormatters(locale);
    formatterCache.set(locale, f);
  }
  return f;
}
