# Wings plugin template

A starting point for a Wings plugin that lives in its own repo. It shows each pane's git branch as a badge.

## Start a plugin

1. Copy this folder into a new repo.
2. In `wings-plugin.json`, set your own `id` (lowercase letters, digits and dashes), `name` and `description`.
3. Replace `main.js`. `wings.d.ts` gives your editor types for the `wings` object.

The full API is in the Wings repo, `plugins/README.md`.

## Try it in Wings

Make a package and drop it on the Wings window, or pick it with Plugins, Choose a file:

```sh
git archive --format=zip --output hello-wings.wings-plugin HEAD
```

The package holds what's committed, so commit first. Wings asks you to approve the plugin's access before it runs.

## Publish it

Bump `version` in `wings-plugin.json`, commit, then tag and push:

```sh
git tag v0.2.0 && git push origin v0.2.0
```

The release workflow attaches `<id>.wings-plugin` to a GitHub release. Anyone can then install it by pasting the repo link into Plugins, and Wings offers the update when a new release comes out. The repo has to be public for that. For a private plugin, share the `.wings-plugin` file instead.
