import { useState } from 'react';
import { useTranslation } from 'react-i18next';
import { toast } from 'sonner';
import { Button } from '@/components/ui/button';
import { useFormat } from '@/lib/use-format';
import { KeyValueRow } from '@/mobile/components/KeyValueRow';
import { SectionPage } from '@/mobile/components/SectionPage';
import { SwitchRow } from '@/mobile/components/SwitchRow';
import { scheduleNotification } from '@/mobile/hooks/use-notification-schedule';
import {
  useNotificationPermission,
  type NotificationPermissionState,
} from '@/mobile/hooks/use-notification-permission';
import { useNotificationPrefs } from '@/mobile/hooks/use-notification-prefs';
import { localTimeOf, notificationId } from '@/mobile/lib/notification-plan';
import { DayPickerSheet } from '@/mobile/sheets/DayPickerSheet';

/** The card the section's controls sit in (`.m-card`), rows `line-soft` apart. */
const CARD =
  'mb-3.5 divide-y divide-line-soft rounded-r4 border border-line bg-paper px-3 shadow-e1';

/** The test notification comes this many seconds after the button ("Přijde za 10 sekund"). */
const TEST_DELAY_SECONDS = 10;

const PERMISSION_VALUE: Record<Exclude<NotificationPermissionState, 'unknown'>, string> = {
  granted: 'notifications.permission.granted',
  denied: 'notifications.permission.denied',
  default: 'notifications.permission.notAsked',
};

/**
 * Nastavení → Oznámení (spec §8.9): whether the phone lets Moony notify and what it notifies
 * about. Everything is local: the scheduler (`useNotificationSchedule`) plans the notifications on
 * every unlock from the milestones, so the groups themselves are switched on Připomínky; here are
 * the three kinds (deadlines, the monthly upkeep and its day, the backup). Permission is asked
 * only from the button on this page. Nothing can be on without it, so the switches are off and
 * disabled and say so. The test button is the person's own check that the phone shows Moony's
 * notifications.
 */
export default function NotificationsSection() {
  const { t } = useTranslation('mobile');
  const fmt = useFormat();
  const { state, request, isPending } = useNotificationPermission();
  const [prefs, setPrefs] = useNotificationPrefs();
  const [dayOpen, setDayOpen] = useState(false);

  const granted = state === 'granted';
  const needed = t('notifications.permission.needed');
  const day = fmt.number(prefs.upkeepDay);

  // Nothing is said about the state until the system has answered
  const permissionValue =
    state === 'unknown'
      ? isPending
        ? ''
        : t('notifications.permission.notGranted')
      : t(PERMISSION_VALUE[state]);

  const sendTest = () => {
    try {
      const when = new Date(Date.now() + TEST_DELAY_SECONDS * 1000);
      scheduleNotification({
        id: notificationId('upkeep', 'test'),
        title: t('notifications.test.title'),
        body: t('notifications.test.body'),
        at: { ...localTimeOf(when), second: when.getSeconds() },
      });
      toast(t('notifications.test.sent'));
    } catch (error) {
      console.warn('[notifications] the test notification failed', error);
    }
  };

  return (
    <SectionPage title={t('settings.rows.notifications')} lead={t('notifications.lead')}>
      <div className={CARD}>
        <KeyValueRow label={t('notifications.permission.label')} value={permissionValue} />
        {state === 'default' && (
          <div className="px-1 py-3">
            <Button
              className="h-12 w-full rounded-[10px] text-m-row"
              loading={isPending}
              onClick={() => void request()}
            >
              {t('notifications.permission.allow')}
            </Button>
          </div>
        )}
        {state === 'denied' && (
          <p className="m-0 px-1 py-3 text-m-sub text-ink-4">
            {t('notifications.permission.deniedHint')}
          </p>
        )}
        {state === 'unknown' && !isPending && (
          <p className="m-0 px-1 py-3 text-m-sub text-ink-4">
            {t('notifications.permission.unavailable')}
          </p>
        )}
      </div>

      <div className={CARD}>
        <SwitchRow
          id="notify-deadlines"
          label={t('notifications.deadlines.label')}
          sub={granted ? t('notifications.deadlines.hint') : needed}
          checked={granted && prefs.deadlines}
          disabled={!granted}
          onCheckedChange={(on) => setPrefs({ ...prefs, deadlines: on })}
        />
        <SwitchRow
          id="notify-upkeep"
          label={t('notifications.upkeep.label')}
          sub={granted ? t('notifications.upkeep.hint', { day }) : needed}
          checked={granted && prefs.upkeep}
          disabled={!granted}
          onCheckedChange={(on) => setPrefs({ ...prefs, upkeep: on })}
        />
        {granted && prefs.upkeep && (
          <KeyValueRow
            label={t('notifications.upkeep.day')}
            value={t('notifications.upkeep.dayValue', { day })}
            onClick={() => setDayOpen(true)}
          />
        )}
        <SwitchRow
          id="notify-backup"
          label={t('notifications.backup.label')}
          sub={granted ? t('notifications.backup.hint') : needed}
          checked={granted && prefs.backup}
          disabled={!granted}
          onCheckedChange={(on) => setPrefs({ ...prefs, backup: on })}
        />
      </div>

      <Button
        variant="secondary"
        className="h-12 w-full rounded-[10px] text-m-row"
        disabled={!granted}
        onClick={sendTest}
      >
        {t('notifications.test.send')}
      </Button>

      <DayPickerSheet
        open={dayOpen}
        onOpenChange={setDayOpen}
        value={prefs.upkeepDay}
        onPick={(picked) => setPrefs({ ...prefs, upkeepDay: picked })}
      />
    </SectionPage>
  );
}
