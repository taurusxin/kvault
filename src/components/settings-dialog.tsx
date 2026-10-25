import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { useTheme } from "@/lib/theme";
import { toast } from "sonner";
import {
  Check,
  Cloud,
  Download,
  HardDrive,
  KeyRound,
  LoaderCircle,
  LockKeyhole,
  Monitor,
  Moon,
  ShieldCheck,
  Sun,
  Upload,
} from "lucide-react";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Switch } from "@/components/ui/switch";
import { Tabs, TabsContent, TabsList, TabsTrigger } from "@/components/ui/tabs";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { SyncForm } from "@/components/sync-form";
import { useVault } from "@/stores/vault";
import { emptyS3, errorMessage, type Snapshot } from "@/lib/types";

export type SettingsTab = "general" | "cloud" | "security";

export function SettingsDialog({
  open,
  onOpenChange,
  initialTab,
}: {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  initialTab?: SettingsTab;
}) {
  const snapshot = useVault((s) => s.snapshot);
  const mutate = useVault((s) => s.mutate);
  const { theme, setTheme } = useTheme();
  const [config, setConfig] = useState(emptyS3);
  const [working, setWorking] = useState(false);
  const [tested, setTested] = useState(false);
  const [security, setSecurity] = useState<
    "change" | "migrate" | "import" | null
  >(null);
  const [oldPassword, setOldPassword] = useState("");
  const [password, setPassword] = useState("");
  const [confirm, setConfirm] = useState("");
  const [replace, setReplace] = useState(false);
  const [error, setError] = useState("");
  useEffect(() => {
    if (open) setConfig(snapshot?.config ?? emptyS3());
    else {
      setConfig(emptyS3());
      setSecurity(null);
      setOldPassword("");
      setPassword("");
      setConfirm("");
      setError("");
      setTested(false);
    }
    // Read only when opening, so background sync doesn't replace unsaved form inputs.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [open]);
  useEffect(() => {
    setOldPassword("");
    setPassword("");
    setConfirm("");
    setError("");
    setReplace(false);
  }, [security]);
  if (!snapshot) return null;
  async function run(task: () => Promise<unknown>, message?: string) {
    setWorking(true);
    setError("");
    try {
      await task();
      if (message) toast.success(message);
    } catch (e) {
      const message = errorMessage(e);
      setError(message);
      toast.error(message);
    } finally {
      setWorking(false);
    }
  }
  async function securitySubmit(event: React.FormEvent) {
    event.preventDefault();
    if (
      security === "change" &&
      (password.length < 8 || password !== confirm)
    ) {
      setError("新密码至少 8 个字符，两次输入需要一致。");
      return;
    }
    const submitted = password;
    const old = oldPassword;
    setPassword("");
    setOldPassword("");
    setConfirm("");
    const lifecycle = useVault.getState().lifecycle;
    await run(async () => {
      const next =
        security === "change"
          ? await invoke<Snapshot>("change_password", {
              sessionId: snapshot!.sessionId,
              oldPassword: old,
              newPassword: submitted,
            })
          : security === "migrate"
            ? await invoke<Snapshot>("migrate_password", {
                sessionId: snapshot!.sessionId,
                password: submitted,
              })
            : await invoke<Snapshot | null>("import_backup", {
                password: submitted,
                replace: true,
                sessionId: snapshot!.sessionId,
              });
      if (next && useVault.getState().lifecycle === lifecycle) {
        useVault.getState().accept(next, true);
        setSecurity(null);
        onOpenChange(false);
        toast.success(
          security === "import"
            ? "加密备份已导入，同步配置已清除"
            : "主密码已更新",
        );
      }
    });
  }
  return (
    <>
      <Dialog
        open={open}
        onOpenChange={(value) => {
          if (!working) onOpenChange(value);
        }}
      >
        <DialogContent className="max-h-[85vh] overflow-y-auto sm:max-w-[500px]">
          <DialogHeader>
            <DialogTitle>保险库设置</DialogTitle>
            <DialogDescription>
              管理外观、云同步与保险库安全。
            </DialogDescription>
          </DialogHeader>
          <Tabs
            key={initialTab}
            defaultValue={
              initialTab ?? (snapshot.recoveryPending ? "security" : "general")
            }
            className="mt-1"
          >
            <TabsList className="w-full">
              <TabsTrigger value="general" className="flex-1">
                通用
              </TabsTrigger>
              <TabsTrigger value="cloud" className="flex-1">
                云同步
              </TabsTrigger>
              <TabsTrigger value="security" className="flex-1">
                安全与备份
              </TabsTrigger>
            </TabsList>
            <TabsContent value="general" className="space-y-6 py-5">
              <div>
                <Label className="text-xs">外观</Label>
                <div className="mt-3 grid grid-cols-3 gap-2">
                  {(
                    [
                      { value: "light", label: "浅色", Icon: Sun },
                      { value: "dark", label: "深色", Icon: Moon },
                      { value: "system", label: "跟随系统", Icon: Monitor },
                    ] as const
                  ).map(({ value, label, Icon }) => (
                    <Button
                      key={value}
                      variant="outline"
                      className={`h-20 flex-col gap-2 text-xs ${theme === value ? "border-foreground bg-muted/40" : ""}`}
                      onClick={() => setTheme(value)}
                    >
                      <Icon className="size-5" strokeWidth={1.5} />
                      {label}
                    </Button>
                  ))}
                </div>
              </div>
              <div className="rounded-xl border bg-muted/20 p-4">
                <p className="flex items-center gap-2 text-xs font-medium">
                  <LockKeyhole className="size-4" />
                  自动锁定
                </p>
                <p className="mt-2 text-xs leading-6 text-muted-foreground">
                  无操作 5
                  分钟后锁定保险库，清除解锁内容。关闭应用后，下一次启动需要重新输入主密码。
                </p>
              </div>
              <div className="space-y-3 text-xs">
                <p className="font-medium">快捷键</p>
                <p className="flex justify-between text-muted-foreground">
                  <span>保存当前记录</span>
                  <kbd className="rounded border bg-muted px-1.5 py-0.5 text-[10px]">
                    {useVault.getState().platform === "macos" ? "⌘" : "Ctrl"} +
                    S
                  </kbd>
                </p>
                <p className="flex justify-between text-muted-foreground">
                  <span>搜索记录</span>
                  <kbd className="rounded border bg-muted px-1.5 py-0.5 text-[10px]">
                    {useVault.getState().platform === "macos" ? "⌘" : "Ctrl"} +
                    K
                  </kbd>
                </p>
                <p className="flex justify-between text-muted-foreground">
                  <span>锁定保险库</span>
                  <kbd className="rounded border bg-muted px-1.5 py-0.5 text-[10px]">
                    {useVault.getState().platform === "macos" ? "⌘" : "Ctrl"} +
                    Shift + L
                  </kbd>
                </p>
              </div>
              <div className="flex items-center justify-between border-t pt-4 text-[10px] text-muted-foreground">
                <span className="flex items-center gap-1.5">
                  <ShieldCheck className="size-3.5" />
                  kvault 0.1.0
                </span>
                <span>本地优先 · 端到端加密</span>
              </div>
            </TabsContent>
            <TabsContent value="cloud" className="space-y-5 py-5">
              <SyncForm
                config={config}
                saved={Boolean(
                  snapshot.config && snapshot.config.type === config.type,
                )}
                disabled={working}
                onChange={(value) => {
                  setConfig(value);
                  setTested(false);
                  setError("");
                }}
              />
              {error && !security && (
                <p role="alert" className="text-xs leading-5 text-destructive">
                  {error}
                </p>
              )}
              <div className="flex justify-end gap-2">
                <Button
                  variant="outline"
                  disabled={working || !config.endpoint}
                  onClick={() =>
                    void run(async () => {
                      await invoke("test_connection", {
                        config,
                        sessionId: snapshot.sessionId,
                      });
                      setTested(true);
                    }, "连接与条件写入验证通过")
                  }
                >
                  {working ? (
                    <LoaderCircle className="animate-spin" />
                  ) : tested ? (
                    <Check />
                  ) : (
                    <Cloud />
                  )}
                  测试连接
                </Button>
                <Button
                  disabled={
                    working ||
                    !config.endpoint ||
                    snapshot.recoveryPending ||
                    Boolean(snapshot.conflicts.length)
                  }
                  onClick={() =>
                    void run(async () => {
                      await mutate({ type: "setSync", config });
                      onOpenChange(false);
                    }, "同步配置已加密保存")
                  }
                >
                  保存并启用
                </Button>
              </div>
              {snapshot.config && (
                <div className="flex items-center justify-between border-t pt-4">
                  <p className="text-[11px] text-muted-foreground">
                    停用后保留本地数据与云端文件。
                  </p>
                  <Button
                    variant="ghost"
                    size="sm"
                    disabled={
                      working ||
                      snapshot.recoveryPending ||
                      Boolean(snapshot.conflicts.length)
                    }
                    onClick={() =>
                      void run(async () => {
                        await mutate({ type: "setSync", config: null });
                        onOpenChange(false);
                      }, "已停用云同步")
                    }
                  >
                    停用同步
                  </Button>
                </div>
              )}
            </TabsContent>
            <TabsContent value="security" className="space-y-5 py-5">
              {snapshot.recoveryPending && (
                <div className="rounded-xl border border-amber-500/25 bg-amber-500/8 p-4">
                  <p className="text-xs font-medium">密码变更待恢复</p>
                  <p className="mt-2 text-xs leading-6 text-muted-foreground">
                    新旧加密文件都已保留。请联网后锁定保险库，重新输入新密码；若云端未提交，使用旧密码可回滚。
                  </p>
                  <Button
                    variant="outline"
                    size="sm"
                    className="mt-3"
                    onClick={() => {
                      onOpenChange(false);
                      useVault.getState().clear();
                      void invoke("lock_vault");
                    }}
                  >
                    锁定并恢复
                  </Button>
                </div>
              )}
              <div className="flex items-center gap-3 rounded-xl border p-4">
                <KeyRound className="size-5 text-muted-foreground" />
                <div className="flex-1">
                  <p className="text-xs font-medium">主密码</p>
                  <p className="mt-1 text-[11px] text-muted-foreground">
                    一个密码，加密所有表和记录
                  </p>
                </div>
                <Button
                  variant="outline"
                  size="sm"
                  disabled={
                    snapshot.recoveryPending ||
                    Boolean(snapshot.conflicts.length)
                  }
                  onClick={() => setSecurity("change")}
                >
                  修改
                </Button>
              </div>
              {snapshot.config && !snapshot.recoveryPending && (
                <div className="flex items-center gap-3 rounded-xl border p-4">
                  <Cloud className="size-5 text-muted-foreground" />
                  <div className="flex-1">
                    <p className="text-xs font-medium">其他设备已修改密码？</p>
                    <p className="mt-1 text-[11px] text-muted-foreground">
                      输入新密码，保留并迁移本地修改
                    </p>
                  </div>
                  <Button
                    variant="outline"
                    size="sm"
                    onClick={() => setSecurity("migrate")}
                  >
                    迁移
                  </Button>
                </div>
              )}
              <div>
                <Label className="text-xs">加密备份</Label>
                <p className="mb-3 mt-2 text-[11px] leading-5 text-muted-foreground">
                  备份不包含云存储凭据。历史备份需要使用生成时的主密码打开。
                </p>
                <div className="grid grid-cols-2 gap-2">
                  <Button
                    variant="outline"
                    disabled={
                      working ||
                      snapshot.recoveryPending ||
                      Boolean(snapshot.conflicts.length)
                    }
                    onClick={() =>
                      void run(async () => {
                        if (
                          await invoke<boolean>("export_backup", {
                            sessionId: snapshot.sessionId,
                          })
                        )
                          toast.success("加密备份已导出");
                      })
                    }
                  >
                    <Download />
                    导出备份
                  </Button>
                  <Button
                    variant="outline"
                    disabled={
                      working ||
                      snapshot.recoveryPending ||
                      Boolean(snapshot.conflicts.length)
                    }
                    onClick={() => setSecurity("import")}
                  >
                    <Upload />
                    导入并替换
                  </Button>
                </div>
              </div>
              <div className="rounded-xl bg-muted/40 p-4 text-[11px] leading-6 text-muted-foreground">
                <p className="mb-1 flex items-center gap-1.5 font-medium text-foreground">
                  <HardDrive className="size-3.5" />
                  加密方式
                </p>
                Argon2id（64 MiB / 3 次迭代）派生密钥，XChaCha20-Poly1305
                加密内容。主密码不上传，也无法找回。
              </div>
            </TabsContent>
          </Tabs>
        </DialogContent>
      </Dialog>
      <Dialog
        open={Boolean(security)}
        onOpenChange={(value) => {
          if (!value && !working) setSecurity(null);
        }}
      >
        <DialogContent>
          <DialogHeader>
            <DialogTitle>
              {security === "change"
                ? "修改主密码"
                : security === "migrate"
                  ? "迁移到新主密码"
                  : "导入并替换保险库"}
            </DialogTitle>
            <DialogDescription>
              {security === "change"
                ? snapshot.config
                  ? "需要联网并先完成同步，所有记录将使用新密钥重新加密。"
                  : "所有本地记录将使用新密钥重新加密。"
                : security === "migrate"
                  ? "本地离线修改会与云端合并，出现冲突时保留双方。"
                  : "此操作将替换本地全部记录并清除同步配置，请先导出当前保险库。"}
            </DialogDescription>
          </DialogHeader>
          <form
            onSubmit={(event) => void securitySubmit(event)}
            className="space-y-4"
          >
            {security === "change" && (
              <div className="space-y-2">
                <Label htmlFor="old-password">当前主密码</Label>
                <Input
                  id="old-password"
                  type="password"
                  autoComplete="current-password"
                  required
                  disabled={working}
                  value={oldPassword}
                  onChange={(e) => setOldPassword(e.target.value)}
                />
              </div>
            )}
            <div className="space-y-2">
              <Label htmlFor="new-password">
                {security === "import" ? "备份的主密码" : "新主密码"}
              </Label>
              <Input
                id="new-password"
                type="password"
                autoComplete={
                  security === "change" ? "new-password" : "current-password"
                }
                required
                disabled={working}
                value={password}
                onChange={(e) => setPassword(e.target.value)}
              />
            </div>
            {security === "change" && (
              <div className="space-y-2">
                <Label htmlFor="new-password-confirm">确认新主密码</Label>
                <Input
                  id="new-password-confirm"
                  type="password"
                  autoComplete="new-password"
                  required
                  disabled={working}
                  value={confirm}
                  onChange={(e) => setConfirm(e.target.value)}
                />
              </div>
            )}
            {security === "import" && (
              <div className="flex items-center justify-between gap-3 rounded-lg bg-destructive/5 p-3">
                <Label htmlFor="confirm-replace" className="text-xs leading-5">
                  我已备份，确认替换当前保险库
                </Label>
                <Switch
                  id="confirm-replace"
                  checked={replace}
                  onCheckedChange={setReplace}
                  disabled={working}
                />
              </div>
            )}
            {error && (
              <p role="alert" className="text-xs leading-5 text-destructive">
                {error}
              </p>
            )}
            <DialogFooter>
              <Button
                type="button"
                variant="outline"
                disabled={working}
                onClick={() => setSecurity(null)}
              >
                取消
              </Button>
              <Button
                type="submit"
                disabled={
                  working || !password || (security === "import" && !replace)
                }
              >
                {working && <LoaderCircle className="animate-spin" />}
                {security === "import" ? "选择文件并替换" : "确认"}
              </Button>
            </DialogFooter>
          </form>
        </DialogContent>
      </Dialog>
    </>
  );
}
