import type { ReactNode } from 'react';
import { Input, InputWrap } from '@/components/ui/input';
import { Label, LabelHint } from '@/components/ui/label';
import { cn } from '@/lib/utils';

interface CalcFieldProps {
  id: string;
  label: ReactNode;
  /** Unit inside the field ("Kč", "%", "let"). */
  unit: ReactNode;
  value: string;
  onChange: (value: string) => void;
  hint?: ReactNode;
  optional?: ReactNode;
  className?: string;
}

/** A calculator input: label, field with unit, optional hint. Values are typed freely ("3 500 000", "5,1"). */
export function CalcField({
  id,
  label,
  unit,
  value,
  onChange,
  hint,
  optional,
  className,
}: CalcFieldProps) {
  return (
    <div className={cn('grid gap-1.5', className)}>
      <Label htmlFor={id}>
        {label}
        {optional && <LabelHint>{optional}</LabelHint>}
      </Label>
      <InputWrap unit={unit}>
        <Input
          id={id}
          inputMode="decimal"
          value={value}
          onChange={(e) => onChange(e.target.value)}
          className="num"
        />
      </InputWrap>
      {hint && <p className="text-micro font-500 text-ink-4">{hint}</p>}
    </div>
  );
}

/** Label + computed value under a form ("Vlastní kapitál  2 400 000 Kč"). */
export function ComputedRow({
  label,
  value,
  tone,
}: {
  label: ReactNode;
  value: ReactNode;
  tone?: 'gain' | 'loss';
}) {
  return (
    <div className="flex items-center justify-between pt-2 text-caption text-ink-3">
      <span>{label}</span>
      <b
        className={cn(
          'font-650 text-ink num',
          tone === 'gain' && 'text-gain',
          tone === 'loss' && 'text-loss'
        )}
      >
        {value}
      </b>
    </div>
  );
}
