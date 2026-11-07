import SwiftUI

private let accent = Color(red: 0.85, green: 1.0, blue: 0.38)
private let background = Color(red: 0.04, green: 0.045, blue: 0.06)

struct BridgeHomeView: View {
    var body: some View {
        ZStack {
            background.ignoresSafeArea()
            ScrollView {
                VStack(alignment: .leading, spacing: 24) {
                    HStack(spacing: 10) {
                        Text("B").font(.system(size: 20, weight: .bold, design: .rounded))
                            .foregroundStyle(background).frame(width: 34, height: 34)
                            .background(accent, in: RoundedRectangle(cornerRadius: 10))
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
                        Text("Secure device pairing is coming in a future update.")
                            .font(.system(size: 13)).foregroundStyle(.white.opacity(0.58))
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
