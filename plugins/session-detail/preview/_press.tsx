// For a fixture of a page that shows more once you click: presses what the page shows after it has drawn, as a person would.
import { useEffect, type ReactNode } from "react";

/** Shows `children`, then clicks the elements `selectors` find, once all of them are on the page. */
export function PressWhenDrawn({ selectors, children }: { selectors: string[]; children: ReactNode }) {
  useEffect(() => {
    const timer = setInterval(() => {
      const found = selectors.map((selector) => document.querySelector<HTMLElement>(selector));
      if (found.some((el) => !el)) return;
      clearInterval(timer);
      for (const el of found) el!.click();
    }, 50);
    return () => clearInterval(timer);
  }, []);
  return children;
}
