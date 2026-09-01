import { useCallback, useState } from "react";
import {
  AlertDialog,
  AlertDialogAction,
  AlertDialogCancel,
  AlertDialogContent,
  AlertDialogDescription,
  AlertDialogFooter,
  AlertDialogHeader,
  AlertDialogTitle,
} from "@/components/ui/alert-dialog";
import { Button } from "@/components/ui/button";
import { t, useLanguagePreference } from "@/lib/i18n";

/** Guided "install the privileged helper, then enable TUN" confirmation
 * dialog shared by the Home and Settings pages. `onConfirm` is expected to
 * install the helper and then persist `tun.enabled = true`; success is only
 * claimed after that persistence resolves (the caller reports busy/errors). */
function TunInstallDialog({
  open,
  onOpenChange,
  onConfirm,
  busy = false,
}: {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  onConfirm: () => void;
  busy?: boolean;
}) {
  useLanguagePreference();
  return (
    <AlertDialog open={open} onOpenChange={onOpenChange}>
      <AlertDialogContent>
        <AlertDialogHeader>
          <AlertDialogTitle>{t("tunDialog.title")}</AlertDialogTitle>
          <AlertDialogDescription>{t("tunDialog.desc")}</AlertDialogDescription>
        </AlertDialogHeader>
        <AlertDialogFooter>
          <AlertDialogCancel asChild>
            <Button type="button" size="sm" variant="outline" disabled={busy}>
              {t("common.cancel")}
            </Button>
          </AlertDialogCancel>
          <AlertDialogAction asChild>
            <Button type="button" size="sm" disabled={busy} onClick={onConfirm}>
              {t("tunDialog.installAndEnable")}
            </Button>
          </AlertDialogAction>
        </AlertDialogFooter>
      </AlertDialogContent>
    </AlertDialog>
  );
}

/** Owns the dialog open state and the "confirm closes the dialog, then runs
 * the install-and-enable flow" glue shared by Home and Settings, so the two
 * pages cannot drift apart (e.g. one forgetting to close the dialog). */
function useTunInstallDialog(onInstallThenEnable: () => void) {
  const [open, setOpen] = useState(false);
  const confirm = useCallback(() => {
    setOpen(false);
    onInstallThenEnable();
  }, [onInstallThenEnable]);
  return { open, setOpen, confirm };
}

export { TunInstallDialog, useTunInstallDialog };