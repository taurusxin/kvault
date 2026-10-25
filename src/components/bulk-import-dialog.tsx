import { useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import {
  Eye,
  EyeOff,
  FileSpreadsheet,
  LoaderCircle,
  Upload,
} from "lucide-react";
import { toast } from "sonner";
import { Button } from "@/components/ui/button";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { Label } from "@/components/ui/label";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { Switch } from "@/components/ui/switch";
import { Tabs, TabsList, TabsTrigger } from "@/components/ui/tabs";
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from "@/components/ui/table";
import { Textarea } from "@/components/ui/textarea";
import {
  errorMessage,
  type ImportDelimiter,
  type ImportHeader,
  type ImportPreview,
} from "@/lib/types";
import { useVault } from "@/stores/vault";

export function BulkImportDialog({ onClose }: { onClose: () => void }) {
  const snapshot = useVault((s) => s.snapshot)!;
  const busy = useVault((s) => s.busy);
  const [source, setSource] = useState("text");
  const [text, setText] = useState("");
  const [fileName, setFileName] = useState("");
  const [delimiter, setDelimiter] = useState<ImportDelimiter>("auto");
  const [header, setHeader] = useState<ImportHeader>("auto");
  const [tableId, setTableId] = useState(
    useVault.getState().selectedTable ?? snapshot.tables[0]?.id ?? "",
  );
  const [policy, setPolicy] = useState<"skip" | "overwrite">("skip");
  const [preview, setPreview] = useState<ImportPreview | null>(null);
  const [skipInvalid, setSkipInvalid] = useState(false);
  const [showValues, setShowValues] = useState(
    useVault.getState().listValuesVisible,
  );
  const [working, setWorking] = useState(false);
  const [error, setError] = useState("");
  const active = useRef(true);
  const request = useRef(0);
  useEffect(() => {
    active.current = true;
    return () => {
      active.current = false;
      request.current += 1;
    };
  }, []);
  const sessionId = snapshot.sessionId;
  const lifecycle = useVault.getState().lifecycle;
  const current = (id: number) =>
    active.current &&
    request.current === id &&
    useVault.getState().lifecycle === lifecycle &&
    useVault.getState().snapshot?.sessionId === sessionId;
  function invalidate() {
    setPreview(null);
    setSkipInvalid(false);
    setError("");
    request.current += 1;
  }
  async function chooseFile() {
    const id = ++request.current;
    setWorking(true);
    setError("");
    setPreview(null);
    try {
      const file = await invoke<{ name: string; text: string } | null>(
        "select_import_csv",
        { sessionId },
      );
      if (current(id) && file) {
        setText(file.text);
        setFileName(file.name);
        setPreview(null);
        setSkipInvalid(false);
      }
    } catch (e) {
      if (current(id)) setError(errorMessage(e));
    } finally {
      if (current(id)) setWorking(false);
    }
  }
  async function analyze() {
    const id = ++request.current;
    setWorking(true);
    setError("");
    setPreview(null);
    setSkipInvalid(false);
    try {
      const next = await invoke<ImportPreview>("preview_import", {
        sessionId,
        text,
        delimiter,
        header,
      });
      if (current(id)) setPreview(next);
    } catch (e) {
      if (current(id)) setError(errorMessage(e));
    } finally {
      if (current(id)) setWorking(false);
    }
  }
  const seen = new Set(
    snapshot.entries.filter((e) => e.tableId === tableId).map((e) => e.key),
  );
  const rows =
    preview?.rows.map((row) => {
      const duplicate = seen.has(row.key);
      seen.add(row.key);
      return {
        ...row,
        decision: duplicate
          ? policy === "overwrite"
            ? "覆盖"
            : "跳过"
          : "新增",
      };
    }) ?? [];
  const added = rows.filter((r) => r.decision === "新增").length;
  const overwritten = rows.filter((r) => r.decision === "覆盖").length;
  const skipped = rows.filter((r) => r.decision === "跳过").length;
  const canImport =
    preview &&
    added + overwritten > 0 &&
    snapshot.tables.some((t) => t.id === tableId) &&
    (!preview.errors.length || skipInvalid);
  async function commit() {
    if (!canImport || !preview) return;
    setWorking(true);
    setError("");
    try {
      const result = await useVault.getState().mutate({
        type: "importEntries",
        tableId,
        rows: preview.rows,
        duplicatePolicy: policy,
      });
      if (result && active.current) {
        onClose();
        toast.success("批量导入完成，记录已加密保存");
      }
    } catch (e) {
      if (active.current) setError(errorMessage(e));
    } finally {
      if (active.current) setWorking(false);
    }
  }
  return (
    <Dialog
      open
      onOpenChange={(open) => {
        if (!open && !working) onClose();
      }}
    >
      <DialogContent className="max-h-[88vh] overflow-y-auto sm:max-w-[680px]">
        <DialogHeader>
          <DialogTitle>批量导入记录</DialogTitle>
          <DialogDescription>
            每行一条记录，依次为
            Key、Value、可选备注。预览确认后一次性加密保存。
          </DialogDescription>
        </DialogHeader>
        <Tabs
          value={source}
          onValueChange={(value) => {
            setSource(value);
            setText("");
            setFileName("");
            setDelimiter(value === "csv" ? "comma" : "auto");
            invalidate();
          }}
        >
          <TabsList className="w-full">
            <TabsTrigger value="text" className="flex-1" disabled={working}>
              粘贴纯文本
            </TabsTrigger>
            <TabsTrigger value="csv" className="flex-1" disabled={working}>
              CSV 文件
            </TabsTrigger>
          </TabsList>
        </Tabs>
        {source === "text" ? (
          <div className="space-y-2">
            <Label htmlFor="import-text" className="text-xs">
              粘贴记录
            </Label>
            <Textarea
              id="import-text"
              className="mono min-h-36 text-xs leading-6"
              placeholder={
                "api.token abc123 生产环境\nusername alice\n\n也支持逗号分隔，例如：\napi.token,abc123,生产环境"
              }
              value={text}
              disabled={working}
              spellCheck={false}
              onChange={(e) => {
                setText(e.target.value);
                invalidate();
              }}
            />
          </div>
        ) : (
          <div className="flex items-center gap-3 rounded-xl border border-dashed p-4">
            <FileSpreadsheet className="size-6 shrink-0 text-muted-foreground" />
            <div className="min-w-0 flex-1">
              <p className="truncate text-xs font-medium">
                {fileName || "选择一个 UTF-8 CSV 文件"}
              </p>
              <p className="mt-1 text-[11px] text-muted-foreground">
                支持带引号、逗号、多行 Value；文件不超过 10 MiB。
              </p>
            </div>
            <Button
              variant="outline"
              size="sm"
              disabled={working}
              onClick={() => void chooseFile()}
            >
              {fileName ? "重新选择" : "选择文件"}
            </Button>
          </div>
        )}
        <div className="grid grid-cols-2 gap-3">
          <div className="space-y-2">
            <Label htmlFor="import-delimiter" className="text-xs">
              分隔符
            </Label>
            <Select
              value={delimiter}
              disabled={working}
              onValueChange={(value) => {
                setDelimiter(value as ImportDelimiter);
                invalidate();
              }}
            >
              <SelectTrigger id="import-delimiter" className="w-full">
                <SelectValue />
              </SelectTrigger>
              <SelectContent>
                <SelectItem value="auto">自动识别</SelectItem>
                <SelectItem value="comma">逗号（CSV）</SelectItem>
                <SelectItem value="space">空格 / Tab</SelectItem>
              </SelectContent>
            </Select>
          </div>
          <div className="space-y-2">
            <Label htmlFor="import-header" className="text-xs">
              表头
            </Label>
            <Select
              value={header}
              disabled={working}
              onValueChange={(value) => {
                setHeader(value as ImportHeader);
                invalidate();
              }}
            >
              <SelectTrigger id="import-header" className="w-full">
                <SelectValue />
              </SelectTrigger>
              <SelectContent>
                <SelectItem value="auto">自动识别 key,value,notes</SelectItem>
                <SelectItem value="present">首行是表头</SelectItem>
                <SelectItem value="none">没有表头</SelectItem>
              </SelectContent>
            </Select>
          </div>
        </div>
        <p className="text-[11px] leading-5 text-muted-foreground">
          同一批次使用相同分隔符。含分隔符的字段用双引号包围，如{" "}
          <code className="mono">token "hello world" "备注内容"</code>。一次最多
          10000 行。
        </p>
        <div className="flex justify-end">
          <Button
            variant="outline"
            disabled={working || !text.trim()}
            onClick={() => void analyze()}
          >
            {working && <LoaderCircle className="animate-spin" />}预览导入
          </Button>
        </div>
        {preview && (
          <div className="space-y-4 border-t pt-4">
            <div className="grid grid-cols-2 gap-3">
              <div className="space-y-2">
                <Label htmlFor="import-table" className="text-xs">
                  目标数据表
                </Label>
                <Select
                  value={tableId}
                  disabled={working}
                  onValueChange={setTableId}
                >
                  <SelectTrigger id="import-table" className="w-full">
                    <SelectValue placeholder="选择目标表" />
                  </SelectTrigger>
                  <SelectContent>
                    {snapshot.tables.map((t) => (
                      <SelectItem key={t.id} value={t.id}>
                        {t.name}
                      </SelectItem>
                    ))}
                  </SelectContent>
                </Select>
              </div>
              <div className="space-y-2">
                <Label htmlFor="import-policy" className="text-xs">
                  重复 Key
                </Label>
                <Select
                  value={policy}
                  disabled={working}
                  onValueChange={(value) =>
                    setPolicy(value as "skip" | "overwrite")
                  }
                >
                  <SelectTrigger id="import-policy" className="w-full">
                    <SelectValue />
                  </SelectTrigger>
                  <SelectContent>
                    <SelectItem value="skip">跳过（保留已有记录）</SelectItem>
                    <SelectItem value="overwrite">
                      覆盖（最后一行生效）
                    </SelectItem>
                  </SelectContent>
                </Select>
              </div>
            </div>
            <div className="flex items-center justify-between gap-2">
              <p className="text-xs text-muted-foreground">
                新增 {added} · 覆盖 {overwritten} · 跳过 {skipped}
                {preview.hasHeader ? " · 已识别表头" : ""}
              </p>
              <Button
                variant="ghost"
                size="sm"
                className="text-xs"
                onClick={() => setShowValues(!showValues)}
              >
                {showValues ? <EyeOff /> : <Eye />}
                {showValues ? "隐藏预览值" : "显示预览值"}
              </Button>
            </div>
            {rows.length ? (
              <div className="max-h-52 overflow-auto rounded-lg border">
                <Table className="w-full table-fixed text-left text-xs">
                  <TableHeader className="sticky top-0 bg-muted">
                    <TableRow>
                      <TableHead className="w-10 p-2 font-medium">行</TableHead>
                      <TableHead className="p-2 font-medium">Key</TableHead>
                      <TableHead className="p-2 font-medium">Value</TableHead>
                      <TableHead className="p-2 font-medium">备注</TableHead>
                      <TableHead className="w-12 p-2 font-medium">
                        处理
                      </TableHead>
                    </TableRow>
                  </TableHeader>
                  <TableBody>
                    {rows.slice(0, 100).map((row, index) => (
                      <TableRow key={index} className="border-t">
                        <TableCell className="p-2 text-muted-foreground">
                          {row.line}
                        </TableCell>
                        <TableCell className="truncate p-2 mono">
                          {row.key}
                        </TableCell>
                        <TableCell className="truncate p-2 mono">
                          {showValues ? row.value || "（空）" : "••••••••"}
                        </TableCell>
                        <TableCell className="truncate p-2 text-muted-foreground">
                          {row.notes}
                        </TableCell>
                        <TableCell className="p-2 text-muted-foreground">
                          {row.decision}
                        </TableCell>
                      </TableRow>
                    ))}
                  </TableBody>
                </Table>
              </div>
            ) : (
              <p className="text-xs text-muted-foreground">
                没有有效记录，请检查内容与分隔符。
              </p>
            )}
            {rows.length > 100 && (
              <p className="text-[11px] text-muted-foreground">
                仅展示前 100 行预览，将按所选规则处理全部 {rows.length} 行。
              </p>
            )}
            {preview.errors.length > 0 && (
              <div className="space-y-2 rounded-lg bg-destructive/5 p-3">
                <p className="text-xs font-medium text-destructive">
                  {preview.errors.length} 行内容无效
                </p>
                {preview.errors.slice(0, 5).map((issue, index) => (
                  <p
                    key={index}
                    className="text-[11px] leading-5 text-destructive"
                  >
                    第 {issue.line} 行：{issue.message}
                  </p>
                ))}
                {preview.errors.length > 5 && (
                  <p className="text-[11px] text-destructive">
                    另有 {preview.errors.length - 5} 行错误。
                  </p>
                )}
                <div className="flex items-center justify-between gap-3 pt-1">
                  <Label htmlFor="skip-invalid" className="text-xs">
                    跳过无效行，仅导入有效记录
                  </Label>
                  <Switch
                    id="skip-invalid"
                    checked={skipInvalid}
                    onCheckedChange={setSkipInvalid}
                    disabled={working}
                  />
                </div>
              </div>
            )}
          </div>
        )}
        {error && (
          <p role="alert" className="text-xs leading-5 text-destructive">
            {error}
          </p>
        )}
        <DialogFooter>
          <Button variant="outline" disabled={working} onClick={onClose}>
            取消
          </Button>
          <Button
            disabled={!canImport || working || busy}
            onClick={() => void commit()}
          >
            {working ? <LoaderCircle className="animate-spin" /> : <Upload />}
            确认导入
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
