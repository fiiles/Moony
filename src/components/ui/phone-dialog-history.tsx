import * as React from 'react';
import { useSheetHistory } from '@/mobile/hooks/use-sheet-history';

interface RootProps {
  open?: boolean;
  defaultOpen?: boolean;
  onOpenChange?: (open: boolean) => void;
  children?: React.ReactNode;
}

/**
 * Set by a phone dialog's content while it withholds its close (it is busy, `hideCloseButton`):
 * Android back then leaves it open.
 */
export const PhoneDialogBusyContext = React.createContext<React.MutableRefObject<boolean> | null>(
  null
);

/**
 * A Radix dialog root for the phone shell (spec §5, §10.3; audit C5): while open, the dialog owns
 * one history entry like the phone's own `Sheet` (`useSheetHistory`), so Android back closes the
 * dialog instead of popping the screen under it, and the sheets and dialogs stack in one order.
 * Controlled or not, as the Radix root it wraps.
 */
export function withPhoneHistory<P extends RootProps>(Root: React.ComponentType<P>) {
  function PhoneHistoryRoot(props: P) {
    const { open: openProp, defaultOpen, onOpenChange } = props;
    const [inner, setInner] = React.useState(defaultOpen ?? false);
    const controlled = openProp !== undefined;
    const open = controlled ? openProp : inner;
    const change = React.useCallback(
      (next: boolean) => {
        if (!controlled) setInner(next);
        onOpenChange?.(next);
      },
      [controlled, onOpenChange]
    );
    const busy = React.useRef(false);
    useSheetHistory(open, () => {
      if (busy.current) return false;
      change(false);
    });
    return (
      <PhoneDialogBusyContext.Provider value={busy}>
        <Root {...props} open={open} onOpenChange={change} />
      </PhoneDialogBusyContext.Provider>
    );
  }
  return PhoneHistoryRoot;
}

/** Tells the phone dialog root whether its content withholds its close (`busy`). */
export function usePhoneDialogBusy(busy: boolean): void {
  const ref = React.useContext(PhoneDialogBusyContext);
  React.useEffect(() => {
    if (!ref) return;
    ref.current = busy;
    return () => {
      ref.current = false;
    };
  }, [ref, busy]);
}
