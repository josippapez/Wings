// Lets a node test import plugin source the way esbuild and tsc read it: `./x` finds `./x.ts`, which node alone
// doesn't. `pnpm test` loads it with `node --import`.
import { registerHooks } from "node:module";

registerHooks({
  resolve(specifier, context, nextResolve) {
    try {
      return nextResolve(specifier, context);
    } catch (error) {
      if (error?.code !== "ERR_MODULE_NOT_FOUND" || !specifier.startsWith(".")) throw error;
      try {
        return nextResolve(`${specifier}.ts`, context);
      } catch {
        throw error;
      }
    }
  },
});
