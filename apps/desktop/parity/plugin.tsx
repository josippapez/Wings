// The plugin side of `pnpm parity`: wings-plugin-ui's components with only its stylesheet, as a plugin page has them.
import { createRoot } from "react-dom/client"
import "wings-plugin-ui/plugin-ui.css"
import { Badge, Button, Chip, Menu, Select, Tabs, TextInput, Textarea } from "wings-plugin-ui"

const variants = ["default", "secondary", "outline", "ghost", "destructive", "link"] as const
const sizes = ["default", "sm", "xs", "lg"] as const
const fruit = [
  { value: "apple", label: "Apple" },
  { value: "pear", label: "Pear" },
]
const tabs = [
  { value: "a", label: "First" },
  { value: "b", label: "Second" },
]
const noop = () => {}

createRoot(document.getElementById("root")!).render(
  <div style={{ display: "flex", flexDirection: "column", gap: 12, padding: 16, width: 520 }}>
    <div style={{ display: "flex", gap: 8, flexWrap: "wrap" }}>
      {variants.map((v) => (
        <Button key={v} variant={v} {...{ "data-case": `button-${v}` }}>{v}</Button>
      ))}
      {sizes.map((s) => (
        <Button key={s} variant="outline" size={s} {...{ "data-case": `button-size-${s}` }}>{s}</Button>
      ))}
    </div>
    <div style={{ display: "flex", gap: 8, flexWrap: "wrap" }}>
      {(["default", "secondary", "destructive", "outline", "ghost", "link"] as const).map((v) => (
        <Badge key={v} variant={v} {...{ "data-case": `badge-${v}` }}>{v}</Badge>
      ))}
    </div>
    <div style={{ display: "flex", gap: 8, flexWrap: "wrap" }}>
      {variants.map((v) => (
        <Button key={v} variant={v} disabled {...{ "data-case": `button-${v}-disabled` }}>{v}</Button>
      ))}
    </div>
    <TextInput {...{ "data-case": "input-disabled" }} disabled defaultValue="Disabled" label="Disabled" />
    <TextInput {...{ "data-case": "input" }} placeholder="Search" label="Search" />
    <TextInput {...{ "data-case": "input-value" }} defaultValue="Typed text" label="Typed" />
    <Textarea {...{ "data-case": "textarea" }} rows={2} defaultValue={"Two lines\nof text"} label="Notes" />
    {(["default", "line"] as const).map((variant) => (
      <Tabs key={variant} variant={variant} items={tabs} value="a" onChange={noop} label="Tabs" {...{ "data-case": `tabs-${variant}` }} />
    ))}
    <Tabs items={[tabs[0], { ...tabs[1], disabled: true }]} value="a" onChange={noop} label="Tabs disabled" {...{ "data-case": "tabs-disabled" }} />
    <div style={{ display: "flex", gap: 8 }}>
      <Chip pressed={false} {...{ "data-case": "chip" }}>Off</Chip>
      <Chip pressed {...{ "data-case": "chip-on" }}>On</Chip>
      <Chip pressed={false} disabled {...{ "data-case": "chip-disabled" }}>Off</Chip>
      <Chip pressed disabled {...{ "data-case": "chip-on-disabled" }}>On</Chip>
    </div>
    <div style={{ width: 200 }} data-case="select">
      <Select options={fruit} defaultValue="apple" label="Fruit" />
    </div>
    <div style={{ width: 200 }} data-case="select-disabled">
      <Select options={fruit} defaultValue="apple" label="Fruit disabled" disabled />
    </div>
    <div data-case="menu">
      <Menu variant="outline" label="Actions" items={[{ id: "r", label: "Rename", onSelect: noop }, { id: "d", label: "Delete", variant: "destructive", onSelect: noop }]} />
    </div>
  </div>
)
