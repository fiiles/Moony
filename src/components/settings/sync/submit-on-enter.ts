import type { KeyboardEvent } from 'react';

/** Enter in a field submits through the form's own handler (the webview's implicit submit did not fire). */
export function submitOnEnter(event: KeyboardEvent<HTMLInputElement>) {
  if (event.key !== 'Enter' || event.nativeEvent.isComposing) return;
  event.preventDefault();
  event.currentTarget.form?.requestSubmit();
}
