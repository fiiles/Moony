import { useLayoutEffect, useRef, type ReactNode } from 'react';
import { useLocation } from 'wouter';

/** Animate navigation without remounting pages or retaining a transformed container. */
export function PageTransition({ children }: { children: ReactNode }) {
  const [location] = useLocation();
  const contentRef = useRef<HTMLDivElement>(null);

  useLayoutEffect(() => {
    const content = contentRef.current;
    const motion = window.matchMedia('(prefers-reduced-motion: reduce)');
    if (!content || motion.matches) return;

    const animation = content.animate(
      [
        { opacity: 0.2, transform: 'translateY(8px)' },
        { opacity: 1, transform: 'translateY(0)' },
      ],
      { duration: 400, easing: 'ease-out' }
    );
    const handleMotionChange = () => {
      if (motion.matches) animation.cancel();
    };
    motion.addEventListener('change', handleMotionChange);
    return () => {
      animation.cancel();
      motion.removeEventListener('change', handleMotionChange);
    };
  }, [location]);

  return <div ref={contentRef}>{children}</div>;
}
