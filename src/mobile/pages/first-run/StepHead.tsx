import { useEffect, useRef } from 'react';

/**
 * A step's title and the sentence under it. The title takes the focus when the step opens, so a
 * screen reader starts on the new step instead of on the button that is gone (no keyboard opens).
 */
export function StepHead({ title, lead }: { title: string; lead: string }) {
  const heading = useRef<HTMLHeadingElement>(null);
  useEffect(() => {
    heading.current?.focus({ preventScroll: true });
  }, []);
  return (
    <>
      <h1
        ref={heading}
        tabIndex={-1}
        className="m-0 hyphens-auto break-words text-m-title-lg text-ink outline-none"
      >
        {title}
      </h1>
      <p className="mb-5 mt-1.5 text-[13px] font-450 leading-[1.4] text-ink-3">{lead}</p>
    </>
  );
}
