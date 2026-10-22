// Plays what a person does on a fixture once it has drawn: click a button or a menu item. A state uses it to show the page
// after a click, like the countdown after Move to Trash. A click names a button by what its text, title or label starts with,
// and waits until there is one.
import { useEffect, type ReactNode } from "react";

export type Step = { click: string };

const find = (named: string) =>
  [...document.querySelectorAll<HTMLElement>("button, [role=menuitem]")].find(
    (el) => [el.textContent, el.title, el.getAttribute("aria-label")].some((text) => text?.startsWith(named)),
  );

export function Steps({ steps, children }: { steps: Step[]; children: ReactNode }) {
  useEffect(() => {
    let next = 0;
    const timer = setInterval(() => {
      if (next >= steps.length) return clearInterval(timer);
      const button = find(steps[next].click);
      if (button) {
        button.click();
        next++;
      }
    }, 80);
    return () => clearInterval(timer);
  }, [steps]);
  return <>{children}</>;
}
