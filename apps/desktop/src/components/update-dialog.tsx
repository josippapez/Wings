import { useEffect, useState } from "react";
import { openUrl } from "@tauri-apps/plugin-opener";
import type { Update } from "@tauri-apps/plugin-updater";
import { Button } from "@/components/ui/button";
import { Dialog, DialogContent, DialogDescription, DialogFooter, DialogHeader, DialogTitle } from "@/components/ui/dialog";
import { Markdown } from "@/components/markdown";
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

  const notes = update.body?.trim() ?? "";
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
        <div className="max-h-[50vh] overflow-y-auto" data-testid="update-notes">
          {!notes ? (
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
            // The notes are this version's CHANGELOG.md section, written in Markdown.
            <Markdown source={notes} className="leading-snug" />
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
