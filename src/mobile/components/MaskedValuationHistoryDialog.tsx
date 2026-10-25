import {
  ValuationHistoryDialog,
  type ValuationHistoryDialogProps,
} from '@/components/valuations/ValuationHistoryDialog';
import { useMoneyText } from '@/mobile/hooks/use-money-text';

/**
 * The desktop's Historie přecenění (the dated estimates of a property or an other asset) as the
 * phone opens it from a `···` menu: every amount — the estimates, their changes and the delete
 * confirmation — goes through the eye of Přehled (`useMoneyText`), so a read-only list does not
 * show what the rest of the app hides.
 */
export function MaskedValuationHistoryDialog(
  props: Omit<ValuationHistoryDialogProps, 'formatMoney'>
) {
  const moneyText = useMoneyText();
  return <ValuationHistoryDialog {...props} formatMoney={moneyText} />;
}
