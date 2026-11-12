# Bridge Windows Display Service

This is the privileged Windows service boundary for Bridge Second Screen. It is a separate process from the normal Rust/egui desktop UI.

## Current development implementation

- Windows Service Control Manager entry point and clean stop/shutdown handling.
- Local named pipe: `\\.\pipe\BridgeDisplayService`.
- Versioned, bounded text protocol: `BRIDGE_DISPLAY/1 PING|QUERY|CHALLENGE|CHALLENGE_RESPONSE|CONNECT|DISCONNECT`.
- `CHALLENGE` and `CHALLENGE_RESPONSE <64-hex-nonce>` require the configured Authenticode publisher pin. The service retains the original client process handle between requests, expires challenges after 10 seconds, and consumes each challenge once.
- The Windows desktop client now requests a challenge when its first live authenticated Android Noise session becomes active, relays the nonce over that encrypted session, and submits the echoed response over a second named-pipe connection. The relay is bounded by the service's 10-second challenge lifetime and an 8-second desktop wait.
- Pipe DACL grants full access to LocalSystem and Administrators, and read/write access to interactive users. Remote network clients are not granted access.
- Captures the connected pipe client's process ID using the Windows named-pipe API and resolves its executable image path from the process handle.
- Upstream driver-control client connects to `\\.\\pipe\\MTTVirtualDisplayPipe` and sends only the vendor protocol's UTF-16 `SETDISPLAYCOUNT 0` or `SETDISPLAYCOUNT 1` command. It does not forward arbitrary driver commands or expose the vendor pipe to Android/network peers.
- The vendor pipe accepts one command per connection and does not acknowledge display-count changes. Bridge reports connect/disconnect as a request, then independently enumerates Windows display adapters and matches the exact upstream INF hardware ID Root\\MttVDD. The topology query distinguishes driver presence from attachment to the Windows desktop. The legacy frame-read adapter remains temporarily present and is not used as evidence of VDD capture.
- Portable protocol parser and response tests run in the Rust workspace.
- Windows x64 executable is built by GitHub Actions and uploaded as an unsigned development artifact.

## Security and installation boundary

The client PID and image path are **attribution metadata, not authorization**. A path does not prove that a binary was published by Bridge, and a process identity does not prove that an authenticated Android Second Screen session is alive. The service validates the pinned Authenticode publisher for challenge commands, but does not yet bind the caller to the active Windows logon or receive a cryptographically verifiable session lease.

For Bridge's own service pipe, the service verifies the pinned desktop publisher, validates the attested desktop process and paired Android fingerprint, and requires the exact active 15-second lease for CONNECT/DISCONNECT. Lease renewal, expiry, process exit, peer replacement, and service shutdown trigger fail-safe monitor removal requests. The service's lease registry is not exposed directly over IPC.

**Residual upstream-driver limitation:** the selected vendor VDD creates `\\.\\pipe\\MTTVirtualDisplayPipe` with a broad local ACL that permits any local user to connect. Bridge uses only the fixed display-count commands and never forwards arbitrary input, but it cannot narrow the vendor pipe's ACL without modifying and re-signing the upstream driver. Other local processes may therefore also request display-count changes. This is a documented local attack surface; the pipe is never exposed to Android or the network.

The SCM installer/account configuration, service recovery policy, production signing, and driver signing are not complete. The service must be installed under an identity authorized by the driver's INF security descriptor (currently LocalSystem or an administrator), while the desktop UI remains unprivileged. Do not install the unsigned driver/service on a daily-use machine.

## Local protocol

Requests are one message per named-pipe connection, UTF-8, at most 256 bytes:

- `BRIDGE_DISPLAY/1 PING`
- `BRIDGE_DISPLAY/1 QUERY` → `BRIDGE_DISPLAY/1 OK vdd_present=0|1 attached=0|1` (actual Windows adapter enumeration; `attached=1` requires `vdd_present=1`)
- `BRIDGE_DISPLAY/1 CHALLENGE` → `BRIDGE_DISPLAY/1 CHALLENGE nonce=<64 hex chars>`
- `BRIDGE_DISPLAY/1 CHALLENGE_RESPONSE <64 hex chars>` → `BRIDGE_DISPLAY/1 OK challenge_verified`
- `BRIDGE_DISPLAY/1 CONNECT lease=<uuid> fingerprint=<64 hex>` → `BRIDGE_DISPLAY/1 OK connect_requested`
- `BRIDGE_DISPLAY/1 DISCONNECT lease=<uuid> fingerprint=<64 hex>` → `BRIDGE_DISPLAY/1 OK disconnect_requested`

The service responds with a versioned status or error line. Unknown versions, malformed UTF-8, empty messages, and oversized requests are rejected.


## Authentication proof requirement before lease IPC

The current challenge relay is not, by itself, a proof the privileged service can independently verify: the service sees a response from its signed desktop client, but it cannot distinguish a nonce actually echoed by Android over Noise from a desktop client that simply resubmits the nonce. The next protocol stage must add a peer-authenticated proof that the service can verify against a pairing-bound Android public key (or an equivalently strong independently verifiable mechanism). Do not expose lease issuance/renewal over the pipe or enable driver IOCTLs until that proof, process-handle binding, and disconnect/expiry revocation are implemented and tested.

## Development service installation

For a local Windows test machine, build the service executable with:

```powershell
cargo build --release --manifest-path apps/windows-service/Cargo.toml --features service-bin --target x86_64-pc-windows-msvc
```

From an elevated PowerShell window, install or update the service:

```powershell
.\scripts\windows\Install-BridgeDisplayService.ps1 -ServiceExecutable .\target\x86_64-pc-windows-msvc\release\bridge-display-service.exe -AllowUnsignedDevelopmentBuild
```

The explicit switch is required for the current unsigned development executable. Only use it with a binary built from trusted Bridge source. A future signed service build should omit the switch and pass Authenticode validation.

To remove the service registration:

```powershell
.\scripts\windows\Uninstall-BridgeDisplayService.ps1
```

The uninstall script removes only the Windows service registration; upstream VDD package installation/removal and installer rollback are not yet integrated. Do not treat the current development scripts as the finished one-installer setup.
