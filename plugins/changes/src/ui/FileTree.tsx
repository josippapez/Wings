import type { ReactNode } from "react";
import { basename } from "wings-plugin-sdk/format";
import { List, ListButton, ListItem, Text } from "wings-plugin-ui";

import type { FileRow } from "../lib/types";

interface Folder {
  name: string;
  path: string;
  folders: Map<string, Folder>;
  files: FileRow[];
}

const STATUS = { M: "Modified", A: "Added", D: "Deleted", R: "Renamed", C: "Copied", T: "Type changed", U: "Conflict", "?": "New, not tracked yet" };

function tree(files: FileRow[]) {
  const root: Folder = { name: "", path: "", folders: new Map(), files: [] };
  for (const row of files) {
    const parts = row[1].split("/");
    let at = root;
    for (const part of parts.slice(0, -1)) {
      const path = at.path ? `${at.path}/${part}` : part;
      if (!at.folders.has(part)) at.folders.set(part, { name: part, path, folders: new Map(), files: [] });
      at = at.folders.get(part)!;
    }
    at.files.push(row);
  }
  return root;
}

interface Props {
  /** Folders you collapsed, by path. */
  collapsed: Set<string>;
  onToggle: (path: string, isOpen: boolean) => void;
  onOpen: (path: string) => void;
}

/** Folder and file rows, a folder with only one folder in it shown as `a/b`. */
function rows(folder: Folder, depth: number, props: Props): ReactNode[] {
  const out: ReactNode[] = [];
  for (const sub of folder.folders.values()) {
    let f = sub;
    let label = f.name;
    while (f.files.length === 0 && f.folders.size === 1) {
      f = [...f.folders.values()][0];
      label += `/${f.name}`;
    }
    const isOpen = !props.collapsed.has(f.path);
    const path = f.path;
    out.push(
      <ListItem key={`d:${path}`}>
        <ListButton className="folder" style={{ paddingLeft: `${6 + depth * 12}px` }} aria-expanded={isOpen} onClick={() => props.onToggle(path, isOpen)}>
          <span className={`chevron${isOpen ? " open" : ""}`} aria-hidden="true" />
          <Text grow truncate>
            {label}
          </Text>
        </ListButton>
      </ListItem>,
    );
    if (isOpen) out.push(...rows(f, depth + 1, props));
  }
  for (const [status, path, added, removed, oldPath] of folder.files) {
    const name = basename(path);
    const what = STATUS[status as keyof typeof STATUS] ?? status;
    out.push(
      <ListItem key={`f:${path}`}>
        <ListButton style={{ paddingLeft: `${18 + depth * 12}px` }} title={oldPath ? `${oldPath} → ${path}` : path} onClick={() => props.onOpen(path)}>
          <span className={`status s-${status === "?" ? "new" : status}`} title={what} aria-label={what}>
            {status}
          </span>
          <Text grow truncate>
            {name}
          </Text>
          {added == null ? <Text tone="muted">{removed == null ? "binary" : "new"}</Text> : null}
          {added ? <Text tone="success" size="tiny" numeric>{`+${added}`}</Text> : null}
          {removed ? <Text tone="danger" size="tiny" numeric>{`−${removed}`}</Text> : null}
        </ListButton>
      </ListItem>,
    );
  }
  return out;
}

/** The changed files as a folder tree. */
export function FileTree({ files, ...props }: Props & { files: FileRow[] }) {
  return <List className="tree">{rows(tree(files), 0, props)}</List>;
}
