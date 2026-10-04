import { useTranslation } from 'react-i18next';
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from '@/components/ui/select';
import { SettingsCard, SettingsRow } from '@/components/settings/SettingsCard';
import { useAutoLockMinutes } from '@/hooks/use-auto-lock';
import { AUTO_LOCK_OPTIONS, parseAutoLockMinutes } from '@/utils/auto-lock';

/**
 * Settings → Security → Auto-lock. A device-level setting kept in
 * `localStorage` (`moony-auto-lock-minutes`); the lock itself is `useAutoLock()`,
 * mounted in AppLayout. The export keeps its stage-C name so SecuritySection is unchanged.
 */
export function AutoLockSlot() {
  const { t } = useTranslation('settings');
  const [minutes, setMinutes] = useAutoLockMinutes();

  return (
    <SettingsCard title={t('autoLock.title')} description={t('autoLock.description')}>
      <SettingsRow
        htmlFor="auto-lock-minutes"
        label={t('autoLock.label')}
        hint={t('autoLock.hint')}
      >
        <Select
          value={String(minutes)}
          onValueChange={(value) => setMinutes(parseAutoLockMinutes(value))}
        >
          <SelectTrigger id="auto-lock-minutes" className="w-[160px]">
            <SelectValue />
          </SelectTrigger>
          <SelectContent>
            {AUTO_LOCK_OPTIONS.map((option) => (
              <SelectItem key={option} value={String(option)}>
                {option === 0 ? t('autoLock.off') : t('autoLock.minutes', { count: option })}
              </SelectItem>
            ))}
          </SelectContent>
        </Select>
      </SettingsRow>
    </SettingsCard>
  );
}
