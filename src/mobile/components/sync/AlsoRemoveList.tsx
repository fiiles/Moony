import { useTranslation } from 'react-i18next';
import type { RemovalPlanDevice } from '@shared/schema';
import { Checkbox } from '@/components/ui/checkbox';
import { useFormat } from '@/lib/use-format';

/**
 * "Odebrat také": the devices the removed ones added (or that joined with the old kit), one 44 pt
 * row each with a tick; the whole row is the target. The label wraps.
 */
export function AlsoRemoveList({
  idPrefix,
  devices,
  ticked,
  onToggle,
}: {
  idPrefix: string;
  devices: readonly RemovalPlanDevice[];
  ticked: readonly string[];
  onToggle: (id: string) => void;
}) {
  const { t } = useTranslation('deviceSync');
  const { date } = useFormat();
  return (
    <ul className="m-0 list-none p-0">
      {devices.map((device) => (
        <li key={device.id}>
          <label
            htmlFor={`${idPrefix}-${device.id}`}
            className="flex min-h-11 items-center gap-3 py-1 text-m-row text-ink"
          >
            <Checkbox
              id={`${idPrefix}-${device.id}`}
              checked={ticked.includes(device.id)}
              onCheckedChange={() => onToggle(device.id)}
            />
            <span className="min-w-0 break-words">
              {t('remove.addedRow', {
                name: device.name,
                date: date(new Date(device.joinedAt * 1000), { dateStyle: 'medium' }),
              })}
            </span>
          </label>
        </li>
      ))}
    </ul>
  );
}
