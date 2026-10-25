import { useEffect, useState } from "react";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { Minus, Square, Copy, X, LockKeyhole, PanelLeft } from "lucide-react";
import { Button } from "@/components/ui/button";
import { desktop, useVault } from "@/stores/vault";

export function WindowBar() {
  const platform = useVault((s) => s.platform);
  const unlocked = useVault((s) => Boolean(s.snapshot));
  const [maximized, setMaximized] = useState(false);
  const mac =
    platform === "macos" || (!platform && /Mac/.test(navigator.userAgent));
  useEffect(() => {
    if (!desktop) return;
    const win = getCurrentWindow();
    let active = true;
    let unlisten: (() => void) | undefined;
    void win.isMaximized().then((value) => {
      if (active) setMaximized(value);
    });
    void win
      .onResized(() => {
        void win.isMaximized().then((value) => {
          if (active) setMaximized(value);
        });
      })
      .then((fn) => {
        if (!active) fn();
        else unlisten = fn;
      });
    return () => {
      active = false;
      unlisten?.();
    };
  }, []);
  return (
    <header
      className={`flex shrink-0 select-none items-center border-b bg-sidebar/70 ${mac ? "h-8" : "h-12"}`}
      data-tauri-drag-region
    >
      <div
        className={`flex h-full w-56 shrink-0 items-center gap-2.5 px-5 ${mac ? "pl-[88px]" : ""}`}
        data-tauri-drag-region
      >
        {!mac && (
          <div className="flex size-6 items-center justify-center rounded-lg bg-foreground text-background">
            <LockKeyhole className="size-3.5" />
          </div>
        )}
        <span
          className={`tracking-tight ${mac ? "text-[13px] font-medium" : "text-sm font-semibold"}`}
          data-tauri-drag-region
        >
          KVault
        </span>
        {unlocked && !mac && (
          <PanelLeft className="ml-auto size-3.5 text-muted-foreground/60" />
        )}
      </div>
      <div
        className="flex flex-1 items-center justify-center gap-1.5 pr-12 text-[11px] text-muted-foreground"
        data-tauri-drag-region
      >
        <LockKeyhole className="size-3" />
        <span data-tauri-drag-region>
          {unlocked ? "端到端加密 · 私人保险库" : "你的记录，只属于你"}
        </span>
      </div>
      {!mac && desktop && (
        <div className="flex h-full items-center">
          <Button
            aria-label="最小化"
            variant="ghost"
            className="h-full w-11 rounded-none"
            onClick={() => void getCurrentWindow().minimize()}
          >
            <Minus className="size-3.5" />
          </Button>
          <Button
            aria-label={maximized ? "还原窗口" : "最大化"}
            variant="ghost"
            className="h-full w-11 rounded-none"
            onClick={() => void getCurrentWindow().toggleMaximize()}
          >
            {maximized ? (
              <Copy className="size-3" />
            ) : (
              <Square className="size-3" />
            )}
          </Button>
          <Button
            aria-label="关闭窗口"
            variant="ghost"
            className="h-full w-11 rounded-none hover:bg-red-500 hover:text-white"
            onClick={() => void getCurrentWindow().close()}
          >
            <X className="size-4" />
          </Button>
        </div>
      )}
    </header>
  );
}
