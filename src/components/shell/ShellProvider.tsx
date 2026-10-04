import { useMemo, useState, type ReactNode } from 'react';
import { ShellContext, type ShellPageInfo } from '@/components/shell/shell-context';

export function ShellProvider({ children }: { children: ReactNode }) {
  const [page, setPage] = useState<ShellPageInfo>({});
  const value = useMemo(() => ({ page, setPage }), [page]);
  return <ShellContext.Provider value={value}>{children}</ShellContext.Provider>;
}
