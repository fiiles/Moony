import { Separator } from '@/components/ui/separator';

interface FormSectionProps {
  /** Omitted when the dialog title already names the section */
  title?: string;
  children: React.ReactNode;
  /** Pass true for the first section — omits the leading separator */
  first?: boolean;
}

/** A titled group of fields inside a form: H3 role, hairline above (never a nested card). */
export function FormSection({ title, children, first = false }: FormSectionProps) {
  return (
    <div className="space-y-4">
      {!first && <Separator />}
      {title && <h3 className="text-h3 text-ink">{title}</h3>}
      {children}
    </div>
  );
}
