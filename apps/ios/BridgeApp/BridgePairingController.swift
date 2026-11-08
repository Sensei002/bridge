import Foundation
import Combine
import Darwin

struct BridgePendingPairing: Identifiable {
    let handle: UInt64
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
    private var stopped = false

    func configure(privateKey: Data, deviceId: String, displayName: String) {
        stopped = false
        privateKeyHex = privateKey.map { String(format: "%02x", $0) }.joined()
        localDescriptor = [
            "id": deviceId,
            "display_name": String(displayName.prefix(64)),
            "platform": "ios",
            "protocol_version": 1
        ]
        do {
            trusted = try BridgeTrustedDeviceStore.load()
        } catch {
            status = "Trusted-device Keychain is unavailable: \(error.localizedDescription)"
            return
        }
        status = "iOS Keychain identity and trusted-device store ready · shared Rust Noise XX enabled."
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

    func pair(_ peer: BridgePeer, expectedFingerprint: String? = nil) {
        let known = trusted.first(where: { $0.deviceId == peer.deviceId })
        let pinnedFingerprint = expectedFingerprint ?? known?.fingerprint
        let request = makeRequest(host: peer.host, expectedFingerprint: pinnedFingerprint, expectedDeviceId: peer.deviceId, port: peer.port)
        Task {
            let response = await Task.detached(priority: .userInitiated) {
                BridgeNativeClient.pairConnect(request: request)
            }.value
            handle(response)
        }
    }

    func approve(_ item: BridgePendingPairing) {
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
        do {
            try BridgeTrustedDeviceStore.save(next)
        } catch {
            BridgeNativeClient.sessionClose(item.handle)
            status = "Could not persist trusted-device approval: \(error.localizedDescription)"
            return
        }
        let sent = BridgeNativeClient.sessionSend(item.handle, message: #"{ "type":"pairing_decision","decision":"accepted" }"#)
        guard (try? JSONSerialization.jsonObject(with: Data(sent.utf8)) as? [String: Any])?["ok"] as? Bool == true else {
            try? BridgeTrustedDeviceStore.save(trusted)
            BridgeNativeClient.sessionClose(item.handle)
            pending.removeAll { $0.id == item.id }
            status = "Could not confirm pairing with the requesting device."
            return
        }
        trusted = next
        activeSessions[item.deviceId] = item.handle
        pending.removeAll { $0.id == item.id }
        status = "\(item.name) is now connected and trusted."
    }

    func reject(_ item: BridgePendingPairing) {
        _ = BridgeNativeClient.sessionSend(item.handle, message: #"{ "type":"pairing_decision","decision":"rejected" }"#)
        BridgeNativeClient.sessionClose(item.handle)
        pending.removeAll { $0.id == item.id }
        status = "Pairing request rejected."
    }

    private func finishInitiatorApproval(handle: UInt64, remote: [String: Any], fingerprint: String, response: String) {
        guard let data = response.data(using: .utf8),
              let root = try? JSONSerialization.jsonObject(with: data) as? [String: Any],
              root["ok"] as? Bool == true,
              let message = root["message"] as? String,
              let decisionData = message.data(using: .utf8),
              let decision = try? JSONSerialization.jsonObject(with: decisionData) as? [String: Any],
              decision["type"] as? String == "pairing_decision",
              decision["decision"] as? String == "accepted" else {
            BridgeNativeClient.sessionClose(handle)
            status = "Pairing was not approved by the other device."
            return
        }
        let deviceId = remote["id"] as? String ?? ""
        let name = remote["display_name"] as? String ?? "Bridge device"
        let platform = remote["platform"] as? String ?? "unknown"
        if let old = trusted.first(where: { $0.deviceId == deviceId }), old.fingerprint != fingerprint {
            BridgeNativeClient.sessionClose(handle)
            status = "Trusted device identity changed. Connection rejected."
            return
        }
        var next = trusted.filter { $0.deviceId != deviceId }
        next.append(BridgeTrustedDevice(deviceId: deviceId, name: name, platform: platform,
                                        fingerprint: fingerprint, pairedAt: Date().timeIntervalSince1970))
        do {
            try BridgeTrustedDeviceStore.save(next)
            trusted = next
            activeSessions[deviceId] = handle
            status = "\(name) accepted. Device connected."
        } catch {
            BridgeNativeClient.sessionClose(handle)
            status = "Could not save the trusted device: \(error.localizedDescription)"
        }
    }

    func revoke(_ deviceId: String) {
        let next = trusted.filter { $0.deviceId != deviceId }
        do {
            try BridgeTrustedDeviceStore.save(next)
        } catch {
            status = "Could not persist device revocation: \(error.localizedDescription)"
            return
        }
        trusted = next
        if let handle = activeSessions.removeValue(forKey: deviceId) {
            BridgeNativeClient.sessionClose(handle)
        }
        status = "Device trust revoked and its active session closed."
    }

    func stop() {
        stopped = true
        acceptTask?.cancel()
        acceptTask = nil
        activeSessions.values.forEach(BridgeNativeClient.sessionClose)
        activeSessions.removeAll()
        pending.forEach { BridgeNativeClient.sessionClose($0.handle) }
        pending.removeAll()
    }

    private func makeRequest(host: String? = nil, bindIp: String? = nil, expectedFingerprint: String? = nil, expectedDeviceId: String? = nil, port: Int = 45821) -> String {
        var request: [String: Any] = [
            "port": port,
            "timeout_ms": 1_000,
            "private_key_hex": privateKeyHex,
            "local_device": localDescriptor,
            "expected_fingerprint": expectedFingerprint as Any? ?? NSNull(),
            "expected_device_id": expectedDeviceId as Any? ?? NSNull()
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
                let error = result["error"] as? String ?? "Secure pairing failed."
                if error != "pairing listener timed out" { status = error }
            } else {
                status = "Secure pairing returned an invalid response."
            }
            return
        }
        let handle = handleNumber.uint64Value
        if stopped {
            BridgeNativeClient.sessionClose(handle)
            return
        }
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
        if result["role"] as? String == "initiator" {
            status = "Request sent to \(name). Waiting for approval on that device…"
            Task.detached(priority: .userInitiated) {
                let response = BridgeNativeClient.sessionReceive(handle)
                await self.finishInitiatorApproval(handle: handle, remote: remote, fingerprint: fingerprint, response: response)
            }
            return
        }
        pending.append(BridgePendingPairing(
            handle: handle, fingerprint: fingerprint,
            deviceId: deviceId, name: name, platform: platform
        ))
        status = "Pairing request from \(name). Review the request to allow this device."
    }

}

struct StoredDevice: Codable {
    let deviceId: String
    let name: String
    let platform: String
    let fingerprint: String
    let pairedAt: Double
}

