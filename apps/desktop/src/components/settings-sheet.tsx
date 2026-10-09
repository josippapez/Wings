import { Label } from "@/components/ui/label";
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from "@/components/ui/select";
import { Sheet, SheetContent, SheetDescription, SheetHeader, SheetTitle } from "@/components/ui/sheet";
import { Switch } from "@/components/ui/switch";
import type { OptionAsMeta, TerminalKeys } from "@/lib/terminal";

const optionChoices: { value: OptionAsMeta; label: string }[] = [
  { value: "off", label: "Off" },
  { value: "left", label: "Left Option only" },
  { value: "right", label: "Right Option only" },
  { value: "both", label: "Both Option keys" },
];

/** Wings settings. For now the terminal's keyboard, as in a native terminal's preferences. */
export function SettingsSheet(props: { open: boolean; onOpenChange: (open: boolean) => void; keys: TerminalKeys; onKeysChange: (keys: TerminalKeys) => void }) {
  const { keys } = props;
  return (
    <Sheet open={props.open} onOpenChange={props.onOpenChange}>
      <SheetContent side="right" className="flex w-[420px] flex-col gap-0 p-0 sm:max-w-[420px]">
        <SheetHeader className="border-b border-hairline px-5 pt-5 pb-4">
          <SheetTitle>Settings</SheetTitle>
          <SheetDescription>These apply to every terminal right away.</SheetDescription>
        </SheetHeader>
        <section className="flex flex-col gap-4 px-5 py-4">
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
      </SheetContent>
    </Sheet>
  );
}
