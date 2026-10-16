# Wings plugin template

A starting point for a Wings plugin that lives in its own repo. It shows each pane's git branch as a badge, and lists the branches in a sidebar.

## Start a plugin

1. Copy this folder into a new repo and run `npm install`.
2. In `wings-plugin.json`, set your own `id` (lowercase letters, digits and dashes), `name` and `description`. Replace `icon.svg`, the plugin's one-colour icon in the Plugins list (24 by 24, up to 32 KB). `minWingsVersion` is the oldest Wings the plugin runs on. This template needs 0.1.20, for `wings.onFocus` and a sidebar with no `icon` of its own, and you raise it when you use something newer.
3. Replace the code in `src/`:
   - `src/main.ts` is the plugin's hidden main script. It sets the badges and shares the branches.
   - `src/pages/sidebar.tsx` is the sidebar's page, in React with `wings-plugin-ui`. Each file in `src/pages/` is a page, and the manifest points at its built `dist/<name>.html`.
   - `src/lib/messages.ts` holds what main and the pages say to each other.
4. Replace this README. Wings shows a plugin's README on its page in Settings, so write it for the people who install your plugin.
5. Commit `src/`, `package.json` and `package-lock.json`. The built files in `dist/` stay out of git: the release workflow builds them.

`npm run check` type-checks the code, then builds and packs the plugin. The build also compares `wings-plugin.json` with the code and fails where they disagree, like a badge set without `badges` in `contributes.ui`.

Each helper in `wings-plugin-sdk` has a short example in its README (`node_modules/wings-plugin-sdk/README.md` once installed). The whole `wings` API is in `plugins/README.md` in the Wings repo.

## Try it in Wings

Package it and drop the file on the Wings window, or pick it with Plugins, Choose a file:

```sh
npm run pack    # writes hello-wings.wings-plugin
```

Wings asks you to approve the plugin's access before it runs.

## Publish it

Bump `version` in `wings-plugin.json`, commit, then tag and push:

```sh
git tag v0.2.0 && git push origin v0.2.0
```

The release workflow builds the plugin and attaches `<id>.wings-plugin` to a GitHub release. Anyone can then install it by pasting the repo link into Plugins, and Wings offers the update when a new release comes out. The repo has to be public for that. For a private plugin, share the `.wings-plugin` file instead.
