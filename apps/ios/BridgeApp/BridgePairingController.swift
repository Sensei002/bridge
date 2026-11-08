import Foundation
import Combine

struct BridgePendingPairing: Identifiable {
    let handle: UInt64
    let sas: String
    let fingerprint: String
    let deviceId: String
    let name: String
    let platform: String
    var id: UInt64 { handle }
}

struct BridgeTrustedDevice: Identifiable {
    let deviceId: String
    let name: String
    let platform: String
    let fingerprint: String
    let pairedAt: Double
    var id: String { deviceId }
}

/// Main-actor coordinator for native iOS pairing. The shared Rust library
/// performs the exact same Noise XX handshake and encrypted transport as Windows.
@MainActor
final class BridgePairingController: ObservableObject {
    @Published private(set) var pending: [BridgePendingPairing] = []
    @Published private(set) var trusted: [BridgeTrustedDevice] = []
    @Published private(set) var status = "Secure pairing is starting…"

    private var privateKeyHex = ""
    private var localDescriptor: [String: Any] = [:]
    private var acceptTask: Task<Void, Never>?
    private var activeSessions: [String: UInt64] = [:]
    private let storeKey = "bridge.trusted.devices.v1"

    func configure(privateKey: Data, deviceId: String, displayName: String) {
        privateKeyHex = privateKey.map { String(format: "%02x", $0) }.joined()
        localDescriptor = [
            "id": deviceId,
            "display_name": String(displayName.prefix(64)),
            "platform": "ios",
            "protocol_version": 1
        ]
        trusted = loadTrusted()
        status = "iOS Keychain identity ready · shared Rust Noise XX enabled."
        let request = makeRequest(bindIp: "0.0.0.0")
        acceptTask = Task {
            while !Task.isCancelled {
                let response = await Task.detached(priority: .userInitiated) {
                    BridgeNativeClient.pairAccept(request: request)
                }.value
                handle(response)
            }
        }
    }

    func pair(_ peer: BridgePeer) {
        let known = trusted.first(where: { $0.deviceId == peer.deviceId })
        let request = makeRequest(host: peer.host, expectedFingerprint: known?.fingerprint)
        Task {
            let response = await Task.detached(priority: .userInitiated) {
                BridgeNativeClient.pairConnect(request: request)
            }.value
            handle(response)
        }
    }

    func approve(_ item: BridgePendingPairing, otherScreenCode: String) {
        guard otherScreenCode.count == 6,
              otherScreenCode.allSatisfy({ $0.isNumber }),
              otherScreenCode == item.sas else {
            status = "Verification code mismatch. Device was not trusted."
            return
        }
        if let old = trusted.first(where: { $0.deviceId == item.deviceId }),
           old.fingerprint != item.fingerprint {
            BridgeNativeClient.sessionClose(item.handle)
            pending.removeAll { $0.id == item.id }
            status = "Identity changed for an existing trusted device. Pairing blocked."
            return
        }
        var next = trusted.filter { $0.deviceId != item.deviceId }
        next.append(BridgeTrustedDevice(
            deviceId: item.deviceId, name: item.name, platform: item.platform,
            fingerprint: item.fingerprint, pairedAt: Date().timeIntervalSince1970
        ))
        guard saveTrusted(next) else {
            BridgeNativeClient.sessionClose(item.handle)
            status = "Could not persist trusted-device approval."
            return
        }
        trusted = next
        activeSessions[item.deviceId] = item.handle
        pending.removeAll { $0.id == item.id }
        status = "\(item.name) is trusted. Encrypted Noise session established."
    }

    func reject(_ item: BridgePendingPairing) {
        BridgeNativeClient.sessionClose(item.handle)
        pending.removeAll { $0.id == item.id }
        status = "Pairing rejected. No trust record was created."
    }

    func revoke(_ deviceId: String) {
        let next = trusted.filter { $0.deviceId != deviceId }
        guard saveTrusted(next) else {
            status = "Could not persist device revocation."
            return
        }
        trusted = next
        if let handle = activeSessions.removeValue(forKey: deviceId) {
            BridgeNativeClient.sessionClose(handle)
        }
        status = "Device trust revoked and its active session closed."
    }

    func stop() {
        acceptTask?.cancel()
        acceptTask = nil
        activeSessions.values.forEach(BridgeNativeClient.sessionClose)
        activeSessions.removeAll()
    }

    private func makeRequest(host: String? = nil, bindIp: String? = nil, expectedFingerprint: String? = nil) -> String {
        var request: [String: Any] = [
            "port": 45821,
            "private_key_hex": privateKeyHex,
            "local_device": localDescriptor,
            "expected_fingerprint": expectedFingerprint as Any? ?? NSNull()
        ]
        if let host { request["host"] = host }
        if let bindIp { request["bind_ip"] = bindIp }
        guard let data = try? JSONSerialization.data(withJSONObject: request),
              let value = String(data: data, encoding: .utf8) else { return "{}" }
        return value
    }

    private func handle(_ json: String) {
        guard let data = json.data(using: .utf8),
              let result = try? JSONSerialization.jsonObject(with: data) as? [String: Any],
              result["ok"] as? Bool == true,
              let remote = result["remote_device"] as? [String: Any],
              let handleNumber = result["handle"] as? NSNumber,
              let fingerprint = result["remote_fingerprint"] as? String else {
            if let data = json.data(using: .utf8),
               let result = try? JSONSerialization.jsonObject(with: data) as? [String: Any] {
                status = result["error"] as? String ?? "Secure pairing failed."
            } else {
                status = "Secure pairing returned an invalid response."
            }
            return
        }
        let handle = handleNumber.uint64Value
        let deviceId = remote["id"] as? String ?? ""
        let name = remote["display_name"] as? String ?? "Bridge device"
        let platform = remote["platform"] as? String ?? "unknown"
        if let existing = trusted.first(where: { $0.deviceId == deviceId }) {
            if existing.fingerprint == fingerprint {
                if let old = activeSessions.updateValue(handle, forKey: deviceId) {
                    BridgeNativeClient.sessionClose(old)
                }
                status = "Secure session re-established with \(existing.name)."
            } else {
                BridgeNativeClient.sessionClose(handle)
                status = "Peer identity mismatch. Existing trust was preserved."
            }
            return
        }
        guard let sas = result["sas"] as? String else {
            BridgeNativeClient.sessionClose(handle)
            status = "Handshake did not return a verification code."
            return
        }
        pending.append(BridgePendingPairing(
            handle: handle, sas: sas, fingerprint: fingerprint,
            deviceId: deviceId, name: name, platform: platform
        ))
        status = "Authenticated peer found. Compare the six-digit code on both devices."
    }

    private func loadTrusted() -> [BridgeTrustedDevice] {
        guard let data = UserDefaults.standard.data(forKey: storeKey),
              let values = try? JSONDecoder().decode([StoredDevice].self, from: data) else { return [] }
        return values.map {
            BridgeTrustedDevice(deviceId: $0.deviceId, name: $0.name, platform: $0.platform, fingerprint: $0.fingerprint, pairedAt: $0.pairedAt)
        }
    }

    private func saveTrusted(_ devices: [BridgeTrustedDevice]) -> Bool {
        let values = devices.map { StoredDevice(deviceId: $0.deviceId, name: $0.name, platform: $0.platform, fingerprint: $0.fingerprint, pairedAt: $0.pairedAt) }
        guard let data = try? JSONEncoder().encode(values) else { return false }
        UserDefaults.standard.set(data, forKey: storeKey)
        return UserDefaults.standard.data(forKey: storeKey) == data
    }
}

private struct StoredDevice: Codable {
    let deviceId: String
    let name: String
    let platform: String
    let fingerprint: String
    let pairedAt: Double
}
