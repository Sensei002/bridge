import SwiftUI
import CoreImage.CIFilterBuiltins
import VisionKit

private let accent = Color(red: 0.13, green: 0.83, blue: 0.93)
private let amber = Color(red: 0.98, green: 0.75, blue: 0.14)
private let bridgeBackground = Color(red: 0.04, green: 0.045, blue: 0.06)

struct BridgeHomeView: View {
    @StateObject private var discovery = BridgeDiscovery()
    @StateObject private var pairing = BridgePairingController()
    @State private var identityStatus = "Preparing secure device identity…"
    @State private var remoteCodes: [UInt64: String] = [:]
    @State private var invitation = ""
    @State private var showInvitation = false
    @State private var showScanner = false

    var body: some View {
        ZStack {
            bridgeBackground.ignoresSafeArea()
            ScrollView {
                VStack(alignment: .leading, spacing: 24) {
                    HStack(spacing: 10) {
                        Image("BridgeBrand")
                            .resizable()
                            .scaledToFit()
                            .frame(width: 42, height: 42)
                        Text("BRIDGE").font(.system(size: 14, weight: .bold, design: .rounded)).tracking(3)
                    }.padding(.top, 20)
                    VStack(alignment: .leading, spacing: 10) {
                        eyebrow("YOUR ECOSYSTEM")
                        Text("One ecosystem.\nEvery device.").font(.system(size: 36, weight: .bold, design: .rounded)).tracking(-1)
                        Text("Bring your iPhone, Android devices and computers together in one private workspace.")
                            .font(.system(size: 14)).foregroundStyle(.white.opacity(0.58))
                    }.padding(.top, 24)
                    VStack(alignment: .leading, spacing: 12) {
                        eyebrow("BRIDGE NETWORK")
                        Text("No devices paired").font(.system(size: 20, weight: .semibold))
                        Text("Local-network discovery and secure pairing are being brought online in stages.")
                            .font(.system(size: 13)).foregroundStyle(.white.opacity(0.58))
                        Text(identityStatus).font(.system(size: 11, weight: .medium)).foregroundStyle(accent).padding(.top, 8)
                        Text(pairing.status).font(.system(size: 11, weight: .medium)).foregroundStyle(amber).padding(.top, 4)

                        HStack(spacing: 10) {
                            Button("Show my QR") {
                                do { invitation = try pairing.createInvitation(host: BridgePairingController.localIPv4Address()); showInvitation = true }
                                catch { pairing.report("Could not create QR invitation: \\(error.localizedDescription)") }
                            }
                            .buttonStyle(.borderedProminent).tint(accent)
                            Button("Scan QR") { showScanner = true }
                                .buttonStyle(.bordered).tint(accent)
                        }
                        Text("QR invitations expire after two minutes. Scanning only supplies an endpoint; Noise XX identity verification and approval are still required.")
                            .font(.system(size: 10)).foregroundStyle(.white.opacity(0.55))
                        Text("NEARBY DEVICES").font(.system(size: 9, weight: .bold)).tracking(1.2).foregroundStyle(accent).padding(.top, 12)
                        if discovery.peers.isEmpty {
                            Text("Searching for Bridge devices on your local network…").font(.system(size: 11)).foregroundStyle(.white.opacity(0.55))
                        } else {
                            ForEach(discovery.peers) { peer in
                                HStack {
                                    VStack(alignment: .leading, spacing: 3) {
                                        Text(peer.name).font(.system(size: 12, weight: .semibold))
                                        Text("\(peer.host):\(peer.port) · \(peer.platform) · unverified")
                                            .font(.system(size: 9)).foregroundStyle(.white.opacity(0.55))
                                    }
                                    Spacer()
                                    Button("Pair") { pairing.pair(peer) }
                                        .font(.system(size: 11, weight: .bold))
                                        .foregroundStyle(bridgeBackground)
                                        .padding(.horizontal, 12).padding(.vertical, 7)
                                        .background(accent, in: RoundedRectangle(cornerRadius: 8))
                                }
                            }
                        }

                        if !pairing.pending.isEmpty {
                            Text("VERIFY DEVICE").font(.system(size: 9, weight: .bold)).tracking(1.2).foregroundStyle(amber).padding(.top, 12)
                            ForEach(pairing.pending) { item in
                                VStack(alignment: .leading, spacing: 8) {
                                    Text(item.name).font(.system(size: 14, weight: .semibold))
                                    Text("Fingerprint · \(item.fingerprint.prefix(24))").font(.system(size: 9)).foregroundStyle(.white.opacity(0.55))
                                    Text(item.sas).font(.system(size: 32, weight: .bold, design: .monospaced)).foregroundStyle(accent)
                                    Text("Compare this code with the other device. Enter the code shown there. Never approve a mismatch.")
                                        .font(.system(size: 10)).foregroundStyle(.white.opacity(0.62))
                                    TextField("Code on the other device", text: Binding(
                                        get: { remoteCodes[item.id] ?? "" },
                                        set: { remoteCodes[item.id] = String($0.filter(\.isNumber).prefix(6)) }
                                    ))
                                    .keyboardType(.numberPad)
                                    .textFieldStyle(.roundedBorder)
                                    HStack {
                                        Button("Confirm and trust") {
                                            pairing.approve(item, otherScreenCode: remoteCodes[item.id] ?? "")
                                            remoteCodes.removeValue(forKey: item.id)
                                        }
                                        .font(.system(size: 11, weight: .bold))
                                        .foregroundStyle(bridgeBackground)
                                        .padding(.horizontal, 10).padding(.vertical, 8)
                                        .background(accent, in: RoundedRectangle(cornerRadius: 8))
                                        Button("Reject") { pairing.reject(item) }
                                            .font(.system(size: 11, weight: .semibold)).foregroundStyle(amber)
                                    }
                                }
                                .padding(12)
                                .background(bridgeBackground, in: RoundedRectangle(cornerRadius: 12))
                            }
                        }

                        Text("TRUSTED DEVICES").font(.system(size: 9, weight: .bold)).tracking(1.2).foregroundStyle(accent).padding(.top, 12)
                        if pairing.trusted.isEmpty {
                            Text("No trusted devices yet.").font(.system(size: 11)).foregroundStyle(.white.opacity(0.55))
                        } else {
                            ForEach(pairing.trusted) { device in
                                HStack {
                                    VStack(alignment: .leading, spacing: 3) {
                                        Text(device.name).font(.system(size: 12, weight: .semibold))
                                        Text("\(device.platform) · \(device.fingerprint.prefix(16))").font(.system(size: 9)).foregroundStyle(.white.opacity(0.55))
                                    }
                                    Spacer()
                                    Button("Revoke") { pairing.revoke(device.deviceId) }
                                        .font(.system(size: 10, weight: .bold)).foregroundStyle(amber)
                                }
                            }
                        }
                    }.padding(20).frame(maxWidth: .infinity, alignment: .leading)
                        .background(Color(red: 0.08, green: 0.095, blue: 0.105), in: RoundedRectangle(cornerRadius: 18))
                        .overlay(RoundedRectangle(cornerRadius: 18).stroke(.white.opacity(0.08)))
                    VStack(alignment: .leading, spacing: 16) {
                        eyebrow("CAPABILITIES")
                        capability("Clipboard sharing")
                        capability("Private file transfer")
                        capability("Connected devices")
                    }
                    Text("EARLY FOUNDATION BUILD · LOCAL-FIRST BY DESIGN")
                        .font(.system(size: 9, weight: .medium)).tracking(1)
                        .foregroundStyle(.white.opacity(0.4)).padding(.top, 18)
                }.padding(.horizontal, 24).padding(.bottom, 32)
            }
        }
        .sheet(isPresented: $showInvitation) {
            VStack(spacing: 16) {
                Text("Pair with Bridge").font(.system(size: 22, weight: .bold))
                if let image = makeQRCode(from: invitation) {
                    Image(uiImage: image).interpolation(.none).resizable().scaledToFit().frame(maxWidth: 280).padding(12).background(.white, in: RoundedRectangle(cornerRadius: 12))
                }
                Text("Scan this code on the other device. It expires in two minutes. Compare the six-digit code on both screens before trusting.")
                    .font(.system(size: 12)).multilineTextAlignment(.center).foregroundStyle(.secondary)
                Button("Done") { showInvitation = false }.buttonStyle(.borderedProminent).tint(accent)
            }.padding(24).presentationDetents([.medium, .large])
        }
        .sheet(isPresented: $showScanner) {
            BridgeQRScanner { value in
                showScanner = false
                pairing.pairFromInvitation(value)
            }
            .ignoresSafeArea()
        }
        .task {
            do {
                let privateKey = try BridgeIdentityStore.loadOrCreate()
                let keyHex = privateKey.map { String(format: "%02x", $0) }.joined()
                let fingerprint = try BridgeNativeClient.identityFingerprint(privateKeyHex: keyHex)
                let defaults = UserDefaults.standard
                let deviceId = defaults.string(forKey: "bridge.device.id") ?? UUID().uuidString.lowercased()
                defaults.set(deviceId, forKey: "bridge.device.id")
                let displayName = ProcessInfo.processInfo.hostName.isEmpty ? "Bridge iPhone" : ProcessInfo.processInfo.hostName
                pairing.configure(privateKey: privateKey, deviceId: deviceId, displayName: displayName)
                discovery.start(deviceId: deviceId, displayName: displayName, fingerprint: fingerprint)
                identityStatus = "Device identity protected by iOS Keychain · fingerprint \(fingerprint.prefix(12))"
            } catch {
                identityStatus = "Secure identity unavailable: \(error.localizedDescription)"
            }
        }
        .onDisappear {
            discovery.stop()
            pairing.stop()
        }
    }
    private func makeQRCode(from value: String) -> UIImage? {
        let filter = CIFilter.qrCodeGenerator()
        filter.message = Data(value.utf8)
        filter.correctionLevel = "M"
        guard let output = filter.outputImage,
              let cgImage = CIContext().createCGImage(output.transformed(by: CGAffineTransform(scaleX: 12, y: 12)), from: output.extent.applying(CGAffineTransform(scaleX: 12, y: 12))) else { return nil }
        return UIImage(cgImage: cgImage)
    }

    private func eyebrow(_ title: String) -> some View {
        Text(title).font(.system(size: 10, weight: .bold)).tracking(1.8).foregroundStyle(accent)
    }
    private func capability(_ title: String) -> some View {
        HStack {
            Text(title).font(.system(size: 14, weight: .medium))
            Spacer()
            Text("PLANNED").font(.system(size: 9, weight: .bold)).tracking(1).foregroundStyle(accent)
        }.padding(.vertical, 5)
    }
}


private struct BridgeQRScanner: UIViewControllerRepresentable {
    let onScan: (String) -> Void

    func makeUIViewController(context: Context) -> BridgeQRScannerController {
        let controller = BridgeQRScannerController()
        controller.onScan = onScan
        return controller
    }

    func updateUIViewController(_ uiViewController: BridgeQRScannerController, context: Context) {
        uiViewController.onScan = onScan
    }
}

private final class BridgeQRScannerController: UIViewController, DataScannerViewControllerDelegate {
    var onScan: ((String) -> Void)?
    private var scanner: DataScannerViewController?

    override func viewDidLoad() {
        super.viewDidLoad()
        view.backgroundColor = UIColor(red: 0.04, green: 0.045, blue: 0.06, alpha: 1)
        guard DataScannerViewController.isSupported, DataScannerViewController.isAvailable else { return }
        let scanner = DataScannerViewController(
            recognizedDataTypes: [.barcode(symbologies: [.qr])],
            qualityLevel: .balanced,
            isHighlightingEnabled: true
        )
        scanner.delegate = self
        self.scanner = scanner
        addChild(scanner)
        scanner.view.translatesAutoresizingMaskIntoConstraints = false
        view.addSubview(scanner.view)
        NSLayoutConstraint.activate([
            scanner.view.leadingAnchor.constraint(equalTo: view.leadingAnchor),
            scanner.view.trailingAnchor.constraint(equalTo: view.trailingAnchor),
            scanner.view.topAnchor.constraint(equalTo: view.topAnchor),
            scanner.view.bottomAnchor.constraint(equalTo: view.bottomAnchor)
        ])
        scanner.didMove(toParent: self)
        do { try scanner.startScanning() } catch { }
    }

    func dataScanner(_ dataScanner: DataScannerViewController, didAdd addedItems: [RecognizedItem], allItems: [RecognizedItem]) {
        for item in addedItems {
            if case .barcode(let barcode) = item, let value = barcode.payloadStringValue, value.hasPrefix("bridge://pair/") {
                dataScanner.stopScanning()
                onScan?(value)
                return
            }
        }
    }
}
