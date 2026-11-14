import { useTranslation } from 'react-i18next';
import type { ServerChoiceState } from '@/hooks/use-server-choice';
import { ServerCheckNotice, ServerChoice } from '@/components/settings/sync/ServerChoice';
import { submitOnEnter } from '@/components/settings/sync/submit-on-enter';
import { Input } from '@/components/ui/input';
import { Label } from '@/components/ui/label';

/** What joining does to this device's data; shown above both join forms. */
export function JoinWarning() {
  const { t } = useTranslation('deviceSync');
  return <p className="m-0 text-body text-ink-2">{t('join.warning')}</p>;
}

interface JoinFieldsProps {
  name: string;
  onName: (value: string) => void;
  password: string;
  onPassword: (value: string) => void;
  /** The sync server: typed here, checked and acknowledged (`useServerChoice`). */
  server: ServerChoiceState;
  /**
   * `field`: the person types an address or a connection code; `fixed`: a Recovery Kit QR names the
   * server, shown with its check and warning; `hidden`: nothing to ask or show.
   */
  serverMode: 'field' | 'fixed' | 'hidden';
  error: string | null;
}

/** Device name, device password and the sync server, as both join forms ask. */
export function JoinFields({
  name,
  onName,
  password,
  onPassword,
  server,
  serverMode,
  error,
}: JoinFieldsProps) {
  const { t } = useTranslation('deviceSync');
  return (
    <>
      <div className="grid gap-1.5">
        <Label htmlFor="join-device-name">{t('join.nameLabel')}</Label>
        <Input
          id="join-device-name"
          value={name}
          maxLength={60}
          onChange={(event) => onName(event.target.value)}
          onKeyDown={submitOnEnter}
        />
      </div>
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
      {serverMode === 'field' && <ServerChoice state={server} id="join-server" showCloud />}
      {serverMode === 'fixed' && <ServerCheckNotice state={server} id="join-server" />}
    </>
  );
}
