import { useEffect, useState } from 'react';

/**
 * Prevents skeleton flash on fast connections by enforcing a minimum
 * display duration before hiding the loading state.
 */
export function useMinLoadTime(isLoading: boolean, minMs = 300): boolean {
  const [show, setShow] = useState(isLoading);

  useEffect(() => {
    if (isLoading) {
      setShow(true);
      return;
    }
    const timer = setTimeout(() => setShow(false), minMs);
    return () => clearTimeout(timer);
  }, [isLoading, minMs]);

  return show;
}
