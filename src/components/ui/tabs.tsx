import * as React from 'react';
import * as TabsPrimitive from '@radix-ui/react-tabs';

import { cn } from '@/lib/utils';

/**
 * Tabs rendered as the segmented control (design system §6, `.seg`): a paper
 * pill with a shadow on a `well-2` track. Use <Segmented> for a plain value
 * switch without panels; Tabs when each value has its own content.
 */
const Tabs = TabsPrimitive.Root;

/** `segment` is the pill switch; `underline` is the detail-page sub-navigation (`.subnav-tabs`). */
type TabsVariant = 'segment' | 'underline';
const TabsVariantContext = React.createContext<TabsVariant>('segment');

const TabsList = React.forwardRef<
  React.ComponentRef<typeof TabsPrimitive.List>,
  React.ComponentPropsWithoutRef<typeof TabsPrimitive.List> & { variant?: TabsVariant }
>(({ className, variant = 'segment', ...props }, ref) => (
  <TabsVariantContext.Provider value={variant}>
    <TabsPrimitive.List
      ref={ref}
      className={cn(
        variant === 'segment'
          ? 'inline-flex items-center gap-px rounded-r2 bg-well-2 p-[3px]'
          : 'mb-[18px] flex gap-0.5 border-b border-line',
        className
      )}
      {...props}
    />
  </TabsVariantContext.Provider>
));
TabsList.displayName = TabsPrimitive.List.displayName;

const TabsTrigger = React.forwardRef<
  React.ComponentRef<typeof TabsPrimitive.Trigger>,
  React.ComponentPropsWithoutRef<typeof TabsPrimitive.Trigger>
>(({ className, ...props }, ref) => {
  const variant = React.useContext(TabsVariantContext);
  return (
    <TabsPrimitive.Trigger
      ref={ref}
      className={cn(
        variant === 'segment'
          ? 'inline-flex h-[26px] items-center justify-center whitespace-nowrap rounded-r1 px-[10px] text-micro font-600 text-ink-3 transition-colors duration-fast hover:text-ink focus-visible:outline-none focus-visible:shadow-focus disabled:pointer-events-none disabled:opacity-45 data-[state=active]:bg-paper data-[state=active]:text-ink data-[state=active]:shadow-seg'
          : '-mb-px inline-flex items-center gap-1.5 whitespace-nowrap border-b-2 border-transparent px-3 py-2.5 text-table font-650 text-ink-3 transition-colors duration-fast hover:text-ink focus-visible:outline-none focus-visible:shadow-focus disabled:pointer-events-none disabled:opacity-45 data-[state=active]:border-ink data-[state=active]:text-ink',
        className
      )}
      {...props}
    />
  );
});
TabsTrigger.displayName = TabsPrimitive.Trigger.displayName;

const TabsContent = React.forwardRef<
  React.ComponentRef<typeof TabsPrimitive.Content>,
  React.ComponentPropsWithoutRef<typeof TabsPrimitive.Content>
>(({ className, ...props }, ref) => (
  <TabsPrimitive.Content
    ref={ref}
    className={cn('mt-4 focus-visible:outline-none', className)}
    {...props}
  />
));
TabsContent.displayName = TabsPrimitive.Content.displayName;

export { Tabs, TabsList, TabsTrigger, TabsContent };
