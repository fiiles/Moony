import { useTranslation } from 'react-i18next';
import type { ServerChoiceState } from '@/hooks/use-server-choice';
import { ServerCheckNotice, ServerChoice } from '@/components/settings/sync/ServerChoice';
import { submitOnEnter } from '@/components/settings/sync/submit-on-enter';
import { Input } from '@/components/ui/input';
import { Label } from '@/components/ui/label';

/** What joining does to this device's data; shown above both join forms in Settings. */
export function JoinWarning() {
  const { t } = useTranslation('deviceSync');
  return <p className="m-0 text-body text-ink-2">{t('join.warning')}</p>;
}

interface JoinFieldsProps {
  /** The device password field (Settings); the first run sends the password typed a step earlier. */
  askPassword: boolean;
  password: string;
  onPassword: (value: string) => void;
  /** The sync server: typed here, checked and acknowledged (`useServerChoice`). */
  server: ServerChoiceState;
  /**
   * `field`: the person types an address or a connection code; `fixed`: a Recovery Kit QR names the
   * server, shown with its check and warning; `hidden`: nothing to ask or show.
   */
  serverMode: 'field' | 'fixed' | 'hidden';
  /** Under the password field, or under the server when there is none. */
  error: string | null;
}

/** Device password and the sync server, as both join forms ask (the device name is asked before). */
export function JoinFields({
  askPassword,
  password,
  onPassword,
  server,
  serverMode,
  error,
}: JoinFieldsProps) {
  const { t } = useTranslation('deviceSync');
  return (
    <>
      {askPassword && (
        <div className="grid gap-1.5">
          <Label htmlFor="join-device-password">{t('join.passwordLabel')}</Label>
          <Input
            id="join-device-password"
            type="password"
            value={password}
            onChange={(event) => onPassword(event.target.value)}
            onKeyDown={submitOnEnter}
            autoComplete="current-password"
            aria-invalid={error ? true : undefined}
          />
          {error && (
            <p className="m-0 text-micro font-500 text-loss" role="alert">
              {error}
            </p>
          )}
        </div>
      )}
      {serverMode === 'field' && <ServerChoice state={server} id="join-server" showCloud />}
      {serverMode === 'fixed' && <ServerCheckNotice state={server} id="join-server" />}
      {!askPassword && error && (
        <p className="m-0 text-micro font-500 text-loss" role="alert">
          {error}
        </p>
      )}
    </>
  );
}
