import { useTranslation } from 'react-i18next';
import type { RemovalPlanDevice } from '@shared/schema';
import { useFormat } from '@/lib/use-format';

/**
 * "Přidáno {date} ze zařízení {name}" under a device of "Odebrat také": when this device first
 * verified it, and from which device it was added (the date alone when that is not known).
 * Nothing without `addedAt`: the certificate's `joinedAt` is written by the device itself, so it
 * is never shown as the date it was added.
 */
export function AddedLine({ device, className }: { device: RemovalPlanDevice; className: string }) {
  const { t } = useTranslation('deviceSync');
  const { date } = useFormat();
  if (device.addedAt === null) return null;
  const when = date(new Date(device.addedAt * 1000), { dateStyle: 'medium' });
  return (
    <span className={className}>
      {device.addedBy === null
        ? t('remove.addedRowNoIssuer', { date: when })
        : t('remove.addedRow', { date: when, name: device.addedBy })}
    </span>
  );
}
