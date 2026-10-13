// The reference side of `pnpm parity`: the app's own shadcn components, as parity/run.mjs probes them.
import { createRoot } from "react-dom/client"
import "../src/index.css"
import { Badge } from "@/components/ui/badge"
import { Button } from "@/components/ui/button"
import { DropdownMenu, DropdownMenuContent, DropdownMenuItem, DropdownMenuTrigger } from "@/components/ui/dropdown-menu"
import { Input } from "@/components/ui/input"
import { Select, SelectContent, SelectGroup, SelectItem, SelectTrigger, SelectValue } from "@/components/ui/select"
import { Tabs, TabsList, TabsTrigger } from "@/components/ui/tabs"
import { Toggle } from "@/components/ui/toggle"

const variants = ["default", "secondary", "outline", "ghost", "destructive", "link"] as const
const sizes = ["default", "sm", "xs", "lg"] as const
const fruit = [
  { value: "apple", label: "Apple" },
  { value: "pear", label: "Pear" },
]

createRoot(document.getElementById("root")!).render(
  <div style={{ display: "flex", flexDirection: "column", gap: 12, padding: 16, width: 520 }}>
    <div style={{ display: "flex", gap: 8, flexWrap: "wrap" }}>
      {variants.map((v) => (
        <Button key={v} variant={v} data-case={`button-${v}`}>{v}</Button>
      ))}
      {sizes.map((s) => (
        <Button key={s} variant="outline" size={s} data-case={`button-size-${s}`}>{s}</Button>
      ))}
    </div>
    <div style={{ display: "flex", gap: 8, flexWrap: "wrap" }}>
      {(["default", "secondary", "destructive", "outline", "ghost", "link"] as const).map((v) => (
        <Badge key={v} variant={v} data-case={`badge-${v}`}>{v}</Badge>
      ))}
    </div>
    <Input data-case="input" placeholder="Search" aria-label="Search" />
    <Input data-case="input-value" defaultValue="Typed text" aria-label="Typed" />
    {(["default", "line"] as const).map((variant) => (
      <Tabs key={variant} defaultValue="a" data-case={`tabs-${variant}`}>
        <TabsList variant={variant}>
          <TabsTrigger value="a">First</TabsTrigger>
          <TabsTrigger value="b">Second</TabsTrigger>
        </TabsList>
      </Tabs>
    ))}
    <div style={{ display: "flex", gap: 8 }}>
      <Toggle size="sm" data-case="chip" aria-label="Off">Off</Toggle>
      <Toggle size="sm" defaultPressed data-case="chip-on" aria-label="On">On</Toggle>
    </div>
    <div style={{ width: 200 }} data-case="select">
      <Select items={fruit} defaultValue="apple">
        <SelectTrigger className="w-full" aria-label="Fruit"><SelectValue /></SelectTrigger>
        <SelectContent>
          <SelectGroup>
            {fruit.map((f) => (<SelectItem key={f.value} value={f.value}>{f.label}</SelectItem>))}
          </SelectGroup>
        </SelectContent>
      </Select>
    </div>
    <div data-case="menu">
      <DropdownMenu>
        <DropdownMenuTrigger render={<Button variant="outline" />}>Actions</DropdownMenuTrigger>
        <DropdownMenuContent>
          <DropdownMenuItem>Rename</DropdownMenuItem>
          <DropdownMenuItem variant="destructive">Delete</DropdownMenuItem>
        </DropdownMenuContent>
      </DropdownMenu>
    </div>
  </div>
)
