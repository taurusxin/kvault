import { useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { LogOut, PanelBottomClose } from "lucide-react";
import { toast } from "sonner";
import { desktop, useVault } from "@/stores/vault";
import { errorMessage } from "@/lib/types";
import { Button } from "@/components/ui/button";
import { Label } from "@/components/ui/label";
import { Switch } from "@/components/ui/switch";
import {
  AlertDialog,
  AlertDialogCancel,
  AlertDialogContent,
  AlertDialogDescription,
  AlertDialogFooter,
  AlertDialogHeader,
  AlertDialogTitle,
} from "@/components/ui/alert-dialog";

function quit() {
  useVault.getState().navigate(() => {
    void invoke("exit_app")
      .then(() => useVault.getState().clear())
      .catch((error) => toast.error(errorMessage(error)));
  });
}

export function CloseDialog() {
  const [open, setOpen] = useState(false);
  const [askAgain, setAskAgain] = useState(true);
  const [working, setWorking] = useState(false);
  const handling = useRef(false);
  useEffect(() => {
    if (!desktop) return;
    let active = true;
    const cleanups: (() => void)[] = [];
    const add = (promise: Promise<() => void>) => {
      void promise.then((cleanup) =>
        active ? cleanups.push(cleanup) : cleanup(),
      );
    };
    add(
      listen("app:close-requested", () => {
        const state = useVault.getState();
        if (handling.current || state.pendingNavigation) return;
        if (state.closeSettings.askBeforeClose) {
          handling.current = true;
          setAskAgain(true);
          setOpen(true);
        } else if (state.closeSettings.minimizeToTray) {
          handling.current = true;
          void invoke("minimize_to_tray")
            .catch((error) => toast.error(errorMessage(error)))
            .finally(() => {
              handling.current = false;
            });
        } else {
          quit();
        }
      }),
    );
    add(
      listen("app:exit-requested", () => {
        if (handling.current || useVault.getState().pendingNavigation) return;
        // An explicit tray-menu or system Quit always exits, with a draft guard.
        quit();
      }),
    );
    return () => {
      active = false;
      cleanups.forEach((cleanup) => cleanup());
    };
  }, []);

  async function choose(minimize: boolean) {
    setWorking(true);
    try {
      await useVault.getState().setCloseSettings({
        minimizeToTray: minimize,
        askBeforeClose: askAgain,
      });
      if (minimize) await invoke("minimize_to_tray");
      setOpen(false);
      if (!minimize) quit();
      handling.current = false;
    } catch (error) {
      toast.error(errorMessage(error));
    } finally {
      setWorking(false);
    }
  }

  return (
    <AlertDialog
      open={open}
      onOpenChange={(value) => {
        if (working) return;
        setOpen(value);
        if (!value) handling.current = false;
      }}
    >
      <AlertDialogContent>
        <AlertDialogHeader>
          <AlertDialogTitle>关闭 KVault</AlertDialogTitle>
          <AlertDialogDescription>
            选择退出应用，或最小化到托盘并继续在后台运行。可通过托盘图标重新打开窗口。
          </AlertDialogDescription>
        </AlertDialogHeader>
        <div className="flex items-center justify-between gap-4 rounded-lg border p-3">
          <Label htmlFor="close-ask-again" className="text-xs">
            下次关闭时仍然询问
          </Label>
          <Switch
            id="close-ask-again"
            checked={askAgain}
            disabled={working}
            onCheckedChange={setAskAgain}
          />
        </div>
        <p className="text-xs text-muted-foreground">
          关闭询问后将记住本次选择，也可在设置 → 通用中修改。
        </p>
        <AlertDialogFooter>
          <AlertDialogCancel disabled={working}>取消</AlertDialogCancel>
          <Button
            variant="outline"
            disabled={working}
            onClick={() => void choose(false)}
          >
            <LogOut />
            退出应用
          </Button>
          <Button disabled={working} onClick={() => void choose(true)}>
            <PanelBottomClose />
            最小化到托盘
          </Button>
        </AlertDialogFooter>
      </AlertDialogContent>
    </AlertDialog>
  );
}
