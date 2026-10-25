import { useEffect, useState } from "react";
import { Eye, EyeOff, GitCompareArrows, LoaderCircle } from "lucide-react";
import { toast } from "sonner";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { errorMessage, type Entity } from "@/lib/types";
import { useVault } from "@/stores/vault";

const reasons: Record<string, string> = {
  entry: "同一条记录在双方被修改",
  table: "同一张表在双方被修改",
  duplicateKey: "合并后出现重复 Key",
  duplicateTable: "合并后出现同名表",
  tableDeletion: "一方删除了表，另一方修改了表内记录",
};
function Version({ title, entity }: { title: string; entity: Entity | null }) {
  const [visible, setVisible] = useState(false);
  return (
    <section className="min-w-0 rounded-xl border bg-muted/15 p-4">
      <h3 className="mb-4 text-xs font-semibold">{title}</h3>
      {!entity || entity.value.deleted ? (
        <p className="text-xs text-destructive">
          已删除{entity?.kind === "table" ? "此表及其记录" : "此记录"}
        </p>
      ) : entity.kind === "table" ? (
        <p className="break-all text-sm">{entity.value.name}</p>
      ) : (
        <div className="space-y-4">
          <div>
            <p className="mb-1 text-[10px] text-muted-foreground">KEY</p>
            <p className="mono break-all text-xs">{entity.value.key}</p>
          </div>
          <div>
            <div className="mb-1 flex items-center justify-between">
              <span className="text-[10px] text-muted-foreground">VALUE</span>
              <Button
                variant="ghost"
                size="icon-xs"
                aria-label={`${visible ? "隐藏" : "显示"}${title} Value`}
                onClick={() => setVisible(!visible)}
              >
                {visible ? <EyeOff /> : <Eye />}
              </Button>
            </div>
            <pre className="mono max-h-40 overflow-auto whitespace-pre-wrap break-all text-xs leading-6">
              {visible ? entity.value.value || "（空）" : "••••••••"}
            </pre>
          </div>
          <div>
            <p className="mb-1 text-[10px] text-muted-foreground">备注</p>
            <p className="max-h-28 overflow-auto whitespace-pre-wrap break-all text-xs leading-6 text-muted-foreground">
              {entity.value.notes || "（无）"}
            </p>
          </div>
        </div>
      )}
    </section>
  );
}
export function ConflictsDialog({
  open,
  onOpenChange,
}: {
  open: boolean;
  onOpenChange: (open: boolean) => void;
}) {
  const conflicts = useVault((s) => s.snapshot?.conflicts ?? []);
  const busy = useVault((s) => s.busy);
  const mutate = useVault((s) => s.mutate);
  const conflict = conflicts[0];
  const [rename, setRename] = useState("");
  useEffect(() => {
    setRename("");
  }, [conflict?.id]);
  async function resolve(choice: "local" | "remote" | "both") {
    if (!conflict) return;
    try {
      const snapshot = await mutate({
        type: "resolveConflict",
        id: conflict.id,
        choice,
        rename,
      });
      if (snapshot && !snapshot.conflicts.length) {
        onOpenChange(false);
        toast.success("所有冲突已解决，稍后自动同步");
      }
    } catch (e) {
      toast.error(errorMessage(e));
    }
  }
  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="max-h-[85vh] overflow-y-auto sm:max-w-[660px]">
        <DialogHeader>
          <DialogTitle className="flex items-center gap-2">
            <GitCompareArrows className="size-4" />
            处理同步冲突
          </DialogTitle>
          <DialogDescription>
            {conflict
              ? `${reasons[conflict.reason] ?? "双方内容存在差异"}。剩余 ${conflicts.length} 项。`
              : "所有冲突均已处理。"}
          </DialogDescription>
        </DialogHeader>
        {conflict && (
          <>
            <div className="grid grid-cols-2 gap-3" key={conflict.id}>
              <Version title="本地版本" entity={conflict.local} />
              <Version title="云端版本" entity={conflict.remote} />
            </div>
            <div className="flex gap-2">
              <Button
                className="flex-1"
                variant="outline"
                disabled={busy}
                onClick={() => void resolve("local")}
              >
                采用本地版本
              </Button>
              <Button
                className="flex-1"
                variant="outline"
                disabled={busy}
                onClick={() => void resolve("remote")}
              >
                采用云端版本
              </Button>
            </div>
            {conflict.local &&
              conflict.remote &&
              !conflict.local.value.deleted &&
              !conflict.remote.value.deleted && (
                <div className="space-y-3 rounded-xl bg-muted/40 p-4">
                  <Label htmlFor="conflict-rename" className="text-xs">
                    或保留双方，为云端副本设置新的
                    {conflict.remote.kind === "table" ? "表名" : " Key"}
                  </Label>
                  <div className="flex gap-2">
                    <Input
                      id="conflict-rename"
                      value={rename}
                      onChange={(e) => setRename(e.target.value)}
                      placeholder={
                        conflict.remote.kind === "table"
                          ? "新的表名"
                          : "新的唯一 Key"
                      }
                      disabled={busy}
                    />
                    <Button
                      disabled={busy || !rename.trim()}
                      onClick={() => void resolve("both")}
                    >
                      {busy && <LoaderCircle className="animate-spin" />}
                      保留双方
                    </Button>
                  </div>
                </div>
              )}
          </>
        )}
      </DialogContent>
    </Dialog>
  );
}
