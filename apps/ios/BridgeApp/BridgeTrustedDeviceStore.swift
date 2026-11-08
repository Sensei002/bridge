import Foundation
import Security

/// Keychain-backed, device-only storage for public peer trust records.
enum BridgeTrustedDeviceStore {
    private static let service = "app.bridge.ios.trust"
    private static let account = "trusted-devices.v1"

    static func load() throws -> [BridgeTrustedDevice] {
        let query: [String: Any] = [
            kSecClass as String: kSecClassGenericPassword,
            kSecAttrService as String: service,
            kSecAttrAccount as String: account,
            kSecReturnData as String: true,
            kSecMatchLimit as String: kSecMatchLimitOne
        ]
        var result: CFTypeRef?
        let status = SecItemCopyMatching(query as CFDictionary, &result)
        if status == errSecItemNotFound { return [] }
        guard status == errSecSuccess, let data = result as? Data else {
            throw NSError(domain: "BridgeTrustStore", code: Int(status),
                          userInfo: [NSLocalizedDescriptionKey: "Could not read trusted devices from Keychain"])
        }
        let values = try JSONDecoder().decode([StoredDevice].self, from: data)
        return values.map {
            BridgeTrustedDevice(deviceId: $0.deviceId, name: $0.name, platform: $0.platform,
                                fingerprint: $0.fingerprint, pairedAt: $0.pairedAt)
        }
    }

    static func save(_ devices: [BridgeTrustedDevice]) throws {
        let values = devices.map {
            StoredDevice(deviceId: $0.deviceId, name: $0.name, platform: $0.platform,
                         fingerprint: $0.fingerprint, pairedAt: $0.pairedAt)
        }
        let data = try JSONEncoder().encode(values)
        let query: [String: Any] = [
            kSecClass as String: kSecClassGenericPassword,
            kSecAttrService as String: service,
            kSecAttrAccount as String: account
        ]
        let attributes: [String: Any] = [
            kSecValueData as String: data,
            kSecAttrAccessible as String: kSecAttrAccessibleAfterFirstUnlockThisDeviceOnly
        ]
        let status = SecItemUpdate(query as CFDictionary, attributes as CFDictionary)
        if status == errSecItemNotFound {
            var insert = query
            attributes.forEach { insert[$0.key] = $0.value }
            let addStatus = SecItemAdd(insert as CFDictionary, nil)
            guard addStatus == errSecSuccess else {
                throw NSError(domain: "BridgeTrustStore", code: Int(addStatus),
                              userInfo: [NSLocalizedDescriptionKey: "Could not save trusted devices to Keychain"])
            }
        } else if status != errSecSuccess {
            throw NSError(domain: "BridgeTrustStore", code: Int(status),
                          userInfo: [NSLocalizedDescriptionKey: "Could not update trusted devices in Keychain"])
        }
    }
}
