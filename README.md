<div align="center">

<img src="./public/icon.svg" alt="KVault 图标" width="112" height="112" />

# KVault

**把零散的 Key，收进自己的加密保险库。**

本地优先的跨平台 Key-Value 记录工具，支持按表整理、批量导入与加密云同步。

<p>
  <img src="https://img.shields.io/badge/Tauri-2-24C8D8?style=flat&amp;logo=tauri&amp;logoColor=white" alt="Tauri 2" />
  <img src="https://img.shields.io/badge/Rust-2024_edition-CE422B?style=flat&amp;logo=rust&amp;logoColor=white" alt="Rust 2024 edition" />
  <img src="https://img.shields.io/badge/React-19-61DAFB?style=flat&amp;logo=react&amp;logoColor=black" alt="React 19" />
  <img src="https://img.shields.io/badge/TypeScript-6-3178C6?style=flat&amp;logo=typescript&amp;logoColor=white" alt="TypeScript 6" />
  <img src="https://img.shields.io/badge/shadcn%2Fui-Nova-18181B?style=flat&amp;logo=shadcnui&amp;logoColor=white" alt="shadcn/ui Nova" />
</p>

[功能](#功能) · [快速开始](#快速开始) · [云同步](#云同步) · [文档](#文档)

</div>

## 功能

- **按表整理** — 每条记录包含 Key、Value 和备注，支持跨表移动与全文搜索。同一张表内 Key 区分大小写且唯一。
- **本地加密** — 使用一个主密码解锁保险库。业务数据加密落盘，断网时也能继续编辑，联网后再同步。
- **加密云同步** — 支持 S3 兼容存储与 WebDAV。独立修改自动合并，冲突保留双方版本供你处理。
- **批量导入** — 读取 UTF-8 CSV，或直接粘贴以空格、Tab、逗号分隔的文本；支持预览、重复 Key 跳过或覆盖。
- **批量操作与排序** — 点击左侧图标连续勾选，右键批量移动、删除；进入编辑模式后拖动排序，确认后保存。
- **按需显示** — 默认遮挡 Value，一键切换显隐，直接复制 Key 或 Value；支持浅色、深色和跟随系统。
- **锁定与备份** — 手动锁定、可配置的自动锁定（默认闲置 5 分钟）、修改主密码，以及加密备份导入和导出；可选 Touch ID / Windows Hello，主密码始终可用。

## 快速开始

准备 Node.js、pnpm、Rust stable，以及目标平台的 [Tauri 开发依赖](https://v2.tauri.app/start/prerequisites/)。克隆仓库后，在项目目录运行：

```sh
pnpm install
pnpm tauri dev
```

首次启动可以创建本地保险库，或配置云存储并输入主密码连接已有保险库。也可以导入已有的加密备份。

构建当前平台的安装包：

```sh
pnpm tauri build
```

仅运行 `pnpm dev` 可预览前端；保险库、文件和云同步功能需要通过 Tauri 启动。平台依赖、调试包、图标生成与验证命令见[开发指南](docs/development.md)。

## 云同步

| 存储后端    | 可配置内容                                                              |
| ----------- | ----------------------------------------------------------------------- |
| S3 兼容存储 | Endpoint、Region、Bucket、访问密钥、Session Token、路径前缀、Path Style |
| WebDAV      | 服务地址、目录、用户名和密码                                            |

每次启用一个同步目标。应用先保存本地修改，再同步加密快照；同步凭据保存在本机系统凭据库，记录保存为加密二进制文件；基础配置独立明文存储。远端服务需要支持 ETag 与条件写入，应用会在连接测试时验证。

从左下角云存储入口可直接进入“云同步”设置。配置与冲突处理见[使用指南](docs/user-guide.md#云同步)。

## 常用操作

| 操作           | 方法                                 |
| -------------- | ------------------------------------ |
| 查看记录       | 点击记录正文                         |
| 勾选与多选     | 点击左侧图标；Shift 点击选择连续范围 |
| 批量移动、删除 | 右键菜单或勾选后的工具栏             |
| 调整顺序       | 点击列表“编辑”，拖动手柄后保存       |
| 保存修改或排序 | Ctrl / Cmd + S                       |
| 搜索记录       | Ctrl / Cmd + K                       |
| 锁定保险库     | Ctrl / Cmd + Shift + L               |

导入格式、多选规则、排序和其他快捷键见[使用指南](docs/user-guide.md)。

## 技术栈

| 层次         | 技术                                                                               |
| ------------ | ---------------------------------------------------------------------------------- |
| 桌面与后端   | Tauri 2、Rust、AWS SDK for Rust、reqwest                                           |
| 界面与交互   | React、TypeScript、Tailwind CSS、shadcn/ui Nova（Radix）、Lucide、Zustand、dnd-kit |
| 加密与持久化 | Argon2id、XChaCha20-Poly1305、版本化加密文件、原子替换                             |
| 测试         | Vitest、React Testing Library、Rust 测试、本地 HTTP 协议测试服务                   |

## 文档

| 文档                                 | 内容                                               |
| ------------------------------------ | -------------------------------------------------- |
| [使用指南](docs/user-guide.md)       | 保险库、记录管理、批量导入、云同步、备份与快捷键   |
| [开发指南](docs/development.md)      | 环境准备、构建、GitHub Release、图标生成与隔离测试 |
| [技术报告](docs/technical-report.md) | 架构、加密格式、本地存储、同步合并、换密恢复与 IPC |

## 当前范围

当前版本为 **0.2.0**，面向个人文本 KV，目标平台为 Windows、macOS 和 Linux。macOS 已完成本地构建与主要交互验证；Windows、Linux 仍需在对应环境完成构建和窗口验收。

首版不包含移动端、附件、多人协作、常规明文导出或签名发布。系统解锁默认关闭；Windows 使用 WebAuthn PRF + AES-256-GCM，需要 API ≥ 8 和支持 PRF 的本机 Hello，不提供旧系统兼容后端。主密码始终可用。本次采用全新 v2 格式，不兼容 v1 保险库和旧版加密备份。

**主密码无法找回，请妥善保管密码和加密备份。** 历史备份仍使用生成备份时的密码。
