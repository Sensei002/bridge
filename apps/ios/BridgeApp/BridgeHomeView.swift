import SwiftUI

private let accent = Color(red: 0.13, green: 0.83, blue: 0.93)
private let amber = Color(red: 0.98, green: 0.75, blue: 0.14)
private let bridgeBackground = Color(red: 0.04, green: 0.045, blue: 0.06)

struct BridgeHomeView: View {
    @StateObject private var discovery = BridgeDiscovery()
    @StateObject private var pairing = BridgePairingController()
    @State private var identityStatus = "Preparing secure device identity…"
    @State private var remoteCodes: [UInt64: String] = [:]

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
