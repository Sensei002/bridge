#ifndef BRIDGE_NATIVE_H
#define BRIDGE_NATIVE_H
#include <stdint.h>
#ifdef __cplusplus
extern "C" {
#endif
char *bridge_identity_fingerprint(const char *private_key_hex);
char *bridge_pair_connect(const char *request_json);
char *bridge_pair_accept(const char *request_json);
char *bridge_qr_invitation_create(const char *request_json);
char *bridge_qr_invitation_consume(const char *uri, uint64_t now_ms);
char *bridge_session_send(uint64_t handle, const char *message);
char *bridge_session_receive(uint64_t handle);
void bridge_session_close(uint64_t handle);
void bridge_string_free(char *value);
#ifdef __cplusplus
}
#endif
#endif
