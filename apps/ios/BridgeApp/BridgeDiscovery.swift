import Foundation
import Combine

struct BridgePeer: Identifiable, Hashable {
    let name: String
    let host: String
    let port: Int
    let deviceId: String
    let fingerprint: String
    let platform: String
    var id: String { deviceId }
}

/// Bonjour advertises this device and resolves local candidates. Advertised
/// TXT metadata is untrusted until the Rust Noise XX handshake authenticates it.
@MainActor
final class BridgeDiscovery: NSObject, ObservableObject, NetServiceBrowserDelegate, NetServiceDelegate {
    @Published private(set) var peers: [BridgePeer] = []
    private var browser: NetServiceBrowser?
    private var publisher: NetService?
    private var resolving: [String: NetService] = [:]
    private var localDeviceId: String?

    func start(deviceId: String, displayName: String, fingerprint: String) {
        guard browser == nil else { return }
        localDeviceId = deviceId
        let txt: [String: Data] = [
            "id": Data(deviceId.utf8),
            "name": Data(displayName.prefix(64).utf8),
            "platform": Data("ios".utf8),
            "version": Data("1".utf8),
            "fingerprint": Data(fingerprint.utf8)
        ]
        let service = NetService(domain: "local.", type: "_bridge._tcp.", name: "Bridge-\(deviceId.prefix(8))", port: 45821)
        service.setTXTRecord(NetService.data(fromTXTRecord: txt))
        service.delegate = self
        publisher = service
        service.publish()

        let browser = NetServiceBrowser()
        browser.delegate = self
        self.browser = browser
        browser.searchForServices(ofType: "_bridge._tcp.", inDomain: "local.")
    }

    func stop() {
        browser?.stop()
        browser = nil
        publisher?.stop()
        publisher = nil
        resolving.values.forEach { $0.stop() }
        resolving.removeAll()
        peers = []
        localDeviceId = nil
    }

    func netServiceBrowser(_ browser: NetServiceBrowser, didFind service: NetService, moreComing: Bool) {
        guard service.name != publisher?.name else { return }
        resolving[service.name] = service
        service.delegate = self
        service.resolve(withTimeout: 5)
    }

    func netServiceBrowser(_ browser: NetServiceBrowser, didRemove service: NetService, moreComing: Bool) {
        peers.removeAll { $0.name == service.name }
        resolving.removeValue(forKey: service.name)
    }

    func netServiceDidResolveAddress(_ sender: NetService) {
        guard let host = sender.hostName?.trimmingCharacters(in: CharacterSet(charactersIn: ".")),
              let data = sender.txtRecordData() else { return }
        let values = NetService.dictionary(fromTXTRecord: data)
        func value(_ key: String) -> String {
            values[key].flatMap { String(data: $0, encoding: .utf8) } ?? ""
        }
        let deviceId = value("id")
        let fingerprint = value("fingerprint")
        guard !deviceId.isEmpty, deviceId != localDeviceId,
              fingerprint.count == 64,
              fingerprint.allSatisfy({ $0.isHexDigit }) else { return }
        let peer = BridgePeer(
            name: value("name").isEmpty ? sender.name : value("name"),
            host: host,
            port: sender.port,
            deviceId: deviceId,
            fingerprint: fingerprint,
            platform: value("platform")
        )
        if !peers.contains(where: { $0.deviceId == deviceId }) {
            peers.append(peer)
            peers.sort { $0.name.localizedCaseInsensitiveCompare($1.name) == .orderedAscending }
        }
    }

    func netService(_ sender: NetService, didNotResolve errorDict: [String: NSNumber]) {
        NSLog("Bridge Bonjour resolution failed: %@", errorDict.description)
    }

    func netServiceDidPublish(_ sender: NetService) {
        NSLog("Bridge Bonjour service published")
    }

    func netService(_ sender: NetService, didNotPublish errorDict: [String: NSNumber]) {
        NSLog("Bridge Bonjour publish failed: %@", errorDict.description)
    }
}
