import Foundation
import CryptoKit
import LocalAuthentication
import Security

// An opaque Secure Enclave key representation is safe to persist in the login
// keychain. It cannot release the DEK without a private-key operation authorized
// by the OS. No protected-keychain entitlement or saved master password is used.
private struct DeviceEnvelope: Codable {
    let version: Int
    let key: Data
    let peer: Data
    let salt: Data
    let ciphertext: Data
}

private func context() -> LAContext {
    let value = LAContext()
    value.touchIDAuthenticationAllowableReuseDuration = 0
    value.localizedReason = "解锁 KVault 保险库"
    return value
}

private func open(_ bytes: Data, account: Data) throws -> Data {
    let envelope = try PropertyListDecoder().decode(DeviceEnvelope.self, from: bytes)
    guard envelope.version == 1, envelope.key.count <= 2048, envelope.peer.count == 65,
          envelope.salt.count == 32, envelope.ciphertext.count == 60 else {
        throw CocoaError(.coderReadCorrupt)
    }
    let auth = context()
    defer { auth.invalidate() }
    let key = try SecureEnclave.P256.KeyAgreement.PrivateKey(dataRepresentation: envelope.key, authenticationContext: auth)
    let peer = try P256.KeyAgreement.PublicKey(x963Representation: envelope.peer)
    // The OS prompts here: accessing key metadata alone does not authorize use.
    let shared = try key.sharedSecretFromKeyAgreement(with: peer)
    let wrappingKey = shared.hkdfDerivedSymmetricKey(using: SHA256.self, salt: envelope.salt,
        sharedInfo: Data("kvault/v2/system-unlock\0".utf8) + account, outputByteCount: 32)
    return try ChaChaPoly.open(ChaChaPoly.SealedBox(combined: envelope.ciphertext), using: wrappingKey, authenticating: account)
}

private func result(_ data: Data, _ output: UnsafeMutablePointer<UnsafeMutableRawPointer?>,
                    _ length: UnsafeMutablePointer<Int>) -> Int32 {
    guard let memory = malloc(data.count) else { return 3 }
    data.copyBytes(to: memory.assumingMemoryBound(to: UInt8.self), count: data.count)
    output.pointee = memory
    length.pointee = data.count
    return 0
}

private func errorCode(_ error: Error) -> Int32 {
    let value = error as NSError
    if value.domain == LAError.errorDomain && [LAError.userCancel.rawValue, LAError.appCancel.rawValue, LAError.systemCancel.rawValue].contains(value.code) { return 1 }
    if value.domain == NSOSStatusErrorDomain && value.code == Int(errSecUserCanceled) { return 1 }
    return 3
}

@_cdecl("kvault_system_available")
func systemAvailable() -> Int32 {
    guard SecureEnclave.isAvailable else { return 0 }
    return LAContext().canEvaluatePolicy(.deviceOwnerAuthentication, error: nil) ? 1 : 0
}

@_cdecl("kvault_system_enroll")
func systemEnroll(_ secret: UnsafePointer<UInt8>, _ secretLength: Int,
                  _ account: UnsafePointer<UInt8>, _ accountLength: Int,
                  _ output: UnsafeMutablePointer<UnsafeMutableRawPointer?>,
                  _ outputLength: UnsafeMutablePointer<Int>) -> Int32 {
    guard secretLength == 32, accountLength > 0, accountLength < 1024, systemAvailable() == 1 else { return 2 }
    do {
        let accountData = Data(bytes: account, count: accountLength)
        guard let access = SecAccessControlCreateWithFlags(nil, kSecAttrAccessibleWhenUnlockedThisDeviceOnly,
            [.privateKeyUsage, .userPresence], nil) else { return 2 }
        let enclave = try SecureEnclave.P256.KeyAgreement.PrivateKey(accessControl: access)
        let peer = P256.KeyAgreement.PrivateKey()
        let shared = try peer.sharedSecretFromKeyAgreement(with: enclave.publicKey)
        var salt = Data(count: 32)
        let randomStatus = salt.withUnsafeMutableBytes { SecRandomCopyBytes(kSecRandomDefault, 32, $0.baseAddress!) }
        guard randomStatus == errSecSuccess else { return 3 }
        let wrappingKey = shared.hkdfDerivedSymmetricKey(using: SHA256.self, salt: salt,
            sharedInfo: Data("kvault/v2/system-unlock\0".utf8) + accountData, outputByteCount: 32)
        var plain = Data(bytes: secret, count: secretLength)
        defer { plain.resetBytes(in: 0..<plain.count) }
        let box = try ChaChaPoly.seal(plain, using: wrappingKey, authenticating: accountData)
        let envelope = DeviceEnvelope(version: 1, key: enclave.dataRepresentation,
            peer: peer.publicKey.x963Representation, salt: salt, ciphertext: box.combined)
        let encoder = PropertyListEncoder()
        encoder.outputFormat = .binary
        let bytes = try encoder.encode(envelope)
        // Verify the complete roundtrip, including a fresh native authorization,
        // before the caller persists the opaque envelope in the login keychain.
        var check = try open(bytes, account: accountData)
        defer { check.resetBytes(in: 0..<check.count) }
        guard check == plain else { return 3 }
        return result(bytes, output, outputLength)
    } catch { return errorCode(error) }
}

@_cdecl("kvault_system_open")
func systemOpen(_ envelope: UnsafePointer<UInt8>, _ envelopeLength: Int,
                _ account: UnsafePointer<UInt8>, _ accountLength: Int,
                _ output: UnsafeMutablePointer<UnsafeMutableRawPointer?>,
                _ outputLength: UnsafeMutablePointer<Int>) -> Int32 {
    guard envelopeLength > 0, envelopeLength <= 2500, accountLength > 0, accountLength < 1024 else { return 2 }
    do {
        var plain = try open(Data(bytes: envelope, count: envelopeLength), account: Data(bytes: account, count: accountLength))
        defer { plain.resetBytes(in: 0..<plain.count) }
        guard plain.count == 32 else { return 3 }
        return result(plain, output, outputLength)
    } catch { return errorCode(error) }
}

@_cdecl("kvault_system_free")
func systemFree(_ memory: UnsafeMutableRawPointer?, _ count: Int) {
    guard let memory else { return }
    memset(memory, 0, count)
    free(memory)
}
