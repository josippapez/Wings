import type * as React from "react";
import { Toaster as Sonner, toast, type ToasterProps, useSonner } from "sonner";
import { Button } from "@/components/ui/button";

// shadcn's sonner component, without next-themes: Wings is always dark.
const Toaster = ({ ...props }: ToasterProps) => {
  const { toasts } = useSonner();
  return (
    <>
      <Sonner
        theme="dark"
        className="toaster group"
        closeButton
        offset={48}
        style={
          {
            "--normal-bg": "var(--popover)",
            "--normal-text": "var(--popover-foreground)",
            "--normal-border": "var(--border)",
            "--border-radius": "var(--radius)",
          } as React.CSSProperties
        }
        {...props}
      />
      {toasts.length >= 2 && (
        // Sits in the 48px gutter under the stack, so it never covers a toast.
        <Button
          variant="ghost"
          size="xs"
          className="fixed right-8 bottom-2"
          onClick={() => toast.dismiss()}
        >
          Clear all
        </Button>
      )}
    </>
  );
};

export { Toaster };
