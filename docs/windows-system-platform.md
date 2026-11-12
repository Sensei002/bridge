# Bridge Windows System Platform

## Goal

Bridge for Windows should be a system-integrated device platform, not merely a desktop window. A Windows background service owns trusted-device sessions and capability permissions. The existing native Rust + egui app remains the control center. Optional drivers expose individual Windows device classes when Windows requires them.

## Architecture

### Bridge Service (user mode)

A small Rust Windows service starts with Windows and runs independently of the desktop UI. It owns device discovery, authenticated trusted sessions, reconnection, transfer orchestration, capability authorization, and service diagnostics. It exposes a narrow local IPC interface to the UI and driver user-mode components.

The service must not display UI. IPC must authenticate the caller, use restrictive Windows ACLs, validate protocol versions and message sizes, and expose only the operations each client needs. Closing the egui window must not stop the service or interrupt an active transfer.

### Bridge Desktop (existing egui app)

The desktop app is the management console for pairing, trust/revocation, connection status, transfers, capability permissions, driver health, updates, startup and privacy settings. A tray process may provide quick controls. The desktop app remains optional while the service runs.

### Capability drivers

Drivers are separate, optional, versioned packages. They must not implement Bridge networking, pairing, cryptography, or policy. Those remain in user mode and in the shared Rust protocol/core. Every capability has independent consent, a visible active state, and immediate revoke/stop controls.

#### Virtual monitor

Use Microsoft's Indirect Display Driver model (IDD / IddCx), not a custom GPU kernel driver. The IDD is a UMDF driver; Windows supplies desktop frames to it, and a user-mode Bridge pipeline encodes and streams them to the phone. The Android client decodes and displays frames and returns input events over the authenticated Bridge session. Prototype Microsoft's IddCx sample before integrating the Bridge transport.

#### Virtual microphone and audio

The goal is for the phone microphone to appear as a selectable Windows recording endpoint. This requires a virtual audio endpoint/adapter using a supported Windows audio driver model. Evaluate current Windows audio guidance and ACX/PortCls options before selecting the implementation. Keep capture, encoding, jitter buffering, resampling, clock-drift correction and network transport in user mode where possible; only the Windows endpoint boundary should be driver code. Test latency, sleep/wake, device removal and conferencing-app compatibility. A PC-to-phone playback endpoint is a separate capability.

#### Future peripherals

Camera, speaker/headphones, keyboard/mouse, gamepad and other device classes each need their own supported Windows device model, threat model and test plan. For example, virtual camera and virtual HID should be evaluated separately. No future driver is implied to exist just because it appears on this roadmap.

## Security and reliability

- A paired device is not automatically authorized to use every capability.
- The service authenticates peers and authorizes each capability; drivers never trust LAN discovery as identity.
- Driver interfaces accept only bounded, validated data and minimum required operations.
- Use bounded queues and backpressure. A slow or disconnected phone must not cause unbounded memory growth.
- On disconnect, stop frame/audio delivery and release resources. Device/service crashes must not crash Windows or leave stuck virtual devices.
- Do not add kernel-mode networking, custom Wi-Fi drivers, or kernel-mode cryptography.
- Production driver packages must be signed and tested on supported Windows versions and representative hardware. CI compilation alone does not make an unsigned driver safe or normally installable.

## Installer and updates

The current MSI is a per-user desktop installer and remains usable without drivers. A later elevated system installer must install/register the service and selected signed driver packages, handle upgrades/rollback/removal, validate signatures and compatibility, and request elevation or reboot only when required. Service and each driver need independent version and health reporting. Never ship test-signed or self-signed drivers as production packages.

## Delivery phases

### Phase A — Service foundation
1. Define service/UI IPC and capability permission contracts.
2. Extract long-lived connection ownership from the UI into a reusable background runtime.
3. Add the Rust Windows service host, lifecycle and recovery.
4. Add authenticated local IPC, health reporting, structured logs and diagnostics.
5. Add an elevated service installation path while preserving driver-free per-user installation.

### Phase B — Virtual monitor
1. Build and validate Microsoft's IddCx sample in CI.
2. Create a minimal Bridge IDD package that reports a virtual monitor to Windows.
3. Connect IDD frame delivery to the Bridge service through a bounded user-mode interface.
4. Add Android decoding, resolution/refresh selection, disconnect handling and input return.
5. Test sleep/resume, display hot-plug, driver removal and multiple GPU vendors on physical hardware.

### Phase C — Virtual audio
1. Prototype a virtual capture endpoint with the selected supported audio driver model.
2. Expose the Android microphone as a Windows recording device.
3. Add format negotiation, jitter buffering, clock correction, mute/consent and disconnect handling.
4. Optionally add a separate PC-to-phone render endpoint.
5. Validate latency and compatibility with common Windows audio APIs and conferencing apps.

### Phase D — Additional peripherals

Evaluate camera, HID and other device classes individually. Each requires a documented driver model, threat model, test plan and independent permission switch.

## Acceptance criteria

The platform is not complete until the service starts, stops, recovers and updates without the UI; trusted sessions survive UI closure; every capability is independently authorized and revoked; untrusted peers cannot inject data; phone/network loss cannot crash Windows or leave a stuck device; installer repair/upgrade/rollback/uninstall leaves no orphaned service or driver; and production drivers are signed and pass physical-device testing.
