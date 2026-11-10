# Windows Display Service Authentication Design

## Status and non-negotiable gate

This document defines the security protocol required before Bridge Display Service may issue a session lease or send monitor CONNECT/DISCONNECT IOCTLs.

**Current implementation remains fail-closed.** PR #12 only records named-pipe PID/image-path attribution and contains an internal lease state machine. It does not authenticate the executable publisher, prove that a Noise session is live, or authorize monitor changes. Do not expose lease issuance through IPC until every gate below is implemented and tested.

A Windows code-signing identity is a prerequisite for the first gate. No publisher thumbprint or production Windows signing certificate is currently configured in this repository. An absent, malformed, or untrusted publisher pin must mean deny—not a development fallback.

## Trust boundaries

- The Windows service is the only component allowed to open the IddCx device interface for mutating IOCTLs.
- The Rust/egui desktop client runs as the interactive user and is not elevated.
- The Android peer is trusted by the existing Bridge pairing store and authenticated Noise XX handshake.
- Named-pipe ACLs reduce exposure but do not establish caller identity or authorization.
- PID, image path, process name, mDNS metadata, a peer fingerprint supplied by IPC, and a Boolean "connected" claim are not proof of authorization.
- The signed desktop client is part of the trusted computing base for attesting that it observed a challenge round-trip over its authenticated Noise session. A client signature must be checked against a pinned Bridge publisher identity.

## Required protocol

### 1. Verify the desktop client before accepting authorization messages

For every authorization-capable pipe request, the service must:

1. Obtain the client PID from the connected pipe handle.
2. Open and retain a process handle; reject if the process exits during verification or request handling.
3. Resolve the image path from that process handle, not from caller-supplied text.
4. Verify the image's Authenticode signature with Windows trust APIs.
5. Extract the actual signing certificate identity and compare it to an explicitly configured Bridge publisher pin. A generic "signed by any trusted publisher" result is insufficient.
6. Fail closed if the pin is absent, malformed, expired/revoked under the selected trust policy, or does not match.
7. Bind any ephemeral client key registration to that verified process instance. Never trust a key supplied by an unverified pipe client.

The publisher pin must be provisioned by the signed installer or another administrator-controlled configuration path. It must not be accepted from an ordinary user's pipe request, environment variable, or writable per-user setting. Certificate rotation needs an explicit overlap window and documented revocation procedure.

### 2. Prove that a trusted Android session is live

A future protocol revision should use a one-time service challenge:

1. The verified desktop client requests an authorization challenge. The service generates a cryptographically random nonce, records it as pending, and binds it to the verified process instance with a short expiry.
2. The desktop client sends the nonce and a display-session challenge message through the already-established encrypted Noise transport to the selected Android peer.
3. Android returns the challenge response only through that same authenticated Noise session. The desktop client must reject a response from any other connection or a session whose authenticated remote fingerprint differs from the selected trusted device.
4. The desktop client creates a canonical, versioned attestation containing the service nonce, a fresh desktop session identifier, the authenticated Android public-key fingerprint, and the observed challenge result. It signs the exact canonical bytes with a per-session ephemeral signing key.
5. The verified desktop client submits the attestation and ephemeral public key to the service. The service verifies the signature, one-time nonce, process binding, expiry, protocol version, fingerprint format, and that the fingerprint is in the desktop client's trusted-device store as part of the signed client's attestation logic.
6. Only then may the service issue a lease bound to that process instance, session identifier, and Android fingerprint.

The ephemeral key is not a substitute for publisher verification. It is useful only after the service has authenticated the signed Bridge client and bound key registration to that exact process instance. The desktop client is trusted to attest to the Noise challenge round-trip; this assumption must be explicit in the threat model.

### 3. Lease renewal and revocation

- Maximum lease lifetime: 15 seconds.
- The desktop client renews only while the same authenticated Noise session remains alive and the same verified process instance owns the lease.
- Every renewal must prove possession of the registered ephemeral key and bind the same lease ID, session ID, and Android fingerprint.
- On Noise disconnect, Android peer change, trust revocation, desktop shutdown, or explicit display disconnect, the desktop client requests revocation immediately.
- The service also expires the lease independently. If renewal stops, authorization expires within 15 seconds.
- Only one Android peer may own the single virtual display lease at a time.
- CONNECT and DISCONNECT must each re-check the active lease immediately before issuing the driver IOCTL. A prior successful QUERY or previously valid lease is not enough.

### 4. Replay, concurrency, and failure handling

- Use a CSPRNG-generated nonce with at least 256 bits of entropy.
- Challenges are single-use and expire quickly (target: 10 seconds).
- Compare IDs and hashes in constant time where applicable.
- Reject duplicate, expired, unknown, cross-process, cross-session, and already-consumed challenges.
- Serialize lease mutations and driver lifecycle transitions.
- A pipe disconnect, service restart, process exit, signature failure, malformed message, or uncertainty about session liveness must not preserve authorization.
- Service startup begins with no active lease. Driver monitor arrival is never inferred from a persisted lease.
- Bound all message sizes and reject unknown protocol versions/fields rather than guessing.

## Implementation order and acceptance gates

1. **Publisher verification:** implement Windows Authenticode verification and pinned publisher identity; unit/integration tests must cover unsigned, untrusted, wrong-publisher, missing-pin, and valid-publisher cases. CONNECT/DISCONNECT remain disabled.
2. **Pipe protocol:** add versioned challenge, key-registration, attestation, renewal, and revoke messages. Keep them unavailable until publisher verification passes.
3. **Noise challenge round-trip:** add an explicit Second Screen control message to the existing encrypted session, with Android and desktop tests proving that a challenge cannot be satisfied by another peer or stale session.
4. **Service binding:** connect the verified process instance, ephemeral attestation key, session identifier, Android fingerprint, and 15-second lease. Add replay, race, expiry, process-exit, and revocation tests.
5. **Enable monitor lifecycle:** only after all prior gates pass, enable CONNECT/DISCONNECT and test against a dedicated disposable Windows machine with a recoverable driver-install procedure.

## Current blocker

The repository does not yet have a configured Windows code-signing identity or pinned publisher identity. Bridge is preparing to apply for SignPath Foundation's free open-source Authenticode signing program for its own Windows user-mode binaries; approval, project configuration, and a first verified signed build are still required. The SignPath Foundation publisher identity must be taken from the actual issued certificate, not guessed. The IddCx driver has a separate Microsoft Hardware Dev Center/Partner Center signing requirement, which requires an EV certificate and account for attestation submission. Until the desktop publisher verification and live-session binding gates pass, the service must continue to reject CONNECT/DISCONNECT. Do not use a self-signed development certificate as a production trust substitute, and do not ask the user to install the unsigned driver or service on their daily-use machine.
