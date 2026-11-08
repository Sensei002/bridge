import Foundation
import Combine
import Darwin

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

    func configure(privateKey: Data, deviceId: String, displayName: String) {
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
        let request = makeRequest(host: peer.host, expectedFingerprint: pinnedFingerprint, port: peer.port)
        Task {
            let response = await Task.detached(priority: .userInitiated) {
                BridgeNativeClient.pairConnect(request: request)
            }.value
            handle(response)
        }
    }

    /// Create a short-lived invitation for a caller-supplied local IPv4 address.
    func createInvitation(host: String) throws -> String {
        let request: [String: Any] = [
            "device": localDescriptor,
            "host": host,
            "port": 45821,
            "fingerprint": try BridgeNativeClient.identityFingerprint(privateKeyHex: privateKeyHex),
            "now_ms": UInt64(max(0, Date().timeIntervalSince1970 * 1000))
        ]
        let data = try JSONSerialization.data(withJSONObject: request)
        let response = BridgeNativeClient.qrInvitationCreate(request: String(decoding: data, as: UTF8.self))
        guard let responseData = response.data(using: .utf8),
              let root = try JSONSerialization.jsonObject(with: responseData) as? [String: Any],
              root["ok"] as? Bool == true,
              let uri = root["uri"] as? String else {
            throw NSError(domain: "BridgePairing", code: 1, userInfo: [NSLocalizedDescriptionKey: "Could not create a valid QR invitation."])
        }
        return uri
    }

    func report(_ message: String) { status = message }

    static func localIPv4Address() throws -> String {
        var head: UnsafeMutablePointer<ifaddrs>?
        guard getifaddrs(&head) == 0, let first = head else {
            throw NSError(domain: "BridgePairing", code: 2, userInfo: [NSLocalizedDescriptionKey: "Could not inspect local network interfaces."])
        }
        defer { freeifaddrs(head) }
        for pointer in sequence(first: first, next: { $0.pointee.ifa_next }) {
            let item = pointer.pointee
            guard let address = item.ifa_addr,
                  Int32(address.pointee.sa_family) == AF_INET,
                  (Int32(item.ifa_flags) & IFF_UP) != 0,
                  (Int32(item.ifa_flags) & IFF_LOOPBACK) == 0,
                  String(cString: item.ifa_name).hasPrefix("en") else { continue }
            let ipv4 = address.withMemoryRebound(to: sockaddr_in.self, capacity: 1) { $0.pointee.sin_addr }
            var buffer = [CChar](repeating: 0, count: Int(INET_ADDRSTRLEN))
            var address = ipv4
            if inet_ntop(AF_INET, &address, &buffer, socklen_t(buffer.count)) != nil {
                let value = String(cString: buffer)
                if value.hasPrefix("10.") || value.hasPrefix("192.168.") || value.range(of: "^172\\.(1[6-9]|2[0-9]|3[0-1])\\.", options: .regularExpression) != nil {
                    return value
                }
            }
        }
        throw NSError(domain: "BridgePairing", code: 3, userInfo: [NSLocalizedDescriptionKey: "No private Wi-Fi IPv4 address found. Connect to Wi-Fi and try again."])
    }

    /// Consume a scanned QR nonce before using its endpoint; pin the fingerprint in Noise XX.
    func pairFromInvitation(_ uri: String) {
        let response = BridgeNativeClient.qrInvitationConsume(
            uri: uri, nowMs: UInt64(max(0, Date().timeIntervalSince1970 * 1000))
        )
        guard let data = response.data(using: .utf8),
              let root = try? JSONSerialization.jsonObject(with: data) as? [String: Any],
              root["ok"] as? Bool == true,
              let invite = root["invitation"] as? [String: Any],
              let host = invite["host"] as? String,
              let port = invite["port"] as? Int,
              let deviceId = invite["device_id"] as? String,
              let name = invite["display_name"] as? String,
              let fingerprint = invite["fingerprint"] as? String,
              let platform = invite["platform"] as? String else {
            status = "QR invitation rejected: invalid, expired, or already used."
            return
        }
        pair(BridgePeer(name: name, host: host, port: port, deviceId: deviceId,
                        fingerprint: fingerprint, platform: platform),
             expectedFingerprint: fingerprint)
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
        do {
            try BridgeTrustedDeviceStore.save(next)
        } catch {
            BridgeNativeClient.sessionClose(item.handle)
            status = "Could not persist trusted-device approval: \(error.localizedDescription)"
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
        acceptTask?.cancel()
        acceptTask = nil
        activeSessions.values.forEach(BridgeNativeClient.sessionClose)
        activeSessions.removeAll()
        pending.forEach { BridgeNativeClient.sessionClose($0.handle) }
        pending.removeAll()
    }

    private func makeRequest(host: String? = nil, bindIp: String? = nil, expectedFingerprint: String? = nil, port: Int = 45821) -> String {
        var request: [String: Any] = [
            "port": port,
            "timeout_ms": 1_000,
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
                let error = result["error"] as? String ?? "Secure pairing failed."
                if error != "pairing listener timed out" { status = error }
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

}

struct StoredDevice: Codable {
    let deviceId: String
    let name: String
    let platform: String
    let fingerprint: String
    let pairedAt: Double
}

