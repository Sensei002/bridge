import SwiftUI

private let accent = Color(red: 0.13, green: 0.83, blue: 0.93)
private let amber = Color(red: 0.98, green: 0.75, blue: 0.14)
private let bridgeBackground = Color(red: 0.04, green: 0.045, blue: 0.06)

struct BridgeHomeView: View {
    @State private var identityStatus = "Preparing secure device identity…"

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
                        Text("PAIRING NOT AVAILABLE YET").font(.system(size: 9, weight: .bold))
                            .tracking(1.4).foregroundStyle(accent).padding(.top, 8)
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
                _ = try BridgeIdentityStore.loadOrCreate()
                identityStatus = "Device identity protected by iOS Keychain"
            } catch {
                identityStatus = "Secure identity unavailable: \\(error.localizedDescription)"
            }
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
