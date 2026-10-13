# Wings plugin template

A starting point for a Wings plugin that lives in its own repo. It shows each pane's git branch as a badge.

## Start a plugin

1. Copy this folder into a new repo and run `npm install`.
2. In `wings-plugin.json`, set your own `id` (lowercase letters, digits and dashes), `name` and `description`. Replace `icon.svg`, the plugin's one-colour icon in the Plugins list (24 by 24, up to 32 KB).
3. Replace `src/main.ts`. A sidebar or other page is `src/<page>.tsx`, which can use React. The `wings-plugin-sdk` README shows how.
4. Commit `src/`, `package.json` and `package-lock.json`. The built files stay out of git: the release workflow builds them.

The full API is in the Wings repo, `plugins/README.md`.

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
