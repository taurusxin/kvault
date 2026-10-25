import { useState, type ReactNode } from "react";
import { DragDropProvider } from "@dnd-kit/react";
import { useSortable, isSortable } from "@dnd-kit/react/sortable";
import { invoke } from "@tauri-apps/api/core";
import { toast } from "sonner";
import {
  ArrowDownAZ,
  ArrowUpDown,
  Check,
  ChevronRight,
  Cloud,
  CloudOff,
  Copy,
  Database,
  Eye,
  EyeOff,
  FileKey2,
  FolderInput,
  GripVertical,
  KeyRound,
  LayoutGrid,
  ListOrdered,
  LoaderCircle,
  LockKeyhole,
  MoreHorizontal,
  Plus,
  RefreshCw,
  Save,
  Search,
  Settings2,
  ShieldCheck,
  Table2,
  Trash2,
  Pencil,
  X,
  TriangleAlert,
  Upload,
} from "lucide-react";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Textarea } from "@/components/ui/textarea";
import { Label } from "@/components/ui/label";
import { Badge } from "@/components/ui/badge";
import { Switch } from "@/components/ui/switch";
import { ScrollArea } from "@/components/ui/scroll-area";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuSeparator,
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu";
import {
  ContextMenu,
  ContextMenuContent,
  ContextMenuItem,
  ContextMenuLabel,
  ContextMenuSeparator,
  ContextMenuTrigger,
} from "@/components/ui/context-menu";
import {
  ResizableHandle,
  ResizablePanel,
  ResizablePanelGroup,
} from "@/components/ui/resizable";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
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
import { useVault } from "@/stores/vault";
import {
  compareEntryOrder,
  errorMessage,
  type Action,
  type Snapshot,
} from "@/lib/types";
import { SettingsDialog, type SettingsTab } from "@/components/settings-dialog";
import { ConflictsDialog } from "@/components/conflicts-dialog";
import { BulkImportDialog } from "@/components/bulk-import-dialog";

const date = (time: number) =>
  new Intl.DateTimeFormat("zh-CN", {
    month: "2-digit",
    day: "2-digit",
    hour: "2-digit",
    minute: "2-digit",
    hour12: false,
  }).format(time);

export function Workspace() {
  const store = useVault();
  const snapshot = store.snapshot!;
  const [settings, setSettings] = useState<SettingsTab | null>(null);
  const [conflicts, setConflicts] = useState(false);
  const [importOpen, setImportOpen] = useState(false);
  const [tableDialog, setTableDialog] = useState<{
    id?: string;
    name: string;
  } | null>(null);
  const [deletion, setDeletion] = useState<{
    title: string;
    detail: string;
    action: Action;
  } | null>(null);
  const [sort, setSort] = useState<"manual" | "updated" | "key">("manual");
  const [dragging, setDragging] = useState(false);
  const ordering = Boolean(store.orderDraft);
  const draftPositions = new Map(
    store.orderDraft?.ids.map((id, index) => [id, index]) ?? [],
  );
  const [move, setMove] = useState<{ ids: string[]; tableId: string } | null>(
    null,
  );
  const readOnly = snapshot.recoveryPending || snapshot.conflicts.length > 0;
  const selectedTable = snapshot.tables.find(
    (t) => t.id === store.selectedTable,
  );
  const filtered = snapshot.entries
    .filter(
      (e) =>
        (!store.selectedTable || e.tableId === store.selectedTable) &&
        (!store.query ||
          [e.key, e.value, e.notes].some((s) =>
            s.toLocaleLowerCase().includes(store.query.toLocaleLowerCase()),
          )),
    )
    .sort((a, b) =>
      ordering
        ? (draftPositions.get(a.id) ?? 0) - (draftPositions.get(b.id) ?? 0)
        : sort === "key"
          ? a.key.localeCompare(b.key)
          : sort === "updated"
            ? b.updatedAt - a.updatedAt
            : compareEntryOrder(a, b),
    );
  const selectedIds = new Set(store.selectedIds);
  const selection = snapshot.entries.filter((entry) =>
    selectedIds.has(entry.id),
  );
  const orderedIds = filtered.map((entry) => entry.id);
  function moveEntries(ids = store.selectedIds) {
    const selected = new Set(ids);
    const entries = snapshot.entries.filter((entry) => selected.has(entry.id));
    const target = snapshot.tables.find((table) =>
      entries.some((entry) => entry.tableId !== table.id),
    );
    if (!entries.length || !target) return;
    store.navigate(() =>
      setMove({ ids: entries.map((entry) => entry.id), tableId: target.id }),
    );
  }
  function deleteEntries(ids = store.selectedIds) {
    const selected = new Set(ids);
    const entries = snapshot.entries.filter((entry) => selected.has(entry.id));
    if (!entries.length) return;
    store.navigate(() =>
      setDeletion({
        title:
          entries.length === 1
            ? "删除这条记录？"
            : `删除选中的 ${entries.length} 条记录？`,
        detail:
          entries.length === 1
            ? `「${entries[0].key}」将从所有同步设备中删除。`
            : `选中的 ${entries.length} 条记录将从所有同步设备中删除。`,
        action: {
          type: "deleteEntries",
          ids: entries.map((entry) => entry.id),
        },
      }),
    );
  }
  async function mutate(action: Action) {
    try {
      return Boolean(await store.mutate(action));
    } catch (error) {
      toast.error(errorMessage(error));
      return false;
    }
  }
  async function syncNow() {
    try {
      await invoke<Snapshot>("sync_now", { sessionId: snapshot.sessionId });
    } catch (error) {
      toast.error(errorMessage(error));
    }
  }
  function lock() {
    store.navigate(() => {
      store.clear();
      void invoke("lock_vault").catch((error) =>
        toast.error(errorMessage(error)),
      );
    });
  }
  async function save() {
    try {
      await store.save();
    } catch (error) {
      toast.error(errorMessage(error));
    }
  }
  async function copy() {
    if (!store.draft?.id) return;
    await copyEntry(store.draft.id, "value");
  }
  async function copyEntry(entryId: string, field: "key" | "value") {
    try {
      await invoke(field === "key" ? "copy_key" : "copy_value", {
        sessionId: snapshot.sessionId,
        entryId,
      });
      toast.success(`${field === "key" ? "Key" : "Value"} 已复制`);
    } catch (error) {
      toast.error(errorMessage(error));
    }
  }
  return (
    <div className="flex min-h-0 flex-1">
      <aside className="flex w-56 shrink-0 flex-col border-r bg-sidebar">
        <div className="px-3 pb-5 pt-5">
          <div className="mb-5 flex items-center gap-2.5 px-2">
            <div className="flex size-8 items-center justify-center rounded-xl border bg-background">
              <Database className="size-4" />
            </div>
            <div>
              <p className="text-xs font-medium">私人保险库</p>
              <p className="mt-0.5 flex items-center gap-1 text-[10px] text-muted-foreground">
                <span className="size-1.5 rounded-full bg-emerald-500" />
                已解锁
              </p>
            </div>
          </div>
          <Button
            variant={store.selectedTable === null ? "secondary" : "ghost"}
            className="h-9 w-full justify-start gap-2 text-xs"
            onClick={() => store.selectTable(null)}
          >
            <LayoutGrid className="size-3.5" />
            全部记录
            <span className="ml-auto text-[10px] text-muted-foreground">
              {snapshot.entries.length}
            </span>
          </Button>
        </div>
        <div className="mb-2 flex items-center justify-between px-5">
          <span className="text-[10px] font-medium tracking-widest text-muted-foreground">
            数据表
          </span>
          <Button
            variant="ghost"
            size="icon-xs"
            aria-label="新建表"
            disabled={readOnly}
            onClick={() => setTableDialog({ name: "" })}
          >
            <Plus className="size-3.5" />
          </Button>
        </div>
        <ScrollArea className="min-h-0 flex-1 px-3">
          <div className="space-y-1 pb-5">
            {snapshot.tables.map((table) => (
              <div
                key={table.id}
                className={`group flex h-9 items-center gap-0.5 rounded-lg ${store.selectedTable === table.id ? "bg-accent" : "hover:bg-accent/60"}`}
              >
                <Button
                  variant="ghost"
                  className="h-full min-w-0 flex-1 justify-start gap-2 text-xs hover:bg-transparent"
                  onClick={() => store.selectTable(table.id)}
                >
                  <Table2 className="size-3.5 text-muted-foreground" />
                  <span className="truncate">{table.name}</span>
                  <span className="ml-auto text-[10px] text-muted-foreground group-hover:hidden">
                    {
                      snapshot.entries.filter((e) => e.tableId === table.id)
                        .length
                    }
                  </span>
                </Button>
                <DropdownMenu>
                  <DropdownMenuTrigger asChild>
                    <Button
                      variant="ghost"
                      size="icon-xs"
                      aria-label={`${table.name}的操作`}
                      className="mr-1 hidden group-hover:flex data-[state=open]:flex"
                      disabled={readOnly}
                    >
                      <MoreHorizontal className="size-3.5" />
                    </Button>
                  </DropdownMenuTrigger>
                  <DropdownMenuContent align="start">
                    <DropdownMenuItem
                      onSelect={() =>
                        store.navigate(() =>
                          setTableDialog({ id: table.id, name: table.name }),
                        )
                      }
                    >
                      <Pencil />
                      重命名
                    </DropdownMenuItem>
                    <DropdownMenuSeparator />
                    <DropdownMenuItem
                      variant="destructive"
                      onSelect={() =>
                        store.navigate(() =>
                          setDeletion({
                            title: `删除「${table.name}」？`,
                            detail: `此表包含 ${snapshot.entries.filter((e) => e.tableId === table.id).length} 条记录，删除后将同步到其他设备。`,
                            action: { type: "deleteTable", id: table.id },
                          }),
                        )
                      }
                    >
                      <Trash2 />
                      删除表
                    </DropdownMenuItem>
                  </DropdownMenuContent>
                </DropdownMenu>
              </div>
            ))}
            {!snapshot.tables.length && (
              <p className="px-2 py-3 text-xs text-muted-foreground">
                新建一张表，开始整理记录。
              </p>
            )}
          </div>
        </ScrollArea>
        <div className="border-t p-3">
          <button
            className="mb-3 flex w-full items-center gap-2 rounded-lg border bg-background/60 p-2.5 text-left transition hover:bg-background"
            onClick={() => store.navigate(() => setSettings("cloud"))}
          >
            {snapshot.config ? (
              <Cloud className="size-3.5 text-muted-foreground" />
            ) : (
              <CloudOff className="size-3.5 text-muted-foreground" />
            )}
            <div className="min-w-0">
              <p className="text-[11px] font-medium">
                {snapshot.config
                  ? snapshot.config.type === "s3"
                    ? "S3 同步"
                    : "WebDAV 同步"
                  : "连接云存储"}
              </p>
              <p className="mt-0.5 truncate text-[10px] text-muted-foreground">
                {snapshot.config
                  ? store.sync.message
                  : "让记录在设备间安全相连"}
              </p>
            </div>
            {!snapshot.config && (
              <ChevronRight className="ml-auto size-3 text-muted-foreground" />
            )}
          </button>
          <div className="flex items-center gap-1">
            <Button
              variant="ghost"
              className="h-8 flex-1 justify-start text-xs text-muted-foreground"
              onClick={() =>
                store.navigate(() =>
                  setSettings(
                    snapshot.recoveryPending ? "security" : "general",
                  ),
                )
              }
            >
              <Settings2 />
              设置
            </Button>
            <Button
              variant="ghost"
              size="icon"
              aria-label="锁定保险库"
              title="锁定保险库"
              className="text-muted-foreground"
              onClick={lock}
            >
              <LockKeyhole />
            </Button>
          </div>
        </div>
      </aside>
      <main className="flex min-w-0 flex-1 flex-col bg-background">
        <div className="flex h-[76px] shrink-0 items-center justify-between gap-3 border-b px-6">
          <div>
            <div className="flex items-center gap-2">
              <h1 className="text-lg font-semibold tracking-tight">
                {selectedTable?.name ?? "全部记录"}
              </h1>
              <Badge
                variant="secondary"
                className="h-5 px-1.5 text-[10px] font-normal"
              >
                {filtered.length}
              </Badge>
            </div>
            <p className="mt-1.5 text-[11px] text-muted-foreground">
              {selectedTable
                ? "一张表，收好这一类重要信息。"
                : "所有重要信息，都有自己的位置。"}
            </p>
          </div>
          <div className="flex gap-2">
            <Button
              variant="outline"
              className="text-xs"
              disabled={
                readOnly || ordering || !snapshot.tables.length || store.busy
              }
              onClick={() => store.navigate(() => setImportOpen(true))}
            >
              <Upload />
              批量导入
            </Button>
            <Button
              variant="outline"
              disabled={
                !snapshot.config ||
                store.sync.state === "syncing" ||
                snapshot.recoveryPending
              }
              onClick={() => void syncNow()}
              className="text-xs"
            >
              <RefreshCw
                className={store.sync.state === "syncing" ? "animate-spin" : ""}
              />
              同步
            </Button>
            <Button
              disabled={readOnly || ordering || !snapshot.tables.length}
              onClick={store.newEntry}
              className="text-xs"
            >
              <Plus />
              新建记录
            </Button>
          </div>
        </div>
        {(readOnly || store.sync.state === "keyChanged") && (
          <div className="flex shrink-0 items-center gap-2 border-b bg-amber-500/8 px-6 py-2 text-xs text-amber-700 dark:text-amber-400">
            <TriangleAlert className="size-3.5" />
            <span className="flex-1">
              {snapshot.recoveryPending
                ? "密码变更结果待核验，保险库暂时只读。"
                : store.sync.state === "keyChanged"
                  ? "云端主密码已修改，本地离线记录已保留。"
                  : `${snapshot.conflicts.length} 项同步冲突，双方内容均已保留。`}
            </span>
            <Button
              size="xs"
              variant="ghost"
              onClick={() =>
                store.navigate(() =>
                  snapshot.conflicts.length
                    ? setConflicts(true)
                    : setSettings("security"),
                )
              }
            >
              {snapshot.conflicts.length ? "处理冲突" : "恢复 / 迁移"}
              <ChevronRight />
            </Button>
          </div>
        )}
        <ResizablePanelGroup
          orientation="horizontal"
          className="min-h-0 flex-1"
        >
          <ResizablePanel defaultSize="58%" minSize="32%">
            <div className="flex h-full min-w-0 flex-col">
              <div className="flex h-14 shrink-0 items-center gap-2 border-b px-5">
                <Search className="size-3.5 text-muted-foreground" />
                <Input
                  aria-label="搜索记录"
                  placeholder="搜索 Key、Value 或备注…"
                  value={store.query}
                  disabled={ordering}
                  onChange={(e) => store.setQuery(e.target.value)}
                  className="h-8 border-0 bg-transparent px-0 text-xs shadow-none focus-visible:ring-0 dark:bg-transparent"
                />
                {store.query && (
                  <Button
                    variant="ghost"
                    size="icon-xs"
                    aria-label="清除搜索"
                    onClick={() => store.setQuery("")}
                  >
                    <X />
                  </Button>
                )}
                <div className="ml-1 flex shrink-0 items-center gap-1.5 border-l pl-3">
                  <Switch
                    id="list-values-visible"
                    aria-label="显示全部 Value"
                    checked={store.listValuesVisible}
                    onCheckedChange={store.toggleValues}
                  />
                  <Label
                    htmlFor="list-values-visible"
                    className="whitespace-nowrap text-[10px] text-muted-foreground"
                  >
                    显示值
                  </Label>
                </div>
                <Button
                  variant="ghost"
                  size="icon-sm"
                  aria-label={
                    sort === "manual"
                      ? "按 Key 排序"
                      : sort === "key"
                        ? "按更新时间排序"
                        : "使用自定义排序"
                  }
                  title={
                    sort === "manual"
                      ? "当前为自定义排序，点击按 Key 排序"
                      : sort === "key"
                        ? "当前按 Key 排序，点击按更新时间排序"
                        : "当前按更新时间排序，点击使用自定义排序"
                  }
                  disabled={ordering}
                  className="ml-1 text-muted-foreground"
                  onClick={() =>
                    setSort((s) =>
                      s === "manual"
                        ? "key"
                        : s === "key"
                          ? "updated"
                          : "manual",
                    )
                  }
                >
                  {sort === "manual" ? (
                    <ListOrdered />
                  ) : sort === "updated" ? (
                    <ArrowUpDown />
                  ) : (
                    <ArrowDownAZ />
                  )}
                </Button>
                <Button
                  variant={ordering ? "default" : "ghost"}
                  size="sm"
                  className="shrink-0 text-xs"
                  aria-label={ordering ? "保存排序" : "编辑排序"}
                  disabled={
                    readOnly ||
                    store.busy ||
                    dragging ||
                    (!ordering && (filtered.length < 2 || Boolean(store.query)))
                  }
                  title={store.query ? "清除搜索后编辑排序" : undefined}
                  onClick={() => {
                    if (ordering)
                      void store
                        .save()
                        .then(() => {
                          if (!useVault.getState().orderDraft)
                            setSort("manual");
                        })
                        .catch((error) => toast.error(errorMessage(error)));
                    else {
                      store.startOrdering(orderedIds);
                      setSort("manual");
                    }
                  }}
                >
                  {ordering ? <Save /> : <Pencil />}
                  {ordering ? "保存排序" : "编辑"}
                </Button>
                {ordering && (
                  <Button
                    variant="ghost"
                    size="icon-xs"
                    aria-label="取消排序"
                    disabled={store.busy || dragging}
                    onClick={store.cancelOrdering}
                  >
                    <X />
                  </Button>
                )}
              </div>
              {!ordering && selection.length === 0 && (
                <div className="grid h-9 shrink-0 grid-cols-[minmax(0,1fr)_100px] items-center border-b bg-muted/25 px-5 text-[10px] font-medium tracking-wide text-muted-foreground">
                  <span>KEY / 备注</span>
                  <span className="text-right">VALUE</span>
                </div>
              )}
              {ordering && (
                <div className="flex h-9 shrink-0 items-center gap-2 border-b bg-accent/40 px-4 text-[11px] text-muted-foreground">
                  <GripVertical className="size-3.5" />
                  拖动左侧手柄调整顺序，完成后点击保存。
                  {store.orderDirty && (
                    <span className="ml-auto text-amber-600 dark:text-amber-400">
                      尚未保存
                    </span>
                  )}
                </div>
              )}
              {selection.length > 0 && !ordering && (
                <div className="flex h-9 shrink-0 items-center gap-1 border-b bg-accent/40 px-4 text-[11px]">
                  <span className="mr-auto" role="status">
                    已选择 {selection.length} 条记录
                  </span>
                  <Button
                    variant="ghost"
                    size="xs"
                    disabled={
                      readOnly || store.busy || snapshot.tables.length < 2
                    }
                    onClick={() => moveEntries()}
                  >
                    <FolderInput />
                    移动
                  </Button>
                  <Button
                    variant="ghost"
                    size="xs"
                    disabled={readOnly || store.busy}
                    onClick={() => deleteEntries()}
                  >
                    <Trash2 />
                    删除
                  </Button>
                  <Button
                    variant="ghost"
                    size="icon-xs"
                    aria-label="取消选择"
                    onClick={() => store.selectEntries([])}
                  >
                    <X />
                  </Button>
                </div>
              )}
              <DragDropProvider
                onDragStart={() => setDragging(true)}
                onDragEnd={(event) => {
                  const { source } = event.operation;
                  const moved = isSortable(source)
                    ? { id: String(source.id), index: source.index }
                    : null;
                  if (!event.canceled && ordering && moved)
                    store.reorderEntry(moved.id, moved.index);
                  setDragging(false);
                }}
              >
                <ScrollArea
                  className="min-h-0 flex-1 outline-none"
                  tabIndex={0}
                  role="group"
                  aria-label="记录列表"
                  onKeyDown={(event) => {
                    if (ordering) return;
                    if (
                      (event.ctrlKey || event.metaKey) &&
                      event.key.toLowerCase() === "a"
                    ) {
                      event.preventDefault();
                      store.selectEntries(orderedIds);
                    } else if (event.key === "Escape") {
                      event.preventDefault();
                      store.selectEntries([]);
                    } else if (
                      event.key === "Delete" &&
                      !readOnly &&
                      !store.busy &&
                      selection.length
                    ) {
                      event.preventDefault();
                      deleteEntries();
                    }
                  }}
                >
                  {filtered.map((entry, index) => (
                    <SortableRecord
                      key={entry.id}
                      id={entry.id}
                      index={index}
                      enabled={ordering}
                      disabled={readOnly || store.busy}
                    >
                      {({ ref, handleRef, isDragSource }) => (
                        <ContextMenu>
                          <ContextMenuTrigger
                            asChild
                            onContextMenu={(event) => {
                              if (ordering) {
                                event.preventDefault();
                                return;
                              }
                              if (!selectedIds.has(entry.id)) {
                                store.selectEntries([entry.id]);
                              }
                            }}
                          >
                            <div
                              ref={ref}
                              className={`record-row flex h-[58px] w-full select-none items-center gap-2 border-b px-4 text-left transition-colors ${isDragSource ? "relative z-10 bg-accent shadow-lg" : selectedIds.has(entry.id) ? "bg-accent/80" : store.selectedEntry === entry.id ? "bg-muted/40" : "hover:bg-muted/40"}`}
                            >
                              {ordering ? (
                                <Button
                                  ref={handleRef}
                                  variant="ghost"
                                  size="icon-sm"
                                  className="shrink-0 cursor-grab touch-none text-muted-foreground active:cursor-grabbing"
                                  aria-label={`拖动排序：${entry.key}`}
                                  title="拖动排序，或按空格后使用方向键调整"
                                  disabled={readOnly || store.busy}
                                >
                                  <GripVertical className="size-4" />
                                </Button>
                              ) : (
                                <Button
                                  variant="ghost"
                                  size="icon-sm"
                                  className={`size-7 shrink-0 rounded-lg border ${selectedIds.has(entry.id) ? "border-foreground/15 bg-background" : "bg-muted/35"}`}
                                  aria-label={`选择记录：${entry.key}`}
                                  aria-pressed={selectedIds.has(entry.id)}
                                  onClick={(event) =>
                                    store.selectEntry(entry, {
                                      additive:
                                        !event.shiftKey ||
                                        event.ctrlKey ||
                                        event.metaKey,
                                      range: event.shiftKey,
                                      orderedIds,
                                    })
                                  }
                                >
                                  {selectedIds.has(entry.id) ? (
                                    <Check className="size-3.5" />
                                  ) : (
                                    <KeyRound
                                      className="size-3.5 text-muted-foreground"
                                      strokeWidth={1.7}
                                    />
                                  )}
                                </Button>
                              )}
                              <button
                                className="flex min-w-0 flex-1 items-center gap-2.5 self-stretch py-2 text-left outline-none focus-visible:ring-2 focus-visible:ring-inset focus-visible:ring-ring"
                                disabled={ordering}
                                onClick={() => store.openEntry(entry)}
                              >
                                <div className="min-w-0 flex-1">
                                  <p className="mono truncate text-xs font-medium">
                                    {entry.key}
                                  </p>
                                  <p className="mt-0.5 truncate text-[10px] text-muted-foreground">
                                    {entry.notes ||
                                      (store.selectedTable
                                        ? "暂无备注"
                                        : (snapshot.tables.find(
                                            (t) => t.id === entry.tableId,
                                          )?.name ?? "待解决的表"))}
                                  </p>
                                </div>
                                <div className="w-24 shrink-0 text-right">
                                  <p
                                    className={`mono truncate text-xs ${store.listValuesVisible ? "text-foreground" : "tracking-[.15em] text-muted-foreground/60"}`}
                                    aria-label={
                                      store.listValuesVisible
                                        ? undefined
                                        : "已隐藏的 Value"
                                    }
                                  >
                                    {store.listValuesVisible
                                      ? entry.value || "（空）"
                                      : "••••••••"}
                                  </p>
                                  <p className="mt-0.5 text-[9px] text-muted-foreground/65">
                                    {date(entry.updatedAt)}
                                  </p>
                                </div>
                              </button>
                              <div className="flex shrink-0 gap-0.5 text-muted-foreground">
                                <Button
                                  variant="ghost"
                                  size="icon-xs"
                                  aria-label={`复制 Key：${entry.key}`}
                                  title="复制 Key"
                                  onClick={() =>
                                    void copyEntry(entry.id, "key")
                                  }
                                >
                                  <KeyRound className="size-3" />
                                </Button>
                                <Button
                                  variant="ghost"
                                  size="icon-xs"
                                  aria-label={`复制 Value：${entry.key}`}
                                  title="复制 Value"
                                  onClick={() =>
                                    void copyEntry(entry.id, "value")
                                  }
                                >
                                  <Copy className="size-3" />
                                </Button>
                              </div>
                            </div>
                          </ContextMenuTrigger>
                          <ContextMenuContent
                            className="w-48"
                            onKeyDown={(event) => event.stopPropagation()}
                          >
                            <ContextMenuLabel>
                              {selection.length > 1
                                ? `已选择 ${selection.length} 条记录`
                                : entry.key}
                            </ContextMenuLabel>
                            <ContextMenuSeparator />
                            {selection.length <= 1 && (
                              <>
                                <ContextMenuItem
                                  disabled={readOnly || store.busy}
                                  onSelect={() => store.openEntry(entry)}
                                >
                                  <Pencil />
                                  编辑记录
                                </ContextMenuItem>
                                <ContextMenuItem
                                  onSelect={() =>
                                    void copyEntry(entry.id, "key")
                                  }
                                >
                                  <KeyRound />
                                  复制 Key
                                </ContextMenuItem>
                                <ContextMenuItem
                                  onSelect={() =>
                                    void copyEntry(entry.id, "value")
                                  }
                                >
                                  <Copy />
                                  复制 Value
                                </ContextMenuItem>
                                <ContextMenuSeparator />
                              </>
                            )}
                            <ContextMenuItem
                              disabled={
                                readOnly ||
                                store.busy ||
                                snapshot.tables.length < 2
                              }
                              onSelect={() => moveEntries()}
                            >
                              <FolderInput />
                              移动到数据表…
                            </ContextMenuItem>
                            <ContextMenuItem
                              variant="destructive"
                              disabled={readOnly || store.busy}
                              onSelect={() => deleteEntries()}
                            >
                              <Trash2 />
                              {selection.length > 1
                                ? "删除所选记录…"
                                : "删除记录…"}
                            </ContextMenuItem>
                            <ContextMenuSeparator />
                            <ContextMenuItem
                              onSelect={() => store.selectEntries(orderedIds)}
                            >
                              全选当前列表
                            </ContextMenuItem>
                            <ContextMenuItem
                              onSelect={() => store.selectEntries([])}
                            >
                              取消选择
                            </ContextMenuItem>
                          </ContextMenuContent>
                        </ContextMenu>
                      )}
                    </SortableRecord>
                  ))}
                  {!filtered.length && (
                    <div className="flex flex-col items-center px-7 py-20 text-center">
                      <div className="mb-4 flex size-12 items-center justify-center rounded-2xl border bg-muted/25">
                        {store.query ? (
                          <Search className="size-5 text-muted-foreground" />
                        ) : (
                          <FileKey2 className="size-5 text-muted-foreground" />
                        )}
                      </div>
                      <p className="text-sm font-medium">
                        {store.query ? "没有找到相关记录" : "这里还很安静"}
                      </p>
                      <p className="mt-2 max-w-[230px] text-xs leading-6 text-muted-foreground">
                        {store.query
                          ? "试试其他关键词，搜索包含所有解锁的内容。"
                          : "保存你的第一条 Key，把重要的信息留在这里。"}
                      </p>
                      {!store.query && (
                        <Button
                          variant="outline"
                          size="sm"
                          className="mt-5 text-xs"
                          disabled={readOnly}
                          onClick={() =>
                            snapshot.tables.length
                              ? store.newEntry()
                              : setTableDialog({ name: "" })
                          }
                        >
                          <Plus />
                          {snapshot.tables.length ? "新建记录" : "新建数据表"}
                        </Button>
                      )}
                    </div>
                  )}
                </ScrollArea>
              </DragDropProvider>
              <div className="flex h-9 shrink-0 items-center justify-between border-t px-5 text-[10px] text-muted-foreground">
                <span>
                  {filtered.length} 条记录
                  {selection.length ? ` · 已选择 ${selection.length} 条` : ""}
                </span>
                <span className="flex items-center gap-1">
                  {store.listValuesVisible ? (
                    <Eye className="size-3" />
                  ) : (
                    <EyeOff className="size-3" />
                  )}
                  {store.listValuesVisible ? "Value 已显示" : "Value 已隐藏"}
                </span>
              </div>
            </div>
          </ResizablePanel>
          <ResizableHandle />
          <ResizablePanel defaultSize="42%" minSize="30%">
            {store.draft ? (
              <section className="flex h-full min-w-0 flex-col">
                <div className="flex h-14 shrink-0 items-center justify-between border-b px-5">
                  <span className="flex items-center gap-2 text-xs font-medium">
                    {store.draft.id ? "记录详情" : "新建记录"}
                    {store.dirty && (
                      <span
                        className="size-1.5 rounded-full bg-amber-500"
                        title="尚未保存"
                      />
                    )}
                  </span>
                  <div className="flex gap-1">
                    {store.draft.id && (
                      <Button
                        variant="ghost"
                        size="icon-sm"
                        aria-label="删除记录"
                        disabled={readOnly || store.busy}
                        className="text-muted-foreground hover:text-destructive"
                        onClick={() => deleteEntries([store.draft!.id!])}
                      >
                        <Trash2 />
                      </Button>
                    )}
                    <Button
                      variant="ghost"
                      size="icon-sm"
                      aria-label="关闭编辑区"
                      onClick={() =>
                        store.navigate(() =>
                          useVault.setState({
                            selectedEntry: null,
                            draft: null,
                            dirty: false,
                            visible: false,
                          }),
                        )
                      }
                    >
                      <X />
                    </Button>
                  </div>
                </div>
                <ScrollArea className="min-h-0 flex-1">
                  <fieldset
                    disabled={readOnly || ordering || store.busy}
                    className="space-y-6 p-5"
                  >
                    <div className="space-y-2">
                      <Label
                        htmlFor="entry-table"
                        className="text-[11px] text-muted-foreground"
                      >
                        所属数据表
                      </Label>
                      <Select
                        value={store.draft.tableId}
                        onValueChange={(tableId) => store.edit({ tableId })}
                      >
                        <SelectTrigger
                          id="entry-table"
                          className="w-full text-xs"
                        >
                          <SelectValue />
                        </SelectTrigger>
                        <SelectContent>
                          {snapshot.tables.map((t) => (
                            <SelectItem key={t.id} value={t.id}>
                              <Table2 className="size-3" />
                              {t.name}
                            </SelectItem>
                          ))}
                        </SelectContent>
                      </Select>
                    </div>
                    <div className="space-y-2">
                      <Label
                        htmlFor="entry-key"
                        className="text-[11px] text-muted-foreground"
                      >
                        Key
                        <span className="text-muted-foreground/50">必填</span>
                      </Label>
                      <Input
                        id="entry-key"
                        autoFocus={!store.draft.id}
                        className="mono h-9 text-xs"
                        placeholder="例如：api.production.token"
                        autoComplete="off"
                        spellCheck={false}
                        value={store.draft.key}
                        onChange={(e) => store.edit({ key: e.target.value })}
                      />
                    </div>
                    <div className="space-y-2">
                      <div className="flex items-center justify-between">
                        <Label
                          htmlFor="entry-value"
                          className="text-[11px] text-muted-foreground"
                        >
                          Value
                        </Label>
                        <div className="flex gap-0.5">
                          {store.draft.id && (
                            <Button
                              type="button"
                              variant="ghost"
                              size="icon-xs"
                              aria-label={
                                store.visible ? "隐藏 Value" : "显示 Value"
                              }
                              onClick={() =>
                                useVault.setState({ visible: !store.visible })
                              }
                            >
                              {store.visible ? <EyeOff /> : <Eye />}
                            </Button>
                          )}
                          <Button
                            type="button"
                            variant="ghost"
                            size="icon-xs"
                            aria-label="复制 Value"
                            disabled={!store.draft.id || store.dirty}
                            title={store.dirty ? "保存后复制" : "复制 Value"}
                            onClick={() => void copy()}
                          >
                            <Copy />
                          </Button>
                        </div>
                      </div>
                      {!store.draft.id || store.visible ? (
                        <Textarea
                          id="entry-value"
                          className="mono min-h-[144px] text-xs leading-6"
                          value={store.draft.value}
                          placeholder="输入内容，支持多行文本"
                          spellCheck={false}
                          onChange={(e) =>
                            store.edit({ value: e.target.value })
                          }
                        />
                      ) : (
                        <button
                          type="button"
                          id="entry-value"
                          className="flex min-h-[144px] w-full flex-col items-center justify-center gap-3 rounded-lg border bg-muted/20 text-muted-foreground hover:bg-muted/40"
                          onClick={() => useVault.setState({ visible: true })}
                        >
                          <span className="mono tracking-[.35em]">
                            ••••••••••••
                          </span>
                          <span className="flex items-center gap-1.5 text-[10px]">
                            <Eye className="size-3" />
                            点击查看或编辑
                          </span>
                        </button>
                      )}
                    </div>
                    <div className="space-y-2">
                      <Label
                        htmlFor="entry-notes"
                        className="text-[11px] text-muted-foreground"
                      >
                        备注
                        <span className="text-muted-foreground/50">可选</span>
                      </Label>
                      <Textarea
                        id="entry-notes"
                        className="min-h-[96px] text-xs leading-6"
                        placeholder="添加使用说明、用途或其他备注…"
                        value={store.draft.notes}
                        onChange={(e) => store.edit({ notes: e.target.value })}
                      />
                    </div>
                    {store.draft.id && (
                      <div className="space-y-2 border-t pt-4 text-[10px] text-muted-foreground/75">
                        <p className="flex justify-between">
                          <span>创建时间</span>
                          <span>
                            {date(
                              snapshot.entries.find(
                                (e) => e.id === store.draft?.id,
                              )?.createdAt ?? 0,
                            )}
                          </span>
                        </p>
                        <p className="flex justify-between">
                          <span>更新时间</span>
                          <span>
                            {date(
                              snapshot.entries.find(
                                (e) => e.id === store.draft?.id,
                              )?.updatedAt ?? 0,
                            )}
                          </span>
                        </p>
                      </div>
                    )}
                  </fieldset>
                </ScrollArea>
                <div className="flex h-[60px] shrink-0 items-center justify-between border-t px-5">
                  <span className="flex items-center gap-1.5 text-[10px] text-muted-foreground">
                    {store.dirty ? (
                      <>
                        <span className="size-1.5 rounded-full bg-amber-500" />
                        有未保存修改
                      </>
                    ) : (
                      <>
                        <Check className="size-3" />
                        {store.draft.id ? "已加密保存" : "尚未保存"}
                      </>
                    )}
                  </span>
                  <Button
                    size="sm"
                    disabled={
                      !store.draft.key.trim() ||
                      !store.dirty ||
                      store.busy ||
                      readOnly
                    }
                    onClick={() => void save()}
                  >
                    {store.busy ? (
                      <LoaderCircle className="animate-spin" />
                    ) : (
                      <Save />
                    )}
                    保存
                    <span className="ml-2 text-[9px] opacity-50">
                      {store.platform === "macos" ? "⌘" : "Ctrl"} S
                    </span>
                  </Button>
                </div>
              </section>
            ) : selection.length > 0 ? (
              <div className="flex h-full flex-col items-center justify-center bg-muted/10 px-8 text-center">
                <div className="mb-5 flex size-14 items-center justify-center rounded-2xl border bg-background">
                  <Check className="size-6 text-muted-foreground" />
                </div>
                <p className="text-sm font-medium">
                  已选择 {selection.length} 条记录
                </p>
                <p className="mt-3 text-xs leading-6 text-muted-foreground">
                  右键打开菜单，可批量移动或删除。
                  <br />
                  点击左侧图标增减选择，Shift 点击连续选择。
                </p>
              </div>
            ) : (
              <div className="flex h-full flex-col items-center justify-center bg-muted/10 px-8 pb-12 text-center">
                <div className="relative mb-6 flex size-20 items-center justify-center rounded-[22px] border bg-background shadow-sm">
                  <FileKey2
                    className="size-8 text-muted-foreground/55"
                    strokeWidth={1.2}
                  />
                  <div className="absolute -bottom-2 -right-2 flex size-7 items-center justify-center rounded-full border bg-background">
                    <LockKeyhole className="size-3 text-muted-foreground" />
                  </div>
                </div>
                <p className="text-sm font-medium">给重要信息一个安全的位置</p>
                <p className="mt-3 max-w-[250px] text-xs leading-6 text-muted-foreground">
                  选择一条记录查看详情，
                  <br />
                  或新建记录，开始整理你的 Key。
                </p>
                <div className="mt-8 flex items-center gap-1.5 text-[10px] text-muted-foreground/70">
                  <ShieldCheck className="size-3" />
                  XChaCha20-Poly1305 加密保护
                </div>
              </div>
            )}
          </ResizablePanel>
        </ResizablePanelGroup>
        <footer className="flex h-7 shrink-0 items-center justify-between border-t bg-muted/15 px-5 text-[10px] text-muted-foreground">
          <span className="flex min-w-0 items-center gap-1.5">
            {store.sync.state === "syncing" ? (
              <LoaderCircle className="size-3 animate-spin" />
            ) : store.sync.state === "error" ||
              store.sync.state === "offline" ? (
              <CloudOff className="size-3" />
            ) : (
              <ShieldCheck className="size-3" />
            )}
            <span className="truncate">{store.sync.message}</span>
          </span>
          <span className="ml-4 shrink-0">
            {snapshot.lastSyncedAt
              ? `上次同步 ${date(snapshot.lastSyncedAt)}`
              : "本地优先 · 离线可用"}
          </span>
        </footer>
      </main>
      <SettingsDialog
        open={settings !== null}
        initialTab={settings ?? undefined}
        onOpenChange={(open) => {
          if (!open) setSettings(null);
        }}
      />
      <ConflictsDialog open={conflicts} onOpenChange={setConflicts} />
      {importOpen && <BulkImportDialog onClose={() => setImportOpen(false)} />}
      <Dialog
        open={Boolean(move)}
        onOpenChange={(open) => {
          if (!open && !store.busy) setMove(null);
        }}
      >
        <DialogContent>
          <DialogHeader>
            <DialogTitle>移动 {move?.ids.length ?? 0} 条记录</DialogTitle>
            <DialogDescription>
              选择目标数据表。若存在重复 Key，整批移动会取消，所有记录保留原位。
            </DialogDescription>
          </DialogHeader>
          <Label htmlFor="move-target">目标数据表</Label>
          <Select
            value={move?.tableId ?? ""}
            disabled={store.busy}
            onValueChange={(tableId) =>
              setMove((current) => (current ? { ...current, tableId } : null))
            }
          >
            <SelectTrigger id="move-target" className="w-full">
              <SelectValue />
            </SelectTrigger>
            <SelectContent>
              {snapshot.tables
                .filter((table) =>
                  snapshot.entries.some(
                    (entry) =>
                      move?.ids.includes(entry.id) &&
                      entry.tableId !== table.id,
                  ),
                )
                .map((table) => (
                  <SelectItem key={table.id} value={table.id}>
                    {table.name}
                  </SelectItem>
                ))}
            </SelectContent>
          </Select>
          <DialogFooter>
            <Button
              variant="outline"
              disabled={store.busy}
              onClick={() => setMove(null)}
            >
              取消
            </Button>
            <Button
              disabled={
                readOnly ||
                store.busy ||
                !move?.tableId ||
                !snapshot.tables.some((table) => table.id === move?.tableId)
              }
              onClick={() => {
                if (move)
                  void mutate({
                    type: "moveEntries",
                    ids: move.ids,
                    tableId: move.tableId,
                  }).then((ok) => {
                    if (ok) {
                      setMove(null);
                      toast.success("记录已移动并加密保存");
                    }
                  });
              }}
            >
              {store.busy && <LoaderCircle className="animate-spin" />}确认移动
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>
      <Dialog
        open={Boolean(tableDialog)}
        onOpenChange={(open) => {
          if (!open) setTableDialog(null);
        }}
      >
        <DialogContent>
          <DialogHeader>
            <DialogTitle>
              {tableDialog?.id ? "重命名数据表" : "新建数据表"}
            </DialogTitle>
            <DialogDescription>
              用不同的表分类整理你的 Key，表名同样加密保存。
            </DialogDescription>
          </DialogHeader>
          <form
            onSubmit={(e) => {
              e.preventDefault();
              if (tableDialog)
                void mutate(
                  tableDialog.id
                    ? {
                        type: "renameTable",
                        id: tableDialog.id,
                        name: tableDialog.name,
                      }
                    : { type: "createTable", name: tableDialog.name },
                ).then((ok) => {
                  if (ok) setTableDialog(null);
                });
            }}
          >
            <Input
              aria-label="表名称"
              autoFocus
              placeholder="例如：工作、个人、开发配置"
              value={tableDialog?.name ?? ""}
              disabled={store.busy}
              onChange={(e) =>
                setTableDialog((t) =>
                  t ? { ...t, name: e.target.value } : null,
                )
              }
            />
            <DialogFooter className="mt-4">
              <Button
                type="button"
                variant="outline"
                onClick={() => setTableDialog(null)}
              >
                取消
              </Button>
              <Button
                type="submit"
                disabled={!tableDialog?.name.trim() || store.busy}
              >
                保存
              </Button>
            </DialogFooter>
          </form>
        </DialogContent>
      </Dialog>
      <AlertDialog
        open={Boolean(deletion)}
        onOpenChange={(open) => {
          if (!open) setDeletion(null);
        }}
      >
        <AlertDialogContent>
          <AlertDialogHeader>
            <AlertDialogTitle>{deletion?.title}</AlertDialogTitle>
            <AlertDialogDescription>{deletion?.detail}</AlertDialogDescription>
          </AlertDialogHeader>
          <AlertDialogFooter>
            <AlertDialogCancel>取消</AlertDialogCancel>
            <AlertDialogAction
              disabled={store.busy || readOnly}
              className="bg-destructive text-white hover:bg-destructive/90"
              onClick={(e) => {
                e.preventDefault();
                if (deletion)
                  void mutate(deletion.action).then((ok) => {
                    if (ok) setDeletion(null);
                  });
              }}
            >
              确认删除
            </AlertDialogAction>
          </AlertDialogFooter>
        </AlertDialogContent>
      </AlertDialog>
    </div>
  );
}

function SortableRecord({
  enabled,
  ...props
}: SortableRecordProps & { enabled: boolean }) {
  return enabled ? (
    <ActiveSortableRecord {...props} />
  ) : (
    props.children({ ref: () => {}, handleRef: () => {}, isDragSource: false })
  );
}

type SortableRecordProps = {
  id: string;
  index: number;
  disabled: boolean;
  children: (
    sortable: Pick<
      ReturnType<typeof useSortable>,
      "ref" | "handleRef" | "isDragSource"
    >,
  ) => ReactNode;
};

function ActiveSortableRecord({
  id,
  index,
  disabled,
  children,
}: SortableRecordProps) {
  const sortable = useSortable({ id, index, disabled });
  return children(sortable);
}
