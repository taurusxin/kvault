import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Switch } from "@/components/ui/switch";
import { Tabs, TabsList, TabsTrigger } from "@/components/ui/tabs";
import { emptyS3, emptyWebdav, type SyncConfig } from "@/lib/types";

export function SyncForm({
  config,
  onChange,
  saved = false,
  disabled = false,
}: {
  config: SyncConfig;
  onChange: (config: SyncConfig) => void;
  saved?: boolean;
  disabled?: boolean;
}) {
  const field = (
    label: string,
    name: string,
    value: string,
    placeholder = "",
    secret = false,
  ) => (
    <div className="space-y-1.5" key={name}>
      <Label htmlFor={`sync-${name}`} className="text-xs text-muted-foreground">
        {label}
      </Label>
      <Input
        id={`sync-${name}`}
        value={value}
        type={secret ? "password" : "text"}
        placeholder={placeholder}
        disabled={disabled}
        autoComplete="off"
        spellCheck={false}
        className="h-9 text-xs"
        onChange={(e) => onChange({ ...config, [name]: e.target.value })}
      />
    </div>
  );
  return (
    <div className="space-y-4">
      <Tabs
        value={config.type}
        onValueChange={(value) =>
          onChange(value === "s3" ? emptyS3() : emptyWebdav())
        }
      >
        <TabsList className="w-full">
          <TabsTrigger value="s3" className="flex-1" disabled={disabled}>
            S3 兼容存储
          </TabsTrigger>
          <TabsTrigger value="webdav" className="flex-1" disabled={disabled}>
            WebDAV
          </TabsTrigger>
        </TabsList>
      </Tabs>
      {field(
        "服务地址",
        "endpoint",
        config.endpoint,
        config.type === "s3"
          ? "https://s3.example.com"
          : "https://dav.example.com/remote.php/dav/files/user",
      )}
      {config.type === "s3" ? (
        <>
          <div className="grid grid-cols-2 gap-3">
            {field("Bucket", "bucket", config.bucket, "my-vault")}
            {field("Region", "region", config.region, "us-east-1")}
          </div>
          {field("Access Key ID", "accessKey", config.accessKey)}
          {field(
            "Secret Access Key",
            "secretKey",
            config.secretKey,
            saved ? "留空保留已保存的密钥" : "输入 Secret Access Key",
            true,
          )}
          <div className="grid grid-cols-2 gap-3">
            {field("路径前缀", "prefix", config.prefix, "kvault")}
            {field(
              "Session Token（可选）",
              "sessionToken",
              config.sessionToken ?? "",
              saved ? "留空保留" : "可选",
              true,
            )}
          </div>
          <div className="flex items-center justify-between">
            <Label
              htmlFor="path-style"
              className="text-xs text-muted-foreground"
            >
              Path Style（适用于 MinIO 等）
            </Label>
            <Switch
              id="path-style"
              checked={config.pathStyle}
              disabled={disabled}
              onCheckedChange={(pathStyle) =>
                onChange({ ...config, pathStyle })
              }
            />
          </div>
        </>
      ) : (
        <>
          {field("目录", "directory", config.directory, "kvault")}
          {field("用户名", "username", config.username)}
          {field(
            "密码 / 应用专用密码",
            "password",
            config.password,
            saved ? "留空保留已保存的密码" : "输入服务密码",
            true,
          )}
        </>
      )}
      {config.endpoint.startsWith("http:") && (
        <p className="rounded-lg bg-amber-500/10 p-2.5 text-xs leading-relaxed text-amber-700 dark:text-amber-400">
          HTTP 连接会明文传输服务凭据，建议使用 HTTPS。
        </p>
      )}
      <p className="text-[11px] leading-relaxed text-muted-foreground">
        只上传加密后的保险库。连接凭据保存在本机系统凭据库，服务需支持 ETag
        条件写入。
      </p>
    </div>
  );
}
