// An extension's own pages (its popup and options): asked whether it holds a permission this WebKit doesn't know, the
// answer is "not granted", as Chrome answers one it was never given, rather than the error WebKit throws for
// `permissions.contains` and rejects with for `permissions.request`. Bitwarden asks about `privacy` before its login page
// shows, and the error left its popup on loading. Nothing is granted here, and every other answer and error is WebKit's.
(() => {
  const notGranted = (error) => {
    if (!/is not a valid permission\b/.test(String(error && error.message))) throw error;
    return false;
  };
  const answer = (method) => function (query, callback) {
    if (typeof callback === "function") {
      try {
        return method(query, callback);
      } catch (error) {
        const granted = notGranted(error);
        Promise.resolve().then(() => callback(granted));
        return undefined;
      }
    }
    try {
      return Promise.resolve(method(query)).catch(notGranted);
    } catch (error) {
      return Promise.resolve(notGranted(error));
    }
  };
  // WebKit's objects keep their own methods, so the page's `browser` and `chrome` become stand-ins that hand out these.
  for (const name of ["browser", "chrome"]) {
    const namespace = globalThis[name];
    const permissions = namespace && namespace.permissions;
    if (!permissions) continue;
    const wrapped = new Proxy(permissions, {
      get(target, key) {
        const value = target[key];
        if (typeof value !== "function") return value;
        const bound = value.bind(target);
        return key === "contains" || key === "request" ? answer(bound) : bound;
      },
    });
    globalThis[name] = new Proxy(namespace, {
      get(target, key) {
        if (key === "permissions") return wrapped;
        const value = target[key];
        return typeof value === "function" ? value.bind(target) : value;
      },
    });
  }
})();
