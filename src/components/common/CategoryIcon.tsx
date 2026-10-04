import { Tag } from 'lucide-react';
import { ICON_MAP } from './category-icons-map';

/** Render icon component from name */
export function CategoryIcon({ iconName, className }: { iconName: string; className?: string }) {
  const IconComponent = ICON_MAP[iconName] || Tag;
  return <IconComponent className={className} />;
}
