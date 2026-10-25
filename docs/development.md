# 开发指南

[返回项目首页](../README.md) · [使用指南](user-guide.md) · [技术报告](technical-report.md)

## 环境与构建

安装 Node.js、pnpm、Rust stable，以及 [Tauri 2 平台依赖](https://v2.tauri.app/start/prerequisites/)。

```sh
pnpm install
pnpm tauri dev

# 在目标平台打包
pnpm tauri build
# macOS 调试版 .app，无需发布签名凭据
pnpm tauri build --debug --bundles app

# 修改 public/icon.svg 后重新生成各平台桌面图标
pnpm icons
```

Windows 使用 WebView2；Linux 需要 WebKitGTK 4.1、GTK 3 和相应构建依赖。安装包必须在对应平台构建和验收，正式分发时另行配置签名、公证。仅运行 `pnpm dev` 可以预览界面，浏览器不会提供保险库、文件或云存储能力。

前端为 React / TypeScript / Tailwind CSS 4，使用 shadcn/ui `radix-nova`、Lucide、Zustand。Rust 按 `vault`、`sync`、`commands` 组织；前端通过 Tauri IPC 调用后端，不直接读写文件或连接云存储。

macOS 使用 32px 紧凑标题栏和原生红绿灯；Windows、Linux 保留自绘窗口按钮。图标源稿为 `public/icon.svg`，采用 1024px 透明画布与 824px 圆角底板，保留 Dock 所需的外部留白。`pnpm icons` 同步更新 PNG、Windows ICO 和 macOS ICNS；移动端资源只在临时目录生成后清理。

## 目录结构

```text
kvault/
├── public/icon.svg           # 图标矢量源稿
├── scripts/                 # 桌面图标生成脚本
├── src/
│   ├── components/          # 业务界面与 shadcn/ui 组件
│   ├── lib/                 # 类型、主题与公共工具
│   ├── stores/              # Zustand 保险库与界面状态
│   └── test/                # 前端流程测试
├── src-tauri/
│   ├── capabilities/        # 窗口权限
│   ├── icons/               # 各平台桌面图标
│   └── src/
│       ├── commands/        # Tauri IPC 命令
│       ├── sync/            # 远端存储、合并与同步队列
│       └── vault/           # 数据模型、加密、持久化与批量导入
└── docs/                    # 使用、开发与实现文档
```

Node.js 依赖统一使用 pnpm 管理。修改图标后运行 `pnpm icons`，再重新打包应用；只替换 Web 页面图标不会更新已构建的应用包资源。

## GitHub Release

[发布工作流](../.github/workflows/release.yml)仅在推送 `v*` tag 时触发，使用 Node.js 24、pnpm 12.15.0 和 Rust stable。工作流先校验 tag 中的版本号与 `package.json`、`src-tauri/tauri.conf.json`、`src-tauri/Cargo.toml` 一致，再并行生成：

| 平台    | 架构                 | 安装包                  |
| ------- | -------------------- | ----------------------- |
| Windows | x64                  | NSIS `.exe`、MSI `.msi` |
| macOS   | Apple Silicon、Intel | 各自独立的 `.dmg`       |
| Linux   | x64                  | `.deb`、`.AppImage`     |

全部构建成功后，工作流汇总安装包、生成 `SHA256SUMS` 和 GitHub 自动发布说明，先上传到草稿 Release，上传成功后发布。任何构建失败都会阻止发布；失败后可在 Actions 中重跑。带预发布后缀的 tag（如 `v0.2.0-beta.1`）发布为 Pre-release。

发布前同步更新上述三个文件的版本号，并将 `pnpm-lock.yaml`、`src-tauri/Cargo.lock` 与版本变更一起提交。然后手动创建并推送 tag，例如当前版本：

```sh
git tag v0.1.0
git push origin v0.1.0
```

工作流使用自动提供的 `GITHUB_TOKEN`，只在发布 job 授予 `contents: write`，无需配置额外密钥。macOS 使用 [Tauri 文档说明的 ad-hoc 签名](https://v2.tauri.app/distribute/sign/macos/)，未配置 Apple 开发者证书、公证或 Windows 代码签名；这是安装包生成与发布流程，实际平台兼容性仍需单独验收。

## 测试与验收

```sh
pnpm test
pnpm build
cd src-tauri
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test --lib
```

Rust 测试使用临时目录与仅绑定 `127.0.0.1` 的 HTTP 服务，覆盖两种后端的条件请求（S3 请求包含 SigV4 签名）、加密与篡改检测、原子保存、并发编辑、双设备冲突、密码更换及重启恢复。没有真实云端凭据时，这些测试不代替特定云厂商的兼容性验收。

macOS 原生 UI 验收可使用独立测试目录，以下功能仅在调试构建启用，不会修改正常保险库：

```sh
# 从项目根目录执行，仅在 .qa-vault 中没有现有保险库时生成示例数据
KVAULT_QA_DIR="$PWD/.qa-vault" cargo test --manifest-path src-tauri/Cargo.toml seed_native_qa_fixture -- --ignored
KVAULT_DATA_DIR="$PWD/.qa-vault" pnpm tauri dev
```

测试数据的密码为 `kvault-test-only`，仅用于可丢弃的示例保险库，不能用作真实主密码。发布构建忽略 `KVAULT_DATA_DIR`。

### 平台验收范围

- macOS 已完成本地构建，以及解锁、编辑、导入、显隐、多选、排序和原生关闭等主要流程检查。
- Windows、Linux 需要在相应系统执行构建、窗口按钮与完整使用流程验收。
- GitHub Actions 工作流已配置；实际云端构建、真实云端兼容性、开发者证书签名与公证仍需在发布环境验证，不能由本地测试结果代替。

本仓库的演示保险库、打包产物与本地检查资源放在被忽略的 `.qa-vault/`、`src-tauri/target/`、`.artifacts/` 中，不应将真实保险库或云端凭据加入仓库。
