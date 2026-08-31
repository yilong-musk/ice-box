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
  return (
    <AlertDialog open={open} onOpenChange={onOpenChange}>
      <AlertDialogContent>
        <AlertDialogHeader>
          <AlertDialogTitle>启用 TUN 需要先安装辅助组件</AlertDialogTitle>
          <AlertDialogDescription>
            辅助组件以系统权限运行 TUN 内核，当前尚未安装或未授权。点击「安装并启用」将弹出系统授权密码框，安装成功后再保存并启用 TUN 设置；取消则不启用。
          </AlertDialogDescription>
        </AlertDialogHeader>
        <AlertDialogFooter>
          <AlertDialogCancel asChild>
            <Button type="button" size="sm" variant="outline" disabled={busy}>
              取消
            </Button>
          </AlertDialogCancel>
          <AlertDialogAction asChild>
            <Button type="button" size="sm" disabled={busy} onClick={onConfirm}>
              安装并启用
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