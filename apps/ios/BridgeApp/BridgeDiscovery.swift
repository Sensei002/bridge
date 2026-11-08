import Foundation
import Network
import Combine

/// Bonjour browser for local Bridge advertisements. Results are untrusted
/// candidates and must never be treated as authenticated devices.
@MainActor
final class BridgeDiscovery: ObservableObject {
    @Published private(set) var peers: [String] = []
    private var browser: NWBrowser?

    func start() {
        guard browser == nil else { return }
        let browser = NWBrowser(for: .bonjour(type: "_bridge._tcp", domain: nil), using: .tcp)
        browser.browseResultsChangedHandler = { [weak self] results, _ in
            let candidates = results.map { result -> String in
                switch result.endpoint {
                case .service(let name, _, _, _):
                    return "\(name) · awaiting authentication"
                default:
                    return "Bridge peer · awaiting authentication"
                }
            }.sorted()
            Task { @MainActor in self?.peers = candidates }
        }
        browser.stateUpdateHandler = { state in
            if case .failed(let error) = state {
                NSLog("Bridge Bonjour discovery failed: %@", error.localizedDescription)
            }
        }
        self.browser = browser
        browser.start(queue: .main)
    }

    func stop() {
        browser?.cancel()
        browser = nil
        peers = []
    }

    deinit {
        browser?.cancel()
    }
}
