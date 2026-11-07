# QR pairing (Windows → Android)

1. Open Bridge on Windows and choose **Pair a device**. A QR code and six-digit one-time code are displayed.
2. On Android, open **Devices** and tap **Scan QR to pair**.
3. Allow camera access, scan the Windows QR code. Android connects to the address embedded in the QR and submits the one-time code automatically.
4. Windows accepts the first valid code and reports the paired Android device.

Both devices must be on the same local network. Allow Bridge through Windows Firewall on private networks. TCP port 45821 is used.

This is the first pairing handshake milestone. The one-time code is a short-lived, single-use pairing secret, but this version does not yet persist trusted identities or complete the Noise XX encrypted identity verification. Do not treat it as production-grade secure pairing yet.