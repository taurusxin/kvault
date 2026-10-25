import { useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import {
  ArrowRight,
  Cloud,
  Fingerprint,
  FolderInput,
  KeyRound,
  LoaderCircle,
  LockKeyhole,
  ShieldCheck,
  ArrowLeft,
} from "lucide-react";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { SyncForm } from "@/components/sync-form";
import { desktop, useVault } from "@/stores/vault";
import { emptyS3, errorMessage, type Snapshot } from "@/lib/types";

export function Onboarding() {
  const exists = useVault((s) => s.exists);
  const [mode, setMode] = useState<"create" | "remote" | "import">("create");
  const [password, setPassword] = useState("");
  const [confirm, setConfirm] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");
  const [config, setConfig] = useState(emptyS3);
  async function submit(event: React.FormEvent) {
    event.preventDefault();
    setError("");
    if (!desktop) {
      setError("请使用桌面应用运行 KVault。浏览器预览无法访问加密保险库。");
      return;
    }
    if (
      !exists &&
      mode === "create" &&
      (password.length < 8 || password !== confirm)
    ) {
      setError("主密码至少 8 个字符，两次输入需要一致。");
      return;
    }
    const submitted = password;
    setPassword("");
    setConfirm("");
    setBusy(true);
    const lifecycle = useVault.getState().lifecycle;
    try {
      const snapshot = exists
        ? await invoke<Snapshot>("unlock_vault", { password: submitted })
        : mode === "remote"
          ? await invoke<Snapshot>("connect_remote", {
              config,
              password: submitted,
            })
          : mode === "import"
            ? await invoke<Snapshot | null>("import_backup", {
                password: submitted,
                replace: false,
                sessionId: null,
              })
            : await invoke<Snapshot>("create_vault", { password: submitted });
      if (snapshot && useVault.getState().lifecycle === lifecycle)
        useVault.getState().accept(snapshot, true);
    } catch (e) {
      setError(errorMessage(e));
    } finally {
      setBusy(false);
    }
  }
  return (
    <main className="relative flex min-h-0 flex-1 items-start justify-center overflow-auto bg-muted/15 px-8 py-10">
      <div className="pointer-events-none absolute inset-0 subtle-grid opacity-45" />
      <div
        className={`relative my-auto grid w-full max-w-[920px] gap-12 ${!exists && mode === "remote" ? "lg:grid-cols-[1fr_400px]" : "lg:grid-cols-[1fr_360px]"}`}
      >
        <div className="flex flex-col justify-center pb-4">
          <div className="mb-8 flex size-14 items-center justify-center rounded-2xl bg-foreground text-background shadow-lg shadow-foreground/5">
            <LockKeyhole className="size-7" strokeWidth={1.6} />
          </div>
          <p className="mb-4 text-[11px] font-medium uppercase tracking-[.24em] text-muted-foreground">
            A quiet place for your keys
          </p>
          <h1 className="text-[38px] font-semibold leading-[1.3] tracking-tight">
            重要的记录，
            <br />
            <span className="text-muted-foreground">安心放在这里。</span>
          </h1>
          <p className="mt-5 max-w-xs text-sm leading-7 text-muted-foreground">
            从一条 Key
            开始，把零散的信息整理成表。一个密码，让所有设备上的记录安全相连。
          </p>
          <div className="mt-10 flex flex-col gap-4 text-xs text-muted-foreground">
            <div className="flex items-center gap-3">
              <ShieldCheck className="size-4" />
              <span>本地与云端，始终加密存储</span>
            </div>
            <div className="flex items-center gap-3">
              <Cloud className="size-4" />
              <span>S3 / WebDAV，云存储由你选择</span>
            </div>
            <div className="flex items-center gap-3">
              <Fingerprint className="size-4" />
              <span>无需账号，主密码只由你掌握</span>
            </div>
          </div>
        </div>
        <section className="self-center rounded-2xl border bg-card p-7 shadow-sm">
          {!exists && mode !== "create" && (
            <Button
              variant="ghost"
              size="sm"
              className="mb-4 -ml-2 text-muted-foreground"
              disabled={busy}
              onClick={() => {
                setMode("create");
                setError("");
                setPassword("");
              }}
            >
              <ArrowLeft />
              返回
            </Button>
          )}
          <div className="mb-6">
            <h2 className="text-lg font-semibold tracking-tight">
              {exists
                ? "欢迎回来"
                : mode === "remote"
                  ? "连接云端保险库"
                  : mode === "import"
                    ? "导入加密备份"
                    : "创建你的保险库"}
            </h2>
            <p className="mt-2 text-xs leading-5 text-muted-foreground">
              {exists
                ? "输入主密码，解锁属于你的记录。"
                : mode === "remote"
                  ? "填写同步服务信息，使用已有保险库的主密码解锁。"
                  : mode === "import"
                    ? "输入备份生成时的主密码，再选择 .kvault 文件。"
                    : "设置一个主密码，保护所有表与记录。"}
            </p>
          </div>
          <form onSubmit={(e) => void submit(e)} className="space-y-4">
            {!exists && mode === "remote" && (
              <SyncForm config={config} onChange={setConfig} disabled={busy} />
            )}
            <div className="space-y-2">
              <Label htmlFor="master-password" className="text-xs">
                主密码
              </Label>
              <Input
                id="master-password"
                autoFocus
                type="password"
                autoComplete={exists ? "current-password" : "new-password"}
                value={password}
                onChange={(e) => setPassword(e.target.value)}
                placeholder={exists ? "输入主密码" : "至少 8 个字符"}
                className="h-10"
                required
                disabled={busy}
              />
            </div>
            {!exists && mode === "create" && (
              <div className="space-y-2">
                <Label htmlFor="confirm-password" className="text-xs">
                  确认主密码
                </Label>
                <Input
                  id="confirm-password"
                  type="password"
                  autoComplete="new-password"
                  value={confirm}
                  onChange={(e) => setConfirm(e.target.value)}
                  placeholder="再次输入主密码"
                  className="h-10"
                  required
                  disabled={busy}
                />
              </div>
            )}
            {error && (
              <p
                role="alert"
                className="rounded-lg bg-destructive/10 px-3 py-2 text-xs leading-5 text-destructive"
              >
                {error}
              </p>
            )}
            <Button
              type="submit"
              className="h-10 w-full"
              disabled={busy || !password}
            >
              {busy ? (
                <LoaderCircle className="animate-spin" />
              ) : exists ? (
                <KeyRound />
              ) : mode === "import" ? (
                <FolderInput />
              ) : null}
              {busy
                ? "正在处理…"
                : exists
                  ? "解锁保险库"
                  : mode === "remote"
                    ? "连接并解锁"
                    : mode === "import"
                      ? "选择备份文件"
                      : "创建保险库"}
              {!busy && <ArrowRight className="ml-auto" />}
            </Button>
          </form>
          {!exists && mode === "create" && (
            <>
              <p className="mt-4 text-[11px] leading-5 text-muted-foreground">
                请妥善保管主密码。KVault
                无法找回密码，也无法解密丢失密码的记录。
              </p>
              <div className="my-5 h-px bg-border" />
              <div className="flex gap-2">
                <Button
                  variant="outline"
                  className="flex-1 text-xs"
                  disabled={busy}
                  onClick={() => {
                    setMode("remote");
                    setError("");
                    setPassword("");
                    setConfirm("");
                  }}
                >
                  <Cloud />
                  连接已有保险库
                </Button>
                <Button
                  variant="ghost"
                  size="icon"
                  aria-label="导入加密备份"
                  title="导入加密备份"
                  disabled={busy}
                  onClick={() => {
                    setMode("import");
                    setPassword("");
                    setConfirm("");
                  }}
                >
                  <FolderInput />
                </Button>
              </div>
            </>
          )}
          {exists && (
            <p className="mt-5 flex items-center justify-center gap-1.5 text-[11px] text-muted-foreground">
              <LockKeyhole className="size-3" />
              离开 5 分钟后自动锁定
            </p>
          )}
        </section>
      </div>
      <span className="absolute bottom-5 right-6 text-[10px] tracking-wider text-muted-foreground/60">
        KVAULT / 0.1.0
      </span>
    </main>
  );
}
