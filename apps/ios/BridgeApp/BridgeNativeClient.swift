import Foundation

enum BridgeNativeClient {
    private static func invoke(_ request: String, _ function: (UnsafePointer<CChar>) -> UnsafeMutablePointer<CChar>?) -> String {
        let pointer = request.withCString { function($0) }
        guard let pointer else { return #"{"ok":false,"error":"native library returned no response"}"# }
        let response = String(cString: pointer)
        bridge_string_free(pointer)
        return response
    }

    static func identityFingerprint(privateKeyHex: String) throws -> String {
        let json = invoke(privateKeyHex, bridge_identity_fingerprint)
        let value = try JSONSerialization.jsonObject(with: Data(json.utf8)) as? [String: Any]
        guard value?["ok"] as? Bool == true, let fingerprint = value?["fingerprint"] as? String else {
            throw NSError(domain: "BridgeNative", code: 1, userInfo: [NSLocalizedDescriptionKey: value?["error"] as? String ?? "Could not derive identity fingerprint"])
        }
        return fingerprint
    }

    static func qrInvitationCreate(request: String) -> String {
        invoke(request, bridge_qr_invitation_create)
    }

    static func qrInvitationConsume(uri: String, nowMs: UInt64) -> String {
        let pointer = uri.withCString { bridge_qr_invitation_consume($0, nowMs) }
        guard let pointer else { return #"{\"ok\":false,\"error\":\"native library returned no response\"}"# }
        let response = String(cString: pointer)
        bridge_string_free(pointer)
        return response
    }

    static func pairConnect(request: String) -> String {
        invoke(request, bridge_pair_connect)
    }

    static func pairAccept(request: String) -> String {
        invoke(request, bridge_pair_accept)
    }

    static func sessionClose(_ handle: UInt64) {
        bridge_session_close(handle)
    }

    static func sessionSend(_ handle: UInt64, message: String) -> String {
        invoke(message) { bridge_session_send(handle, $0) }
    }

    static func sessionReceive(_ handle: UInt64) -> String {
        guard let pointer = bridge_session_receive(handle) else {
            return #"{"ok":false,"error":"native library returned no response"}"#
        }
        let response = String(cString: pointer)
        bridge_string_free(pointer)
        return response
    }
}
