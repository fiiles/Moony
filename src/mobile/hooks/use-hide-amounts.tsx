import { createContext, useCallback, useContext, useMemo, useState, type ReactNode } from 'react';
import { deviceStorage } from '@/utils/device-storage';
import { readHideAmounts, writeHideAmounts } from '@/mobile/lib/hide-amounts';

interface HideAmounts {
  /** Every amount on the phone shows as a mask. */
  hidden: boolean;
  toggle: () => void;
}

const HideAmountsContext = createContext<HideAmounts | null>(null);

/**
 * The hide-amounts state for the whole phone shell (spec D6): one eye on Přehled, read by every
 * screen through `useHideAmounts()`. It starts as the last choice (or hidden, with "hide on
 * launch") and every toggle is stored on this device.
 */
export function HideAmountsProvider({ children }: { children: ReactNode }) {
  const [hidden, setHidden] = useState(() => readHideAmounts(deviceStorage()));

  const toggle = useCallback(() => {
    writeHideAmounts(deviceStorage(), !hidden);
    setHidden(!hidden);
  }, [hidden]);

  const value = useMemo(() => ({ hidden, toggle }), [hidden, toggle]);
  return <HideAmountsContext.Provider value={value}>{children}</HideAmountsContext.Provider>;
}

// eslint-disable-next-line react-refresh/only-export-components -- context hook lives with its provider
export function useHideAmounts(): HideAmounts {
  const context = useContext(HideAmountsContext);
  if (!context) {
    throw new Error('useHideAmounts must be used within a HideAmountsProvider');
  }
  return context;
}
