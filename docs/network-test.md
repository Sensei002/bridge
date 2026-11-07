# Bridge local network test (first transport milestone)

This milestone provides a direct TCP connectivity check between the Windows and Android builds.

## Test

1. Install and open Bridge on both devices.
2. Connect both devices to the same Wi-Fi/LAN.
3. Allow Bridge through the Windows firewall when prompted. The listener uses TCP port **45821**.
4. On Android, open **Devices** and note the local-network test panel. On Windows, open **Devices**.
5. Enter the other device's LAN IPv4 address (for example, `192.168.1.20`) and choose **Test connection**.
6. A successful test reports a Bridge ping/pong response.

Both apps listen on TCP port 45821. The test sends only `BRIDGE_PING/1` and expects `BRIDGE_PONG/1`.

## Current limits

- This is a transport smoke test, not device discovery, pairing, or an authenticated session.
- Traffic is not encrypted in this milestone. Do not send private data through it.
- Automatic mDNS discovery, Noise-authenticated pairing, and permission negotiation are subsequent milestones.
- iOS currently has the updated brand UI; the network listener/browser is not implemented in this milestone.
