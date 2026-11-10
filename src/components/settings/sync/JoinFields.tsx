import { useTranslation } from 'react-i18next';
import { ChevronRight } from 'lucide-react';
import { submitOnEnter } from '@/components/settings/sync/submit-on-enter';
import { Collapsible, CollapsibleContent, CollapsibleTrigger } from '@/components/ui/collapsible';
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
  relayUrl: string;
  onRelayUrl: (value: string) => void;
  error: string | null;
}

/** Device name, device password and the Advanced sync server address, as both join forms ask. */
export function JoinFields({
  name,
  onName,
  password,
  onPassword,
  relayUrl,
  onRelayUrl,
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
      <Collapsible>
        <CollapsibleTrigger className="group flex items-center gap-1 text-caption font-700 text-ink-2">
          <ChevronRight className="size-3.5 transition-transform group-data-[state=open]:rotate-90" />
          {t('join.advanced')}
        </CollapsibleTrigger>
        <CollapsibleContent className="grid gap-1.5 pt-2.5">
          <Label htmlFor="join-relay-url">{t('join.serverLabel')}</Label>
          <Input
            id="join-relay-url"
            value={relayUrl}
            onChange={(event) => onRelayUrl(event.target.value)}
            onKeyDown={submitOnEnter}
            placeholder={t('setup.advanced.placeholder')}
            autoCapitalize="off"
            autoCorrect="off"
            spellCheck={false}
          />
          <p className="m-0 text-micro font-500 text-ink-4">{t('setup.advanced.hint')}</p>
        </CollapsibleContent>
      </Collapsible>
    </>
  );
}
