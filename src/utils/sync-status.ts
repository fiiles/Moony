/** Self-hosted sync servers never end the subscription: they report a `paidUntil` in year 9000 or later. */
export function isSelfHostedPaidUntil(paidUntil: number | null | undefined): boolean {
  return typeof paidUntil === 'number' && new Date(paidUntil * 1000).getUTCFullYear() >= 9000;
}

/** The file name of a path the backend returned (either separator). */
export function backupFileName(path: string): string {
  return path.split(/[\\/]/).filter(Boolean).pop() ?? path;
}
