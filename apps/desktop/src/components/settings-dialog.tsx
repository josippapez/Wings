import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from "@/components/ui/select";
import { Dialog, DialogContent, DialogDescription, DialogTitle } from "@/components/ui/dialog";
import { Switch } from "@/components/ui/switch";
import { Tabs, TabsContent, TabsList, TabsTrigger } from "@/components/ui/tabs";
import { KeyboardIcon, PuzzleIcon, ScrollTextIcon, TextCursorIcon, TypeIcon } from "lucide-react";
import { PluginsSection } from "@/components/plugins-section";
import type { PluginView } from "@/lib/api";
import { cn } from "@/lib/utils";
import type { CursorInactiveStyle, CursorStyle, OptionAsMeta, TerminalKeys, TerminalLook } from "@/lib/terminal";

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
  { id: "text", label: "Text", icon: TypeIcon },
  { id: "cursor", label: "Cursor", icon: TextCursorIcon },
  { id: "history", label: "History and selection", icon: ScrollTextIcon },
  { id: "keyboard", label: "Keyboard", icon: KeyboardIcon },
  { id: "plugins", label: "Plugins", icon: PuzzleIcon },
] as const;

export type SettingsSection = (typeof sections)[number]["id"];

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
export function SettingsDialog(props: {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  section: SettingsSection;
  onSectionChange: (section: SettingsSection) => void;
  pluginsProps: PluginsProps;
  keys: TerminalKeys;
  onKeysChange: (keys: TerminalKeys) => void;
  look: TerminalLook;
  onLookChange: (look: TerminalLook) => void;
}) {
  const { keys, look } = props;
  const setLook = (patch: Partial<TerminalLook>) => props.onLookChange({ ...look, ...patch });
  return (
    <Dialog open={props.open} onOpenChange={props.onOpenChange}>
      <DialogContent className={cn(
          "flex gap-0 overflow-hidden p-0 transition-[width,height] duration-200 ease-out sm:max-w-none",
          // Plugins has a list and a details pane side by side, so it gets a bigger window.
          props.section === "plugins" ? "h-[min(680px,calc(100%-4rem))] w-[min(980px,calc(100%-4rem))]" : "h-[min(560px,calc(100%-4rem))] w-[min(760px,calc(100%-4rem))]",
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
                  className="h-8 flex-none justify-start gap-2 rounded-md px-2 text-[13px] data-active:bg-white/[0.07] dark:group-data-[variant=line]/tabs-list:data-active:bg-white/[0.07] after:hidden"
                >
                  <Icon aria-hidden />
                  <span className="truncate">{label}</span>
                </TabsTrigger>
              ))}
            </TabsList>
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
              </Group>
            </TabsContent>
            <TabsContent value="plugins" className="min-h-0 flex-1">
              <PluginsSection {...props.pluginsProps} />
            </TabsContent>
          </div>
        </Tabs>
      </DialogContent>
    </Dialog>
  );
}
