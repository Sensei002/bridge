# Bridge Windows Display Service

This is the privileged Windows service boundary for Bridge Second Screen. It is a separate process from the normal Rust/egui desktop UI.

## Current development implementation

- Windows Service Control Manager entry point and clean stop/shutdown handling.
- Local named pipe: `\\.\pipe\BridgeDisplayService`.
- Versioned, bounded text protocol: `BRIDGE_DISPLAY/1 PING|QUERY|CONNECT|DISCONNECT`.
- Pipe DACL grants full access to LocalSystem and Administrators, and read/write access to interactive users. Remote network clients are not granted access.
- Captures the connected pipe client's process ID using the Windows named-pipe API and resolves its executable image path from the process handle.
- Driver client enumerates the Bridge device-interface GUID and sends the driver's versioned QUERY/CONNECT/DISCONNECT IOCTLs.
- Portable protocol parser and response tests run in the Rust workspace.
- Windows x64 executable is built by GitHub Actions and uploaded as an unsigned development artifact.

## Security and installation boundary

The client PID and image path are **attribution metadata, not authorization**. A path does not prove that a binary was published by Bridge, and a process identity does not prove that an authenticated Android Second Screen session is alive. The service does not yet validate a publisher signature, bind the caller to the active Windows logon, or receive a cryptographically verifiable session lease.

The current interactive-user pipe ACL is intentionally a development boundary only. CONNECT and DISCONNECT remain fail-closed and are rejected; only PING and read-only QUERY are available. Do not enable monitor lifecycle operations until the service has a verified Bridge desktop-client identity and a live authenticated-session binding with expiry/revocation handling.

The service library now contains a bounded single-device lease state machine: 15-second maximum TTL, peer-fingerprint binding, renewal, expiry, replacement, and explicit revocation. Unit tests cover these transitions. This state machine is not exposed over IPC and is not yet wired to a live Noise session; issuance must only happen after independent client and session authentication. CONNECT/DISCONNECT remain rejected.

The SCM installer/account configuration, service recovery policy, production signing, and driver signing are not complete. The service must be installed under an identity authorized by the driver's INF security descriptor (currently LocalSystem or an administrator), while the desktop UI remains unprivileged. Do not install the unsigned driver/service on a daily-use machine.

## Local protocol

Requests are one message per named-pipe connection, UTF-8, at most 256 bytes:

- `BRIDGE_DISPLAY/1 PING`
- `BRIDGE_DISPLAY/1 QUERY`
- `BRIDGE_DISPLAY/1 CONNECT` (currently rejected)
- `BRIDGE_DISPLAY/1 DISCONNECT` (currently rejected)

The service responds with a versioned status or error line. Unknown versions, malformed UTF-8, empty messages, and oversized requests are rejected.
