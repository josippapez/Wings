# wings-plugin-sdk

Types, a React hook and a build command for [Wings](https://github.com/josippapez/Wings) plugins.

Wings itself gives each plugin the `wings` object at runtime. This package doesn't contain it: it types it, and
bundles your TypeScript and React into the plain scripts Wings loads.

## Set up

```sh
npm install --save-dev wings-plugin-sdk typescript
npm install --save-dev react react-dom @types/react @types/react-dom   # only for React pages
```

`tsconfig.json`:

```json
{
  "compilerOptions": {
    "target": "ES2022",
    "module": "ESNext",
    "moduleResolution": "bundler",
    "jsx": "react-jsx",
    "strict": true,
    "noEmit": true,
    "types": ["wings-plugin-sdk"]
  },
  "include": ["src"]
}
```

## Build

Put the code in `src/`, next to `wings-plugin.json`. Every file at the top of `src/` builds to the file of the
same name in the plugin folder, so `src/main.ts` becomes `main.js` and `src/sidebar.tsx` becomes `sidebar.js`.
Folders inside `src/` hold the modules they import.

```sh
npx wings-plugin build           # writes main.js, sidebar.js, ...
npx wings-plugin build --check   # fails if a built file is older than its source
```

Commit the built files. Wings installs a plugin from GitHub as the files in the repo, without building it.

## React pages

A sidebar, popover or view page is your own HTML that loads its built script:

```html
<main id="app"></main>
<script src="sidebar.js"></script>
```

```tsx
import { useState } from "react";
import { createRoot } from "react-dom/client";
import { useBroadcast } from "wings-plugin-sdk/react";

function Sidebar() {
  const [state, setState] = useState<unknown>(null);
  useBroadcast((message) => setState(message));
  return <pre>{JSON.stringify(state, null, 2)}</pre>;
}

createRoot(document.getElementById("app")!).render(<Sidebar />);
```

Every `wings.on*` call returns a function that removes the listener, so you can also return it from a
`useEffect` yourself. The API reference is
[plugins/README.md](https://github.com/josippapez/Wings/blob/main/plugins/README.md) in the Wings repo.
