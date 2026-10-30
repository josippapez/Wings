// Plays what a person does on a fixture once it has drawn: click a button, type into a field, press a key, scroll an
// element to its end. A state uses it to show a page after a failed save, which only a click and some typing reach.
import { useEffect, type ReactNode } from "react";

export type Step = { click: string } | { type: [label: string, text: string] } | { key: [label: string, key: string] } | { scroll: string };

const find = (selector: string, named: string) => [...document.querySelectorAll<HTMLElement>(selector)].find((el) => el.textContent === named || el.title === named || el.getAttribute("aria-label") === named);

function play(step: Step) {
  if ("scroll" in step) {
    const el = document.querySelector(step.scroll);
    return el ? (el.scrollTop = el.scrollHeight, true) : false;
  }
  if ("click" in step) {
    const button = find("button, [role=menuitem]", step.click);
    return button ? (button.click(), true) : false;
  }
  const [label, value] = "type" in step ? step.type : step.key;
  const field = document.querySelector<HTMLInputElement | HTMLTextAreaElement>(`input[aria-label="${label}"], textarea[aria-label="${label}"]`);
  if (!field) return false;
  if ("type" in step) {
    const set = Object.getOwnPropertyDescriptor(field instanceof HTMLTextAreaElement ? HTMLTextAreaElement.prototype : HTMLInputElement.prototype, "value")?.set;
    set?.call(field, value);
    field.dispatchEvent(new Event("input", { bubbles: true }));
  } else {
    field.dispatchEvent(new KeyboardEvent("keydown", { key: value, bubbles: true, cancelable: true }));
  }
  return true;
}

export function Steps({ steps, children }: { steps: Step[]; children: ReactNode }) {
  useEffect(() => {
    let next = 0;
    const timer = setInterval(() => {
      if (next >= steps.length) return clearInterval(timer);
      if (play(steps[next])) next++;
    }, 80);
    return () => clearInterval(timer);
  }, [steps]);
  return <>{children}</>;
}
