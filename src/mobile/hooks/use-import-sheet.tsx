import { createContext, useCallback, useContext, useMemo, useState, type ReactNode } from 'react';
import { useAuth } from '@/hooks/use-auth';
import type { ImportRequest } from '@/mobile/lib/import-flow';
import { ImportSheet } from '@/mobile/sheets/ImportSheet';

export type { ImportRequest } from '@/mobile/lib/import-flow';

interface ImportSheetContextValue {
  /**
   * Opens the import sheet: on the picker, or — with `path` (a shared file) — straight on reading
   * it. `accountId` is the account the file goes to when the entry point knows it. A sheet that is
   * already open is left as it is: the import in front of the person finishes first.
   */
  openImport: (request: ImportRequest) => void;
  /** An import sheet is open. */
  open: boolean;
}

const ImportSheetContext = createContext<ImportSheetContextValue | null>(null);

/**
 * Owns the one import sheet of the phone (spec §8.6): every entry point — an account's "Importovat
 * výpis", the Údržba card, Nastavení → Import, a statement shared into the app — asks for it
 * through `useImportSheet().openImport`, and it renders here, above the routes, so it survives a
 * navigation and a screen's unmount. It is mounted only while unlocked: a lock takes the sheet
 * (and the file it held) with it. Each opening starts a fresh flow.
 */
export function ImportSheetProvider({ children }: { children: ReactNode }) {
  const { appStatus } = useAuth();
  const unlocked = appStatus === 'unlocked';
  // The opening that is on screen, or was and is sliding away; its id is the flow's key
  const [session, setSession] = useState<{
    id: number;
    request: ImportRequest;
    open: boolean;
  } | null>(null);

  // Locking closes everything: nothing of an import outlives the unlocked profile
  if (!unlocked && session !== null) setSession(null);

  const openImport = useCallback(
    (request: ImportRequest) =>
      setSession((current) =>
        current?.open ? current : { id: (current?.id ?? 0) + 1, request, open: true }
      ),
    []
  );
  const handleOpenChange = useCallback(
    (open: boolean) => setSession((current) => (current ? { ...current, open } : current)),
    []
  );

  const open = session?.open ?? false;
  const value = useMemo(() => ({ openImport, open }), [openImport, open]);

  return (
    <ImportSheetContext.Provider value={value}>
      {children}
      {unlocked && session && (
        <ImportSheet
          // A new opening is a new flow: nothing of the last file is left
          key={session.id}
          request={session.request}
          open={session.open}
          onOpenChange={handleOpenChange}
        />
      )}
    </ImportSheetContext.Provider>
  );
}

// eslint-disable-next-line react-refresh/only-export-components -- context hook lives with its provider
export function useImportSheet(): ImportSheetContextValue {
  const context = useContext(ImportSheetContext);
  if (!context) throw new Error('useImportSheet needs an ImportSheetProvider above it');
  return context;
}
