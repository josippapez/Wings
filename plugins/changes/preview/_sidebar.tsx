// The sidebar page with one button pressed once it has drawn and `after` ms have passed. Which tab shows is the page's own
// state, so a fixture reaches Branch or History the way a click does.
import { useEffect } from "react";
import Sidebar from "../src/pages/sidebar";

export function Pressed({ button, after = 0 }: { button: string; after?: number }) {
  useEffect(() => {
    const start = Date.now();
    const timer = setInterval(() => {
      if (Date.now() - start < after) return;
      const found = [...document.querySelectorAll("button")].find((b) => b.textContent === button || b.title === button);
      if (!found) return;
      clearInterval(timer);
      found.click();
    }, 50);
    return () => clearInterval(timer);
  }, [button, after]);
  return <Sidebar />;
}
