/**
 * Screenshots the fixtures in each folder's `preview/` folder with headless Chrome: `args` are what `wings-plugin shot`
 * takes (filters, `--width`, `--height`, `--out`) and `folders` the plugin folders, named by their folder name in the
 * PNGs. Resolves the exit code: 0, 1 when a fixture throws or logs a console error, 2 without Chrome or a match.
 */
export function shot(args: string[], folders: string[]): Promise<number>;
