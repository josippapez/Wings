import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from "@/components/ui/select";
import { Sheet, SheetContent, SheetDescription, SheetHeader, SheetTitle } from "@/components/ui/sheet";
import { Switch } from "@/components/ui/switch";
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
    <div className="flex items-center justify-between gap-4">
      <div className="min-w-0">
        <Label htmlFor={props.id} className="text-[13px]">
          {props.label}
        </Label>
        {props.hint && <p className="text-[12px] leading-snug text-muted-foreground">{props.hint}</p>}
      </div>
      {props.children}
    </div>
  );
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

/** Wings settings. For now the terminal's keyboard, as in a native terminal's preferences. */
export function SettingsSheet(props: {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  keys: TerminalKeys;
  onKeysChange: (keys: TerminalKeys) => void;
  look: TerminalLook;
  onLookChange: (look: TerminalLook) => void;
}) {
  const { keys, look } = props;
  const setLook = (patch: Partial<TerminalLook>) => props.onLookChange({ ...look, ...patch });
  return (
    <Sheet open={props.open} onOpenChange={props.onOpenChange}>
      <SheetContent side="right" className="flex w-[420px] flex-col gap-0 p-0 sm:max-w-[420px]">
        <SheetHeader className="border-b border-hairline px-5 pt-5 pb-4">
          <SheetTitle>Settings</SheetTitle>
          <SheetDescription>These apply to every terminal right away.</SheetDescription>
        </SheetHeader>
        <div className="min-h-0 flex-1 overflow-y-auto">
        <section className="flex flex-col gap-4 px-5 py-4">
          <h3 className="text-[12px] font-medium text-muted-foreground">Text</h3>
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
        </section>
        <section className="flex flex-col gap-4 border-t border-hairline px-5 py-4">
          <h3 className="text-[12px] font-medium text-muted-foreground">Cursor</h3>
          <Row id="cursor-style" label="Cursor style">
            <Choice id="cursor-style" items={cursorChoices} value={look.cursorStyle} onChange={(cursorStyle) => setLook({ cursorStyle })} />
          </Row>
          <Row id="cursor-blink" label="Blink">
            <Switch id="cursor-blink" checked={look.cursorBlink} onCheckedChange={(cursorBlink) => setLook({ cursorBlink })} />
          </Row>
          <Row id="cursor-inactive" label="When the pane isn't focused">
            <Choice id="cursor-inactive" items={inactiveCursorChoices} value={look.cursorInactiveStyle} onChange={(cursorInactiveStyle) => setLook({ cursorInactiveStyle })} />
          </Row>
        </section>
        <section className="flex flex-col gap-4 border-t border-hairline px-5 py-4">
          <h3 className="text-[12px] font-medium text-muted-foreground">History and selection</h3>
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
        </section>
        <section className="flex flex-col gap-4 border-t border-hairline px-5 py-4">
          <h3 className="text-[12px] font-medium text-muted-foreground">Keyboard</h3>
          <div className="flex items-center justify-between gap-4">
            <div className="min-w-0">
              <Label htmlFor="option-as-meta" className="text-[13px]">
                Use Option as Meta
              </Label>
              <p className="text-[12px] leading-snug text-muted-foreground">The other Option key still types characters like ~ and @.</p>
            </div>
            <Select
              items={optionChoices}
              value={keys.optionAsMeta}
              onValueChange={(value) => value && props.onKeysChange({ ...keys, optionAsMeta: value as OptionAsMeta })}
            >
              <SelectTrigger id="option-as-meta" className="w-44 shrink-0">
                <SelectValue />
              </SelectTrigger>
              <SelectContent>
                {optionChoices.map((c) => (
                  <SelectItem key={c.value} value={c.value}>
                    {c.label}
                  </SelectItem>
                ))}
              </SelectContent>
            </Select>
          </div>
          <div className="flex items-center justify-between gap-4">
            <div className="min-w-0">
              <Label htmlFor="shift-return" className="text-[13px]">
                Shift-Return sends Meta Return
              </Label>
              <p className="text-[12px] leading-snug text-muted-foreground">Adds a new line in Claude Code instead of sending the prompt.</p>
            </div>
            <Switch
              id="shift-return"
              checked={keys.shiftReturnSendsMetaReturn}
              onCheckedChange={(on) => props.onKeysChange({ ...keys, shiftReturnSendsMetaReturn: on })}
            />
          </div>
        </section>
        </div>
      </SheetContent>
    </Sheet>
  );
}
