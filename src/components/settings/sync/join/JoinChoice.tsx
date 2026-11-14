import { useTranslation } from 'react-i18next';
import { submitOnEnter } from '@/components/settings/sync/submit-on-enter';
import { ChoiceCard } from '@/components/ui/choice-card';
import { Input } from '@/components/ui/input';
import { Label } from '@/components/ui/label';
import { RadioGroup } from '@/components/ui/radio-group';

/** How this device joins: pairing with a phone that syncs, or with the Sync Recovery Kit. */
export type JoinMethod = 'pair' | 'kit';

interface JoinChoiceProps {
  name: string;
  onName: (value: string) => void;
  method: JoinMethod;
  onMethod: (method: JoinMethod) => void;
}

/**
 * The first screen of a join (Settings → Sync and the first run's Připojení step): this device's
 * name and the two ways in, as choice cards.
 */
export function JoinChoice({ name, onName, method, onMethod }: JoinChoiceProps) {
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
      <RadioGroup
        value={method}
        onValueChange={(value) => onMethod(value as JoinMethod)}
        aria-label={t('intro.joinTitle')}
        className="gap-2.5"
      >
        <ChoiceCard
          value="pair"
          id="join-method-pair"
          selected={method === 'pair'}
          title={t('join.choice.pair.title')}
          description={t('join.choice.pair.lead')}
        />
        <ChoiceCard
          value="kit"
          id="join-method-kit"
          selected={method === 'kit'}
          title={t('join.choice.kit.title')}
          description={t('join.choice.kit.lead')}
        />
      </RadioGroup>
    </>
  );
}
