# 存储与系统解锁（v2）

[技术报告](technical-report.md) · [使用指南](user-guide.md)

本次以全新保险库开始，不实现 v1 数据迁移。v1 JSON 密文文件和旧版加密备份会被明确拒绝。已有云目标仍可能保留 v1 文件，请为 v2 使用新的目录或前缀，应用不会自动覆盖不兼容格式。

## 数据分层

| 位置               | 内容                                                                                 | 访问条件                                                                 |
| ------------------ | ------------------------------------------------------------------------------------ | ------------------------------------------------------------------------ |
| `config.json`      | 版本、主题、自动锁定开关及分钟数、关闭行为与询问开关、系统解锁授权的文件头指纹       | 本机明文，严格字段和取值校验；16 KiB 上限                                |
| `vault.kvault`     | 表、Key、Value、备注、排序、时间戳、墓碑、合并基线、冲突、修订号、恢复日志、凭据引用 | v2 二进制认证加密                                                        |
| 系统凭据库         | S3/WebDAV 完整配置及密码、密钥、Session Token                                        | macOS login Keychain / Windows Credential Manager / Linux Secret Service |
| 独立的系统解锁凭据 | 由本机硬件或 Windows Hello 保护的数据密钥封装                                        | 每次解封需要系统验证，默认不创建                                         |
| 云端、加密备份     | 业务 `Document`                                                                      | 同样的 v2 二进制加密；没有云凭据、本机设置或系统解锁授权                 |

`LocalState.config` 和 `autoLock` 仅在解锁会话中使用，序列化时跳过。云凭据写入新的不可变凭据条目，其随机 UUID 引用与新的同步基线一起原子写进保险库；保存失败不会切换到新的配置。旧条目在提交后清理，未成功清理的引用保留在加密的待清理列表中，在后续保存或解锁时重试。凭据账户名称包含本机数据目录的哈希和保险库 UUID，独立调试目录不会复用正式保险库的凭据。

系统凭据服务不可用时，不向文件降级保存密码。已保存的业务数据仍可用主密码解锁和编辑，此时提示凭据错误并暂停云同步及换密；用户可恢复服务、重新配置或停用同步。单条配置限制为 2500 字节，以兼容 Windows Credential Manager。

主题通过 Rust 接口保存到 `config.json`，WebView 不使用 localStorage 保存配置或业务数据。自动锁定设置在首次解密前就可读取，锁定页显示真实配置；导入备份保留本机主题和自动锁定设置。

## 密钥层级

1. 每个保险库生成随机 256 位数据密钥（DEK）。
2. 主密码经 Argon2id v19 派生 256 位封装密钥（KEK）：16 字节随机盐、65536 KiB、3 次迭代、并行度 1。
3. KEK 使用 XChaCha20-Poly1305 封装 DEK，使用独立随机 24 字节 nonce；KEK 不留在会话中。
4. DEK 使用 XChaCha20-Poly1305 加密 CBOR 业务载荷，每次写入重新生成 24 字节 nonce。
5. 修改主密码生成新盐、KEK 和 DEK，重新加密业务状态及云端快照，并撤销本机系统解锁授权。历史备份仍需历史密码。

密码、KEK、DEK 不写入配置、日志或前端 Store。Rust 会话密钥与解封缓冲区使用 `Zeroizing`；云配置字符串在释放时清零。前端解锁后仍需持有展示所需的明文，JavaScript、系统剪贴板、交换文件及第三方库中的历史副本不保证安全擦除。

## 二进制文件布局

所有整数为小端，无 JSON/Base64 外层。完整文件上限 64 MiB。

| 偏移 | 字节数 | 内容                                                        |
| ---- | ------ | ----------------------------------------------------------- |
| 0    | 8      | Magic：`4b 56 41 55 4c 54 02 00`（`KVAULT` + 版本 2 + NUL） |
| 8    | 16     | 保险库 UUID 的原始字节                                      |
| 24   | 8      | 非零密钥代次                                                |
| 32   | 4      | Argon2 内存：65536 KiB                                      |
| 36   | 4      | Argon2 迭代：3                                              |
| 40   | 4      | Argon2 并行度：1                                            |
| 44   | 16     | 随机盐                                                      |
| 60   | 24     | DEK 封装 nonce                                              |
| 84   | 48     | 封装后的 32 字节 DEK + 16 字节认证标签                      |
| 132  | 1      | 用途：1=本地状态，2=云端/备份快照，3=内存中的密码验证       |
| 133  | 24     | 本次载荷加密 nonce                                          |
| 157  | 可变   | CBOR 载荷密文，尾部为 16 字节认证标签                       |

DEK 封装的 AAD 为 UTF-8 `kvault/v2/password-wrap` + NUL + 文件前 84 字节。载荷的 AAD 为完整 132 字节文件头和 1 字节用途。解析先校验版本、长度、用途、非零代次和固定 KDF 参数，再派生密钥，拒绝任意增大计算成本。密文、盐、nonce、封装或文件头发生修改都会被拒绝；文件大小、UUID 和请求时间仍可被本机或存储服务观察。

文件头指纹为上述 132 字节的 SHA-256，用来绑定本机系统解锁授权。明文修改 `config.json` 的授权引用不能生成解密能力。

## 可选系统解锁

功能在“设置 → 安全与备份 → 系统解锁”中显式启用，默认关闭。启用须重新确认主密码并完成系统验证。每次解锁仍可以直接输入主密码；取消验证、设备不可用或授权失效都不会关闭主密码入口。前端不会在启动或打开设置时自动弹出系统验证。

### macOS

Swift/CryptoKit 桥接生成带有 `.privateKeyUsage + .userPresence` 访问控制的 Secure Enclave P-256 密钥。软件临时 P-256 密钥与其公钥进行 ECDH，使用 HKDF-SHA256 派生封装密钥，再用 ChaChaPoly 加密 DEK。本机账户引用参与 HKDF 上下文和 AEAD 认证。

login Keychain 只保存二进制封装：Secure Enclave 的不透明私钥表示、临时公钥、盐和密文。恢复不透明表示后，必须调用硬件私钥完成 ECDH 才能取回 DEK；该操作触发 Touch ID 或系统允许的本机验证。每次使用全新 LAContext，重用时长为 0，完成后立即 invalidate。该方案不把明文 DEK 放进普通钥匙串，也不依赖需要额外授权的 Data Protection Keychain 条目；ad-hoc 构建可继续使用，首次访问或应用签名变化时仍可能有系统钥匙串访问提示。

需要 macOS 11+、Secure Enclave 和可用的设备所有者验证。不具备条件时开关禁用，使用主密码。Swift 输出的临时缓冲区复制进 Rust `Zeroizing` 后清零释放。

### Windows

通过原生 Win32 WebAuthn PRF（hmac-secret）和 AES-256-GCM 封装 DEK。不保留 Passport RSA、DPAPI 或旧版 Windows 的兼容后端，不引入第三方生物识别插件。系统解锁仍为默认关闭的本机选项，主密码始终可用。

能力检查分两阶段：启动和启用前先要求 WebAuthn API ≥ 8 且本机用户验证认证器可用；首次启用再注册驻留平台凭据，要求 PRF、`credProtect=userVerificationRequired` 和注册时 PRF 求值。返回值必须包含 32 字节 PRF 输出；验证 RP ID 哈希及 UP/UV 标志，拒绝可备份/已备份凭据，只接受内部传输的本机认证器。不能仅凭 Hello 弹窗成功就认为 PRF 可用。

稳定 RP ID 为 `kvault.taurusxin.com`，对应 client-data origin 只用于本地 WebAuthn 调用，不发起网络请求。每次注册使用随机 32 字节用户句柄，每次封装使用随机 32 字节 PRF 输入和随机 12 字节 nonce。PRF 输出直接作为 AES-256-GCM 密钥，不保存；AAD 认证协议版本、算法域、RP ID、账户引用、凭据 ID、PRF 输入和 nonce。账户引用包含本机目录哈希和保险库文件头指纹，绑定保险库 UUID 与密钥代次。

本机凭据条目为 `webauthn-prf:<account>`。二进制封装以 `KVPRF\x01\0\0` 开头，随后为 2 字节小端凭据 ID 长度、凭据 ID（1–2048 字节）、32 字节 PRF 输入、12 字节 nonce、48 字节 DEK 密文及认证标签；最大 2150 字节。严格校验版本与长度，不解析旧 RSA 条目。

启用时先通过一次独立 assertion 重现 PRF 并实际解密验证，成功写入 Credential Manager 后，命令层才保存启用标记。已有当前格式凭据通过新的用户验证复用，避免重新注册提前替换已生效授权。取消或保存失败保留已有封装；失败的新注册会尝试删除。每次解锁都发起新的 assertion，只允许指定的凭据 ID，并再次要求 UV、设备绑定和正确 PRF。原生输出与 Rust PRF/DEK 缓冲区在使用后清理，前端不接收密钥。

Windows 错误显示具体阶段和十六进制 `HRESULT`，取消独立返回；能力不足时明确提示更新 Windows 或使用主密码。旧 Windows 系统授权需要使用主密码重新启用，不迁移旧授权。交叉类型检查和协议测试不等于 Windows Hello 实机验收，详见 [Windows Hello 调查与复测](windows-hello-investigation.md)。

### 授权清理与并发

关闭系统解锁会删除本机封装与授权标记，Windows 同时删除相应 WebAuthn 平台凭据。密码变更、更换保险库或迁移到新密钥代次时清除旧授权，需重新启用。macOS 删除的是本机封装；不要将系统凭据条目复制为额外备份。

原生验证不持有保险库互斥锁。返回结果提交前校验会话代次及文件头；验证期间发生锁定或更换保险库时拒绝迟到结果，清理尚未提交的新授权。恢复日志或换密暂存文件存在时，使用主密码处理恢复。

## 验证边界

自动化测试用可注入的内存凭据库验证可选启用、错误主密码、取消、锁定竞态、换密撤销、凭据失败时的本地编辑，以及磁盘无业务明文。原生钥匙串、Secure Enclave 和 Windows Hello 需要各平台实际验收，不由测试替身代替。没有独立安全审计结论。

2026-10-25 在本机 macOS 的独立测试保险库中完成主密码解锁、默认关闭的设置展示、本人完成系统验证后的启用、锁定后的系统解锁，以及取消系统验证后使用主密码解锁。磁盘核对确认 v2 magic、业务数据无明文、文件权限 `0600`，配置只含非敏感字段。Windows 安全模块完成 Rust 交叉编译检查；真实 Windows Hello 和 Linux 安装包仍待对应设备验收。

## 插件选择

官方 [`tauri-plugin-biometric`](https://v2.tauri.app/plugin/biometric/) 面向 Android/iOS；第三方 [`tauri-plugin-biometry`](https://github.com/Choochmeque/tauri-plugin-biometry) 支持桌面和受验证保护的存储，是可减少平台维护工作的候选，并非仅提供验证弹窗。

本次 macOS 保留原生适配的具体约束是 ad-hoc 发布兼容性：该插件的 macOS 存储走 Data Protection Keychain，而本机同路径探针返回 `errSecMissingEntitlement (-34018)`。当前实现使用 Secure Enclave 操作保护密钥，login Keychain 只存封装，已经通过实际验证。该取舍不代表自定义代码天然比插件安全；如后续采用插件，应先核对指定发布版本、签名条件和平台要求，并从 Rust 调用安全存取接口，避免将 DEK 暴露给前端。`SecretStore` 隔离了平台差异，替换适配无需改变业务加密格式。

参考：[Apple Secure Enclave 私钥 API](https://developer.apple.com/documentation/cryptokit/secureenclave/p256/keyagreement/privatekey)、[Microsoft WebAuthn](https://learn.microsoft.com/en-us/windows/win32/api/webauthn/)、[keyring 平台适配](https://docs.rs/keyring/3.6.3/keyring/)。
