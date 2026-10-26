# Windows Hello 启用后报错调查

## 现象与定位范围

用户反馈 Windows Hello 提示可以正常出现，确认后应用显示「Windows Hello 不可用或授权已失效，请使用主密码并重新启用」。旧实现将所有 CNG 非取消错误统一转换为这句话，未保留调用阶段和状态码，因此目前不能据此确认是设备验证失败、提供程序兼容问题还是密钥操作失败。

启用路径依次为：删除旧授权、创建 Passport RSA 密钥、设置策略、`NCryptFinalizeKey`（可能出现 Hello 提示）、读取并校验策略、RSA-OAEP/SHA256 加密、写入 Credential Manager、重新打开密钥并完成一次实际解密验证。应用仅在全部成功后保存启用标记。Hello 弹窗确认成功不代表后面的步骤已经成功。

## 本次处理

- 每个 CNG 失败附带操作阶段和 `0xXXXXXXXX` 状态码；属性失败同时显示属性名，取消仍单独返回 `auth_cancelled`。
- 对认证缓存属性增加旧名 `NgcCacheTypeProperty` 兼容，仅在 `NgcCacheType` 返回不支持或无效参数时尝试；读写均仍要求认证强制策略。
- 校验密钥允许解密，同时保留禁止导入、禁止导出和 2048 位长度检查。
- 启用失败后的凭据清理不再覆盖原始 CNG 错误，保留最初失败位置。
- 保持 OAEP/SHA256 和账户 label；未根据猜测降级 padding、认证策略或软件密钥。

属性兼容处理参考 [KeePassWinHello 提供程序实现](https://github.com/sirAndros/KeePassWinHello/blob/master/src/AuthProviders/WinHelloProvider.cs)。Microsoft 的 [NCryptEncrypt](https://learn.microsoft.com/en-us/windows/win32/api/ncrypt/nf-ncrypt-ncryptencrypt) 和 [NCryptGetProperty](https://learn.microsoft.com/en-us/windows/win32/api/ncrypt/nf-ncrypt-ncryptgetproperty) 文档说明了 padding、属性和状态返回规则。Passport 扩展属性仍有平台兼容性边界，不能用 macOS 上的类型检查证明其可用。

## Windows 复测

使用独立测试保险库，启用系统解锁并完成 Hello 提示。如果仍失败，记录界面中的完整阶段和错误码，不需要提供主密码、PIN 或业务数据。若阶段是 `Export Policy`、`NgcCacheType` 或 `Key Usage`，调查提供程序属性支持；若是 RSA-OAEP/SHA256，则调查该设备提供程序的 padding、hash 和 label 支持；若是解密授权，则检查 Hello/TPM 状态与返回码。

成功后需分别验证重启后的 Hello 解锁、每次使用需要验证、取消后主密码仍可用，以及关闭系统解锁和修改主密码会撤销旧授权。本次没有 Windows 实机环境，根因仍等待原生错误码确认。
