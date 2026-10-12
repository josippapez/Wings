import { useEffect, useState } from "react";
import { openUrl } from "@tauri-apps/plugin-opener";
import type { Update } from "@tauri-apps/plugin-updater";
import { Button } from "@/components/ui/button";
import { Dialog, DialogContent, DialogDescription, DialogFooter, DialogHeader, DialogTitle } from "@/components/ui/dialog";
import { parseNotes } from "@/lib/release-notes";
import { installUpdate, onShowUpdate } from "@/lib/updater";

/** What an update brings, shown before installing it. Opened from the updater's toast and by Check for Updates…. */
export function UpdateDialog() {
  const [update, setUpdate] = useState<Update | null>(null);
  const [open, setOpen] = useState(false);
  useEffect(
    () =>
      onShowUpdate((u) => {
        setUpdate(u);
        setOpen(true);
      }),
    [],
  );
  if (!update) return null;

  const notes = parseNotes(update.body);
  const releaseUrl = `https://github.com/josippapez/Wings/releases/tag/v${update.version}`;
  return (
    <Dialog open={open} onOpenChange={setOpen}>
      <DialogContent className="sm:max-w-md" data-testid="update-dialog">
        <DialogHeader>
          <DialogTitle>Wings {update.version}</DialogTitle>
          <DialogDescription>
            {update.currentVersion} → {update.version}
          </DialogDescription>
        </DialogHeader>
        <div className="flex max-h-[50vh] flex-col gap-2 overflow-y-auto text-[13px] leading-snug" data-testid="update-notes">
          {notes.length === 0 ? (
            <p className="text-muted-foreground">
              No release notes for this version.{" "}
              <a
                href={releaseUrl}
                className="text-foreground underline underline-offset-2"
                onClick={(e) => {
                  e.preventDefault();
                  void openUrl(releaseUrl);
                }}
              >
                See it on GitHub
              </a>
            </p>
          ) : (
            notes.map((n, i) =>
              n.list ? (
                <ul key={i} className="list-disc space-y-1 pl-5">
                  {n.items.map((item, j) => (
                    <li key={j}>{item}</li>
                  ))}
                </ul>
              ) : (
                <p key={i}>{n.text}</p>
              ),
            )
          )}
        </div>
        <DialogFooter>
          <Button variant="outline" onClick={() => setOpen(false)} data-testid="update-later">
            Later
          </Button>
          <Button
            onClick={() => {
              setOpen(false);
              void installUpdate(update);
            }}
            data-testid="update-install"
          >
            Install and restart
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
