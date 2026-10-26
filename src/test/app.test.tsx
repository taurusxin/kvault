import {
  act,
  fireEvent,
  render,
  screen,
  waitFor,
  within,
} from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import App from "@/App";
import { useVault } from "@/stores/vault";
import type { Snapshot } from "@/lib/types";

vi.mock("@tauri-apps/api/core", () => ({
  isTauri: () => true,
  invoke: vi.fn(),
}));
vi.mock("@tauri-apps/api/event", () => ({
  listen: vi.fn(async () => () => {}),
}));
const windowMock = vi.hoisted(() => ({
  isMaximized: async () => false,
  onResized: async () => () => {},
  close: vi.fn(async () => {}),
  minimize: vi.fn(),
  toggleMaximize: vi.fn(),
}));
vi.mock("@tauri-apps/api/window", () => ({
  getCurrentWindow: () => windowMock,
}));

const fixture = (): Snapshot => ({
  sessionId: 1,
  vaultId: "vault",
  epoch: 1,
  revision: 1,
  dirty: false,
  lastSyncedAt: null,
  config: null,
  conflicts: [],
  recoveryPending: false,
  autoLock: { enabled: true, timeoutMinutes: 5 },
  systemUnlock: {
    available: true,
    enabled: false,
    label: "Touch ID / 系统验证",
    reason: null,
  },
  credentialError: null,
  tables: [
    { id: "table", name: "工作", deleted: false, createdAt: 1, updatedAt: 1 },
  ],
  entries: [
    {
      id: "entry",
      tableId: "table",
      key: "api.production.token",
      value: "private-value-123",
      notes: "生产接口",
      createdAt: 1,
      updatedAt: 1,
      deleted: false,
    },
  ],
});

const bootFixture = () => ({
  exists: true,
  recoveryPending: false,
  platform: "macos",
  preferences: {
    version: 1,
    autoLock: { enabled: true, timeoutMinutes: 5 },
    theme: "system",
    systemUnlock: null,
    close: { minimizeToTray: true, askBeforeClose: true },
  },
  systemUnlock: fixture().systemUnlock,
});

const selectionFixture = (): Snapshot => {
  const snapshot = fixture();
  snapshot.tables.push({
    id: "target",
    name: "归档",
    deleted: false,
    createdAt: 1,
    updatedAt: 1,
  });
  snapshot.entries = ["alpha", "beta", "gamma", "delta"].map((key, index) => ({
    ...snapshot.entries[0],
    id: key,
    key,
    updatedAt: index + 1,
  }));
  return snapshot;
};

beforeEach(() => {
  vi.clearAllMocks();
  localStorage.clear();
  useVault.getState().clear();
  useVault.setState({
    initialized: true,
    exists: true,
    platform: "macos",
    autoLockHint: null,
    systemUnlock: null,
    closeSettings: { minimizeToTray: true, askBeforeClose: true },
  });
  vi.mocked(invoke).mockImplementation(async (command, args) =>
    command === "bootstrap"
      ? {
          exists: true,
          recoveryPending: false,
          platform: "macos",
          preferences: {
            autoLock: { enabled: true, timeoutMinutes: 5 },
            theme: "system",
          },
          systemUnlock: fixture().systemUnlock,
        }
      : command === "set_close_settings"
        ? (args as { settings: unknown }).settings
        : undefined,
  );
});

function requestClose(event = "app:close-requested") {
  const registrations = vi
    .mocked(listen)
    .mock.calls.filter(([name]) => name === event);
  const registration = registrations[registrations.length - 1];
  expect(registration).toBeDefined();
  act(() => registration![1]({ event, id: 1, payload: null }));
}

describe("保险库界面与会话", () => {
  it("系统解锁默认关闭，主密码入口始终存在且不会自动触发系统验证", async () => {
    render(<App />);
    expect(await screen.findByLabelText("主密码")).toBeInTheDocument();
    expect(
      screen.queryByRole("button", { name: /使用.*系统验证.*解锁/ }),
    ).not.toBeInTheDocument();
    expect(invoke).not.toHaveBeenCalledWith("unlock_with_system");
  });

  it("系统验证取消后可直接改用主密码", async () => {
    vi.mocked(invoke).mockImplementation(async (command) => {
      if (command === "bootstrap")
        return {
          ...bootFixture(),
          systemUnlock: { ...fixture().systemUnlock, enabled: true },
        };
      if (command === "unlock_with_system")
        throw {
          code: "auth_cancelled",
          message: "已取消系统验证，可使用主密码解锁",
        };
      if (command === "unlock_vault") return fixture();
    });
    render(<App />);
    fireEvent.click(
      await screen.findByRole("button", { name: /使用.*系统验证.*解锁/ }),
    );
    expect(await screen.findByRole("alert")).toHaveTextContent(
      "已取消系统验证",
    );
    const input = screen.getByLabelText("主密码");
    expect(input).toBeEnabled();
    fireEvent.change(input, { target: { value: "master-password" } });
    fireEvent.click(screen.getByRole("button", { name: "解锁保险库" }));
    await waitFor(() => expect(useVault.getState().snapshot).not.toBeNull());
    expect(invoke).toHaveBeenCalledWith("unlock_vault", {
      password: "master-password",
    });
  });

  it("锁定后忽略未完成系统验证的返回值", async () => {
    let finish!: (snapshot: Snapshot) => void;
    vi.mocked(invoke).mockImplementation(async (command) => {
      if (command === "bootstrap")
        return {
          ...bootFixture(),
          systemUnlock: { ...fixture().systemUnlock, enabled: true },
        };
      if (command === "unlock_with_system")
        return new Promise<Snapshot>((resolve) => {
          finish = resolve;
        });
    });
    render(<App />);
    fireEvent.click(
      await screen.findByRole("button", { name: /使用.*系统验证.*解锁/ }),
    );
    await act(async () => {
      useVault.getState().clear();
      finish(fixture());
    });
    expect(useVault.getState().snapshot).toBeNull();
    expect(screen.getByLabelText("主密码")).toBeInTheDocument();
  });

  it("系统解锁在安全设置中显式启用并确认主密码，也可以关闭", async () => {
    const snapshot = fixture();
    let revision = snapshot.revision;
    useVault.getState().accept(snapshot, true);
    vi.mocked(invoke).mockImplementation(async (command, args) => {
      if (command === "bootstrap") return bootFixture();
      if (command === "set_system_unlock")
        return {
          ...snapshot,
          revision: ++revision,
          systemUnlock: {
            ...snapshot.systemUnlock,
            enabled: (args as { enabled: boolean }).enabled,
          },
        };
    });
    render(<App />);
    fireEvent.click(await screen.findByRole("button", { name: "设置" }));
    fireEvent.mouseDown(screen.getByRole("tab", { name: "安全与备份" }), {
      button: 0,
      ctrlKey: false,
    });
    expect(screen.getByRole("switch", { name: "系统解锁" })).not.toBeChecked();
    fireEvent.click(screen.getByRole("switch", { name: "系统解锁" }));
    const dialog = screen.getByRole("dialog", { name: "启用系统解锁" });
    const password = within(dialog).getByLabelText("当前主密码");
    fireEvent.change(password, { target: { value: "master-password" } });
    act(() =>
      useVault.setState({
        draft: {
          id: "entry",
          tableId: "table",
          key: "api.production.token",
          value: "unsaved-draft",
          notes: "",
        },
        selectedEntry: "entry",
        dirty: true,
      }),
    );
    fireEvent.click(within(dialog).getByRole("button", { name: "启用并验证" }));
    expect(password).toHaveValue("");
    await waitFor(() =>
      expect(screen.getByRole("switch", { name: "系统解锁" })).toBeChecked(),
    );
    expect(invoke).toHaveBeenCalledWith("set_system_unlock", {
      sessionId: 1,
      enabled: true,
      password: "master-password",
    });
    expect(useVault.getState().dirty).toBe(true);
    expect(useVault.getState().draft?.value).toBe("unsaved-draft");
    expect(JSON.stringify(useVault.getState())).not.toContain(
      "master-password",
    );
    fireEvent.click(screen.getByRole("switch", { name: "系统解锁" }));
    await waitFor(() =>
      expect(
        screen.getByRole("switch", { name: "系统解锁" }),
      ).not.toBeChecked(),
    );
    expect(invoke).toHaveBeenCalledWith("set_system_unlock", {
      sessionId: 1,
      enabled: false,
      password: "",
    });
  });

  it("不支持系统验证时禁用开关，仍保留主密码功能", async () => {
    const snapshot = fixture();
    snapshot.systemUnlock = {
      available: false,
      enabled: false,
      label: "系统验证",
      reason: "此构建未配置钥匙串签名授权",
    };
    useVault.getState().accept(snapshot, true);
    render(<App />);
    fireEvent.click(await screen.findByRole("button", { name: "设置" }));
    fireEvent.mouseDown(screen.getByRole("tab", { name: "安全与备份" }), {
      button: 0,
      ctrlKey: false,
    });
    expect(screen.getByRole("switch", { name: "系统解锁" })).toBeDisabled();
    expect(screen.getByText("此构建未配置钥匙串签名授权")).toBeInTheDocument();
  });

  it("外观使用 Rust 配置接口保存，不写入 WebView localStorage", async () => {
    useVault.getState().accept(fixture(), true);
    render(<App />);
    fireEvent.click(await screen.findByRole("button", { name: "设置" }));
    fireEvent.click(screen.getByRole("button", { name: "深色" }));
    await waitFor(() =>
      expect(invoke).toHaveBeenCalledWith("set_theme", { theme: "dark" }),
    );
    expect(document.documentElement).toHaveClass("dark");
    expect(localStorage.getItem("kvault-theme")).toBeNull();
  });
  it("首次启动从本机配置读取自动锁定时长，无需解密", async () => {
    render(<App />);
    expect(
      await screen.findByText("闲置 5 分钟后自动锁定"),
    ).toBeInTheDocument();
    expect(screen.queryByText("解锁后可设置自动锁定")).not.toBeInTheDocument();
  });
  it.each([
    {
      settings: { enabled: true, timeoutMinutes: 15 },
      text: "闲置 15 分钟后自动锁定",
    },
    {
      settings: { enabled: false, timeoutMinutes: 30 },
      text: "自动锁定已关闭",
    },
  ])(
    "锁定页显示本次会话实际的自动锁定设置：$text",
    async ({ settings, text }) => {
      const snapshot = fixture();
      snapshot.autoLock = settings;
      useVault.getState().accept(snapshot, true);
      render(<App />);
      fireEvent.click(
        await screen.findByRole("button", { name: "锁定保险库" }),
      );
      expect(await screen.findByText(text)).toBeInTheDocument();
      expect(useVault.getState().snapshot).toBeNull();
      expect(useVault.getState().draft).toBeNull();
      act(() =>
        useVault.getState().accept({
          ...snapshot,
          autoLock: { enabled: true, timeoutMinutes: 5 },
          revision: 2,
        }),
      );
      expect(screen.getByText(text)).toBeInTheDocument();
      expect(useVault.getState().snapshot).toBeNull();
    },
  );
  it.each([false, true])(
    "云同步入口直接选中云同步页，普通设置入口仍打开通用页（已配置：%s）",
    async (configured) => {
      const snapshot = fixture();
      if (configured)
        snapshot.config = {
          type: "webdav",
          endpoint: "https://example.test",
          directory: "kvault",
          username: "test",
          password: "",
        };
      useVault.getState().accept(snapshot, true);
      render(<App />);
      fireEvent.click(
        await screen.findByRole("button", {
          name: configured ? /^WebDAV 同步/ : /连接云存储/,
        }),
      );
      expect(screen.getByRole("tablist", { name: "设置分类" })).toHaveAttribute(
        "aria-orientation",
        "vertical",
      );
      expect(screen.getByRole("tab", { name: "云同步" })).toHaveAttribute(
        "aria-selected",
        "true",
      );
      expect(screen.getByRole("tab", { name: "通用" })).toHaveAttribute(
        "aria-selected",
        "false",
      );
      fireEvent.click(screen.getByRole("button", { name: "关闭" }));
      fireEvent.click(screen.getByRole("button", { name: "设置" }));
      expect(screen.getByRole("tab", { name: "通用" })).toHaveAttribute(
        "aria-selected",
        "true",
      );
      fireEvent.click(screen.getByRole("button", { name: "关闭" }));
      fireEvent.click(
        screen.getByRole("button", {
          name: configured ? /^WebDAV 同步/ : /连接云存储/,
        }),
      );
      expect(screen.getByRole("tab", { name: "云同步" })).toHaveAttribute(
        "aria-selected",
        "true",
      );
    },
  );
  it("自动锁定可配置时长和开关，保存后重新打开仍显示已保存设置", async () => {
    let saved = fixture();
    useVault.getState().accept(saved, true);
    vi.mocked(invoke).mockImplementation(async (command, args) => {
      if (command === "bootstrap") return bootFixture();
      if (command === "mutate_vault") {
        const action = (
          args as { action: { type: string; settings: Snapshot["autoLock"] } }
        ).action;
        saved = {
          ...saved,
          autoLock: action.settings,
          revision: saved.revision + 1,
        };
        return saved;
      }
    });
    render(<App />);
    fireEvent.click(await screen.findByRole("button", { name: "设置" }));
    expect(screen.getByRole("switch", { name: "自动锁定" })).toBeChecked();
    fireEvent.change(screen.getByRole("spinbutton", { name: "闲置时长" }), {
      target: { value: "30" },
    });
    fireEvent.click(screen.getByRole("button", { name: "保存设置" }));
    await waitFor(() =>
      expect(useVault.getState().snapshot?.autoLock).toEqual({
        enabled: true,
        timeoutMinutes: 30,
      }),
    );
    expect(invoke).toHaveBeenCalledWith("mutate_vault", {
      sessionId: 1,
      action: {
        type: "setAutoLock",
        settings: { enabled: true, timeoutMinutes: 30 },
      },
    });
    await waitFor(() =>
      expect(screen.getByRole("switch", { name: "自动锁定" })).toBeEnabled(),
    );
    fireEvent.click(screen.getByRole("switch", { name: "自动锁定" }));
    expect(screen.getByRole("spinbutton", { name: "闲置时长" })).toBeDisabled();
    fireEvent.click(screen.getByRole("button", { name: "保存设置" }));
    await waitFor(() =>
      expect(useVault.getState().snapshot?.autoLock).toEqual({
        enabled: false,
        timeoutMinutes: 30,
      }),
    );
    await waitFor(() =>
      expect(screen.getByRole("switch", { name: "自动锁定" })).toBeEnabled(),
    );
    fireEvent.click(screen.getByRole("button", { name: "关闭" }));
    fireEvent.click(screen.getByRole("button", { name: "设置" }));
    expect(screen.getByRole("switch", { name: "自动锁定" })).not.toBeChecked();
    expect(screen.getByRole("spinbutton", { name: "闲置时长" })).toHaveValue(
      30,
    );
    expect(screen.getByRole("button", { name: "保存设置" })).toBeDisabled();
  });
  it("自动锁定拒绝无效时长，保存失败时保留原设置和待重试输入", async () => {
    useVault.getState().accept(fixture(), true);
    vi.mocked(invoke).mockImplementation(async (command) => {
      if (command === "bootstrap") return bootFixture();
      if (command === "mutate_vault")
        throw { code: "storage", message: "无法写入保险库" };
    });
    render(<App />);
    fireEvent.click(await screen.findByRole("button", { name: "设置" }));
    for (const value of ["", "0", "1.5", "1441"]) {
      fireEvent.change(screen.getByRole("spinbutton", { name: "闲置时长" }), {
        target: { value },
      });
      expect(screen.getByRole("button", { name: "保存设置" })).toBeDisabled();
    }
    expect(invoke).not.toHaveBeenCalledWith("mutate_vault", expect.anything());
    fireEvent.change(screen.getByRole("spinbutton", { name: "闲置时长" }), {
      target: { value: "15" },
    });
    fireEvent.click(screen.getByRole("button", { name: "保存设置" }));
    expect(await screen.findByRole("alert")).toHaveTextContent(
      "无法写入保险库",
    );
    expect(useVault.getState().snapshot?.autoLock).toEqual({
      enabled: true,
      timeoutMinutes: 5,
    });
    expect(screen.getByRole("spinbutton", { name: "闲置时长" })).toHaveValue(
      15,
    );
    expect(screen.getByRole("button", { name: "保存设置" })).toBeEnabled();
  });
  it("关闭时默认询问，最小化保留草稿并保存不再询问的选择", async () => {
    useVault.getState().accept(fixture(), true);
    render(<App />);
    fireEvent.click(await screen.findByText("api.production.token"));
    fireEvent.change(screen.getByRole("textbox", { name: /Key/ }), {
      target: { value: "unsaved.key" },
    });
    requestClose();
    expect(screen.getByText("关闭 KVault")).toBeInTheDocument();
    fireEvent.click(screen.getByRole("switch", { name: "下次关闭时仍然询问" }));
    fireEvent.click(screen.getByRole("button", { name: "最小化到托盘" }));
    await waitFor(() =>
      expect(invoke).toHaveBeenCalledWith("minimize_to_tray"),
    );
    expect(useVault.getState().closeSettings).toEqual({
      minimizeToTray: true,
      askBeforeClose: false,
    });
    expect(useVault.getState().dirty).toBe(true);
    expect(useVault.getState().draft?.key).toBe("unsaved.key");
    expect(useVault.getState().pendingNavigation).toBeNull();
    expect(invoke).not.toHaveBeenCalledWith("exit_app");
    requestClose();
    await waitFor(() =>
      expect(
        vi
          .mocked(invoke)
          .mock.calls.filter(([command]) => command === "minimize_to_tray"),
      ).toHaveLength(2),
    );
    expect(screen.queryByText("关闭 KVault")).not.toBeInTheDocument();
  });
  it("选择退出时保护草稿，取消后仍可编辑，确认放弃后退出", async () => {
    useVault.getState().accept(fixture(), true);
    render(<App />);
    fireEvent.click(await screen.findByText("api.production.token"));
    fireEvent.change(screen.getByRole("textbox", { name: /Key/ }), {
      target: { value: "unsaved.key" },
    });
    requestClose();
    fireEvent.click(screen.getByRole("button", { name: "退出应用" }));
    await screen.findByText("有尚未保存的修改");
    fireEvent.click(screen.getByRole("button", { name: "继续编辑" }));
    expect(useVault.getState().dirty).toBe(true);
    expect(invoke).not.toHaveBeenCalledWith("exit_app");
    requestClose("app:exit-requested");
    fireEvent.click(screen.getByRole("button", { name: "放弃修改" }));
    await waitFor(() => expect(invoke).toHaveBeenCalledWith("exit_app"));
  });
  it("关闭询问后按配置直接退出，托盘退出不受最小化设置影响", async () => {
    render(<App />);
    await screen.findByLabelText("主密码");
    act(() =>
      useVault.setState({
        closeSettings: { minimizeToTray: false, askBeforeClose: false },
      }),
    );
    requestClose();
    await waitFor(() => expect(invoke).toHaveBeenCalledWith("exit_app"));
    expect(screen.queryByText("关闭 KVault")).not.toBeInTheDocument();
    vi.mocked(invoke).mockClear();
    act(() =>
      useVault.setState({
        closeSettings: { minimizeToTray: true, askBeforeClose: true },
      }),
    );
    requestClose("app:exit-requested");
    await waitFor(() => expect(invoke).toHaveBeenCalledWith("exit_app"));
    expect(invoke).not.toHaveBeenCalledWith("minimize_to_tray");
  });
  it("通用设置中的两项关闭选项立即保存，失败时保留原值", async () => {
    useVault.getState().accept(fixture(), true);
    render(<App />);
    fireEvent.click(await screen.findByRole("button", { name: "设置" }));
    const minimize = screen.getByRole("switch", { name: "关闭时最小化到托盘" });
    fireEvent.click(minimize);
    await waitFor(() => expect(minimize).not.toBeChecked());
    expect(invoke).toHaveBeenCalledWith("set_close_settings", {
      settings: { minimizeToTray: false, askBeforeClose: true },
    });
    vi.mocked(invoke).mockRejectedValueOnce({ message: "无法保存关闭设置" });
    fireEvent.click(screen.getByRole("switch", { name: "关闭时再次询问" }));
    await screen.findByText("无法保存关闭设置");
    expect(useVault.getState().closeSettings.askBeforeClose).toBe(true);
  });
  it("支持 Ctrl/Cmd 增减、Shift 按当前排序连续选择，以及筛选后的全选", async () => {
    useVault.getState().accept(selectionFixture(), true);
    render(<App />);
    fireEvent.click(await screen.findByRole("button", { name: "按 Key 排序" }));
    fireEvent.click(screen.getByRole("button", { name: "选择记录：beta" }));
    fireEvent.click(screen.getByRole("button", { name: "选择记录：gamma" }), {
      ctrlKey: true,
    });
    expect(useVault.getState().selectedIds).toEqual(["beta", "gamma"]);
    fireEvent.click(screen.getByRole("button", { name: "选择记录：beta" }), {
      ctrlKey: true,
    });
    expect(useVault.getState().selectedIds).toEqual(["gamma"]);
    fireEvent.click(screen.getByRole("button", { name: "选择记录：alpha" }), {
      metaKey: true,
    });
    expect(useVault.getState().selectedIds).toEqual(["gamma", "alpha"]);
    fireEvent.click(screen.getByRole("button", { name: "选择记录：gamma" }), {
      shiftKey: true,
    });
    expect(useVault.getState().selectedIds).toEqual([
      "alpha",
      "beta",
      "delta",
      "gamma",
    ]);
    fireEvent.click(screen.getByRole("button", { name: "选择记录：beta" }), {
      shiftKey: true,
    });
    expect(useVault.getState().selectedIds).toEqual(["alpha", "beta"]);
    fireEvent.click(screen.getByRole("button", { name: "选择记录：delta" }), {
      shiftKey: true,
      ctrlKey: true,
    });
    expect(useVault.getState().selectedIds).toEqual(["alpha", "beta", "delta"]);
    fireEvent.keyDown(screen.getByRole("group", { name: "记录列表" }), {
      key: "a",
      metaKey: true,
    });
    expect(useVault.getState().selectedIds).toHaveLength(4);
    fireEvent.change(screen.getByRole("textbox", { name: "搜索记录" }), {
      target: { value: "beta" },
    });
    expect(useVault.getState().selectedIds).toEqual([]);
    fireEvent.keyDown(screen.getByRole("group", { name: "记录列表" }), {
      key: "a",
      ctrlKey: true,
    });
    expect(useVault.getState().selectedIds).toEqual(["beta"]);
    fireEvent.keyDown(screen.getByRole("group", { name: "记录列表" }), {
      key: "Escape",
    });
    expect(useVault.getState().selectedIds).toEqual([]);
  });
  it("右键保留多选，移动一次性提交，右键未选项目改为单选并可复制", async () => {
    const snapshot = selectionFixture();
    useVault.getState().accept(snapshot, true);
    vi.mocked(invoke).mockImplementation(async (command) =>
      command === "mutate_vault"
        ? {
            ...snapshot,
            revision: 2,
            entries: snapshot.entries.map((entry) =>
              ["alpha", "beta"].includes(entry.id)
                ? { ...entry, tableId: "target" }
                : entry,
            ),
          }
        : command === "bootstrap"
          ? bootFixture()
          : undefined,
    );
    render(<App />);
    fireEvent.click(
      await screen.findByRole("button", { name: "选择记录：alpha" }),
    );
    fireEvent.click(screen.getByRole("button", { name: "选择记录：beta" }), {
      ctrlKey: true,
    });
    fireEvent.contextMenu(
      screen.getByRole("button", { name: "选择记录：alpha" }),
    );
    expect(useVault.getState().selectedIds).toEqual(["alpha", "beta"]);
    fireEvent.click(
      await screen.findByRole("menuitem", { name: "移动到数据表…" }),
    );
    expect(
      await screen.findByRole("heading", { name: "移动 2 条记录" }),
    ).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "确认移动" }));
    await waitFor(() =>
      expect(invoke).toHaveBeenCalledWith("mutate_vault", {
        sessionId: 1,
        action: {
          type: "moveEntries",
          ids: ["alpha", "beta"],
          tableId: "target",
        },
      }),
    );
    await waitFor(() =>
      expect(
        screen.queryByRole("heading", { name: "移动 2 条记录" }),
      ).not.toBeInTheDocument(),
    );
    fireEvent.contextMenu(
      screen.getByRole("button", { name: "选择记录：gamma" }),
    );
    expect(useVault.getState().selectedIds).toEqual(["gamma"]);
    fireEvent.click(await screen.findByRole("menuitem", { name: "复制 Key" }));
    await waitFor(() =>
      expect(invoke).toHaveBeenCalledWith("copy_key", {
        sessionId: 1,
        entryId: "gamma",
      }),
    );
  });
  it("多选右键删除显示条数并整批提交，随后清理失效选择", async () => {
    const snapshot = selectionFixture();
    useVault.getState().accept(snapshot, true);
    vi.mocked(invoke).mockImplementation(async (command) =>
      command === "mutate_vault"
        ? {
            ...snapshot,
            revision: 2,
            entries: snapshot.entries.filter(
              (entry) => !["alpha", "beta"].includes(entry.id),
            ),
          }
        : command === "bootstrap"
          ? bootFixture()
          : undefined,
    );
    render(<App />);
    fireEvent.click(
      await screen.findByRole("button", { name: "选择记录：alpha" }),
    );
    fireEvent.click(screen.getByRole("button", { name: "选择记录：beta" }), {
      ctrlKey: true,
    });
    fireEvent.contextMenu(
      screen.getByRole("button", { name: "选择记录：beta" }),
    );
    fireEvent.click(
      await screen.findByRole("menuitem", { name: "删除所选记录…" }),
    );
    expect(
      await screen.findByText("删除选中的 2 条记录？"),
    ).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "确认删除" }));
    await waitFor(() =>
      expect(invoke).toHaveBeenCalledWith("mutate_vault", {
        sessionId: 1,
        action: { type: "deleteEntries", ids: ["alpha", "beta"] },
      }),
    );
    await waitFor(() => expect(useVault.getState().selectedIds).toEqual([]));
    expect(useVault.getState().selectionAnchor).toBeNull();
  });
  it("图标直接多选并保留草稿，正文只查看详情，批量操作遵守未保存提示", async () => {
    useVault.getState().accept(selectionFixture(), true);
    render(<App />);
    fireEvent.click(await screen.findByText("alpha"));
    expect(useVault.getState().selectedIds).toEqual([]);
    fireEvent.change(screen.getByRole("textbox", { name: /Key/ }), {
      target: { value: "unsaved" },
    });
    fireEvent.click(screen.getByRole("button", { name: "选择记录：alpha" }));
    fireEvent.click(screen.getByRole("button", { name: "选择记录：beta" }));
    expect(useVault.getState().selectedIds).toEqual(["alpha", "beta"]);
    expect(useVault.getState().draft?.key).toBe("unsaved");
    expect(useVault.getState().pendingNavigation).toBeNull();
    fireEvent.contextMenu(screen.getByText("alpha"));
    fireEvent.click(
      await screen.findByRole("menuitem", { name: "移动到数据表…" }),
    );
    expect(screen.getByText("有尚未保存的修改")).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "继续编辑" }));
    fireEvent.click(screen.getByText("beta"));
    fireEvent.click(screen.getByRole("button", { name: "放弃修改" }));
    expect(useVault.getState().selectedIds).toEqual(["alpha", "beta"]);
    expect(useVault.getState().draft?.id).toBe("beta");
    fireEvent.click(screen.getByRole("button", { name: "选择记录：alpha" }));
    expect(useVault.getState().selectedIds).toEqual(["beta"]);
    act(() => useVault.getState().clear());
    expect(useVault.getState().selectedIds).toEqual([]);
    expect(useVault.getState().selectionAnchor).toBeNull();
  });
  it("编辑排序只暂存位置，再次点击保存后才提交并显示自定义顺序", async () => {
    const snapshot = selectionFixture();
    useVault.getState().accept(snapshot, true);
    vi.mocked(invoke).mockImplementation(async (command, args) => {
      if (command === "bootstrap") return bootFixture();
      if (command === "mutate_vault") {
        const { action } = args as { action: { ids: string[] } };
        return {
          ...snapshot,
          revision: 2,
          entries: snapshot.entries.map((entry) => ({
            ...entry,
            sortOrder: action.ids.indexOf(entry.id) + 1,
          })),
        };
      }
    });
    render(<App />);
    fireEvent.click(await screen.findByRole("button", { name: "编辑排序" }));
    expect(screen.getAllByRole("button", { name: /^拖动排序：/ })).toHaveLength(
      4,
    );
    expect(screen.getByRole("textbox", { name: "搜索记录" })).toBeDisabled();
    act(() => useVault.getState().reorderEntry("alpha", 0));
    expect(invoke).not.toHaveBeenCalledWith("mutate_vault", expect.anything());
    expect(useVault.getState().orderDirty).toBe(true);
    fireEvent.click(screen.getByRole("button", { name: "保存排序" }));
    await waitFor(() =>
      expect(invoke).toHaveBeenCalledWith("mutate_vault", {
        sessionId: 1,
        action: {
          type: "reorderEntries",
          ids: ["alpha", "delta", "gamma", "beta"],
          tableId: null,
        },
      }),
    );
    await waitFor(() =>
      expect(
        screen.getByRole("button", { name: "编辑排序" }),
      ).toBeInTheDocument(),
    );
    expect(
      screen
        .getAllByRole("button", { name: /^选择记录：/ })
        .map((button) => button.getAttribute("aria-label")),
    ).toEqual([
      "选择记录：alpha",
      "选择记录：delta",
      "选择记录：gamma",
      "选择记录：beta",
    ]);
    expect(useVault.getState().orderDirty).toBe(false);
  });
  it("排序取消恢复原位，未保存排序关闭前提示，锁定清理排序状态", async () => {
    useVault.getState().accept(selectionFixture(), true);
    render(<App />);
    fireEvent.click(await screen.findByRole("button", { name: "编辑排序" }));
    act(() => useVault.getState().reorderEntry("alpha", 0));
    requestClose("app:exit-requested");
    expect(screen.getByText("有尚未保存的排序")).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "继续编辑" }));
    expect(useVault.getState().orderDirty).toBe(true);
    fireEvent.click(screen.getByRole("button", { name: "取消排序" }));
    expect(useVault.getState().orderDraft).toBeNull();
    expect(
      screen
        .getAllByRole("button", { name: /^选择记录：/ })
        .map((button) => button.getAttribute("aria-label")),
    ).toEqual([
      "选择记录：delta",
      "选择记录：gamma",
      "选择记录：beta",
      "选择记录：alpha",
    ]);
    expect(invoke).not.toHaveBeenCalledWith("mutate_vault", expect.anything());
    fireEvent.click(screen.getByRole("button", { name: "编辑排序" }));
    act(() => {
      useVault.getState().reorderEntry("alpha", 0);
      useVault.getState().clear();
    });
    expect(useVault.getState().orderDraft).toBeNull();
    expect(useVault.getState().orderDirty).toBe(false);
  });
  it("新建记录的 Value 直接可编辑", async () => {
    useVault.getState().accept(fixture(), true);
    render(<App />);
    fireEvent.click(await screen.findByRole("button", { name: "新建记录" }));
    const value = screen.getByRole("textbox", { name: "Value" });
    fireEvent.change(value, { target: { value: "new value" } });
    expect(useVault.getState().draft?.value).toBe("new value");
    expect(
      screen.queryByRole("button", { name: "显示 Value" }),
    ).not.toBeInTheDocument();
  });
  it("列表支持直接复制 Key 和 Value，以及统一显隐且锁定重置", async () => {
    useVault.getState().accept(fixture(), true);
    render(<App />);
    fireEvent.click(
      await screen.findByRole("button", {
        name: "复制 Value：api.production.token",
      }),
    );
    await waitFor(() =>
      expect(invoke).toHaveBeenCalledWith("copy_value", {
        sessionId: 1,
        entryId: "entry",
      }),
    );
    expect(useVault.getState().selectedEntry).toBeNull();
    fireEvent.click(
      screen.getByRole("button", { name: "复制 Key：api.production.token" }),
    );
    await waitFor(() =>
      expect(invoke).toHaveBeenCalledWith("copy_key", {
        sessionId: 1,
        entryId: "entry",
      }),
    );
    expect(screen.queryByText("private-value-123")).not.toBeInTheDocument();
    fireEvent.click(screen.getByRole("switch", { name: "显示全部 Value" }));
    expect(screen.getByText("private-value-123")).toBeInTheDocument();
    fireEvent.click(screen.getByText("api.production.token"));
    expect(screen.getByDisplayValue("private-value-123")).toBeInTheDocument();
    fireEvent.click(screen.getByRole("switch", { name: "显示全部 Value" }));
    expect(
      screen.queryByDisplayValue("private-value-123"),
    ).not.toBeInTheDocument();
    expect(screen.queryByText("private-value-123")).not.toBeInTheDocument();
    act(() => {
      useVault.getState().toggleValues();
      useVault.getState().clear();
    });
    expect(useVault.getState().listValuesVisible).toBe(false);
  });
  it("纯文本先预览，无效行默认阻止导入，确认跳过后整批提交", async () => {
    const snapshot = fixture();
    useVault.getState().accept(snapshot, true);
    const rows = [
      { line: 1, key: "imported.key", value: "imported secret", notes: "备注" },
    ];
    vi.mocked(invoke).mockImplementation(async (command) =>
      command === "bootstrap"
        ? bootFixture()
        : command === "preview_import"
          ? {
              rows,
              errors: [{ line: 2, message: "需要 2 或 3 列" }],
              hasHeader: false,
              delimiter: "space",
            }
          : command === "mutate_vault"
            ? { ...snapshot, revision: 2 }
            : undefined,
    );
    render(<App />);
    fireEvent.click(await screen.findByRole("button", { name: "批量导入" }));
    fireEvent.change(screen.getByLabelText("粘贴记录"), {
      target: { value: "imported.key abc 备注\nbad" },
    });
    fireEvent.click(screen.getByRole("button", { name: "预览导入" }));
    expect(await screen.findByText("imported.key")).toBeInTheDocument();
    expect(screen.queryByText("imported secret")).not.toBeInTheDocument();
    expect(screen.getByRole("button", { name: "确认导入" })).toBeDisabled();
    fireEvent.click(
      screen.getByRole("switch", { name: "跳过无效行，仅导入有效记录" }),
    );
    fireEvent.click(screen.getByRole("button", { name: "确认导入" }));
    await waitFor(() =>
      expect(invoke).toHaveBeenCalledWith("mutate_vault", {
        sessionId: 1,
        action: {
          type: "importEntries",
          tableId: "table",
          rows,
          duplicatePolicy: "skip",
        },
      }),
    );
    await waitFor(() =>
      expect(screen.queryByLabelText("粘贴记录")).not.toBeInTheDocument(),
    );
  });
  it("CSV 文件使用原生选择器，并在锁定后忽略晚到的预览内容", async () => {
    useVault.getState().accept(fixture(), true);
    let finish!: (result: unknown) => void;
    vi.mocked(invoke).mockImplementation(async (command) =>
      command === "bootstrap"
        ? bootFixture()
        : command === "select_import_csv"
          ? { name: "records.csv", text: "key,value\nfile-key,file-secret" }
          : command === "preview_import"
            ? new Promise((resolve) => {
                finish = resolve;
              })
            : undefined,
    );
    render(<App />);
    fireEvent.click(await screen.findByRole("button", { name: "批量导入" }));
    fireEvent.mouseDown(screen.getByRole("tab", { name: "CSV 文件" }), {
      button: 0,
      ctrlKey: false,
    });
    fireEvent.click(await screen.findByRole("button", { name: "选择文件" }));
    expect(await screen.findByText("records.csv")).toBeInTheDocument();
    expect(invoke).toHaveBeenCalledWith("select_import_csv", { sessionId: 1 });
    fireEvent.click(screen.getByRole("button", { name: "预览导入" }));
    await waitFor(() =>
      expect(invoke).toHaveBeenCalledWith("preview_import", {
        sessionId: 1,
        text: "key,value\nfile-key,file-secret",
        delimiter: "comma",
        header: "auto",
      }),
    );
    await act(async () => {
      useVault.getState().clear();
      finish({
        rows: [{ line: 2, key: "file-key", value: "file-secret", notes: "" }],
        errors: [],
        hasHeader: true,
        delimiter: "comma",
      });
    });
    expect(screen.queryByText("file-key")).not.toBeInTheDocument();
    expect(screen.getByText("欢迎回来")).toBeInTheDocument();
  });
  it("冲突界面默认隐藏双方 Value，并提交保留双方的唯一 Key", async () => {
    const snapshot = fixture();
    snapshot.conflicts = [
      {
        id: "conflict",
        reason: "entry",
        local: {
          kind: "entry",
          value: { ...snapshot.entries[0], value: "device-local-secret" },
        },
        remote: {
          kind: "entry",
          value: { ...snapshot.entries[0], value: "device-remote-secret" },
        },
        localEntries: [],
        remoteEntries: [],
      },
    ];
    useVault.getState().accept(snapshot, true);
    vi.mocked(invoke).mockImplementation(async (command) =>
      command === "bootstrap"
        ? bootFixture()
        : command === "mutate_vault"
          ? { ...snapshot, revision: 2, conflicts: [] }
          : undefined,
    );
    render(<App />);
    fireEvent.click(await screen.findByRole("button", { name: /处理冲突/ }));
    expect(screen.getByText("本地版本")).toBeInTheDocument();
    expect(screen.getByText("云端版本")).toBeInTheDocument();
    expect(screen.queryByText("device-local-secret")).not.toBeInTheDocument();
    expect(screen.queryByText("device-remote-secret")).not.toBeInTheDocument();
    fireEvent.change(screen.getByPlaceholderText("新的唯一 Key"), {
      target: { value: "api.token.remote" },
    });
    fireEvent.click(screen.getByRole("button", { name: "保留双方" }));
    await waitFor(() =>
      expect(invoke).toHaveBeenCalledWith("mutate_vault", {
        sessionId: 1,
        action: {
          type: "resolveConflict",
          id: "conflict",
          choice: "both",
          rename: "api.token.remote",
        },
      }),
    );
    await waitFor(() =>
      expect(screen.queryByText("本地版本")).not.toBeInTheDocument(),
    );
  });
  it("默认遮挡 Value，按需显示，并通过 Rust 命令复制", async () => {
    useVault.getState().accept(fixture(), true);
    render(<App />);
    fireEvent.click(await screen.findByText("api.production.token"));
    expect(
      screen.queryByDisplayValue("private-value-123"),
    ).not.toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "显示 Value" }));
    expect(screen.getByDisplayValue("private-value-123")).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "复制 Value" }));
    await waitFor(() =>
      expect(invoke).toHaveBeenCalledWith("copy_value", {
        sessionId: 1,
        entryId: "entry",
      }),
    );
    expect(localStorage.getItem("private-value-123")).toBeNull();
  });
  it("未保存编辑离开前提示，可继续编辑或放弃修改", async () => {
    useVault.getState().accept(fixture(), true);
    render(<App />);
    fireEvent.click(await screen.findByText("api.production.token"));
    fireEvent.change(screen.getByRole("textbox", { name: /Key/ }), {
      target: { value: "edited.key" },
    });
    fireEvent.click(screen.getByRole("button", { name: /全部记录/ }));
    expect(screen.getByText("有尚未保存的修改")).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "继续编辑" }));
    expect(useVault.getState().dirty).toBe(true);
    fireEvent.click(screen.getByRole("button", { name: /全部记录/ }));
    fireEvent.click(screen.getByRole("button", { name: "放弃修改" }));
    await waitFor(() => expect(useVault.getState().draft).toBeNull());
    expect(useVault.getState().dirty).toBe(false);
  });
  it("锁定后清空内容、草稿、可见状态与弹窗，拒绝旧事件", async () => {
    const old = fixture();
    useVault.getState().accept(old, true);
    render(<App />);
    fireEvent.click(await screen.findByText("api.production.token"));
    fireEvent.click(screen.getByRole("button", { name: "显示 Value" }));
    act(() => useVault.getState().clear());
    expect(screen.queryByText("api.production.token")).not.toBeInTheDocument();
    expect(screen.getByText("欢迎回来")).toBeInTheDocument();
    act(() => useVault.getState().accept({ ...old, revision: 10 }));
    expect(useVault.getState().snapshot).toBeNull();
    expect(useVault.getState().visible).toBe(false);
    expect(useVault.getState().draft).toBeNull();
  });
  it("锁定期间返回的旧保存请求不能恢复明文状态", async () => {
    const old = fixture();
    useVault.getState().accept(old, true);
    let complete!: (value: Snapshot) => void;
    vi.mocked(invoke).mockImplementationOnce(
      () =>
        new Promise<Snapshot>((resolve) => {
          complete = resolve;
        }),
    );
    const request = useVault
      .getState()
      .mutate({ type: "createTable", name: "新表" });
    useVault.getState().clear();
    complete({ ...old, revision: 2 });
    await request;
    expect(useVault.getState().snapshot).toBeNull();
    expect(useVault.getState().busy).toBe(false);
  });
  it("后端同步事件保留正在编辑的草稿", () => {
    const old = fixture();
    useVault.getState().accept(old, true);
    useVault.getState().openEntry(old.entries[0]);
    useVault.getState().edit({ notes: "未保存的本地说明" });
    useVault.getState().accept({
      ...old,
      revision: 2,
      entries: [{ ...old.entries[0], value: "new remote" }],
    });
    expect(useVault.getState().draft?.notes).toBe("未保存的本地说明");
    expect(useVault.getState().dirty).toBe(true);
  });

  it("云端删除正在编辑的记录时保留草稿，可另存为新记录", () => {
    const old = fixture();
    useVault.getState().accept(old, true);
    useVault.getState().openEntry(old.entries[0]);
    useVault.getState().edit({ value: "unsaved secret" });
    useVault.getState().accept({ ...old, revision: 2, entries: [] });
    expect(useVault.getState().draft?.id).toBeNull();
    expect(useVault.getState().draft?.value).toBe("unsaved secret");
    expect(useVault.getState().dirty).toBe(true);
  });
});
