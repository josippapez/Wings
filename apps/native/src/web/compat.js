// What this WebKit lacks that an extension built for Chrome uses, added only where it is missing. It runs first in an
// extension's own pages (popup, options) and, from a file Wings adds to its folder as it is added, in its background
// worker. Nothing here grants anything, and an API WebKit has is left as it is.
//
// - `scripting.ExecutionWorld`, the names of the worlds `executeScript` takes, which WebKit takes but doesn't list:
//   Bitwarden's background stopped at `chrome.scripting.ExecutionWorld.ISOLATED`, so it injected no autofill script.
// - A permission this WebKit doesn't know is "not granted", as Chrome answers one never given, rather than the error
//   WebKit throws for `permissions.contains` and rejects with for `permissions.request`: Bitwarden asks about `privacy`
//   before its login page shows, and the error left its popup on loading.
(() => {
  const api = globalThis.chrome || globalThis.browser;
  if (!api) return;
  // WebKit makes its objects again once nothing holds them, which drops what was added to them; these hold them. Only
  // the objects themselves are added to, never `chrome` or `browser`: a worker whose `chrome` and `browser` are both
  // stand-ins hears no reply to its messages.
  const kept = [];
  Object.defineProperty(globalThis, Symbol("wings-compat"), { value: kept });
  const scripting = api.scripting;
  if (scripting && scripting.ExecutionWorld === undefined) {
    scripting.ExecutionWorld = Object.freeze({ ISOLATED: "ISOLATED", MAIN: "MAIN" });
    kept.push(scripting);
  }
  const permissions = api.permissions;
  if (!permissions) return;
  const notGranted = (error) => {
    if (!/is not a valid permission\b/.test(String(error && error.message))) throw error;
    return false;
  };
  for (const name of ["contains", "request"]) {
    const method = permissions[name];
    if (typeof method !== "function") continue;
    permissions[name] = function (query, callback) {
      if (typeof callback === "function") {
        try {
          return method.call(permissions, query, callback);
        } catch (error) {
          const granted = notGranted(error);
          Promise.resolve().then(() => callback(granted));
          return undefined;
        }
      }
      try {
        return Promise.resolve(method.call(permissions, query)).catch(notGranted);
      } catch (error) {
        return Promise.resolve(notGranted(error));
      }
    };
  }
  kept.push(permissions);
})();
