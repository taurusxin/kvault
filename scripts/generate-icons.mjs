import { spawnSync } from "node:child_process";
import {
  copyFileSync,
  mkdirSync,
  mkdtempSync,
  readdirSync,
  rmSync,
} from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const root = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const temporary = mkdtempSync(join(tmpdir(), "kvault-icons-"));
try {
  const result = spawnSync(
    process.execPath,
    [
      join(root, "node_modules", "@tauri-apps", "cli", "tauri.js"),
      "icon",
      "public/icon.svg",
      "--output",
      temporary,
    ],
    { cwd: root, stdio: "inherit" },
  );
  if (result.error) throw result.error;
  if (result.status !== 0) throw new Error("Icon generation failed");
  const destination = join(root, "src-tauri", "icons");
  mkdirSync(destination, { recursive: true });
  // Keep desktop assets only; mobile icon sets are outside this app's scope.
  for (const name of readdirSync(temporary)) {
    if (/\.(png|icns|ico)$/.test(name)) {
      copyFileSync(join(temporary, name), join(destination, name));
    }
  }
  console.log("Updated desktop icons from public/icon.svg");
} finally {
  rmSync(temporary, { recursive: true, force: true });
}
