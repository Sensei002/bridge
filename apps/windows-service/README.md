# Bridge Windows Display Service

This is the privileged Windows service boundary for Bridge Second Screen. It is a separate process from the normal Rust/egui desktop UI.

## Current development implementation

- Windows Service Control Manager entry point and clean stop/shutdown handling.
- Local named pipe: `\\.\pipe\BridgeDisplayService`.
- Versioned, bounded text protocol: `BRIDGE_DISPLAY/1 PING|QUERY|CONNECT|DISCONNECT`.
- Pipe DACL grants full access to LocalSystem and Administrators, and read/write access to interactive users. Remote network clients are not granted access.
- Driver client enumerates the Bridge device-interface GUID and sends the versioned IddCx QUERY/CONNECT/DISCONNECT IOCTLs.
- Portable protocol parser and response tests run in the Rust workspace.
- Windows x64 executable is built by GitHub Actions and uploaded as an unsigned development artifact.

## Security and installation boundary

This is an integration foundation, not an installable production service. The SCM installer/account configuration, service recovery policy, named-pipe client identity validation, authenticated Bridge-session binding, and driver signing are not complete.

The current interactive-user pipe ACL is intentionally a development boundary only: it does not yet prove that the connecting process is the genuine Bridge desktop client or that a trusted Android Second Screen session is active. Do not package or deploy it as a production service until those checks are implemented. The service must be installed under an identity authorized by the driver's INF security descriptor (currently LocalSystem or an administrator), while the desktop UI remains unprivileged.

The service does not start a monitor merely on startup. A future desktop integration must send CONNECT only after the authenticated Bridge session is confirmed, and DISCONNECT on session loss. Do not install the unsigned driver/service on a daily-use machine.

## Local protocol

Requests are one message per named-pipe connection, UTF-8, at most 256 bytes:

- `BRIDGE_DISPLAY/1 PING`
- `BRIDGE_DISPLAY/1 QUERY`
- `BRIDGE_DISPLAY/1 CONNECT`
- `BRIDGE_DISPLAY/1 DISCONNECT`

The service responds with a versioned status or error line. Unknown versions, malformed UTF-8, empty messages, and oversized requests are rejected.
