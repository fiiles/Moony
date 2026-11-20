import type { RemovalPlanDevice } from '@shared/schema';
import { Checkbox } from '@/components/ui/checkbox';
import { AddedLine } from '@/components/settings/sync/AddedLine';

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
              <span className="block">{device.name}</span>
              <AddedLine device={device} className="block text-m-sub text-ink-3" />
            </span>
          </label>
        </li>
      ))}
    </ul>
  );
}
