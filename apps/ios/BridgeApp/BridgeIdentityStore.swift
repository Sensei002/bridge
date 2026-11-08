import Foundation
import Security

enum BridgeIdentityStoreError: Error {
    case randomGenerationFailed(OSStatus)
    case keychainFailure(OSStatus)
    case invalidStoredIdentity
}

/// Stores the device's 32-byte Noise static private key in the iOS Keychain.
/// The key is device-only and is not included in iCloud or device backups.
enum BridgeIdentityStore {
    private static let service = "app.bridge.ios.identity"
    private static let account = "noise-static-private-key"

    static func loadOrCreate() throws -> Data {
        if let existing = try read() {
            guard existing.count == 32 else {
                throw BridgeIdentityStoreError.invalidStoredIdentity
            }
            return existing
        }

        var key = Data(count: 32)
        let status = key.withUnsafeMutableBytes { buffer in
            SecRandomCopyBytes(kSecRandomDefault, 32, buffer.baseAddress!)
        }
        guard status == errSecSuccess else {
            throw BridgeIdentityStoreError.randomGenerationFailed(status)
        }

        let query: [String: Any] = [
            kSecClass as String: kSecClassGenericPassword,
            kSecAttrService as String: service,
            kSecAttrAccount as String: account,
            kSecValueData as String: key,
            kSecAttrAccessible as String: kSecAttrAccessibleAfterFirstUnlockThisDeviceOnly
        ]
        let addStatus = SecItemAdd(query as CFDictionary, nil)
        if addStatus == errSecDuplicateItem, let winner = try read() {
            return winner
        }
        guard addStatus == errSecSuccess else {
            throw BridgeIdentityStoreError.keychainFailure(addStatus)
        }
        return key
    }

    private static func read() throws -> Data? {
        let query: [String: Any] = [
            kSecClass as String: kSecClassGenericPassword,
            kSecAttrService as String: service,
            kSecAttrAccount as String: account,
            kSecReturnData as String: true,
            kSecMatchLimit as String: kSecMatchLimitOne
        ]
        var result: CFTypeRef?
        let status = SecItemCopyMatching(query as CFDictionary, &result)
        if status == errSecItemNotFound { return nil }
        guard status == errSecSuccess, let data = result as? Data else {
            throw BridgeIdentityStoreError.keychainFailure(status)
        }
        return data
    }
}
