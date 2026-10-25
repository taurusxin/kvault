export interface Table {
  id: string;
  name: string;
  createdAt: number;
  updatedAt: number;
  deleted: boolean;
}
export interface Entry {
  id: string;
  tableId: string;
  key: string;
  value: string;
  notes: string;
  sortOrder?: number;
  createdAt: number;
  updatedAt: number;
  deleted: boolean;
}
export const compareEntryOrder = (a: Entry, b: Entry) =>
  (a.sortOrder ?? 0) - (b.sortOrder ?? 0) ||
  b.updatedAt - a.updatedAt ||
  (a.id < b.id ? -1 : a.id > b.id ? 1 : 0);
export type Entity =
  { kind: "table"; value: Table } | { kind: "entry"; value: Entry };
export interface Conflict {
  id: string;
  reason: string;
  local: Entity | null;
  remote: Entity | null;
  localEntries: Entry[];
  remoteEntries: Entry[];
}
export type SyncConfig =
  | {
      type: "s3";
      endpoint: string;
      region: string;
      bucket: string;
      accessKey: string;
      secretKey: string;
      sessionToken: string | null;
      prefix: string;
      pathStyle: boolean;
    }
  | {
      type: "webdav";
      endpoint: string;
      directory: string;
      username: string;
      password: string;
    };
export interface Snapshot {
  sessionId: number;
  vaultId: string;
  epoch: number;
  tables: Table[];
  entries: Entry[];
  conflicts: Conflict[];
  config: SyncConfig | null;
  revision: number;
  lastSyncedAt: number | null;
  dirty: boolean;
  recoveryPending: boolean;
}
export interface Bootstrap {
  exists: boolean;
  recoveryPending: boolean;
  platform: string;
}
export interface SyncStatus {
  state: string;
  message: string;
  lastSyncedAt: number | null;
}
export interface Draft {
  id: string | null;
  tableId: string;
  key: string;
  value: string;
  notes: string;
}
export type Action =
  | { type: "createTable"; name: string }
  | { type: "renameTable"; id: string; name: string }
  | { type: "deleteTable"; id: string }
  | { type: "saveEntry"; input: Draft }
  | { type: "deleteEntry"; id: string }
  | { type: "deleteEntries"; ids: string[] }
  | { type: "moveEntries"; ids: string[]; tableId: string }
  | { type: "reorderEntries"; ids: string[]; tableId: string | null }
  | {
      type: "importEntries";
      tableId: string;
      rows: ImportRow[];
      duplicatePolicy: "skip" | "overwrite";
    }
  | { type: "setSync"; config: SyncConfig | null }
  | {
      type: "resolveConflict";
      id: string;
      choice: "local" | "remote" | "both";
      rename: string;
    };
export interface AppError {
  code: string;
  message: string;
}
export interface ImportRow {
  line: number;
  key: string;
  value: string;
  notes: string;
}
export interface ImportPreview {
  rows: ImportRow[];
  errors: { line: number; message: string }[];
  hasHeader: boolean;
  delimiter: "comma" | "space";
}
export type ImportDelimiter = "auto" | "comma" | "space";
export type ImportHeader = "auto" | "present" | "none";
export function errorMessage(error: unknown): string {
  if (typeof error === "object" && error !== null && "message" in error)
    return String(error.message);
  return typeof error === "string" ? error : "操作失败，请重试";
}
export const emptyS3 = (): SyncConfig => ({
  type: "s3",
  endpoint: "",
  region: "us-east-1",
  bucket: "",
  accessKey: "",
  secretKey: "",
  sessionToken: null,
  prefix: "kvault",
  pathStyle: true,
});
export const emptyWebdav = (): SyncConfig => ({
  type: "webdav",
  endpoint: "",
  directory: "kvault",
  username: "",
  password: "",
});
