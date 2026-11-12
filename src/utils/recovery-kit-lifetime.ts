/** How long the Recovery Kit stays on screen (it holds the vault key). */
export const KIT_VIEW_MS = 5 * 60_000;
/** How long a code copied through the webview's clipboard may stay there. */
export const CLIPBOARD_CLEAR_MS = 60_000;

/** The kit leaves the screen after KIT_VIEW_MS or as soon as the app is hidden. */
export function kitExpired(shownAt: number, now: number, hidden: boolean): boolean {
  return hidden || now - shownAt >= KIT_VIEW_MS;
}

/** Clear the clipboard only while it still holds exactly the copied code. */
export function shouldClearClipboard(current: string | null, code: string): boolean {
  return current !== null && current === code;
}

/**
 * Copies `code` through the webview's clipboard (Linux, Android: Rust copies natively elsewhere) and
 * empties the clipboard after CLIPBOARD_CLEAR_MS if it still holds exactly that code. Not tied to a
 * component: the clearing must outlive the kit leaving the screen. A failed read (no permission,
 * no focus) leaves the clipboard alone: the on-screen hint still asks the person to clear it.
 */
export async function copyToWebviewClipboard(code: string): Promise<void> {
  await navigator.clipboard.writeText(code);
  setTimeout(() => {
    void navigator.clipboard
      .readText()
      .then((current) =>
        shouldClearClipboard(current, code) ? navigator.clipboard.writeText('') : undefined
      )
      .catch(() => undefined);
  }, CLIPBOARD_CLEAR_MS);
}
