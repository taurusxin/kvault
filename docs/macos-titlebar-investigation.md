# macOS Release 红绿灯位置调查

调查日期：2026-10-25。对象：KVault `v0.1.0`，当前复现系统为 macOS 27.0.1。

## 结论

Actions 产物与本地调试包链接了不同版本的 macOS SDK。AppKit 根据可执行文件的 SDK 信息采用不同的原生窗口按钮几何；当前固定的 `trafficLightPosition.y = 18` 无法同时让两种布局居中。

前端 [WindowBar](../src/components/window-bar.tsx) 在 macOS 下使用 `h-8`，当前高度为 32 CSS px；默认 WebView 缩放下对应 32pt，目标中心线为距窗口顶部 16pt。根据已安装产物的 SDK 信息与原生复现实验，当前 Actions 配置对应的按钮中心为 20pt，向下偏移 4pt；在 2× Retina 显示下约为 8 个物理像素，与反馈截图相符。

## 产物与源码证据

从可执行文件的 Mach-O `LC_BUILD_VERSION` 读取到：

| 产物                                                                                 | 链接 SDK | 最低 macOS 版本 |
| ------------------------------------------------------------------------------------ | -------- | --------------- |
| 已安装的 Actions Release（`/Applications/KVault.app/Contents/MacOS/kvault`）         | 15.5     | 11.0            |
| 本地调试包（`src-tauri/target/debug/bundle/macos/kvault.app/Contents/MacOS/kvault`） | 27.0     | 11.0            |

可使用以下命令检查实际产物；SDK 版本应从二进制读取，不能仅根据 runner 名称判断：

```sh
otool -l /Applications/KVault.app/Contents/MacOS/kvault | rg -A 8 'LC_BUILD_VERSION|LC_VERSION_MIN_MACOSX'
```

调查时的 macOS 窗口配置使用 `Overlay`、原生 decorations、隐藏系统标题，以及 `{ "x": 14, "y": 18 }`。本地与 Release 使用相同的该配置；[Actions](../.github/workflows/release.yml)用 `--locked` 构建，当前 Cargo.lock 固定 Tao 0.37.2。修复后已从[窗口配置](../src-tauri/tauri.macos.conf.json)移除固定 `trafficLightPosition`，由原生布局逻辑负责定位。

Tao 0.37.2 的 `inset_traffic_lights` 并非直接设置按钮的顶部 Y 坐标，而是：

1. 读取关闭按钮的原生 frame。
2. 将标题栏容器高度设为 `button.height + configuredY`，并将该容器锚定到窗口顶部。
3. 调整三个按钮的水平位置，保留原生按钮的垂直 origin。

因此按钮中心距窗口顶部的位置为：

```text
centerFromTop = configuredY + button.height / 2 - button.origin.y
```

相关背景：[Tauri 窗口配置](https://v2.tauri.app/reference/config/#windowconfig)、[Tao 0.37.2 原生窗口实现](https://docs.rs/crate/tao/0.37.2/source/src/platform_impl/macos/view.rs)。Tauri 对 Overlay 样式也提示了不同系统版本的标题栏几何差异。

## 原生控件复现

[复现程序](repros/macos-traffic-light-probe.m)创建同样样式的 AppKit 窗口，仅测量原生按钮与标题栏容器，不读取保险库，也不显示窗口。

将同一份程序构建后，使用 `vtool` 为副本设置 SDK 15.5 的 Mach-O 元数据，并重新进行 ad-hoc 签名。在同一台机器上运行两份程序，可以隔离 SDK 链接信息对 AppKit 布局的影响；这不是使用完整旧 SDK 重新编译整个应用。

实测结果，单位均为逻辑 pt：

| SDK 元数据 | 按钮 frame 高度 | 原生按钮 origin.y | 配置 y = 18 时中心 | 配置 y = 14 时中心 |
| ---------- | --------------- | ----------------- | ------------------ | ------------------ |
| 15.5       | 16              | 6                 | 20                 | 16                 |
| 27.0       | 14              | 9                 | 16                 | 12                 |

复现命令，从项目根目录执行；原生探针二进制与输出均放在被忽略的 `.artifacts/` 中：

```sh
mkdir -p .artifacts
clang -fobjc-arc -framework AppKit -mmacosx-version-min=11.0 docs/repros/macos-traffic-light-probe.m -o .artifacts/traffic-light-probe-sdk27
vtool -set-build-version macos 11.0 15.5 -replace -output .artifacts/traffic-light-probe-sdk15 .artifacts/traffic-light-probe-sdk27
codesign --force --sign - .artifacts/traffic-light-probe-sdk15
.artifacts/traffic-light-probe-sdk15
.artifacts/traffic-light-probe-sdk27
```

上述命令中的 `sdk27` 命名对应本次使用的本地 SDK 27.0。其他开发环境应先使用 `xcrun --sdk macosx --show-sdk-version` 与 `otool` 核验，不能把文件名当作 SDK 版本证据。

## 已实施的动态修复

[macos.rs](../src-tauri/src/macos.rs)仅在 macOS 编译。在主线程上读取原生按钮的实际 frame，将标题栏容器高度统一为 32pt，再把距窗口顶部 16pt 的目标中心转换为各按钮父视图的坐标，保留原生水平间距和 14pt 左边距。

启动、窗口尺寸变化、显示缩放变化、重新聚焦和系统主题变化后会重新定位。进入系统全屏时交由系统管理原生按钮；Tao 在退出全屏后发出的 `Resized` 事件会恢复普通窗口的居中布局。未替换原生按钮或窗口 delegate，Tao 的固定 inset 配置已移除，避免两套逻辑同时调整容器。

前端标题栏移除中间状态文案，`KVault` 通过相对整个标题栏定位的布局居中，不再受左侧红绿灯、应用图标或右侧窗口按钮组的宽度影响。

### 验证结果

使用独立 bundle identifier 与工作区内的独立数据目录运行实际应用测试副本，并为其中一份副本设置 SDK 15.5 的 Mach-O 元数据。以下验证覆盖生产 Rust 定位逻辑，SDK 15.5 副本仍属于同一份代码的兼容布局验证，不等同于在 Actions 上重新编译：

| SDK 元数据 | 原生按钮高度 | 三个按钮中心距顶部 | 缩放及退出全屏后 |
| ---------- | ------------ | ------------------ | ---------------- |
| 15.5       | 16pt         | 均为 16pt          | 均为 16pt        |
| 27.0       | 14pt         | 均为 16pt          | 均为 16pt        |

在 1200px 和 960px 前端窗口中测量了标题文字的实际边界，其中心与整个标题栏中心一致。Windows 样式预览的子像素误差小于 0.01px。

调试构建可设置 `KVAULT_TITLEBAR_DIAGNOSTICS=1`，输出各原生按钮的高度、左边距和中心坐标。该诊断分支不包含在发布构建中，也不输出业务内容。其他 macOS 系统版本、Intel 产物及真实多显示器切换仍需在相应环境验收。

### 构建策略

针对当前 SDK 15.5 产物，`y = 14` 可以达到 16pt 的中心线；但将项目配置全局改为 14，会使本地 SDK 27.0 构建上偏 4pt。因此不能只根据 debug/release 模式或 runner 操作系统名称选择这个值。

统一 CI 与本地 SDK 可作为短期构建策略，但仍应在目标 macOS 版本上验证启动、调整窗口大小、最大化、进入/退出全屏及多显示器缩放。当前数值来自上述系统与产物，其他系统和 Intel 产物仍需分别验收。

此次修复采用动态原生布局，保留 Actions 现有的 SDK 选择；原生探针继续用于复现旧的固定 inset 差异。
