import { useEffect, useState } from "react";

export const page = { place: "view", fill: true };

export default function GraphPage() {
  const [native, setNative] = useState<React.ReactNode>(null);
  useEffect(() => {
    let cleanup: (() => void) | undefined;
    let disposed = false;
    if (WINGS_NATIVE) {
      void import("../ui/GraphNative").then(({ GraphNative }) => {
        if (!disposed) setNative(<GraphNative />);
      });
    } else {
      void import("../ui/graph-browser").then(({ mountGraph }) => {
        if (!disposed) cleanup = mountGraph();
      });
    }
    return () => { disposed = true; cleanup?.(); };
  }, []);
  return native;
}
