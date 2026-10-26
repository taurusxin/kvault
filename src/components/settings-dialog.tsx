import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { useTheme } from "@/lib/theme";
import { toast } from "sonner";
import {
  Check,
  Cloud,
  Download,
  HardDrive,
  Fingerprint,
  KeyRound,
  LoaderCircle,
  LockKeyhole,
  Monitor,
  Moon,
  ShieldCheck,
  Settings2,
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
  const closeSettings = useVault((s) => s.closeSettings);
  const setCloseSettings = useVault((s) => s.setCloseSettings);
  const { theme, setTheme } = useTheme();
  const [config, setConfig] = useState(emptyS3);
  const [working, setWorking] = useState(false);
  const [tested, setTested] = useState(false);
  const [security, setSecurity] = useState<
    "change" | "migrate" | "import" | "system" | null
  >(null);
  const [oldPassword, setOldPassword] = useState("");
  const [password, setPassword] = useState("");
  const [confirm, setConfirm] = useState("");
  const [replace, setReplace] = useState(false);
  const [error, setError] = useState("");
  const [autoLockEnabled, setAutoLockEnabled] = useState(true);
  const [autoLockMinutes, setAutoLockMinutes] = useState("5");
  useEffect(() => {
    if (open) {
      setConfig(snapshot?.config ?? emptyS3());
      setAutoLockEnabled(snapshot?.autoLock.enabled ?? true);
      setAutoLockMinutes(String(snapshot?.autoLock.timeoutMinutes ?? 5));
    } else {
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
  const minutes = Number(autoLockMinutes);
  const validMinutes =
    Number.isInteger(minutes) && minutes >= 1 && minutes <= 1440;
  const autoLock = {
    enabled: autoLockEnabled,
    timeoutMinutes: validMinutes ? minutes : snapshot.autoLock.timeoutMinutes,
  };
  const autoLockChanged =
    autoLock.enabled !== snapshot.autoLock.enabled ||
    autoLock.timeoutMinutes !== snapshot.autoLock.timeoutMinutes;
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
        security === "system"
          ? await invoke<Snapshot>("set_system_unlock", {
              sessionId: snapshot!.sessionId,
              enabled: true,
              password: submitted,
            })
          : security === "change"
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
        useVault.getState().accept(next, security !== "system");
        setSecurity(null);
        if (security !== "system") onOpenChange(false);
        toast.success(
          security === "import"
            ? "加密备份已导入，同步配置已清除"
            : security === "system"
              ? "已启用本机系统解锁，主密码仍可使用"
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
        <DialogContent className="flex h-[min(640px,85vh)] flex-col gap-0 overflow-hidden p-0 sm:max-w-[760px]">
          <DialogHeader className="shrink-0 border-b px-6 py-5">
            <DialogTitle>保险库设置</DialogTitle>
            <DialogDescription className="text-xs">
              管理外观、云同步与保险库安全。
            </DialogDescription>
          </DialogHeader>
          <Tabs
            key={initialTab}
            orientation="vertical"
            defaultValue={
              initialTab ?? (snapshot.recoveryPending ? "security" : "general")
            }
            onValueChange={() => setError("")}
            className="min-h-0 flex-1 gap-0"
          >
            <TabsList
              aria-label="设置分类"
              className="w-44 shrink-0 items-stretch justify-start gap-1 rounded-none border-r bg-muted/20 px-3 py-4 group-data-vertical/tabs:h-full"
            >
              <TabsTrigger
                value="general"
                className="h-10 flex-none gap-2 px-3 text-xs"
              >
                <Settings2 />
                通用
              </TabsTrigger>
              <TabsTrigger
                value="cloud"
                className="h-10 flex-none gap-2 px-3 text-xs"
              >
                <Cloud />
                云同步
              </TabsTrigger>
              <TabsTrigger
                value="security"
                className="h-10 flex-none gap-2 px-3 text-xs"
              >
                <ShieldCheck />
                安全与备份
              </TabsTrigger>
            </TabsList>
            <TabsContent
              value="general"
              className="m-0 min-h-0 min-w-0 space-y-6 overflow-y-auto p-6"
            >
              <div>
                <h2 className="text-sm font-medium">通用</h2>
                <p className="mt-1 text-xs text-muted-foreground">
                  外观、自动锁定与常用快捷键。
                </p>
              </div>
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
                      variant={theme === value ? "default" : "outline"}
                      aria-pressed={theme === value}
                      className="relative h-20 flex-col gap-2 text-xs"
                      onClick={() => setTheme(value)}
                    >
                      <Icon className="size-5" strokeWidth={1.5} />
                      {label}
                      {theme === value && (
                        <Check
                          className="absolute right-2 top-2 size-3.5"
                          aria-hidden="true"
                        />
                      )}
                    </Button>
                  ))}
                </div>
              </div>
              <div className="space-y-4 rounded-xl border p-4">
                <p className="text-xs font-medium">窗口关闭行为</p>
                <div className="flex items-start justify-between gap-4">
                  <div>
                    <Label htmlFor="close-minimize" className="text-xs">
                      关闭时最小化到托盘
                    </Label>
                    <p className="mt-2 text-[11px] leading-5 text-muted-foreground">
                      开启后继续在后台运行；关闭后退出应用。托盘菜单中的退出始终退出应用。
                    </p>
                  </div>
                  <Switch
                    id="close-minimize"
                    checked={closeSettings.minimizeToTray}
                    disabled={working}
                    onCheckedChange={(value) =>
                      void run(() =>
                        setCloseSettings({
                          ...closeSettings,
                          minimizeToTray: value,
                        }),
                      )
                    }
                  />
                </div>
                <div className="flex items-start justify-between gap-4">
                  <div>
                    <Label htmlFor="close-ask" className="text-xs">
                      关闭时再次询问
                    </Label>
                    <p className="mt-2 text-[11px] leading-5 text-muted-foreground">
                      每次关闭时选择退出或最小化；关闭询问后直接使用上方设置。更改后立即保存。
                    </p>
                  </div>
                  <Switch
                    id="close-ask"
                    checked={closeSettings.askBeforeClose}
                    disabled={working}
                    onCheckedChange={(value) =>
                      void run(() =>
                        setCloseSettings({
                          ...closeSettings,
                          askBeforeClose: value,
                        }),
                      )
                    }
                  />
                </div>
              </div>
              <form
                className="space-y-4 rounded-xl border p-4"
                onSubmit={(event) => {
                  event.preventDefault();
                  if (!autoLockChanged || (autoLockEnabled && !validMinutes))
                    return;
                  void run(async () => {
                    const saved = await mutate({
                      type: "setAutoLock",
                      settings: autoLock,
                    });
                    if (saved) toast.success("自动锁定设置已保存");
                  });
                }}
              >
                <div className="flex items-start justify-between gap-4">
                  <div>
                    <Label
                      htmlFor="auto-lock-enabled"
                      className="gap-2 text-xs font-medium"
                    >
                      <LockKeyhole className="size-4" />
                      自动锁定
                    </Label>
                    <p
                      id="auto-lock-description"
                      className="mt-2 text-[11px] leading-5 text-muted-foreground"
                    >
                      闲置一段时间后锁定保险库，清除解锁内容。
                    </p>
                  </div>
                  <Switch
                    id="auto-lock-enabled"
                    aria-describedby="auto-lock-description"
                    checked={autoLockEnabled}
                    disabled={working || snapshot.recoveryPending}
                    onCheckedChange={setAutoLockEnabled}
                  />
                </div>
                <div className="space-y-2">
                  <Label htmlFor="auto-lock-minutes" className="text-xs">
                    闲置时长
                  </Label>
                  <div className="flex items-center gap-2">
                    <Input
                      id="auto-lock-minutes"
                      type="number"
                      min={1}
                      max={1440}
                      step={1}
                      className="w-24 text-xs"
                      value={autoLockMinutes}
                      disabled={
                        !autoLockEnabled || working || snapshot.recoveryPending
                      }
                      aria-invalid={autoLockEnabled && !validMinutes}
                      aria-describedby="auto-lock-hint"
                      onChange={(event) =>
                        setAutoLockMinutes(event.target.value)
                      }
                    />
                    <span className="text-xs text-muted-foreground">分钟</span>
                  </div>
                  <p
                    id="auto-lock-hint"
                    className="text-[11px] leading-5 text-muted-foreground"
                  >
                    {autoLockEnabled
                      ? "支持 1–1440 分钟；自动锁定会丢弃未保存的编辑。"
                      : autoLockChanged
                        ? "已选择关闭自动锁定，保存后生效。仍可手动锁定保险库。"
                        : "自动锁定已关闭。仍可手动锁定保险库。"}
                  </p>
                  {autoLockEnabled && !validMinutes && (
                    <p role="alert" className="text-[11px] text-destructive">
                      请输入 1–1440 之间的整数。
                    </p>
                  )}
                </div>
                <div className="flex items-center justify-between gap-3 border-t pt-3">
                  <p className="text-[10px] leading-5 text-muted-foreground">
                    仅在本机生效，关闭应用后仍需重新解锁。
                  </p>
                  <Button
                    type="submit"
                    size="sm"
                    disabled={
                      working ||
                      snapshot.recoveryPending ||
                      !autoLockChanged ||
                      (autoLockEnabled && !validMinutes)
                    }
                  >
                    {working && <LoaderCircle className="animate-spin" />}
                    保存设置
                  </Button>
                </div>
                {error && !security && (
                  <p
                    role="alert"
                    className="text-xs leading-5 text-destructive"
                  >
                    {error}
                  </p>
                )}
              </form>
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
                  KVault 0.1.0
                </span>
                <span>本地优先 · 端到端加密</span>
              </div>
            </TabsContent>
            <TabsContent
              value="cloud"
              className="m-0 min-h-0 min-w-0 space-y-5 overflow-y-auto p-6"
            >
              <div>
                <h2 className="text-sm font-medium">云同步</h2>
                <p className="mt-1 text-xs text-muted-foreground">
                  配置一个 S3 或 WebDAV 同步目标。
                </p>
              </div>
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
              {snapshot.credentialError && (
                <p
                  role="alert"
                  className="rounded-lg bg-destructive/10 p-3 text-xs leading-5 text-destructive"
                >
                  {snapshot.credentialError}
                  。本地记录可继续编辑；请恢复凭据服务、重新填写配置或停用同步。
                </p>
              )}
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
              {(snapshot.config || snapshot.credentialError) && (
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
            <TabsContent
              value="security"
              className="m-0 min-h-0 min-w-0 space-y-5 overflow-y-auto p-6"
            >
              <div>
                <h2 className="text-sm font-medium">安全与备份</h2>
                <p className="mt-1 text-xs text-muted-foreground">
                  管理可选的系统解锁、主密码和加密备份。
                </p>
              </div>
              <div className="space-y-3 rounded-xl border p-4">
                <div className="flex items-start justify-between gap-4">
                  <div>
                    <Label
                      htmlFor="system-unlock-enabled"
                      className="gap-2 text-xs font-medium"
                    >
                      <Fingerprint className="size-4" />
                      系统解锁
                    </Label>
                    <p className="mt-2 text-[11px] leading-5 text-muted-foreground">
                      {snapshot.systemUnlock.label}
                      ，仅在本机生效。默认关闭，主密码始终可用。
                    </p>
                  </div>
                  <Switch
                    id="system-unlock-enabled"
                    checked={snapshot.systemUnlock.enabled}
                    disabled={
                      working ||
                      snapshot.recoveryPending ||
                      (!snapshot.systemUnlock.enabled &&
                        !snapshot.systemUnlock.available)
                    }
                    onCheckedChange={(enabled) => {
                      if (enabled) setSecurity("system");
                      else
                        void run(async () => {
                          const lifecycle = useVault.getState().lifecycle;
                          const next = await invoke<Snapshot>(
                            "set_system_unlock",
                            {
                              sessionId: snapshot.sessionId,
                              enabled: false,
                              password: "",
                            },
                          );
                          if (useVault.getState().lifecycle === lifecycle)
                            useVault.getState().accept(next);
                        }, "已关闭系统解锁");
                    }}
                  />
                </div>
                <p className="text-[11px] leading-5 text-muted-foreground">
                  {snapshot.systemUnlock.reason ??
                    "启用时需确认主密码并完成系统验证。更换主密码后需重新启用。"}
                </p>
              </div>
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
              {security === "system"
                ? "启用系统解锁"
                : security === "change"
                  ? "修改主密码"
                  : security === "migrate"
                    ? "迁移到新主密码"
                    : "导入并替换保险库"}
            </DialogTitle>
            <DialogDescription>
              {security === "system"
                ? "先确认当前主密码，再完成系统验证。你仍可随时使用主密码解锁，也可以在设置中关闭此功能。"
                : security === "change"
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
                {security === "system"
                  ? "当前主密码"
                  : security === "import"
                    ? "备份的主密码"
                    : "新主密码"}
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
                {security === "system"
                  ? "启用并验证"
                  : security === "import"
                    ? "选择文件并替换"
                    : "确认"}
              </Button>
            </DialogFooter>
          </form>
        </DialogContent>
      </Dialog>
    </>
  );
}
