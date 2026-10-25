import { create } from "zustand";
import { invoke, isTauri } from "@tauri-apps/api/core";
import { compareEntryOrder } from "@/lib/types";
import type {
  Action,
  AutoLockSettings,
  Bootstrap,
  Draft,
  Entry,
  Snapshot,
  SyncStatus,
  SystemUnlock,
} from "@/lib/types";

export const desktop = isTauri();
export const toDraft = (e: Entry): Draft => ({
  id: e.id,
  tableId: e.tableId,
  key: e.key,
  value: e.value,
  notes: e.notes,
});
interface SelectionModifiers {
  additive?: boolean;
  range?: boolean;
  orderedIds: string[];
}
interface VaultStore {
  initialized: boolean;
  exists: boolean;
  platform: string;
  snapshot: Snapshot | null;
  // Only the non-sensitive lock-screen hint survives locking, in memory only.
  autoLockHint: AutoLockSettings | null;
  systemUnlock: SystemUnlock | null;
  lifecycle: number;
  selectedTable: string | null;
  selectedEntry: string | null;
  selectedIds: string[];
  selectionAnchor: string | null;
  draft: Draft | null;
  dirty: boolean;
  orderDraft: {
    ids: string[];
    originalIds: string[];
    tableId: string | null;
  } | null;
  orderDirty: boolean;
  visible: boolean;
  listValuesVisible: boolean;
  query: string;
  busy: boolean;
  sync: SyncStatus;
  pendingNavigation: (() => void) | null;
  initialize: () => Promise<void>;
  accept: (snapshot: Snapshot, fresh?: boolean) => void;
  clear: () => void;
  navigate: (action: () => void) => void;
  discardAndNavigate: () => void;
  selectTable: (id: string | null) => void;
  selectEntry: (entry: Entry, modifiers?: SelectionModifiers) => void;
  openEntry: (entry: Entry) => void;
  selectEntries: (ids: string[], anchor?: string | null) => void;
  setQuery: (query: string) => void;
  startOrdering: (ids: string[]) => void;
  reorderEntry: (id: string, index: number) => void;
  cancelOrdering: () => void;
  newEntry: () => void;
  edit: (patch: Partial<Draft>) => void;
  save: () => Promise<void>;
  mutate: (action: Action) => Promise<Snapshot | null>;
  setSync: (status: SyncStatus) => void;
  toggleValues: () => void;
}
const localSync: SyncStatus = {
  state: "local",
  message: "仅保存在本地",
  lastSyncedAt: null,
};
export const useVault = create<VaultStore>((set, get) => ({
  initialized: false,
  exists: false,
  platform: "",
  snapshot: null,
  autoLockHint: null,
  systemUnlock: null,
  lifecycle: 0,
  selectedTable: null,
  selectedEntry: null,
  selectedIds: [],
  selectionAnchor: null,
  draft: null,
  dirty: false,
  orderDraft: null,
  orderDirty: false,
  visible: false,
  listValuesVisible: false,
  query: "",
  busy: false,
  sync: localSync,
  pendingNavigation: null,
  initialize: async () => {
    if (!desktop) {
      set({ initialized: true });
      return;
    }
    const boot = await invoke<Bootstrap>("bootstrap");
    set((state) => ({
      initialized: true,
      exists: boot.exists,
      platform: boot.platform,
      autoLockHint: state.snapshot?.autoLock ?? boot.preferences.autoLock,
      systemUnlock: state.snapshot?.systemUnlock ?? boot.systemUnlock,
    }));
  },
  accept: (snapshot, fresh = false) => {
    const current = get();
    if (
      !fresh &&
      (!current.snapshot ||
        current.snapshot.sessionId !== snapshot.sessionId ||
        current.snapshot.revision > snapshot.revision)
    )
      return;
    const autoLockHint = { ...snapshot.autoLock };
    if (fresh) {
      set({
        snapshot,
        autoLockHint,
        systemUnlock: snapshot.systemUnlock,
        exists: true,
        selectedTable: null,
        selectedEntry: null,
        selectedIds: [],
        selectionAnchor: null,
        draft: null,
        dirty: false,
        orderDraft: null,
        orderDirty: false,
        visible: false,
        listValuesVisible: false,
        query: "",
        busy: false,
        sync: {
          state: snapshot.credentialError
            ? "error"
            : snapshot.recoveryPending
              ? "recovery"
              : snapshot.conflicts.length
                ? "conflict"
                : snapshot.config
                  ? snapshot.dirty
                    ? "pending"
                    : "synced"
                  : "local",
          message:
            snapshot.credentialError ??
            (snapshot.recoveryPending
              ? "密码变更待恢复"
              : snapshot.config
                ? "保险库已解锁"
                : "已保存在本地"),
          lastSyncedAt: snapshot.lastSyncedAt,
        },
      });
      return;
    }
    const selected = snapshot.entries.find(
      (e) => e.id === current.selectedEntry,
    );
    const visibleIds = new Set(
      snapshot.entries
        .filter(
          (entry) =>
            (!current.selectedTable ||
              entry.tableId === current.selectedTable) &&
            (!current.query ||
              [entry.key, entry.value, entry.notes].some((text) =>
                text
                  .toLocaleLowerCase()
                  .includes(current.query.toLocaleLowerCase()),
              )),
        )
        .map((entry) => entry.id),
    );
    const selectedIds = current.selectedIds.filter((id) => visibleIds.has(id));
    let orderDraft = current.orderDraft;
    if (orderDraft) {
      const originalIds = snapshot.entries
        .filter(
          (entry) =>
            !orderDraft!.tableId || entry.tableId === orderDraft!.tableId,
        )
        .sort(compareEntryOrder)
        .map((entry) => entry.id);
      const scope = new Set(originalIds);
      const retained = new Set(orderDraft.ids);
      const ids = [
        ...orderDraft.ids.filter((id) => scope.has(id)),
        ...originalIds.filter((id) => !retained.has(id)),
      ];
      orderDraft = { ...orderDraft, ids, originalIds };
    }
    set({
      snapshot,
      autoLockHint,
      systemUnlock: snapshot.systemUnlock,
      orderDraft,
      orderDirty: orderDraft
        ? JSON.stringify(orderDraft.ids) !==
          JSON.stringify(orderDraft.originalIds)
        : false,
      selectedIds,
      selectionAnchor: selectedIds.includes(current.selectionAnchor ?? "")
        ? current.selectionAnchor
        : null,
      ...(current.dirty
        ? current.draft
          ? {
              draft: {
                ...current.draft,
                id: current.draft.id && !selected ? null : current.draft.id,
                tableId: snapshot.tables.some(
                  (table) => table.id === current.draft?.tableId,
                )
                  ? current.draft.tableId
                  : (snapshot.tables[0]?.id ?? ""),
              },
              selectedEntry: selected?.id ?? null,
            }
          : {}
        : {
            draft: selected ? toDraft(selected) : null,
            selectedEntry: selected?.id ?? null,
          }),
      ...(current.selectedTable &&
      !snapshot.tables.some((t) => t.id === current.selectedTable)
        ? { selectedTable: null }
        : {}),
    });
  },
  clear: () =>
    set((s) => ({
      snapshot: null,
      draft: null,
      selectedEntry: null,
      selectedIds: [],
      selectionAnchor: null,
      selectedTable: null,
      dirty: false,
      orderDraft: null,
      orderDirty: false,
      visible: false,
      listValuesVisible: false,
      query: "",
      busy: false,
      pendingNavigation: null,
      lifecycle: s.lifecycle + 1,
      sync: { state: "locked", message: "保险库已锁定", lastSyncedAt: null },
    })),
  navigate: (action) => {
    if (get().dirty || get().orderDirty) set({ pendingNavigation: action });
    else {
      set({ orderDraft: null, orderDirty: false });
      action();
    }
  },
  discardAndNavigate: () => {
    const state = get();
    const action = state.pendingNavigation;
    const original = state.snapshot?.entries.find(
      (entry) => entry.id === state.draft?.id,
    );
    set({
      pendingNavigation: null,
      dirty: false,
      orderDraft: null,
      orderDirty: false,
      draft: original ? toDraft(original) : null,
    });
    action?.();
  },
  selectTable: (id) =>
    get().navigate(() =>
      set({
        selectedTable: id,
        selectedEntry: null,
        selectedIds: [],
        selectionAnchor: null,
        draft: null,
        dirty: false,
        visible: false,
      }),
    ),
  selectEntry: (entry, modifiers) => {
    const state = get();
    let ids = [entry.id];
    let anchor = entry.id;
    if (modifiers?.range) {
      const ordered = modifiers.orderedIds;
      const start = ordered.indexOf(state.selectionAnchor ?? entry.id);
      const end = ordered.indexOf(entry.id);
      if (start >= 0 && end >= 0) {
        const range = ordered.slice(
          Math.min(start, end),
          Math.max(start, end) + 1,
        );
        ids = modifiers.additive
          ? [...new Set([...state.selectedIds, ...range])]
          : range;
        anchor = state.selectionAnchor ?? entry.id;
      }
    } else if (modifiers?.additive) {
      ids = state.selectedIds.includes(entry.id)
        ? state.selectedIds.filter((id) => id !== entry.id)
        : [...state.selectedIds, entry.id];
    }
    state.selectEntries(ids, anchor);
  },
  openEntry: (entry) => {
    if (get().draft?.id === entry.id) return;
    get().navigate(() => {
      const latest = get().snapshot?.entries.find(
        (current) => current.id === entry.id,
      );
      if (latest)
        set({
          selectedEntry: latest.id,
          draft: toDraft(latest),
          dirty: false,
          visible: get().listValuesVisible,
        });
    });
  },
  selectEntries: (ids, anchor = ids[0] ?? null) => {
    const state = get();
    if (state.orderDraft) return;
    const entries = state.snapshot?.entries ?? [];
    const byId = new Map(entries.map((entry) => [entry.id, entry]));
    const selectedIds = [...new Set(ids)].filter((id) => byId.has(id));
    set({
      selectedIds,
      selectionAnchor: anchor,
    });
  },
  setQuery: (query) => set({ query, selectedIds: [], selectionAnchor: null }),
  startOrdering: (ids) =>
    get().navigate(() => {
      const state = get();
      const originalIds = (state.snapshot?.entries ?? [])
        .filter(
          (entry) =>
            !state.selectedTable || entry.tableId === state.selectedTable,
        )
        .sort(compareEntryOrder)
        .map((entry) => entry.id);
      const scope = new Set(originalIds);
      const provided = new Set(ids);
      const ordered = [
        ...ids.filter((id) => scope.has(id)),
        ...originalIds.filter((id) => !provided.has(id)),
      ];
      set({
        orderDraft: { ids: ordered, originalIds, tableId: state.selectedTable },
        orderDirty: JSON.stringify(ordered) !== JSON.stringify(originalIds),
        selectedIds: [],
        selectionAnchor: null,
      });
    }),
  reorderEntry: (id, index) =>
    set((state) => {
      if (!state.orderDraft || state.busy) return {};
      const ids = [...state.orderDraft.ids];
      const previous = ids.indexOf(id);
      if (previous < 0 || index < 0 || index >= ids.length) return {};
      ids.splice(previous, 1);
      ids.splice(index, 0, id);
      return {
        orderDraft: { ...state.orderDraft, ids },
        orderDirty:
          JSON.stringify(ids) !== JSON.stringify(state.orderDraft.originalIds),
      };
    }),
  cancelOrdering: () => set({ orderDraft: null, orderDirty: false }),
  newEntry: () => {
    const state = get();
    const tableId = state.selectedTable ?? state.snapshot?.tables[0]?.id;
    if (!tableId) return;
    state.navigate(() =>
      set({
        selectedEntry: null,
        selectedIds: [],
        selectionAnchor: null,
        draft: { id: null, tableId, key: "", value: "", notes: "" },
        dirty: false,
        visible: true,
      }),
    );
  },
  edit: (patch) =>
    set((state) => {
      if (!state.draft || state.orderDraft) return {};
      const draft = { ...state.draft, ...patch };
      const original = state.snapshot?.entries.find((e) => e.id === draft.id);
      const dirty = original
        ? JSON.stringify(draft) !== JSON.stringify(toDraft(original))
        : Boolean(draft.key || draft.value || draft.notes);
      return { draft, dirty };
    }),
  mutate: async (action) => {
    const state = get();
    if (!state.snapshot || state.busy) return null;
    const sessionId = state.snapshot.sessionId;
    const lifecycle = state.lifecycle;
    set({ busy: true });
    try {
      const snapshot = await invoke<Snapshot>("mutate_vault", {
        sessionId,
        action,
      });
      if (
        get().lifecycle !== lifecycle ||
        get().snapshot?.sessionId !== sessionId
      )
        return null;
      get().accept(snapshot);
      return snapshot;
    } finally {
      if (get().lifecycle === lifecycle) set({ busy: false });
    }
  },
  save: async () => {
    const ordering = get();
    if (ordering.orderDraft) {
      if (!ordering.orderDirty) {
        set({ orderDraft: null, orderDirty: false });
        return;
      }
      const result = await get().mutate({
        type: "reorderEntries",
        ids: ordering.orderDraft.ids,
        tableId: ordering.orderDraft.tableId,
      });
      if (result && get().lifecycle === ordering.lifecycle)
        set({ orderDraft: null, orderDirty: false });
      return;
    }
    const { draft, lifecycle } = get();
    if (!draft) return;
    const snapshot = await get().mutate({
      type: "saveEntry",
      input: { ...draft, key: draft.key.trim() },
    });
    if (!snapshot || get().lifecycle !== lifecycle) return;
    const entry = snapshot.entries.find((e) =>
      draft.id
        ? e.id === draft.id
        : e.tableId === draft.tableId && e.key === draft.key.trim(),
    );
    if (entry)
      set({
        draft: toDraft(entry),
        selectedEntry: entry.id,
        dirty: false,
        selectedTable: get().selectedTable ? entry.tableId : null,
      });
  },
  setSync: (sync) => {
    if (get().snapshot) set({ sync });
  },
  toggleValues: () =>
    set((state) => ({
      listValuesVisible: !state.listValuesVisible,
      visible: !state.listValuesVisible,
    })),
}));
