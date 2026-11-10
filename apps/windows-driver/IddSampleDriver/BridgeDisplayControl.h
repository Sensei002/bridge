#pragma once
#include <winioctl.h>

// Shared driver/service control contract. The interface is restricted to LocalSystem
// and local administrators; the normal Bridge UI must use the service named pipe.
#define BRIDGE_DISPLAY_INTERFACE_GUID { 0x7f0d8b31, 0x5c4a, 0x4a72, { 0x9d, 0x8f, 0x43, 0x21, 0x6b, 0x70, 0x2e, 0xa1 } }
#define BRIDGE_DISPLAY_CONTROL_VERSION 1
#define IOCTL_BRIDGE_DISPLAY_CONNECT CTL_CODE(FILE_DEVICE_UNKNOWN, 0x800, METHOD_BUFFERED, FILE_WRITE_ACCESS)
#define IOCTL_BRIDGE_DISPLAY_DISCONNECT CTL_CODE(FILE_DEVICE_UNKNOWN, 0x801, METHOD_BUFFERED, FILE_WRITE_ACCESS)
#define IOCTL_BRIDGE_DISPLAY_QUERY CTL_CODE(FILE_DEVICE_UNKNOWN, 0x802, METHOD_BUFFERED, FILE_READ_ACCESS)

typedef struct _BRIDGE_DISPLAY_STATUS {
    ULONG Size;
    ULONG Version;
    ULONG IsConnected;
    ULONG SupportedModeCount;
} BRIDGE_DISPLAY_STATUS, *PBRIDGE_DISPLAY_STATUS;
