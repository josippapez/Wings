import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from "@/components/ui/select";
import { Dialog, DialogContent, DialogDescription, DialogTitle } from "@/components/ui/dialog";
import { Switch } from "@/components/ui/switch";
import { Tabs, TabsContent, TabsList, TabsTrigger } from "@/components/ui/tabs";
import { getVersion } from "@tauri-apps/api/app";
import { useEffect, useRef, useState } from "react";
import { BellIcon, BotIcon, CodeIcon, SlidersHorizontalIcon, KeyboardIcon, PuzzleIcon, ScrollTextIcon, TextCursorIcon, TypeIcon } from "lucide-react";
import { PluginsSection } from "@/components/plugins-section";
import { defaultEditor, useDefaultEditorId, useEditors } from "@/lib/editors";
import type { PluginView } from "@/lib/api";
import { pluginSettingsSection, type NewProjectOpens, type NewTabEntry } from "@/lib/plugin-pages";
import { playSound, SOUNDS, type SoundSettings } from "@/lib/sound";
import { SLEEP_MINUTES, type SleepSettings } from "@/lib/sleep";
import type { PluginHost } from "@/lib/plugins";
import { cn } from "@/lib/utils";
import { acceleratorFromEvent, DEFAULT_GLOBAL_HOTKEY, hotkeyLabel, type GlobalHotkey } from "@/lib/hotkey";
import { isMac, type CursorInactiveStyle, type CursorStyle, type OptionAsMeta, type TerminalKeys, type TerminalLook } from "@/lib/terminal";

const optionChoices: { value: OptionAsMeta; label: string }[] = [
  { value: "off", label: "Off" },
  { value: "left", label: "Left Option only" },
  { value: "right", label: "Right Option only" },
  { value: "both", label: "Both Option keys" },
];

const lineHeightChoices = [1, 1.1, 1.2, 1.3, 1.4, 1.5, 1.6].map((n) => ({ value: n, label: n.toFixed(1) }));

const cursorChoices: { value: CursorStyle; label: string }[] = [
  { value: "block", label: "Block" },
  { value: "bar", label: "Bar" },
  { value: "underline", label: "Underline" },
];

const inactiveCursorChoices: { value: CursorInactiveStyle; label: string }[] = [
  { value: "outline", label: "Outline" },
  { value: "block", label: "Block" },
  { value: "bar", label: "Bar" },
  { value: "underline", label: "Underline" },
  { value: "none", label: "Hidden" },
];

const scrollbackChoices = [1_000, 5_000, 10_000, 25_000, 50_000, 100_000].map((n) => ({ value: n, label: `${n.toLocaleString("en-US")} lines` }));

const contrastChoices = [
  { value: 1, label: "Off" },
  { value: 4.5, label: "Readable (4.5)" },
  { value: 7, label: "High (7)" },
];

/** A label and optional hint on the left, a control on the right. */
function Row(props: { id: string; label: string; hint?: string; children: React.ReactNode }) {
  return (
    <div className="flex items-center justify-between gap-6 px-4 py-3">
      <div className="min-w-0">
        <Label htmlFor={props.id} className="text-[13px]">
          {props.label}
        </Label>
        {props.hint && <p className="mt-0.5 text-[12px] leading-snug text-muted-foreground">{props.hint}</p>}
      </div>
      {props.children}
    </div>
  );
}

/** Shows a shortcut; click it, then press a new one. Escape cancels, and Backspace puts the default back. */
function HotkeyRecorder(props: { value: string; disabled: boolean; onChange: (accelerator: string) => void }) {
  const [recording, setRecording] = useState(false);
  return (
    <button
      type="button"
      disabled={props.disabled}
      aria-label="Global shortcut. Press to change it"
      onClick={() => setRecording(true)}
      onBlur={() => setRecording(false)}
      onKeyDown={(e) => {
        if (!recording) return;
        e.preventDefault();
        e.stopPropagation();
        if (e.key === "Escape") return setRecording(false);
        if (e.key === "Backspace") props.onChange(DEFAULT_GLOBAL_HOTKEY.accelerator);
        else {
          const accelerator = acceleratorFromEvent(e);
          if (!accelerator) return;
          props.onChange(accelerator);
        }
        setRecording(false);
      }}
      className="h-8 min-w-24 rounded-md border border-hairline bg-white/[0.04] px-3 text-[13px] outline-none focus-visible:ring-2 focus-visible:ring-ring disabled:opacity-50"
    >
      {recording ? "Press a shortcut…" : hotkeyLabel(props.value, isMac)}
    </button>
  );
}

/** Rows grouped on one card, as in System Settings. */
function Group(props: { children: React.ReactNode }) {
  return <div className="divide-y divide-hairline rounded-xl border border-hairline bg-white/[0.03]">{props.children}</div>;
}

function Choice<T extends string | number>(props: { id: string; items: { value: T; label: string }[]; value: T; onChange: (value: T) => void }) {
  return (
    <Select items={props.items} value={props.value} onValueChange={(value) => value !== null && props.onChange(value as T)}>
      <SelectTrigger id={props.id} className="w-44 shrink-0">
        <SelectValue />
      </SelectTrigger>
      <SelectContent>
        {props.items.map((c) => (
          <SelectItem key={c.value} value={c.value}>
            {c.label}
          </SelectItem>
        ))}
      </SelectContent>
    </Select>
  );
}

const sections = [
  { id: "general", label: "General", icon: SlidersHorizontalIcon },
  { id: "text", label: "Text", icon: TypeIcon },
  { id: "cursor", label: "Cursor", icon: TextCursorIcon },
  { id: "history", label: "History and selection", icon: ScrollTextIcon },
  { id: "keyboard", label: "Keyboard", icon: KeyboardIcon },
  { id: "editor", label: "Editor", icon: CodeIcon },
  { id: "agents", label: "Agents", icon: BotIcon },
  { id: "notifications", label: "Notifications", icon: BellIcon },
  { id: "plugins", label: "Plugins", icon: PuzzleIcon },
] as const;

const sectionTrigger = "h-8 flex-none justify-start gap-2 rounded-md px-2 text-[13px] data-active:bg-white/[0.07] dark:group-data-[variant=line]/tabs-list:data-active:bg-white/[0.07] after:hidden";

/** Which editor "Open in editor" uses. */
function EditorSettings() {
  const editors = useEditors();
  const [chosen, choose] = useDefaultEditorId();
  const current = defaultEditor(editors, chosen);
  return (
    <Group>
      <Row
        id="default-editor"
        label="Open projects in"
        hint={current ? "The first item of a project's Open in menu. The others are one click further." : "No editor found. Install VS Code, Cursor or Zed, or an editor app in /Applications."}
      >
        {current && <Choice id="default-editor" items={editors.map((e) => ({ value: e.id, label: e.name }))} value={current.id} onChange={choose} />}
      </Row>
    </Group>
  );
}

/** One of Wings' own sections, or a plugin's settings page as `plugin:<id>`. */
export type SettingsSection = (typeof sections)[number]["id"] | ReturnType<typeof pluginSettingsSection>;

/** A plugin's settings page. `key` changes when the plugin restarts, so its page loads again. */
export type PluginSettingsRef = { plugin: PluginView; key: string };

/** A plugin's settings page, there while its section shows: leaving the section or closing Settings removes it. */
function PluginSettingsPage({ entry, host }: { entry: PluginSettingsRef; host: PluginHost }) {
  const box = useRef<HTMLDivElement>(null);
  useEffect(() => {
    if (!box.current) return;
    let remove: (() => void) | undefined;
    let gone = false;
    void host
      .mountPage(entry.plugin, null, box.current, "settings")
      .then((r) => (gone ? r.remove() : (remove = r.remove)))
      .catch((e) => console.error(e));
    return () => {
      gone = true;
      remove?.();
    };
  }, [entry.key, host]);
  return <div ref={box} data-plugin-settings={entry.plugin.id} className="size-full" />;
}

/** What the Plugins section needs from App: the list, the selection and the install and update state. */
export type PluginsProps = {
  plugins: PluginView[];
  selected: string | null;
  onSelect: (id: string) => void;
  dropped: string | null;
  onDroppedHandled: () => void;
  review: string | null;
  onReviewHandled: () => void;
  onChanged: (plugins: PluginView[], restart?: string) => void;
  updates: Record<string, string>;
  checking: boolean;
  onCheck: () => void;
};

/** Wings settings, in a window-sized dialog with a section list on the left, like a native app's Settings. */
/** The running Wings version, at the foot of the settings sidebar. */
function AppVersion() {
  const [version, setVersion] = useState<string | null>(null);
  useEffect(() => void getVersion().then(setVersion, () => {}), []);
  if (!version) return null;
  return <p className="mt-auto px-2 text-[12px] text-muted-foreground tabular-nums">Wings {version}</p>;
}

export function SettingsDialog(props: {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  section: SettingsSection;
  onSectionChange: (section: SettingsSection) => void;
  pluginsProps: PluginsProps;
  /** Plugins that are on and have a settings page, listed under Plugin settings. */
  pluginSettings: PluginSettingsRef[];
  host: PluginHost | null;
  keys: TerminalKeys;
  onKeysChange: (keys: TerminalKeys) => void;
  hotkey: GlobalHotkey;
  onHotkeyChange: (hotkey: GlobalHotkey) => void;
  look: TerminalLook;
  onLookChange: (look: TerminalLook) => void;
  sleep: SleepSettings;
  onSleepChange: (sleep: SleepSettings) => void;
  notifySound: SoundSettings;
  onNotifySoundChange: (sound: SoundSettings) => void;
  /** Plugin pages the + menu offers, and the one the + button opens (null for a terminal). */
  newTabs: NewTabEntry[];
  newTabPage: string;
  onNewTabPageChange: (choice: string) => void;
  newProjectOpens: NewProjectOpens;
  onNewProjectOpensChange: (value: NewProjectOpens) => void;
}) {
  const { keys, look, host } = props;
  const setLook = (patch: Partial<TerminalLook>) => props.onLookChange({ ...look, ...patch });
  return (
    <Dialog open={props.open} onOpenChange={props.onOpenChange}>
      <DialogContent className={cn(
          "flex gap-0 overflow-hidden p-0 transition-[width,height] duration-200 ease-out sm:max-w-none",
          // Plugins has a list and a details pane side by side, so it gets a bigger window.
          props.section === "plugins" ? "h-[min(760px,calc(100%-4rem))] w-[min(1100px,calc(100%-4rem))]" : "h-[min(600px,calc(100%-4rem))] w-[min(820px,calc(100%-4rem))]",
        )}>
        <Tabs value={props.section} onValueChange={(value) => props.onSectionChange(value as SettingsSection)} orientation="vertical" className="min-h-0 flex-1 gap-0">
          <nav className="flex w-52 shrink-0 flex-col gap-3 border-r border-hairline bg-black/15 p-3 pt-4">
            <div className="px-2">
              <DialogTitle>Settings</DialogTitle>
              <DialogDescription className="mt-1 text-[12px]">These apply to every terminal right away.</DialogDescription>
            </div>
            <TabsList variant="line" className="w-full items-stretch gap-0.5">
              {sections.map(({ id, label, icon: Icon }) => (
                <TabsTrigger
                  key={id}
                  value={id}
                  className={sectionTrigger}
                >
                  <Icon aria-hidden />
                  <span className="truncate">{label}</span>
                </TabsTrigger>
              ))}
              {props.pluginSettings.length > 0 && (
                <div role="presentation" className="px-2 pt-3 pb-1 text-[12px] font-medium text-muted-foreground">
                  Plugin settings
                </div>
              )}
              {props.pluginSettings.map(({ plugin }) => (
                <TabsTrigger
                  key={plugin.id}
                  value={pluginSettingsSection(plugin.id)}
                  className={sectionTrigger}
                >
                  <PuzzleIcon aria-hidden />
                  <span className="truncate">{plugin.name}</span>
                </TabsTrigger>
              ))}
            </TabsList>
            <AppVersion />
          </nav>
          <div className="flex min-h-0 min-w-0 flex-1 flex-col overflow-y-auto">
            <TabsContent value="text" className="flex flex-col gap-3 p-6">
              <h3 className="text-[15px] font-semibold">Text</h3>
              <Group>
                <Row id="font-family" label="Font family" hint="Tried before the default monospace fonts. Leave empty for the default.">
                  <Input
                    id="font-family"
                    className="w-44 shrink-0"
                    placeholder="JetBrains Mono"
                    value={look.fontFamily}
                    onChange={(e) => setLook({ fontFamily: e.target.value })}
                    spellCheck={false}
                    autoComplete="off"
                  />
                </Row>
                <Row id="line-height" label="Line height" hint="Spacing between lines, as a multiple of the font size.">
                  <Choice id="line-height" items={lineHeightChoices} value={look.lineHeight} onChange={(lineHeight) => setLook({ lineHeight })} />
                </Row>
                <Row id="min-contrast" label="Minimum contrast" hint="Lightens or darkens text that is hard to read on its background.">
                  <Choice id="min-contrast" items={contrastChoices} value={look.minimumContrastRatio} onChange={(minimumContrastRatio) => setLook({ minimumContrastRatio })} />
                </Row>
              </Group>
            </TabsContent>
            <TabsContent value="cursor" className="flex flex-col gap-3 p-6">
              <h3 className="text-[15px] font-semibold">Cursor</h3>
              <Group>
                <Row id="cursor-style" label="Cursor style">
                  <Choice id="cursor-style" items={cursorChoices} value={look.cursorStyle} onChange={(cursorStyle) => setLook({ cursorStyle })} />
                </Row>
                <Row id="cursor-blink" label="Blink">
                  <Switch id="cursor-blink" checked={look.cursorBlink} onCheckedChange={(cursorBlink) => setLook({ cursorBlink })} />
                </Row>
                <Row id="cursor-inactive" label="When the pane isn't focused">
                  <Choice id="cursor-inactive" items={inactiveCursorChoices} value={look.cursorInactiveStyle} onChange={(cursorInactiveStyle) => setLook({ cursorInactiveStyle })} />
                </Row>
              </Group>
            </TabsContent>
            <TabsContent value="history" className="flex flex-col gap-3 p-6">
              <h3 className="text-[15px] font-semibold">History and selection</h3>
              <Group>
                <Row
                  id="scrollback"
                  label="Scrollback"
                  hint="Lines kept per pane. A full 10,000 lines takes about 49 MB per pane. A smaller number drops older history."
                >
                  <Choice id="scrollback" items={scrollbackChoices} value={look.scrollback} onChange={(scrollback) => setLook({ scrollback })} />
                </Row>
                <Row id="copy-on-select" label="Copy on select" hint="Copies the selection to the clipboard when you let go of the mouse.">
                  <Switch id="copy-on-select" checked={look.copyOnSelect} onCheckedChange={(copyOnSelect) => setLook({ copyOnSelect })} />
                </Row>
              </Group>
              <Group>
                <Row
                  id="terminal-images"
                  label="Show images in the terminal"
                  hint="Draws images that programs print, such as SIXEL and iTerm2 inline images. Each pane keeps up to 24 MB of them."
                >
                  <Switch id="terminal-images" checked={look.images} onCheckedChange={(images) => setLook({ images })} />
                </Row>
                <Row
                  id="clipboard-write"
                  label="Let programs copy to the clipboard"
                  hint="Lets a program in a pane, such as tmux or a remote shell, put text on your clipboard (OSC 52). Programs can never read it."
                >
                  <Switch id="clipboard-write" checked={look.clipboardWrite} onCheckedChange={(clipboardWrite) => setLook({ clipboardWrite })} />
                </Row>
              </Group>
            </TabsContent>
            <TabsContent value="keyboard" className="flex flex-col gap-3 p-6">
              <h3 className="text-[15px] font-semibold">Keyboard</h3>
              <Group>
                <Row id="option-as-meta" label="Use Option as Meta" hint="The other Option key still types characters like ~ and @.">
                  <Choice
                    id="option-as-meta"
                    items={optionChoices}
                    value={keys.optionAsMeta}
                    onChange={(optionAsMeta) => props.onKeysChange({ ...keys, optionAsMeta })}
                  />
                </Row>
                <Row id="shift-return" label="Shift-Return sends Meta Return" hint="Adds a new line in Claude Code instead of sending the prompt.">
                  <Switch
                    id="shift-return"
                    checked={keys.shiftReturnSendsMetaReturn}
                    onCheckedChange={(on) => props.onKeysChange({ ...keys, shiftReturnSendsMetaReturn: on })}
                  />
                </Row>
                <Row id="global-hotkey" label="Bring Wings forward from any app" hint="Shows Wings and goes to the agent that needs you, or stays where you were if none does.">
                  <div className="flex items-center gap-3">
                    <HotkeyRecorder value={props.hotkey.accelerator} disabled={!props.hotkey.enabled} onChange={(accelerator) => props.onHotkeyChange({ ...props.hotkey, accelerator })} />
                    <Switch id="global-hotkey" checked={props.hotkey.enabled} onCheckedChange={(enabled) => props.onHotkeyChange({ ...props.hotkey, enabled })} />
                  </div>
                </Row>
              </Group>
            </TabsContent>
            <TabsContent value="general" className="flex flex-col gap-3 p-6">
              <h3 className="text-[15px] font-semibold">General</h3>
              <Group>
                <Row id="new-tab-page" label="New tab button opens" hint="What the + button does: open a terminal or the Project page, choose each time from the menu, or open a plugin page. Plugin pages that declare &quot;newTab&quot;: true in their view show here and in the menu next to the button. Cmd+T always opens a terminal.">
                  <Choice
                    id="new-tab-page"
                    items={[{ value: "terminal", label: "Terminal" }, { value: "ask", label: "Choose each time" }, { value: "project", label: "Project page" }, ...props.newTabs.map((e) => ({ value: e.key, label: e.title }))]}
                    value={["ask", "project"].includes(props.newTabPage) || props.newTabs.some((e) => e.key === props.newTabPage) ? props.newTabPage : "terminal"}
                    onChange={props.onNewTabPageChange}
                  />
                </Row>
                <Row id="new-project-opens" label="New projects open with" hint="The first tab of a project you add to Wings.">
                  <Choice
                    id="new-project-opens"
                    items={[{ value: "terminal", label: "Terminal" }, { value: "project", label: "Project page" }]}
                    value={props.newProjectOpens}
                    onChange={(v) => props.onNewProjectOpensChange(v === "project" ? "project" : "terminal")}
                  />
                </Row>
              </Group>
            </TabsContent>
            <TabsContent value="editor" className="flex flex-col gap-3 p-6">
              <h3 className="text-[15px] font-semibold">Editor</h3>
              <EditorSettings />
            </TabsContent>
            <TabsContent value="agents" className="flex flex-col gap-3 p-6">
              <h3 className="text-[15px] font-semibold">Agents</h3>
              <Group>
                <Row
                  id="sleep-agents"
                  label="Put idle agents to sleep"
                  hint="Ends an idle Claude Code or opencode that isn't on screen, which frees about 400 MB each. It stays in the Agents list as sleeping, and opening it picks the session up again."
                >
                  <Switch id="sleep-agents" checked={props.sleep.enabled} onCheckedChange={(enabled) => props.onSleepChange({ ...props.sleep, enabled })} />
                </Row>
                <Row id="sleep-after" label="Sleep after" hint="Time out of sight and idle. An agent that is working, needs you or has text typed in its prompt is left alone.">
                  <Choice
                    id="sleep-after"
                    items={SLEEP_MINUTES.map((m) => ({ value: m, label: m === 60 ? "1 hour" : `${m} minutes` }))}
                    value={props.sleep.minutes}
                    onChange={(minutes) => props.onSleepChange({ ...props.sleep, minutes })}
                  />
                </Row>
              </Group>
            </TabsContent>
            <TabsContent value="notifications" className="flex flex-col gap-3 p-6">
              <h3 className="text-[15px] font-semibold">Notifications</h3>
              <Group>
                <Row id="notify-sound" label="Play a sound" hint="When an agent needs you or finishes, a plugin notifies, or an update is ready. Several at once play one sound.">
                  <Switch id="notify-sound" checked={props.notifySound.enabled} onCheckedChange={(enabled) => props.onNotifySoundChange({ ...props.notifySound, enabled })} />
                </Row>
                <Row id="notify-sound-pick" label="Sound">
                  <div className="flex items-center gap-2">
                    <Button type="button" variant="outline" size="sm" data-testid="notify-sound-preview" onClick={() => playSound(props.notifySound.sound)}>
                      Preview
                    </Button>
                    <Choice
                      id="notify-sound-pick"
                      items={SOUNDS.map((s) => ({ value: s.id, label: s.label }))}
                      value={props.notifySound.sound}
                      onChange={(sound) => props.onNotifySoundChange({ ...props.notifySound, sound })}
                    />
                  </div>
                </Row>
                <Row id="notify-sound-watching" label="Also for the pane on screen" hint="Play it when the agent you're looking at needs you or finishes.">
                  <Switch id="notify-sound-watching" checked={props.notifySound.watching} disabled={!props.notifySound.enabled} onCheckedChange={(watching) => props.onNotifySoundChange({ ...props.notifySound, watching })} />
                </Row>
              </Group>
            </TabsContent>
            <TabsContent value="plugins" className="min-h-0 flex-1">
              <PluginsSection {...props.pluginsProps} />
            </TabsContent>
            {host &&
              props.pluginSettings.map((entry) => (
                <TabsContent key={entry.key} value={pluginSettingsSection(entry.plugin.id)} className="min-h-0 flex-1">
                  <PluginSettingsPage entry={entry} host={host} />
                </TabsContent>
              ))}
          </div>
        </Tabs>
      </DialogContent>
    </Dialog>
  );
}
