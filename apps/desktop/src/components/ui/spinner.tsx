import { cn } from "cn"
import { LoaderCircleIcon } from "lucide-react"

function Spinner({ className, ...props }: React.ComponentProps<"svg">) {
  return (
    <LoaderCircleIcon data-slot="spinner" role="status" aria-label="Loading" className={cn("size-4 motion-safe:animate-spin", className)} {...props} />
  )
}

export { Spinner }
