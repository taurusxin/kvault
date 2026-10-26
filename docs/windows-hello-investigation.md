# Windows Hello 启用后报错调查

## 当前实现：WebAuthn PRF

2026-10-26 已按产品决定移除 Passport CNG/RSA，改用原生 WebAuthn PRF + AES-256-GCM；不保留旧 Windows 兼容后端。下方 CNG 部分保留为历史调查，不能用其中的 API 阶段解释新版错误。当前格式与能力检查见 [存储说明](storage-security.md#windows)。

基础检查要求 WebAuthn API ≥ 8 和可进行用户验证的本机平台认证器，检查失败时设置开关禁用并显示原因。启用时必须实际取得 PRF 输出，并完成独立 assertion 和 AES-GCM 解封验证；基础检查通过不等于认证器一定支持 PRF。只有验证和凭据保存全部成功才设置启用标记。

## 现象与定位范围

用户反馈 Windows Hello 提示可以正常出现，确认后应用显示「Windows Hello 不可用或授权已失效，请使用主密码并重新启用」。旧实现将所有 CNG 非取消错误统一转换为这句话，未保留调用阶段和状态码，因此目前不能据此确认是设备验证失败、提供程序兼容问题还是密钥操作失败。

启用路径依次为：删除旧授权、创建 Passport RSA 密钥、设置策略、`NCryptFinalizeKey`（可能出现 Hello 提示）、读取并校验策略、RSA-OAEP/SHA256 加密、写入 Credential Manager、重新打开密钥并完成一次实际解密验证。应用仅在全部成功后保存启用标记。Hello 弹窗确认成功不代表后面的步骤已经成功。

## 原 CNG 实现的历史处理

- 每个 CNG 失败附带操作阶段和 `0xXXXXXXXX` 状态码；属性失败同时显示属性名，取消仍单独返回 `auth_cancelled`。
- 对认证缓存属性增加旧名 `NgcCacheTypeProperty` 兼容，仅在 `NgcCacheType` 返回不支持或无效参数时尝试；读写均仍要求认证强制策略。
- 校验密钥允许解密，同时保留禁止导入、禁止导出和 2048 位长度检查。
- 启用失败后的凭据清理不再覆盖原始 CNG 错误，保留最初失败位置。
- 保持 OAEP/SHA256 和账户 label；未根据猜测降级 padding、认证策略或软件密钥。

属性兼容处理参考 [KeePassWinHello 提供程序实现](https://github.com/sirAndros/KeePassWinHello/blob/master/src/AuthProviders/WinHelloProvider.cs)。Microsoft 的 [NCryptEncrypt](https://learn.microsoft.com/en-us/windows/win32/api/ncrypt/nf-ncrypt-ncryptencrypt) 和 [NCryptGetProperty](https://learn.microsoft.com/en-us/windows/win32/api/ncrypt/nf-ncrypt-ncryptgetproperty) 文档说明了 padding、属性和状态返回规则。Passport 扩展属性仍有平台兼容性边界，不能用 macOS 上的类型检查证明其可用。

## Windows 复测

使用独立测试保险库，启用系统解锁并完成 Hello 提示。如果仍失败，记录界面中的完整阶段和错误码，不需要提供主密码、PIN 或业务数据。若阶段是 `Export Policy`、`NgcCacheType` 或 `Key Usage`，调查提供程序属性支持；若是 RSA-OAEP/SHA256，则调查该设备提供程序的 padding、hash 和 label 支持；若是解密授权，则检查 Hello/TPM 状态与返回码。

成功后需分别验证重启后的 Hello 解锁、每次使用需要验证、取消后主密码仍可用，以及关闭系统解锁和修改主密码会撤销旧授权。本次没有 Windows 实机环境，根因仍等待原生错误码确认。

## 新版 Windows 实机验收

1. 用独立测试保险库，在“安全与备份”确认系统解锁默认关闭、主密码可用；检查基础能力提示。
2. 启用并确认测试主密码，完成保存本机通行密钥和独立验证。应成功保存启用状态；不返回 PRF、缺少 UV 或凭据可备份时必须失败。
3. 锁定后连续解锁两次，再重启应用解锁，每次均应有系统用户验证；取消一次后，主密码仍能解锁。
4. 验证禁用后本机平台凭据和 Credential Manager 封装被删除；更换主密码后旧授权不再可用。
5. 重置 Hello PIN 或删除系统中的对应通行密钥后，系统解锁应明确失败，主密码仍可恢复并重新启用。
6. 在 API < 8 或未设置 Hello 的环境，开关应禁用；API 足够但不返回 PRF 时，启用应失败且不提交授权。旧版 CNG 授权不迁移，使用主密码重新启用。

新版失败阶段为“检查本机认证器能力”“注册本机 PRF 凭据”“读取 PRF / Hello 验证”“撤销本机 PRF 凭据”等，附带 `HRESULT`。当前已执行的自动化检查覆盖协议与 Windows 模块类型，不代替上述原生提示验收。
