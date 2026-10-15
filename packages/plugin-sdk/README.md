# wings-plugin-sdk

The `wings` API types, the page frame, React hooks and the build command for [Wings](https://github.com/josippapez/Wings)
plugins. The components for plugin pages are in [wings-plugin-ui](../plugin-ui/README.md).

Wings itself gives each plugin the `wings` object at runtime. This package doesn't contain it: it types it, and
bundles your TypeScript and React into the plain scripts Wings loads.

## Set up

```sh
npm install --save-dev wings-plugin-sdk typescript
npm install --save-dev wings-plugin-ui react react-dom @types/react @types/react-dom   # only for React pages
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

Put the code in `src/`, next to `wings-plugin.json`. The entries are `src/main.ts` and each file in `src/pages/`,
and they build into `dist/` under their own names: `src/main.ts` becomes `dist/main.js` and
`src/pages/sidebar.tsx` becomes `dist/sidebar.js`. The manifest points there, with `"main": "dist/main.js"` and
`"page": "dist/sidebar.html"`. A page that uses `wings-plugin-ui` also gets its `dist/<name>.html` written for
it, unless you wrote your own there, and a plugin with no `src/main.ts` gets an empty `dist/main.js`, since the
manifest needs a `main`. Nothing else in `src/` is an entry. Keep the rest in `src/ui/` (React components),
`src/lib/` (logic with no UI, and its tests) and, if your main script grows, `src/main/`, with a page's CSS
next to what imports it.

```sh
npx wings-plugin pack            # builds and packs the plugin as <id>.wings-plugin, see Pack
npx wings-plugin build           # builds into dist/, for a repo that commits its built files
npx wings-plugin build --check   # fails if a committed file in dist/ is older than its source
```

A plugin in its own repo uses `pack`, and its repo holds only source. `build` is for a folder that Wings loads
as it is, like the plugins in the Wings repo, which commit `dist/`.

## Pages

A sidebar, popover, view, block, tab or settings page is a file in `src/pages/`. A React page default-exports its
component and says where it shows with `export const page = { place: "sidebar" }`: `place` is `sidebar`,
`popover`, `view`, `block`, `tab` or `settings`, and `fill: true` lays the page out edge to edge. The build writes
the mount, which calls the matching `show*` function of [wings-plugin-ui](../plugin-ui/README.md) (a block mounts
as a view), and the page's HTML, so a test or a fixture can import the page without it mounting. A page that builds
its DOM itself, without React, is a `.ts` file built as it is, and gets the same frame from
`mountShell(place, options)` in `wings-plugin-sdk/shell`, which returns the element to fill. Every
`wings.on*` call returns a function that removes the listener, so `useBroadcast` from
`wings-plugin-sdk/react`, or your own `useEffect`, cleans up when a component goes. The API reference is
[plugins/README.md](https://github.com/josippapez/Wings/blob/main/plugins/README.md) in the Wings repo.

## Polling

Wings is often in the background, and a timer that runs `git` or a network call then costs battery for nothing.
`pollWhileFocused(tick, ms)` from `wings-plugin-sdk/poll` runs `tick` every `ms` only while a Wings window is in
front, and once at once when you come back. It uses `wings.onFocus`.

## Pack

A plugin in its own repo doesn't need to commit what it builds. `wings-plugin pack` builds it into a temporary
folder and zips that as `<id>.wings-plugin`, the file you install in Wings' Plugins manager or attach to a GitHub
release:

```sh
npx wings-plugin pack   # writes hello-wings.wings-plugin and prints its path
```

The package holds `wings-plugin.json` at its top, the build in `dist/` (every entry, the generated pages and CSS),
and the files your plugin ships, like its icon, its own `.html`, `.css` and images, and its `README.md`, which
Wings shows in Settings. It leaves out `src/`, `preview/`, `node_modules`, `package.json` and lock files,
`tsconfig*.json`, `HANDOVER*`, dotfiles and other `.wings-plugin` files. Your folder gets no built files. From a
script, `pack(dir, { out })` in `wings-plugin-sdk/build` does the same and resolves the package's path.
