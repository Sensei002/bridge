/*++

Copyright (c) Microsoft Corporation

Abstract:

    This module contains a sample implementation of an indirect display driver. See the included README.md file and the
    various TODO blocks throughout this file and all accompanying files for information on building a production driver.

    MSDN documentation on indirect displays can be found at https://msdn.microsoft.com/en-us/library/windows/hardware/mt761968(v=vs.85).aspx.

Environment:

    User Mode, UMDF

--*/

#include "Driver.h"
#include "BridgeDisplayControl.h"
#include "Driver.tmh"

using namespace std;
using namespace Microsoft::IndirectDisp;
using namespace Microsoft::WRL;

#pragma region BridgeMonitorModes

// Initial portrait profile for the first Bridge Android target.
// Actual modes will be gated by runtime encoder/decoder and transport capability.
struct BridgeMonitorMode
{
    DWORD Width;
    DWORD Height;
    DWORD VSync;
};

static constexpr DWORD IDD_BRIDGE_MONITOR_COUNT = 1;

static const BridgeMonitorMode s_BridgeMonitorModes[] =
{
    { 1080, 2340, 60 },
    {  720, 1560, 60 },
    {  540, 1170, 60 },
};

#pragma endregion

#pragma region helpers

static inline void FillSignalInfo(DISPLAYCONFIG_VIDEO_SIGNAL_INFO& Mode, DWORD Width, DWORD Height, DWORD VSync, bool bMonitorMode)
{
    Mode.totalSize.cx = Mode.activeSize.cx = Width;
    Mode.totalSize.cy = Mode.activeSize.cy = Height;

    // See https://docs.microsoft.com/en-us/windows/win32/api/wingdi/ns-wingdi-displayconfig_video_signal_info
    Mode.AdditionalSignalInfo.vSyncFreqDivider = bMonitorMode ? 0 : 1;
    Mode.AdditionalSignalInfo.videoStandard = 255;

    Mode.vSyncFreq.Numerator = VSync;
    Mode.vSyncFreq.Denominator = 1;
    Mode.hSyncFreq.Numerator = VSync * Height;
    Mode.hSyncFreq.Denominator = 1;

    Mode.scanLineOrdering = DISPLAYCONFIG_SCANLINE_ORDERING_PROGRESSIVE;

    Mode.pixelRate = ((UINT64) VSync) * ((UINT64) Width) * ((UINT64) Height);
}

static IDDCX_MONITOR_MODE CreateIddCxMonitorMode(DWORD Width, DWORD Height, DWORD VSync, IDDCX_MONITOR_MODE_ORIGIN Origin = IDDCX_MONITOR_MODE_ORIGIN_DRIVER)
{
    IDDCX_MONITOR_MODE Mode = {};

    Mode.Size = sizeof(Mode);
    Mode.Origin = Origin;
    FillSignalInfo(Mode.MonitorVideoSignalInfo, Width, Height, VSync, true);

    return Mode;
}

static IDDCX_TARGET_MODE CreateIddCxTargetMode(DWORD Width, DWORD Height, DWORD VSync)
{
    IDDCX_TARGET_MODE Mode = {};

    Mode.Size = sizeof(Mode);
    FillSignalInfo(Mode.TargetVideoSignalInfo.targetVideoSignalInfo, Width, Height, VSync, false);

    return Mode;
}

#pragma endregion

extern "C" DRIVER_INITIALIZE DriverEntry;

EVT_WDF_DRIVER_DEVICE_ADD IddSampleDeviceAdd;
EVT_WDF_DEVICE_D0_ENTRY IddSampleDeviceD0Entry;

EVT_IDD_CX_ADAPTER_INIT_FINISHED IddSampleAdapterInitFinished;
EVT_IDD_CX_ADAPTER_COMMIT_MODES IddSampleAdapterCommitModes;
EVT_IDD_CX_DEVICE_IO_CONTROL IddSampleDeviceIoControl;

EVT_IDD_CX_PARSE_MONITOR_DESCRIPTION IddSampleParseMonitorDescription;
EVT_IDD_CX_MONITOR_GET_DEFAULT_DESCRIPTION_MODES IddSampleMonitorGetDefaultModes;
EVT_IDD_CX_MONITOR_QUERY_TARGET_MODES IddSampleMonitorQueryModes;

EVT_IDD_CX_MONITOR_ASSIGN_SWAPCHAIN IddSampleMonitorAssignSwapChain;
EVT_IDD_CX_MONITOR_UNASSIGN_SWAPCHAIN IddSampleMonitorUnassignSwapChain;

struct IndirectDeviceContextWrapper
{
    IndirectDeviceContext* pContext;

    void Cleanup()
    {
        delete pContext;
        pContext = nullptr;
    }
};

struct IndirectMonitorContextWrapper
{
    IndirectMonitorContext* pContext;

    void Cleanup()
    {
        delete pContext;
        pContext = nullptr;
    }
};

// This macro creates the methods for accessing an IndirectDeviceContextWrapper as a context for a WDF object
WDF_DECLARE_CONTEXT_TYPE(IndirectDeviceContextWrapper);

WDF_DECLARE_CONTEXT_TYPE(IndirectMonitorContextWrapper);

extern "C" BOOL WINAPI DllMain(
    _In_ HINSTANCE hInstance,
    _In_ UINT dwReason,
    _In_opt_ LPVOID lpReserved)
{
    UNREFERENCED_PARAMETER(hInstance);
    UNREFERENCED_PARAMETER(lpReserved);
    UNREFERENCED_PARAMETER(dwReason);

    return TRUE;
}

_Use_decl_annotations_
extern "C" NTSTATUS DriverEntry(
    PDRIVER_OBJECT  pDriverObject,
    PUNICODE_STRING pRegistryPath
)
{
    WDF_DRIVER_CONFIG Config;
    NTSTATUS Status;

    WDF_OBJECT_ATTRIBUTES Attributes;
    WDF_OBJECT_ATTRIBUTES_INIT(&Attributes);

    WDF_DRIVER_CONFIG_INIT(&Config,
        IddSampleDeviceAdd
    );

    Status = WdfDriverCreate(pDriverObject, pRegistryPath, &Attributes, &Config, WDF_NO_HANDLE);
    if (!NT_SUCCESS(Status))
    {
        return Status;
    }

    return Status;
}

_Use_decl_annotations_
NTSTATUS IddSampleDeviceAdd(WDFDRIVER Driver, PWDFDEVICE_INIT pDeviceInit)
{
    NTSTATUS Status = STATUS_SUCCESS;
    WDF_PNPPOWER_EVENT_CALLBACKS PnpPowerCallbacks;

    UNREFERENCED_PARAMETER(Driver);

    // Register for power callbacks - in this sample only power-on is needed
    WDF_PNPPOWER_EVENT_CALLBACKS_INIT(&PnpPowerCallbacks);
    PnpPowerCallbacks.EvtDeviceD0Entry = IddSampleDeviceD0Entry;
    WdfDeviceInitSetPnpPowerEventCallbacks(pDeviceInit, &PnpPowerCallbacks);

    IDD_CX_CLIENT_CONFIG IddConfig;
    IDD_CX_CLIENT_CONFIG_INIT(&IddConfig);

    // If the driver wishes to handle custom IoDeviceControl requests, it's necessary to use this callback since IddCx
    // redirects IoDeviceControl requests to an internal queue. This sample does not need this.
    // IddConfig.EvtIddCxDeviceIoControl = IddSampleIoDeviceControl;

    IddConfig.EvtIddCxDeviceIoControl = IddSampleDeviceIoControl;
    IddConfig.EvtIddCxAdapterInitFinished = IddSampleAdapterInitFinished;

    IddConfig.EvtIddCxParseMonitorDescription = IddSampleParseMonitorDescription;
    IddConfig.EvtIddCxMonitorGetDefaultDescriptionModes = IddSampleMonitorGetDefaultModes;
    IddConfig.EvtIddCxMonitorQueryTargetModes = IddSampleMonitorQueryModes;
    IddConfig.EvtIddCxAdapterCommitModes = IddSampleAdapterCommitModes;
    IddConfig.EvtIddCxMonitorAssignSwapChain = IddSampleMonitorAssignSwapChain;
    IddConfig.EvtIddCxMonitorUnassignSwapChain = IddSampleMonitorUnassignSwapChain;

    Status = IddCxDeviceInitConfig(pDeviceInit, &IddConfig);
    if (!NT_SUCCESS(Status))
    {
        return Status;
    }

    WDF_OBJECT_ATTRIBUTES Attr;
    WDF_OBJECT_ATTRIBUTES_INIT_CONTEXT_TYPE(&Attr, IndirectDeviceContextWrapper);
    Attr.EvtCleanupCallback = [](WDFOBJECT Object)
    {
        // Automatically cleanup the context when the WDF object is about to be deleted
        auto* pContext = WdfObjectGet_IndirectDeviceContextWrapper(Object);
        if (pContext)
        {
            pContext->Cleanup();
        }
    };

    // UMDF device-stack access is secured through the INF Security directive.
    // WdfDeviceInitAssignSDDLString is KMDF-only and must not be used here.

    WDFDEVICE Device = nullptr;
    Status = WdfDeviceCreate(&pDeviceInit, &Attr, &Device);
    if (!NT_SUCCESS(Status))
    {
        return Status;
    }

    Status = IddCxDeviceInitialize(Device);
    if (!NT_SUCCESS(Status)) return Status;

    auto* pContext = WdfObjectGet_IndirectDeviceContextWrapper(Device);
    pContext->pContext = new IndirectDeviceContext(Device);

    static const GUID BridgeDisplayInterfaceGuid = BRIDGE_DISPLAY_INTERFACE_GUID;
    Status = WdfDeviceCreateDeviceInterface(Device, &BridgeDisplayInterfaceGuid, nullptr);
    if (!NT_SUCCESS(Status)) return Status;
    return STATUS_SUCCESS;
}

_Use_decl_annotations_
NTSTATUS IddSampleDeviceD0Entry(WDFDEVICE Device, WDF_POWER_DEVICE_STATE PreviousState)
{
    UNREFERENCED_PARAMETER(PreviousState);

    // This function is called by WDF to start the device in the fully-on power state.

    auto* pContext = WdfObjectGet_IndirectDeviceContextWrapper(Device);
    if (pContext == nullptr || pContext->pContext == nullptr) return STATUS_DEVICE_NOT_READY;
    return pContext->pContext->InitAdapter();
}

#pragma region Direct3DDevice

Direct3DDevice::Direct3DDevice(LUID AdapterLuid) : AdapterLuid(AdapterLuid)
{

}

Direct3DDevice::Direct3DDevice()
{
    AdapterLuid = LUID{};
}

HRESULT Direct3DDevice::Init()
{
    // The DXGI factory could be cached, but if a new render adapter appears on the system, a new factory needs to be
    // created. If caching is desired, check DxgiFactory->IsCurrent() each time and recreate the factory if !IsCurrent.
    HRESULT hr = CreateDXGIFactory2(0, IID_PPV_ARGS(&DxgiFactory));
    if (FAILED(hr))
    {
        return hr;
    }

    // Find the specified render adapter
    hr = DxgiFactory->EnumAdapterByLuid(AdapterLuid, IID_PPV_ARGS(&Adapter));
    if (FAILED(hr))
    {
        return hr;
    }

    // Create a D3D device using the render adapter. BGRA support is required by the WHQL test suite.
    hr = D3D11CreateDevice(Adapter.Get(), D3D_DRIVER_TYPE_UNKNOWN, nullptr, D3D11_CREATE_DEVICE_BGRA_SUPPORT, nullptr, 0, D3D11_SDK_VERSION, &Device, nullptr, &DeviceContext);
    if (FAILED(hr))
    {
        // If creating the D3D device failed, it's possible the render GPU was lost (e.g. detachable GPU) or else the
        // system is in a transient state.
        return hr;
    }

    return S_OK;
}

#pragma endregion

#pragma region SwapChainProcessor

SwapChainProcessor::SwapChainProcessor(IDDCX_SWAPCHAIN hSwapChain, shared_ptr<Direct3DDevice> Device, HANDLE NewFrameEvent, IndirectDeviceContext* FrameSink)
    : m_hSwapChain(hSwapChain), m_Device(std::move(Device)), m_hAvailableBufferEvent(NewFrameEvent), m_FrameSink(FrameSink)
{
}

NTSTATUS SwapChainProcessor::Start()
{
    // The frame event is owned by IddCx; it must be valid for the entire worker lifetime.
    if (m_hSwapChain == nullptr || m_hAvailableBufferEvent == nullptr || !m_Device)
    {
        return STATUS_INVALID_PARAMETER;
    }

    m_hTerminateEvent.Attach(CreateEvent(nullptr, FALSE, FALSE, nullptr));
    if (!m_hTerminateEvent.Get())
    {
        return STATUS_INSUFFICIENT_RESOURCES;
    }

    // Start only after all resources are ready so callbacks never report a live worker
    // whose thread failed to start.
    m_hThread.Attach(CreateThread(nullptr, 0, RunThread, this, 0, nullptr));
    if (!m_hThread.Get())
    {
        return STATUS_INSUFFICIENT_RESOURCES;
    }

    return STATUS_SUCCESS;
}

SwapChainProcessor::~SwapChainProcessor()
{
    // A successfully started worker always has a termination event and thread.
    if (m_hThread.Get())
    {
        if (m_hTerminateEvent.Get())
        {
            SetEvent(m_hTerminateEvent.Get());
        }

        // Wait until Run() has released the swap-chain before destroying this object.
        WaitForSingleObject(m_hThread.Get(), INFINITE);
    }
}

DWORD CALLBACK SwapChainProcessor::RunThread(LPVOID Argument)
{
    reinterpret_cast<SwapChainProcessor*>(Argument)->Run();
    return 0;
}

void SwapChainProcessor::Run()
{
    // For improved performance, make use of the Multimedia Class Scheduler Service, which will intelligently
    // prioritize this thread for improved throughput in high CPU-load scenarios.
    DWORD AvTask = 0;
    HANDLE AvTaskHandle = AvSetMmThreadCharacteristicsW(L"Distribution", &AvTask);

    RunCore();

    // Always delete the swap-chain object when swap-chain processing loop terminates in order to kick the system to
    // provide a new swap-chain if necessary.
    WdfObjectDelete((WDFOBJECT)m_hSwapChain);
    m_hSwapChain = nullptr;

    if (AvTaskHandle)
    {
        AvRevertMmThreadCharacteristics(AvTaskHandle);
    }
}

void SwapChainProcessor::RunCore()
{
    // Get the DXGI device interface
    ComPtr<IDXGIDevice> DxgiDevice;
    HRESULT hr = m_Device->Device.As(&DxgiDevice);
    if (FAILED(hr))
    {
        return;
    }

    IDARG_IN_SWAPCHAINSETDEVICE SetDevice = {};
    SetDevice.pDevice = DxgiDevice.Get();

    hr = IddCxSwapChainSetDevice(m_hSwapChain, &SetDevice);
    if (FAILED(hr))
    {
        return;
    }

    // Acquire and release buffers in a loop
    for (;;)
    {
        ComPtr<IDXGIResource> AcquiredBuffer;

        // Ask for the next buffer from the producer
        IDARG_OUT_RELEASEANDACQUIREBUFFER Buffer = {};
        hr = IddCxSwapChainReleaseAndAcquireBuffer(m_hSwapChain, &Buffer);

        // AcquireBuffer immediately returns STATUS_PENDING if no buffer is yet available
        if (hr == E_PENDING)
        {
            // We must wait for a new buffer
            HANDLE WaitHandles [] =
            {
                m_hAvailableBufferEvent,
                m_hTerminateEvent.Get()
            };
            DWORD WaitResult = WaitForMultipleObjects(ARRAYSIZE(WaitHandles), WaitHandles, FALSE, 16);
            if (WaitResult == WAIT_OBJECT_0 || WaitResult == WAIT_TIMEOUT)
            {
                // We have a new buffer, so try the AcquireBuffer again
                continue;
            }
            else if (WaitResult == WAIT_OBJECT_0 + 1)
            {
                // We need to terminate
                break;
            }
            else
            {
                // Preserve the actual Win32 failure instead of treating WAIT_FAILED
                // (0xFFFFFFFF) as a Win32 error code.
                hr = (WaitResult == WAIT_FAILED)
                    ? HRESULT_FROM_WIN32(GetLastError())
                    : E_UNEXPECTED;
                break;
            }
        }
        else if (SUCCEEDED(hr))
        {
            // We have new frame to process, the surface has a reference on it that the driver has to release
            AcquiredBuffer.Attach(Buffer.MetaData.pSurface);

            // Read back the current surface through a staging texture. Only the
            // latest complete frame is retained; no unbounded frame queue is created.
            ComPtr<ID3D11Texture2D> SourceTexture;
            hr = AcquiredBuffer.As(&SourceTexture);
            if (SUCCEEDED(hr))
            {
                D3D11_TEXTURE2D_DESC SourceDesc = {};
                SourceTexture->GetDesc(&SourceDesc);
                const bool SupportedFormat =
                    SourceDesc.Format == DXGI_FORMAT_B8G8R8A8_UNORM ||
                    SourceDesc.Format == DXGI_FORMAT_B8G8R8A8_UNORM_SRGB;
                const ULONGLONG RequiredBytes =
                    static_cast<ULONGLONG>(SourceDesc.Width) * SourceDesc.Height * 4ull;
                if (!SupportedFormat || SourceDesc.Width == 0 || SourceDesc.Height == 0 ||
                    SourceDesc.Width > 4096 || SourceDesc.Height > 4096 ||
                    RequiredBytes > BRIDGE_DISPLAY_FRAME_MAX_BYTES)
                {
                    hr = E_INVALIDARG;
                }
                else
                {
                    D3D11_TEXTURE2D_DESC StagingDesc = SourceDesc;
                    StagingDesc.MipLevels = 1;
                    StagingDesc.ArraySize = 1;
                    StagingDesc.SampleDesc.Count = 1;
                    StagingDesc.SampleDesc.Quality = 0;
                    StagingDesc.Usage = D3D11_USAGE_STAGING;
                    StagingDesc.BindFlags = 0;
                    StagingDesc.MiscFlags = 0;
                    StagingDesc.CPUAccessFlags = D3D11_CPU_ACCESS_READ;
                    ComPtr<ID3D11Texture2D> StagingTexture;
                    hr = m_Device->Device->CreateTexture2D(&StagingDesc, nullptr, &StagingTexture);
                    if (SUCCEEDED(hr))
                    {
                        m_Device->DeviceContext->CopyResource(StagingTexture.Get(), SourceTexture.Get());
                        D3D11_MAPPED_SUBRESOURCE Mapped = {};
                        hr = m_Device->DeviceContext->Map(StagingTexture.Get(), 0, D3D11_MAP_READ, 0, &Mapped);
                        if (SUCCEEDED(hr))
                        {
                            const ULONG TightStride = SourceDesc.Width * 4u;
                            const size_t FrameBytes = static_cast<size_t>(TightStride) * SourceDesc.Height;
                            std::vector<BYTE> Pixels;
                            try { Pixels.resize(FrameBytes); }
                            catch (...) { Pixels.clear(); }
                            if (!Pixels.empty())
                            {
                                for (UINT Row = 0; Row < SourceDesc.Height; ++Row)
                                {
                                    memcpy(Pixels.data() + static_cast<size_t>(Row) * TightStride,
                                        static_cast<const BYTE*>(Mapped.pData) + static_cast<size_t>(Row) * Mapped.RowPitch,
                                        TightStride);
                                }
                            }
                            m_Device->DeviceContext->Unmap(StagingTexture.Get(), 0);
                            if (m_FrameSink && !Pixels.empty()) m_FrameSink->PublishLatestFrame(
                                SourceDesc.Width, SourceDesc.Height, TightStride,
                                BRIDGE_DISPLAY_FRAME_PIXEL_FORMAT_BGRA8, Pixels.data(), Pixels.size());
                        }
                    }
                }
            }
            // Frame readback is best-effort: always release the IddCx surface and
            // finish processing so a transient capture error cannot stall the display.
            hr = S_OK;

            // We have finished processing this frame hence we release the reference on it.
            // If the driver forgets to release the reference to the surface, it will be leaked which results in the
            // surfaces being left around after swapchain is destroyed.
            // NOTE: Although in this sample we release reference to the surface here; the driver still
            // owns the Buffer.MetaData.pSurface surface until IddCxSwapChainReleaseAndAcquireBuffer returns
            // S_OK and gives us a new frame, a driver may want to use the surface in future to re-encode the desktop 
            // for better quality if there is no new frame for a while
            AcquiredBuffer.Reset();
            
            // Indicate to OS that we have finished inital processing of the frame, it is a hint that
            // OS could start preparing another frame
            hr = IddCxSwapChainFinishedProcessingFrame(m_hSwapChain);
            if (FAILED(hr))
            {
                break;
            }

            // ==============================
            // TODO: Report frame statistics once the asynchronous encode/send work is completed
            //
            // Drivers should report information about sub-frame timings, like encode time, send time, etc.
            // ==============================
            // IddCxSwapChainReportFrameStatistics(m_hSwapChain, ...);
        }
        else
        {
            // The swap-chain was likely abandoned (e.g. DXGI_ERROR_ACCESS_LOST), so exit the processing loop
            break;
        }
    }
}

#pragma endregion

#pragma region IndirectDeviceContext

IndirectDeviceContext::IndirectDeviceContext(_In_ WDFDEVICE WdfDevice) :
    m_WdfDevice(WdfDevice),
    m_Monitor(nullptr)
{
    m_Adapter = {};
}

IndirectDeviceContext::~IndirectDeviceContext()
{
}

NTSTATUS IndirectDeviceContext::InitAdapter()
{
    m_AdapterReady.store(false);

    // ==============================
    // TODO: Update the below diagnostic information in accordance with the target hardware. The strings and version
    // numbers are used for telemetry and may be displayed to the user in some situations.
    //
    // This is also where static per-adapter capabilities are determined.
    // ==============================

    IDDCX_ADAPTER_CAPS AdapterCaps = {};
    AdapterCaps.Size = sizeof(AdapterCaps);

    // Declare basic feature support for the adapter (required)
    AdapterCaps.MaxMonitorsSupported = IDD_BRIDGE_MONITOR_COUNT;
    AdapterCaps.EndPointDiagnostics.Size = sizeof(AdapterCaps.EndPointDiagnostics);
    AdapterCaps.EndPointDiagnostics.GammaSupport = IDDCX_FEATURE_IMPLEMENTATION_NONE;
    AdapterCaps.EndPointDiagnostics.TransmissionType = IDDCX_TRANSMISSION_TYPE_WIRED_OTHER;

    // Declare your device strings for telemetry (required)
    AdapterCaps.EndPointDiagnostics.pEndPointFriendlyName = L"Bridge Virtual Display";
    AdapterCaps.EndPointDiagnostics.pEndPointManufacturerName = L"Bridge";
    AdapterCaps.EndPointDiagnostics.pEndPointModelName = L"Bridge Second Screen";

    // Declare your hardware and firmware versions (required)
    IDDCX_ENDPOINT_VERSION Version = {};
    Version.Size = sizeof(Version);
    Version.MajorVer = 1;
    AdapterCaps.EndPointDiagnostics.pFirmwareVersion = &Version;
    AdapterCaps.EndPointDiagnostics.pHardwareVersion = &Version;

    // Initialize a WDF context that can store a pointer to the device context object
    WDF_OBJECT_ATTRIBUTES Attr;
    WDF_OBJECT_ATTRIBUTES_INIT_CONTEXT_TYPE(&Attr, IndirectDeviceContextWrapper);

    IDARG_IN_ADAPTER_INIT AdapterInit = {};
    AdapterInit.WdfDevice = m_WdfDevice;
    AdapterInit.pCaps = &AdapterCaps;
    AdapterInit.ObjectAttributes = &Attr;

    // Start the initialization of the adapter, which will trigger the AdapterFinishInit callback later
    IDARG_OUT_ADAPTER_INIT AdapterInitOut;
    NTSTATUS Status = IddCxAdapterInitAsync(&AdapterInit, &AdapterInitOut);

    if (!NT_SUCCESS(Status))
    {
        m_AdapterReady.store(false);
        return Status;
    }

    {
        // Store a reference to the WDF adapter handle
        m_Adapter = AdapterInitOut.AdapterObject;

        // Store the device context object into the WDF object context
        auto* pContext = WdfObjectGet_IndirectDeviceContextWrapper(AdapterInitOut.AdapterObject);
        pContext->pContext = this;
    }

    return STATUS_SUCCESS;
}

NTSTATUS IndirectDeviceContext::ConnectMonitor(UINT ConnectorIndex)
{
    std::lock_guard<std::mutex> lock(m_MonitorLock);
    if (!m_AdapterReady.load()) return STATUS_DEVICE_NOT_READY;
    if (m_Monitor != nullptr) return STATUS_SUCCESS;
    WDF_OBJECT_ATTRIBUTES Attr;
    WDF_OBJECT_ATTRIBUTES_INIT_CONTEXT_TYPE(&Attr, IndirectMonitorContextWrapper);
    Attr.EvtCleanupCallback = [](WDFOBJECT Object)
    {
        // Release the monitor context and stop its swap-chain worker before WDF destroys the object.
        auto* pContext = WdfObjectGet_IndirectMonitorContextWrapper(Object);
        if (pContext)
        {
            pContext->Cleanup();
        }
    };

    IDDCX_MONITOR_INFO MonitorInfo = {};
    MonitorInfo.Size = sizeof(MonitorInfo);
    MonitorInfo.MonitorType = DISPLAYCONFIG_OUTPUT_TECHNOLOGY_HDMI;
    MonitorInfo.ConnectorIndex = ConnectorIndex;
    MonitorInfo.MonitorDescription.Size = sizeof(MonitorInfo.MonitorDescription);
    MonitorInfo.MonitorDescription.Type = IDDCX_MONITOR_DESCRIPTION_TYPE_EDID;
    MonitorInfo.MonitorDescription.DataSize = 0;
    MonitorInfo.MonitorDescription.pData = nullptr;
    if (FAILED(CoCreateGuid(&MonitorInfo.MonitorContainerId))) return STATUS_UNSUCCESSFUL;
    IDARG_IN_MONITORCREATE MonitorCreate = {};
    MonitorCreate.ObjectAttributes = &Attr;
    MonitorCreate.pMonitorInfo = &MonitorInfo;
    IDARG_OUT_MONITORCREATE MonitorCreateOut = {};
    NTSTATUS Status = IddCxMonitorCreate(m_Adapter, &MonitorCreate, &MonitorCreateOut);
    if (!NT_SUCCESS(Status)) return Status;
    auto* pMonitorContextWrapper = WdfObjectGet_IndirectMonitorContextWrapper(MonitorCreateOut.MonitorObject);
    pMonitorContextWrapper->pContext = new IndirectMonitorContext(MonitorCreateOut.MonitorObject, this);
    IDARG_OUT_MONITORARRIVAL ArrivalOut = {};
    Status = IddCxMonitorArrival(MonitorCreateOut.MonitorObject, &ArrivalOut);
    if (!NT_SUCCESS(Status)) { WdfObjectDelete((WDFOBJECT)MonitorCreateOut.MonitorObject); return Status; }
    m_Monitor = MonitorCreateOut.MonitorObject;
    return STATUS_SUCCESS;
}

NTSTATUS IndirectDeviceContext::DisconnectMonitor()
{
    std::lock_guard<std::mutex> lock(m_MonitorLock);
    if (m_Monitor == nullptr) return STATUS_SUCCESS;
    NTSTATUS Status = IddCxMonitorDeparture(m_Monitor);
    if (NT_SUCCESS(Status)) { m_Monitor = nullptr; ClearLatestFrame(); }
    return Status;
}

NTSTATUS IndirectDeviceContext::CopyLatestFrame(PVOID Buffer, size_t BufferLength, size_t* BytesWritten)
{
    if (BytesWritten == nullptr) return STATUS_INVALID_PARAMETER;
    *BytesWritten = 0;
    std::lock_guard<std::mutex> lock(m_FrameLock);
    if (m_LatestFrame.empty()) return STATUS_NO_MORE_ENTRIES;
    const size_t Required = sizeof(BRIDGE_DISPLAY_FRAME_HEADER) + m_LatestFrame.size();
    if (Buffer == nullptr || BufferLength < Required) return STATUS_BUFFER_TOO_SMALL;
    memcpy(Buffer, &m_LatestFrameHeader, sizeof(m_LatestFrameHeader));
    memcpy(static_cast<BYTE*>(Buffer) + sizeof(m_LatestFrameHeader), m_LatestFrame.data(), m_LatestFrame.size());
    *BytesWritten = Required;
    return STATUS_SUCCESS;
}

void IndirectDeviceContext::PublishLatestFrame(ULONG Width, ULONG Height, ULONG Stride, ULONG PixelFormat, const BYTE* Data, size_t DataSize)
{
    if (Data == nullptr || DataSize == 0 || DataSize > BRIDGE_DISPLAY_FRAME_MAX_BYTES ||
        Width == 0 || Height == 0 || Width > 4096 || Height > 4096 ||
        static_cast<ULONGLONG>(Stride) * Height != DataSize) return;
    std::vector<BYTE> Frame;
    try { Frame.assign(Data, Data + DataSize); }
    catch (...) { return; }
    std::lock_guard<std::mutex> lock(m_FrameLock);
    m_LatestFrame.swap(Frame);
    m_LatestFrameHeader = {};
    m_LatestFrameHeader.Size = sizeof(BRIDGE_DISPLAY_FRAME_HEADER);
    m_LatestFrameHeader.Version = BRIDGE_DISPLAY_CONTROL_VERSION;
    m_LatestFrameHeader.Width = Width;
    m_LatestFrameHeader.Height = Height;
    m_LatestFrameHeader.Stride = Stride;
    m_LatestFrameHeader.PixelFormat = PixelFormat;
    m_LatestFrameHeader.Sequence = ++m_FrameSequence;
    m_LatestFrameHeader.TimestampNs = GetTickCount64() * 1000000ull;
    m_LatestFrameHeader.DataSize = static_cast<ULONG>(DataSize);
}

void IndirectDeviceContext::ClearLatestFrame()
{
    std::lock_guard<std::mutex> lock(m_FrameLock);
    m_LatestFrame.clear();
    m_LatestFrameHeader = {};
}

void IndirectDeviceContext::GetDisplayStatus(BRIDGE_DISPLAY_STATUS& Status)
{
    std::lock_guard<std::mutex> lock(m_MonitorLock);
    Status = {};
    Status.Size = sizeof(Status);
    Status.Version = BRIDGE_DISPLAY_CONTROL_VERSION;
    Status.IsConnected = (m_Monitor != nullptr) ? 1u : 0u;
    Status.SupportedModeCount = ARRAYSIZE(s_BridgeMonitorModes);
}

IndirectMonitorContext::IndirectMonitorContext(_In_ IDDCX_MONITOR Monitor, IndirectDeviceContext* FrameSink) :
    m_Monitor(Monitor),
    m_FrameSink(FrameSink)
{
}

IndirectMonitorContext::~IndirectMonitorContext()
{
    m_ProcessingThread.reset();
}

NTSTATUS IndirectMonitorContext::AssignSwapChain(IDDCX_SWAPCHAIN SwapChain, LUID RenderAdapter, HANDLE NewFrameEvent)
{
    // Stop the previous worker before replacing its swap-chain.
    m_ProcessingThread.reset();

    auto Device = make_shared<Direct3DDevice>(RenderAdapter);
    HRESULT Hr = Device->Init();
    if (FAILED(Hr))
    {
        // Delete the failed swap-chain so IddCx can provide a fresh one after
        // the transient adapter/device failure is resolved.
        WdfObjectDelete(SwapChain);
        return STATUS_DEVICE_NOT_READY;
    }

    auto Processor = make_unique<SwapChainProcessor>(SwapChain, Device, NewFrameEvent, m_FrameSink);
    NTSTATUS Status = Processor->Start();
    if (!NT_SUCCESS(Status))
    {
        // Start() guarantees that no worker owns the swap-chain on failure.
        WdfObjectDelete(SwapChain);
        return Status;
    }

    m_ProcessingThread = std::move(Processor);
    return STATUS_SUCCESS;
}

void IndirectMonitorContext::UnassignSwapChain()
{
    // Stop processing the last swap-chain
    m_ProcessingThread.reset();
}

#pragma endregion

#pragma region DDI Callbacks

_Use_decl_annotations_
NTSTATUS IddSampleAdapterInitFinished(IDDCX_ADAPTER AdapterObject, const IDARG_IN_ADAPTER_INIT_FINISHED* pInArgs)
{
    // This is called when the OS has finished setting up the adapter for use by the IddCx driver. It's now possible
    // to report attached monitors.

    auto* pDeviceContextWrapper = WdfObjectGet_IndirectDeviceContextWrapper(AdapterObject);
    if (pDeviceContextWrapper == nullptr || pDeviceContextWrapper->pContext == nullptr)
    {
        return STATUS_DEVICE_NOT_READY;
    }

    // IddCx initialization is asynchronous. Do not allow CONNECT until this callback
    // confirms that Windows has finished initializing the adapter.
    pDeviceContextWrapper->pContext->SetAdapterReady(NT_SUCCESS(pInArgs->AdapterInitStatus));

    // Preserve the adapter initialization failure so IddCx and WDF can observe it.
    if (!NT_SUCCESS(pInArgs->AdapterInitStatus)) return pInArgs->AdapterInitStatus;

    // The adapter starts without a monitor. The service requests arrival only after an authenticated session.
    return STATUS_SUCCESS;
}

_Use_decl_annotations_
VOID IddSampleDeviceIoControl(WDFDEVICE Device, WDFREQUEST Request, size_t OutputBufferLength, size_t InputBufferLength, ULONG IoControlCode)
{
    auto* pDeviceContextWrapper = WdfObjectGet_IndirectDeviceContextWrapper(Device);
    if (pDeviceContextWrapper == nullptr || pDeviceContextWrapper->pContext == nullptr) { WdfRequestComplete(Request, STATUS_DEVICE_NOT_READY); return; }
    auto* DeviceContext = pDeviceContextWrapper->pContext;
    NTSTATUS Status = STATUS_INVALID_DEVICE_REQUEST;
    switch (IoControlCode)
    {
    case IOCTL_BRIDGE_DISPLAY_CONNECT:
        Status = (InputBufferLength == 0 && OutputBufferLength == 0) ? DeviceContext->ConnectMonitor(0) : STATUS_INVALID_BUFFER_SIZE;
        break;
    case IOCTL_BRIDGE_DISPLAY_DISCONNECT:
        Status = (InputBufferLength == 0 && OutputBufferLength == 0) ? DeviceContext->DisconnectMonitor() : STATUS_INVALID_BUFFER_SIZE;
        break;
    case IOCTL_BRIDGE_DISPLAY_READ_FRAME:
        if (InputBufferLength != 0) { Status = STATUS_INVALID_BUFFER_SIZE; break; }
        else {
            PVOID pFrameBuffer = nullptr;
            Status = WdfRequestRetrieveOutputBuffer(Request, OutputBufferLength, &pFrameBuffer, nullptr);
            if (NT_SUCCESS(Status)) {
                size_t BytesWritten = 0;
                Status = DeviceContext->CopyLatestFrame(pFrameBuffer, OutputBufferLength, &BytesWritten);
                if (NT_SUCCESS(Status)) { WdfRequestCompleteWithInformation(Request, STATUS_SUCCESS, BytesWritten); return; }
            }
        }
        break;
    case IOCTL_BRIDGE_DISPLAY_QUERY:
        if (InputBufferLength != 0 || OutputBufferLength < sizeof(BRIDGE_DISPLAY_STATUS)) { Status = STATUS_BUFFER_TOO_SMALL; break; }
        else {
            PBRIDGE_DISPLAY_STATUS pStatus = nullptr;
            Status = WdfRequestRetrieveOutputBuffer(Request, sizeof(BRIDGE_DISPLAY_STATUS), reinterpret_cast<PVOID*>(&pStatus), nullptr);
            if (NT_SUCCESS(Status)) { DeviceContext->GetDisplayStatus(*pStatus); WdfRequestCompleteWithInformation(Request, STATUS_SUCCESS, sizeof(BRIDGE_DISPLAY_STATUS)); return; }
        }
        break;
    default:
        Status = STATUS_INVALID_DEVICE_REQUEST;
        break;
    }
    WdfRequestComplete(Request, Status);
}

_Use_decl_annotations_
NTSTATUS IddSampleAdapterCommitModes(IDDCX_ADAPTER AdapterObject, const IDARG_IN_COMMITMODES* pInArgs)
{
    UNREFERENCED_PARAMETER(AdapterObject);
    UNREFERENCED_PARAMETER(pInArgs);

    // For the sample, do nothing when modes are picked - the swap-chain is taken care of by IddCx

    // ==============================
    // TODO: In a real driver, this function would be used to reconfigure the device to commit the new modes. Loop
    // through pInArgs->pPaths and look for IDDCX_PATH_FLAGS_ACTIVE. Any path not active is inactive (e.g. the monitor
    // should be turned off).
    // ==============================

    return STATUS_SUCCESS;
}

_Use_decl_annotations_
NTSTATUS IddSampleParseMonitorDescription(const IDARG_IN_PARSEMONITORDESCRIPTION* pInArgs, IDARG_OUT_PARSEMONITORDESCRIPTION* pOutArgs)
{
    UNREFERENCED_PARAMETER(pInArgs);
    UNREFERENCED_PARAMETER(pOutArgs);

    // Bridge reports an EDID-less monitor and supplies modes through
    // IddSampleMonitorGetDefaultModes. EDID parsing is not used in this phase.
    return STATUS_INVALID_PARAMETER;
}

_Use_decl_annotations_
NTSTATUS IddSampleMonitorGetDefaultModes(IDDCX_MONITOR MonitorObject, const IDARG_IN_GETDEFAULTDESCRIPTIONMODES* pInArgs, IDARG_OUT_GETDEFAULTDESCRIPTIONMODES* pOutArgs)
{
    UNREFERENCED_PARAMETER(MonitorObject);

    const UINT modeCount = ARRAYSIZE(s_BridgeMonitorModes);
    pOutArgs->DefaultMonitorModeBufferOutputCount = modeCount;

    if (pInArgs->DefaultMonitorModeBufferInputCount == 0)
    {
        return STATUS_SUCCESS;
    }

    if (pInArgs->DefaultMonitorModeBufferInputCount < modeCount)
    {
        return STATUS_BUFFER_TOO_SMALL;
    }

    for (UINT modeIndex = 0; modeIndex < modeCount; ++modeIndex)
    {
        pInArgs->pDefaultMonitorModes[modeIndex] = CreateIddCxMonitorMode(
            s_BridgeMonitorModes[modeIndex].Width,
            s_BridgeMonitorModes[modeIndex].Height,
            s_BridgeMonitorModes[modeIndex].VSync,
            IDDCX_MONITOR_MODE_ORIGIN_DRIVER
        );
    }

    pOutArgs->PreferredMonitorModeIdx = 0;
    return STATUS_SUCCESS;
}

_Use_decl_annotations_
NTSTATUS IddSampleMonitorQueryModes(IDDCX_MONITOR MonitorObject, const IDARG_IN_QUERYTARGETMODES* pInArgs, IDARG_OUT_QUERYTARGETMODES* pOutArgs)
{
    UNREFERENCED_PARAMETER(MonitorObject);

    vector<IDDCX_TARGET_MODE> TargetModes;
    TargetModes.reserve(ARRAYSIZE(s_BridgeMonitorModes));

    for (const auto& mode : s_BridgeMonitorModes)
    {
        TargetModes.push_back(CreateIddCxTargetMode(mode.Width, mode.Height, mode.VSync));
    }

    pOutArgs->TargetModeBufferOutputCount = static_cast<UINT>(TargetModes.size());

    if (pInArgs->TargetModeBufferInputCount == 0)
    {
        return STATUS_SUCCESS;
    }

    if (pInArgs->TargetModeBufferInputCount < TargetModes.size())
    {
        return STATUS_BUFFER_TOO_SMALL;
    }

    copy(TargetModes.begin(), TargetModes.end(), pInArgs->pTargetModes);
    return STATUS_SUCCESS;
}

_Use_decl_annotations_
NTSTATUS IddSampleMonitorAssignSwapChain(IDDCX_MONITOR MonitorObject, const IDARG_IN_SETSWAPCHAIN* pInArgs)
{
    auto* pMonitorContextWrapper = WdfObjectGet_IndirectMonitorContextWrapper(MonitorObject);
    if (pMonitorContextWrapper == nullptr || pMonitorContextWrapper->pContext == nullptr)
    {
        WdfObjectDelete(pInArgs->hSwapChain);
        return STATUS_DEVICE_NOT_READY;
    }

    return pMonitorContextWrapper->pContext->AssignSwapChain(
        pInArgs->hSwapChain,
        pInArgs->RenderAdapterLuid,
        pInArgs->hNextSurfaceAvailable
    );
}

_Use_decl_annotations_
NTSTATUS IddSampleMonitorUnassignSwapChain(IDDCX_MONITOR MonitorObject)
{
    auto* pMonitorContextWrapper = WdfObjectGet_IndirectMonitorContextWrapper(MonitorObject);
    pMonitorContextWrapper->pContext->UnassignSwapChain();
    return STATUS_SUCCESS;
}

#pragma endregion
