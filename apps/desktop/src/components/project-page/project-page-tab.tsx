import { ErrorBoundary } from "@/components/error-boundary";
import type { PluginView } from "@/lib/api";
import type { PluginHost } from "@/lib/plugins";
import { cn } from "@/lib/utils";

import { ProjectPage } from "./project-page";

/** A project's page as a tab: it fills the pane area like a plugin page and stays mounted while another tab shows. */
export function ProjectPageTab({ space, branch, plugins, host, visible }: { space: { id: string; name: string; path: string } | undefined; branch?: string | null; plugins: PluginView[]; host: PluginHost | null; visible: boolean }) {
  if (!space) return null;
  return (
    <div className={cn("absolute inset-0 flex p-2 pt-0", !visible && "invisible")} aria-hidden={!visible} inert={!visible}>
      <div data-project-scroll className="min-h-0 min-w-0 flex-1 overflow-y-auto rounded-[14px] border border-hairline bg-surface">
        <ErrorBoundary fallback={<p role="alert" className="p-6 text-[13px] text-muted-foreground">The project page couldn't be shown. Reload Wings, or edit it through an agent.</p>}>
          <ProjectPage space={space} branch={branch} plugins={plugins} host={host} visible={visible} />
        </ErrorBoundary>
      </div>
    </div>
  );
}
