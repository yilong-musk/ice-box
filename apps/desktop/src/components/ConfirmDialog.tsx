import { AlertDialog as AlertDialogPrimitive } from "radix-ui";

import { Button } from "@/components/ui/button";
import { cn } from "@/lib/utils";

type ConfirmDialogProps = {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  title: string;
  description?: string;
  confirmLabel?: string;
  cancelLabel?: string;
  busy?: boolean;
  onConfirm: () => void;
};

/**
 * In-app confirmation dialog (radix AlertDialog). Replaces `window.confirm`,
 * which is a silent no-op on the macOS WKWebView (Tauri v2).
 */
function ConfirmDialog({
  className,
  title,
  description,
  confirmLabel = "确认",
  cancelLabel = "取消",
  busy = false,
  onConfirm,
  ...props
}: ConfirmDialogProps & { className?: string }) {
  return (
    <AlertDialogPrimitive.Root
      open={props.open}
      onOpenChange={props.onOpenChange}
    >
      <AlertDialogPrimitive.Portal>
        <AlertDialogPrimitive.Overlay
          data-slot="alert-dialog-overlay"
          className="fixed inset-0 z-50 bg-black/40 data-open:animate-in data-open:fade-in-0 data-closed:animate-out data-closed:fade-out-0"
        />
        <AlertDialogPrimitive.Content
          data-slot="alert-dialog-content"
          className={cn(
            "fixed left-1/2 top-1/2 z-50 grid w-full max-w-sm -translate-x-1/2 -translate-y-1/2 gap-4 border border-border bg-background p-5 shadow-lg outline-none data-open:animate-in data-open:fade-in-0 data-open:zoom-in-95 data-closed:animate-out data-closed:fade-out-0 data-closed:zoom-out-95",
            className,
          )}
        >
          <div className="flex flex-col gap-1.5">
            <AlertDialogPrimitive.Title
              data-slot="alert-dialog-title"
              className="font-heading text-sm font-medium"
            >
              {title}
            </AlertDialogPrimitive.Title>
            {description ? (
              <AlertDialogPrimitive.Description
                data-slot="alert-dialog-description"
                className="break-all text-sm text-muted-foreground"
              >
                {description}
              </AlertDialogPrimitive.Description>
            ) : null}
          </div>
          <div className="flex flex-col-reverse justify-end gap-2 sm:flex-row sm:justify-end">
            <Button
              type="button"
              size="sm"
              variant="outline"
              disabled={busy}
              onClick={() => props.onOpenChange(false)}
            >
              {cancelLabel}
            </Button>
            <Button
              type="button"
              size="sm"
              variant="destructive"
              disabled={busy}
              onClick={onConfirm}
            >
              {confirmLabel}
            </Button>
          </div>
        </AlertDialogPrimitive.Content>
      </AlertDialogPrimitive.Portal>
    </AlertDialogPrimitive.Root>
  );
}

export { ConfirmDialog };
