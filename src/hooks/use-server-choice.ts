import { useState } from 'react';
import { useTranslation } from 'react-i18next';
import type { SyncServerCheck } from '@shared/schema';
import { useSyncServerCheck } from '@/hooks/use-device-sync';
import { useDebouncedValue } from '@/hooks/use-debounced-value';
import { translateApiError } from '@/lib/translate-api-error';
import { parseConnectionCode, wrongCodeKey, type CodeContext } from '@/utils/sync-server';
import { acknowledgedAfterEdit, serverContinue } from '@/utils/sync-setup';

export interface ServerChoiceState {
  input: string;
  /** Typing resets the plain-http acknowledgement: it belongs to the server that was shown. */
  setInput: (value: string) => void;
  accepts: 'connectionCode' | 'addressOrCode';
  /** What the server answered for the current input; `undefined` until it did (or after a change). */
  check: SyncServerCheck | undefined;
  checking: boolean;
  /** The translated problem with the input or the server, or `null`. */
  problem: string | null;
  acknowledged: boolean;
  setAcknowledged: (value: boolean) => void;
  /** The input names a server that answered and, for plain http, was acknowledged. */
  ready: boolean;
}

/**
 * The server field of setup and join: reads the input the way the backend will (a wrong kind of
 * code names itself), asks the server what it is, and says when the person may go on.
 */
export function useServerChoice(
  accepts: 'connectionCode' | 'addressOrCode',
  context: CodeContext,
  /**
   * A server that is not typed (a Recovery Kit QR's) replaces the typed input; `''` names none.
   * `undefined`: the input is the person's.
   */
  forced?: string
): ServerChoiceState {
  const { t } = useTranslation('deviceSync');
  const { t: tc } = useTranslation('common');
  const [typed, setInputState] = useState('');
  const [acknowledged, setAcknowledged] = useState(false);
  // The acknowledgement belongs to the server it was given for: another forced one clears it
  const [previousForced, setPreviousForced] = useState(forced);
  if (forced !== previousForced) {
    setPreviousForced(forced);
    setAcknowledged(false);
  }
  const input = forced ?? typed;
  const trimmed = input.trim();

  const wrong = trimmed === '' ? null : wrongCodeKey(context, trimmed);
  let shapeProblem: string | null = null;
  if (trimmed !== '' && wrong === null && accepts === 'connectionCode') {
    const read = parseConnectionCode(trimmed);
    if (!read.ok) shapeProblem = read.problem;
  }
  const askable = trimmed !== '' && wrong === null && shapeProblem === null;

  const query = useSyncServerCheck(trimmed, askable);
  // The query follows the input after 400 ms; until then its answer is for the previous input.
  const settled = useDebouncedValue(trimmed, 400) === trimmed;
  const failed = askable && settled && query.isError;
  const check = askable && settled && !query.isError ? query.data : undefined;
  const checking = askable && !check && !failed;

  let problem: string | null = null;
  if (wrong) problem = t(wrong);
  else if (shapeProblem) problem = tc(shapeProblem);
  else if (failed) problem = translateApiError(query.error, tc);

  return {
    input,
    setInput: (value) => {
      setAcknowledged(acknowledgedAfterEdit(typed, value, acknowledged));
      setInputState(value);
    },
    accepts,
    check,
    checking,
    problem,
    acknowledged,
    setAcknowledged,
    ready: askable && serverContinue(check, acknowledged),
  };
}
