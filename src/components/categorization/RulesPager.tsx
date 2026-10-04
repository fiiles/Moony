import { useTranslation } from 'react-i18next';
import { ChevronLeft, ChevronRight } from 'lucide-react';
import { Button } from '@/components/ui/button';
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from '@/components/ui/select';
import { PAGE_SIZES } from '@/utils/learned-rules';

interface RulesPagerProps {
  /** Zero-based current page. */
  page: number;
  pageCount: number;
  pageSize: number;
  total: number;
  /** 1-based position of the first / last row shown. */
  from: number;
  to: number;
  onPageChange: (page: number) => void;
  onPageSizeChange: (pageSize: number) => void;
}

/** Table footer of a long rules table: range left, page size and previous / next right (RUL-01). */
export function RulesPager({
  page,
  pageCount,
  pageSize,
  total,
  from,
  to,
  onPageChange,
  onPageSizeChange,
}: RulesPagerProps) {
  const { t } = useTranslation('categorization');

  return (
    <div className="flex items-center justify-between gap-6 border-t border-line px-[14px] py-2 text-micro font-500 text-ink-4">
      <span aria-live="polite">{t('learnedRules.pageRange', { from, to, total })}</span>
      <span className="flex items-center gap-3">
        <span className="flex items-center gap-2">
          {t('learnedRules.rowsPerPage')}
          <Select value={String(pageSize)} onValueChange={(v) => onPageSizeChange(Number(v))}>
            <SelectTrigger
              className="h-7 w-[72px] text-micro"
              aria-label={t('learnedRules.rowsPerPage')}
            >
              <SelectValue />
            </SelectTrigger>
            <SelectContent>
              {PAGE_SIZES.map((size) => (
                <SelectItem key={size} value={String(size)}>
                  {size}
                </SelectItem>
              ))}
            </SelectContent>
          </Select>
        </span>
        <span className="flex items-center gap-1">
          <Button
            variant="ghost"
            size="icon-sm"
            aria-label={t('learnedRules.previousPage')}
            disabled={page <= 0}
            onClick={() => onPageChange(page - 1)}
          >
            <ChevronLeft />
          </Button>
          <span className="min-w-[88px] text-center num">
            {t('learnedRules.pageOf', { page: page + 1, pages: pageCount })}
          </span>
          <Button
            variant="ghost"
            size="icon-sm"
            aria-label={t('learnedRules.nextPage')}
            disabled={page >= pageCount - 1}
            onClick={() => onPageChange(page + 1)}
          >
            <ChevronRight />
          </Button>
        </span>
      </span>
    </div>
  );
}
