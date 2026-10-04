import { Toaster as Sonner } from 'sonner';
import { Check, CircleAlert, Info, Loader2 } from 'lucide-react';

type ToasterProps = React.ComponentProps<typeof Sonner>;

/**
 * Toasts confirm finished actions: bottom right, 2.6 s, past-tense verb
 * ("Účet přidán"). E4 surface (paper, sh-pop), 12/600, icon in gain for success.
 */
const Toaster = ({ ...props }: ToasterProps) => {
  return (
    <Sonner
      theme="light"
      position="bottom-right"
      duration={2600}
      offset={20}
      gap={8}
      className="toaster group"
      icons={{
        success: <Check className="size-3.5 text-gain" strokeWidth={2.25} />,
        error: <CircleAlert className="size-3.5 text-loss" strokeWidth={2} />,
        info: <Info className="size-3.5 text-ink-3" strokeWidth={2} />,
        loading: <Loader2 className="size-3.5 animate-spin text-ink-3" strokeWidth={2} />,
      }}
      toastOptions={{
        unstyled: true,
        classNames: {
          toast:
            'group flex w-[356px] items-start gap-2.5 rounded-[10px] border border-line bg-paper px-3.5 py-[11px] text-table font-600 text-ink shadow-pop animate-toast-in',
          title: 'text-table font-600 text-ink leading-[1.4]',
          description: 'mt-0.5 text-caption font-500 text-ink-3 leading-[1.4]',
          icon: 'mt-px flex size-4 shrink-0 items-center justify-center',
          actionButton:
            'ml-auto inline-flex h-7 shrink-0 items-center rounded-r1 bg-dark px-2.5 text-micro font-650 text-ink-inverse',
          cancelButton:
            'ml-auto inline-flex h-7 shrink-0 items-center rounded-r1 px-2.5 text-micro font-650 text-ink-3 hover:bg-ink-hover',
          closeButton: 'hidden',
        },
      }}
      {...props}
    />
  );
};

export { Toaster };
