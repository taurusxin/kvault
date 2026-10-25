import { Suspense, lazy, useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { ThemeProvider } from "@/lib/theme";
import { toast } from "sonner";
import { LoaderCircle } from "lucide-react";
import { Toaster } from "@/components/ui/sonner";
import { TooltipProvider } from "@/components/ui/tooltip";
import { Button } from "@/components/ui/button";
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
import { WindowBar } from "@/components/window-bar";
import { Onboarding } from "@/components/onboarding";
import { desktop, useVault } from "@/stores/vault";
import { errorMessage, type Snapshot, type SyncStatus } from "@/lib/types";
import "./App.css";

const Workspace = lazy(() =>
  import("@/components/workspace").then((module) => ({
    default: module.Workspace,
  })),
);

function App() {
  const initialized = useVault((s) => s.initialized);
  const snapshot = useVault((s) => s.snapshot);
  const pending = useVault((s) => s.pendingNavigation);
  const busy = useVault((s) => s.busy);
  const orderDirty = useVault((s) => s.orderDirty);
  const [loadError, setLoadError] = useState("");
  useEffect(() => {
    void useVault
      .getState()
      .initialize()
      .catch((e) => setLoadError(errorMessage(e)));
    if (!desktop) return;
    let active = true;
    const cleanups: (() => void)[] = [];
    const add = (promise: Promise<() => void>) => {
      void promise.then((fn) => {
        if (active) cleanups.push(fn);
        else fn();
      });
    };
    add(
      listen<Snapshot>("vault:changed", (e) =>
        useVault.getState().accept(e.payload),
      ),
    );
    add(
      listen("vault:locked", () => {
        useVault.getState().clear();
        toast.dismiss();
      }),
    );
    add(
      listen<SyncStatus>("sync:status", (e) =>
        useVault.getState().setSync(e.payload),
      ),
    );
    let closing = false;
    add(
      getCurrentWindow().onCloseRequested((event) => {
        if (
          !closing &&
          (useVault.getState().dirty || useVault.getState().orderDirty)
        ) {
          event.preventDefault();
          useVault.getState().navigate(() => {
            closing = true;
            void getCurrentWindow().close();
          });
        }
      }),
    );
    let lastTouch = 0;
    const touch = () => {
      const state = useVault.getState();
      if (!state.snapshot || Date.now() - lastTouch < 2000) return;
      lastTouch = Date.now();
      void invoke("touch_vault", { sessionId: state.snapshot.sessionId }).catch(
        () => {},
      );
    };
    const focus = () => {
      const s = useVault.getState().snapshot;
      if (s?.config && !s.recoveryPending)
        void invoke("sync_now", { sessionId: s.sessionId }).catch(() => {});
    };
    document.addEventListener("pointerdown", touch);
    document.addEventListener("pointermove", touch);
    document.addEventListener("keydown", touch);
    window.addEventListener("focus", focus);
    return () => {
      active = false;
      cleanups.forEach((fn) => fn());
      document.removeEventListener("pointerdown", touch);
      document.removeEventListener("pointermove", touch);
      document.removeEventListener("keydown", touch);
      window.removeEventListener("focus", focus);
    };
  }, []);
  useEffect(() => {
    const shortcut = (event: KeyboardEvent) => {
      const store = useVault.getState();
      if (!store.snapshot || !(event.ctrlKey || event.metaKey)) return;
      if (event.key.toLowerCase() === "s") {
        event.preventDefault();
        if ((store.dirty || store.orderDraft) && !store.busy)
          void store.save().catch((e) => toast.error(errorMessage(e)));
      }
      if (event.key.toLowerCase() === "k") {
        event.preventDefault();
        document
          .querySelector<HTMLInputElement>('input[aria-label="搜索记录"]')
          ?.focus();
      }
      if (event.shiftKey && event.key.toLowerCase() === "l") {
        event.preventDefault();
        store.navigate(() => {
          store.clear();
          void invoke("lock_vault").catch((e) => toast.error(errorMessage(e)));
        });
      }
    };
    document.addEventListener("keydown", shortcut);
    return () => document.removeEventListener("keydown", shortcut);
  }, []);
  return (
    <ThemeProvider>
      <TooltipProvider>
        <div className="flex h-full flex-col">
          <WindowBar />
          {loadError ? (
            <main className="flex flex-1 flex-col items-center justify-center gap-4">
              <p className="text-sm text-destructive">{loadError}</p>
              <Button
                variant="outline"
                onClick={() => window.location.reload()}
              >
                重试
              </Button>
            </main>
          ) : !initialized ? (
            <div className="flex flex-1 items-center justify-center">
              <LoaderCircle className="size-5 animate-spin text-muted-foreground" />
            </div>
          ) : snapshot ? (
            <Suspense
              fallback={
                <div className="flex flex-1 items-center justify-center">
                  <LoaderCircle className="size-5 animate-spin text-muted-foreground" />
                </div>
              }
            >
              <Workspace key={useVault.getState().lifecycle} />
            </Suspense>
          ) : (
            <Onboarding />
          )}
        </div>
        <AlertDialog
          open={Boolean(pending)}
          onOpenChange={(open) => {
            if (!open) useVault.setState({ pendingNavigation: null });
          }}
        >
          <AlertDialogContent>
            <AlertDialogHeader>
              <AlertDialogTitle>
                {orderDirty ? "有尚未保存的排序" : "有尚未保存的修改"}
              </AlertDialogTitle>
              <AlertDialogDescription>
                {orderDirty
                  ? "当前排序尚未保存。你可以先加密保存，或放弃排序后继续操作。"
                  : "离开后，这些修改将被丢弃。你可以先加密保存，再继续操作。"}
              </AlertDialogDescription>
            </AlertDialogHeader>
            <AlertDialogFooter>
              <AlertDialogCancel>继续编辑</AlertDialogCancel>
              <Button
                variant="outline"
                disabled={busy}
                onClick={() => useVault.getState().discardAndNavigate()}
              >
                {orderDirty ? "放弃排序" : "放弃修改"}
              </Button>
              <AlertDialogAction
                disabled={busy}
                onClick={(event) => {
                  event.preventDefault();
                  void useVault
                    .getState()
                    .save()
                    .then(() => {
                      if (
                        !useVault.getState().dirty &&
                        !useVault.getState().orderDirty
                      )
                        useVault.getState().discardAndNavigate();
                    })
                    .catch((e) => toast.error(errorMessage(e)));
                }}
              >
                保存并继续
              </AlertDialogAction>
            </AlertDialogFooter>
          </AlertDialogContent>
        </AlertDialog>
        <Toaster position="bottom-right" closeButton richColors />
      </TooltipProvider>
    </ThemeProvider>
  );
}

export default App;
