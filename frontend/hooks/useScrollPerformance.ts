import { useEffect, useRef } from 'react';

/**
 * Attaches passive scroll/touch listeners to a container ref for
 * improved mobile scroll performance. Passive listeners allow the
 * browser to optimise scrolling without waiting for JS to complete.
 */
export function useScrollPerformance<T extends HTMLElement>() {
  const ref = useRef<T>(null);

  useEffect(() => {
    const el = ref.current;
    if (!el) return;

    const noop = () => {};

    // Passive listeners — browser can scroll without blocking on JS
    el.addEventListener('touchstart', noop, { passive: true });
    el.addEventListener('touchmove', noop, { passive: true });
    el.addEventListener('wheel', noop, { passive: true });

    return () => {
      el.removeEventListener('touchstart', noop);
      el.removeEventListener('touchmove', noop);
      el.removeEventListener('wheel', noop);
    };
  }, []);

  return ref;
}
