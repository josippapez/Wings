// The map's toolbar: the way back to the disks, back and forward through the folders you went into, the folder you are in as a
// path from the scan's root, and what can be done to it. Under 760px the buttons on the right fold into one menu.

import { Button, Icon, Menu, Row, Text, Toolbar } from "wings-plugin-ui";
import type { Crumb } from "./model";

export interface MapToolbarProps {
  /** The root first, the folder in the middle of the map last. */
  crumbs: Crumb[];
  canBack: boolean;
  canForward: boolean;
  onDisks: () => void;
  onBack: () => void;
  onForward: () => void;
  onFocus: (path: string) => void;
  onRescan: () => void;
  onReveal: () => void;
}

/** More folders than this and the ones in the middle go behind a menu. */
const MAX_CRUMBS = 4;

function Crumbs({ crumbs, onFocus }: { crumbs: Crumb[]; onFocus: (path: string) => void }) {
  const folded = crumbs.length > MAX_CRUMBS ? crumbs.slice(1, -2) : [];
  const shown = crumbs.length > MAX_CRUMBS ? [crumbs[0], ...crumbs.slice(-2)] : crumbs;
  return (
    <Row role="navigation" aria-label="Folder" gap="none" grow className="dk-crumbs">
      {shown.map((crumb, i) => {
        const last = crumb === crumbs.at(-1);
        return (
          <Row gap="none" className="dk-crumb-item" key={crumb.path}>
            {i > 0 ? <Text tone="muted" className="dk-sep" aria-hidden>›</Text> : null}
            {i === 1 && folded.length ? (
              <>
                <Menu variant="ghost" size="sm" label="…" aria-label="Folders in between" items={folded.map((c) => ({ id: c.path, label: c.name, onSelect: () => onFocus(c.path) }))} />
                <Text tone="muted" className="dk-sep" aria-hidden>›</Text>
              </>
            ) : null}
            {last ? (
              <Text weight="strong" truncate className="dk-crumb-current" aria-current="page" title={crumb.path}>{crumb.name}</Text>
            ) : (
              <Button variant="ghost" size="sm" className="dk-crumb" title={crumb.path} onClick={() => onFocus(crumb.path)}>
                <Text truncate>{crumb.name}</Text>
              </Button>
            )}
          </Row>
        );
      })}
    </Row>
  );
}

export function MapToolbar({ crumbs, canBack, canForward, onDisks, onBack, onForward, onFocus, onRescan, onReveal }: MapToolbarProps) {
  const buttons = (
    <Row gap="s" className="dk-wide">
      <Button variant="outline" size="sm" onClick={onRescan}>Rescan</Button>
      <Button variant="outline" size="sm" onClick={onReveal}>Reveal in Finder</Button>
    </Row>
  );
  return (
    <Toolbar
      className="dk-toolbar"
      actions={
        WINGS_NATIVE ? (
          buttons
        ) : (
          <>
            {buttons}
            <Menu
              className="dk-narrow"
              variant="outline"
              size="sm"
              align="end"
              label="Actions"
              items={[
                { id: "rescan", label: "Rescan", onSelect: onRescan },
                { id: "reveal", label: "Reveal in Finder", onSelect: onReveal },
              ]}
            />
          </>
        )
      }
    >
      <Button variant="ghost" size="sm" className="dk-disks" onClick={onDisks}>← Disks</Button>
      <Row gap="none" className="dk-history">
        {/* The native app can't turn an icon round, so its arrows are text. */}
        <Button variant="ghost" size="icon-sm" className="dk-back" aria-label="Back" title="Back" disabled={!canBack} onClick={onBack}>
          {WINGS_NATIVE ? "‹" : <Icon name="chevron-down" />}
        </Button>
        <Button variant="ghost" size="icon-sm" className="dk-forward" aria-label="Forward" title="Forward" disabled={!canForward} onClick={onForward}>
          {WINGS_NATIVE ? "›" : <Icon name="chevron-down" />}
        </Button>
      </Row>
      <Crumbs crumbs={crumbs} onFocus={onFocus} />
    </Toolbar>
  );
}
