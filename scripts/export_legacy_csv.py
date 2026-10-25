#!/usr/bin/env -S uv run --script
# /// script
# requires-python = ">=3.11"
# dependencies = ["argon2-cffi==25.1.0", "PyNaCl==1.6.2"]
# ///
"""Temporary, read-only CSV exporter for the original KVault v1 format.

Run with: uv run scripts/export_legacy_csv.py
The master password is requested in the terminal without echoing it.
This script never changes the vault or connects to cloud storage.
"""

from __future__ import annotations

import argparse
import base64
import binascii
import csv
import getpass
import json
import os
from pathlib import Path
import secrets
import sys
from datetime import datetime
from uuid import UUID
import warnings

from argon2.low_level import Type, hash_secret_raw
from nacl.bindings import crypto_aead_xchacha20poly1305_ietf_decrypt
from nacl.exceptions import CryptoError

MAX_FILE_SIZE = 64 * 1024 * 1024
# Field order must match the Rust Header struct, because it is part of the AAD.
HEADER_FIELDS = (
    "format", "version", "vaultId", "epoch", "kdf", "memory",
    "iterations", "parallelism", "salt",
)
RECORD_COLUMNS = ("table", "key", "value", "notes")
CONFLICT_COLUMNS = (
    "conflict_id", "reason", "side", "kind", "table", "table_id",
    "record_id", "key", "value", "notes", "deleted",
)


class ExportError(Exception):
    pass


def default_vault_path() -> Path:
    if sys.platform == "darwin":
        root = Path.home() / "Library/Application Support"
    elif sys.platform == "win32":
        root = Path(os.environ.get("APPDATA", Path.home() / "AppData/Roaming"))
    else:
        root = Path(os.environ.get("XDG_DATA_HOME", Path.home() / ".local/share"))
    return root / "com.taurusxin.kvault/vault.kvault"


def unique_object(pairs: list[tuple[str, object]]) -> dict:
    result = {}
    for key, value in pairs:
        if key in result:
            raise ExportError("文件包含重复 JSON 字段。")
        result[key] = value
    return result


def decode_json(data: bytes) -> dict:
    try:
        result = json.loads(data, object_pairs_hook=unique_object)
    except (ValueError, UnicodeError, RecursionError) as exc:
        raise ExportError("保险库数据格式无效。") from exc
    if not isinstance(result, dict):
        raise ExportError("保险库数据格式无效。")
    return result


def decode_base64(value: object, size: int | None = None) -> bytes:
    try:
        if not isinstance(value, str):
            raise ValueError
        result = base64.b64decode(value, validate=True)
        if size is not None and len(result) != size:
            raise ValueError
        return result
    except (ValueError, binascii.Error) as exc:
        raise ExportError("保险库的密码学参数无效。") from exc


def read_envelope(path: Path) -> dict:
    with path.open("rb") as source:
        data = source.read(MAX_FILE_SIZE + 1)
    if len(data) > MAX_FILE_SIZE:
        raise ExportError("保险库超过 64 MiB 上限。")
    envelope = decode_json(data)
    if set(envelope) != {"header", "purpose", "nonce", "ciphertext"}:
        raise ExportError("不支持的保险库格式；本脚本仅支持原始 v1 格式。")
    header = envelope["header"]
    if not isinstance(header, dict) or set(header) != set(HEADER_FIELDS):
        raise ExportError("保险库文件头无效。")
    expected = {"format": "kvault", "version": 1, "kdf": "argon2id-v19",
                "memory": 65536, "iterations": 3, "parallelism": 1}
    if any(type(header[k]) is not type(v) or header[k] != v for k, v in expected.items()):
        raise ExportError("不支持的版本或 KDF 参数，已停止导出。")
    if type(header["epoch"]) is not int or not 1 <= header["epoch"] <= 2**64 - 1:
        raise ExportError("密钥代次无效。")
    try:
        UUID(header["vaultId"])
    except (ValueError, TypeError, AttributeError) as exc:
        raise ExportError("保险库 UUID 无效。") from exc
    if envelope["purpose"] not in ("local", "snapshot"):
        raise ExportError("不支持的保险库用途。")
    decode_base64(header["salt"], 16)
    decode_base64(envelope["nonce"], 24)
    if len(decode_base64(envelope["ciphertext"])) < 16:
        raise ExportError("密文长度无效。")
    return envelope


def decrypt(envelope: dict, password: str) -> dict:
    header = {field: envelope["header"][field] for field in HEADER_FIELDS}
    key = hash_secret_raw(
        password.encode("utf-8"), decode_base64(header["salt"], 16),
        time_cost=3, memory_cost=65536, parallelism=1, hash_len=32,
        type=Type.ID, version=19,
    )
    aad = json.dumps([header, envelope["purpose"]], ensure_ascii=False,
                     separators=(",", ":")).encode("utf-8")
    try:
        plain = crypto_aead_xchacha20poly1305_ietf_decrypt(
            decode_base64(envelope["ciphertext"]), aad,
            decode_base64(envelope["nonce"], 24), key,
        )
    except CryptoError as exc:
        raise ExportError("主密码不正确，或保险库内容已损坏。未创建 CSV。") from exc
    finally:
        # Python cannot guarantee zeroization; never persist or log these values.
        del key, password
    return decode_json(plain)


def require_list(value: object) -> list:
    if not isinstance(value, list):
        raise ExportError("表、记录或冲突列表格式无效。")
    return value


def validate_entity(value: object, kind: str) -> dict:
    fields = ("id", "name") if kind == "table" else ("id", "tableId", "key", "value", "notes")
    if not isinstance(value, dict) or any(not isinstance(value.get(k), str) for k in fields):
        raise ExportError("表或记录字段格式无效。")
    if type(value.get("deleted")) is not bool:
        raise ExportError("表或记录的删除标记无效。")
    return value


def build_csvs(payload: dict, purpose: str) -> tuple[dict, int]:
    document = payload.get("document") if purpose == "local" else payload
    if not isinstance(document, dict):
        raise ExportError("保险库中缺少记录文档。")
    tables = [validate_entity(t, "table") for t in require_list(document.get("tables"))]
    entries = [validate_entity(e, "entry") for e in require_list(document.get("entries"))]
    by_id = {t["id"]: t for t in tables}
    if len(by_id) != len(tables):
        raise ExportError("保险库中存在重复表 ID。")
    records = []
    for entry in entries:
        if entry["deleted"]:
            continue
        table = by_id.get(entry["tableId"])
        if table is None:
            raise ExportError("存在分类缺失的记录，已停止导出以免遗漏数据。")
        if not table["deleted"]:
            records.append((table["name"], entry["key"], entry["value"], entry["notes"]))
    files = {
        "records.csv": (RECORD_COLUMNS, records),
        "tables.csv": (("id", "name"), [(t["id"], t["name"]) for t in tables if not t["deleted"]]),
    }
    conflicts = require_list(payload.get("conflicts", [])) if purpose == "local" else []
    conflict_rows = []
    for conflict in conflicts:
        if not isinstance(conflict, dict) or any(
            not isinstance(conflict.get(k), str) for k in ("id", "reason")
        ):
            raise ExportError("冲突格式无效。")
        for side in ("local", "remote"):
            entity = conflict.get(side)
            side_tables = dict(by_id)
            side_entries = []
            if entity is not None:
                if not isinstance(entity, dict) or entity.get("kind") not in ("table", "entry"):
                    raise ExportError("冲突内容格式无效。")
                value = validate_entity(entity.get("value"), entity["kind"])
                if entity["kind"] == "table":
                    side_tables[value["id"]] = value
                    conflict_rows.append((conflict["id"], conflict["reason"], side, "table",
                                          value["name"], value["id"], "", "", "", "", value["deleted"]))
                else:
                    side_entries.append(value)
            else:
                conflict_rows.append((conflict["id"], conflict["reason"], side, "absent",
                                      "", "", "", "", "", "", True))
            side_entries.extend(validate_entity(e, "entry") for e in require_list(
                conflict.get(side + "Entries", [])
            ))
            for entry in side_entries:
                table_name = side_tables.get(entry["tableId"], {}).get("name", "")
                conflict_rows.append((conflict["id"], conflict["reason"], side, "entry", table_name,
                                      entry["tableId"], entry["id"], entry["key"], entry["value"],
                                      entry["notes"], entry["deleted"]))
    if conflicts:
        files["conflicts.csv"] = (CONFLICT_COLUMNS, conflict_rows)
    return files, len(conflicts)


def write_csvs(directory: Path, files: dict) -> None:
    # An exclusively created directory prevents overwriting any previous export.
    directory.mkdir(mode=0o700)
    created = []
    try:
        for name, (columns, rows) in files.items():
            path = directory / name
            fd = os.open(path, os.O_WRONLY | os.O_CREAT | os.O_EXCL, 0o600)
            created.append(path)
            with os.fdopen(fd, "w", encoding="utf-8-sig", newline="") as output:
                writer = csv.writer(output)
                writer.writerow(columns)
                writer.writerows(rows)
                output.flush()
                os.fsync(output.fileno())
    except BaseException:
        for path in created:
            path.unlink(missing_ok=True)
        directory.rmdir()
        raise


def main() -> int:
    parser = argparse.ArgumentParser(description="只读导出 KVault v1 为明文 CSV，交互输入主密码。")
    parser.add_argument("--vault", type=Path, default=default_vault_path(),
                        help="保险库或加密备份路径（默认自动定位本机保险库）")
    parser.add_argument("--output-dir", type=Path,
                        help="新的输出目录，父目录须存在；不会覆盖现有目录")
    args = parser.parse_args()
    try:
        envelope = read_envelope(args.vault.expanduser())
        if not sys.stdin.isatty():
            raise ExportError("请在交互式终端运行，主密码仅支持隐藏输入，不接受管道或命令行传参。")
        print(f"源文件：{args.vault.expanduser()}")
        print("请先保存记录并退出 KVault。输出是明文 CSV，请妥善保管，使用后删除。")
        with warnings.catch_warnings():
            warnings.simplefilter("error", getpass.GetPassWarning)
            password = getpass.getpass("主密码（输入时不显示）：")
        try:
            # Read again in case the user saved and closed the app at the prompt.
            envelope = read_envelope(args.vault.expanduser())
            payload = decrypt(envelope, password)
        finally:
            del password
        files, conflicts = build_csvs(payload, envelope["purpose"])
        del payload
        previous_umask = os.umask(0o077)
        try:
            if args.output_dir is None:
                parent = Path(__file__).resolve().parent.parent / ".private-exports"
                parent.mkdir(mode=0o700, exist_ok=True)
                stamp = datetime.now().strftime("%Y%m%d-%H%M%S")
                directory = parent / f"kvault-{stamp}-{secrets.token_hex(3)}"
            else:
                directory = args.output_dir.expanduser().absolute()
            write_csvs(directory, files)
        finally:
            os.umask(previous_umask)
        print(f"导出成功：{len(files['records.csv'][1])} 条记录、{len(files['tables.csv'][1])} 个表。")
        print(f"输出目录：{directory}")
        print("records.csv：table、key、value、notes；tables.csv：包含空表在内的分类清单。")
        if conflicts:
            print(f"另有 {conflicts} 个未解决冲突，双方内容已写入 conflicts.csv。")
        print("原始保险库未修改；已删除的记录不包含在 records.csv 中。")
        return 0
    except (ExportError, getpass.GetPassWarning) as exc:
        print(f"导出失败：{exc}", file=sys.stderr)
    except OSError as exc:
        print(f"导出失败：{exc.strerror}。请检查路径、权限和剩余空间；已有文件不会被覆盖。", file=sys.stderr)
    except (KeyboardInterrupt, EOFError):
        print("\n已取消，原始保险库未修改。", file=sys.stderr)
    return 1


if __name__ == "__main__":
    raise SystemExit(main())
