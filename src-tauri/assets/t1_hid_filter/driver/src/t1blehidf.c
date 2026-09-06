/*++
  t1blehidf - T1 BLE-only KMDF lower filter.
  On read completion, zero Consumer AC Search (usage 0x0221) reports
  that match the T1 voice pulse pattern (report id 0x02, usage lo/hi 0x21 0x02).
  Build with WDK. Do not bind non-BLE devices (INF + install script allowlist).
--*/

#include <ntddk.h>
#include <wdf.h>

DRIVER_INITIALIZE DriverEntry;
EVT_WDF_DRIVER_DEVICE_ADD T1BleEvtDeviceAdd;
EVT_WDF_IO_QUEUE_IO_READ T1BleEvtIoRead;
EVT_WDF_REQUEST_COMPLETION_ROUTINE T1BleEvtReadComplete;

typedef struct _T1BLE_DEVICE_CONTEXT {
    WDFDEVICE Device;
} T1BLE_DEVICE_CONTEXT, *PT1BLE_DEVICE_CONTEXT;

WDF_DECLARE_CONTEXT_TYPE_WITH_NAME(T1BLE_DEVICE_CONTEXT, T1BleGetContext);

/* T1 BLE voice / AC Search pulse commonly observed as 02-21-02 (press) / 02-00-00 (release). */
static VOID T1BleStripAcSearch(_Inout_updates_bytes_(Length) PUCHAR Buffer, _In_ size_t Length)
{
    if (Buffer == NULL || Length < 3) {
        return;
    }
    /* report-id=0x02, usage=0x0221 little-endian bytes 0x21 0x02 */
    if (Buffer[0] == 0x02 && Buffer[1] == 0x21 && Buffer[2] == 0x02) {
        Buffer[1] = 0x00;
        Buffer[2] = 0x00;
    }
}

VOID T1BleEvtReadComplete(
    _In_ WDFREQUEST Request,
    _In_ WDFIOTARGET Target,
    _In_ PWDF_REQUEST_COMPLETION_PARAMS Params,
    _In_ WDFCONTEXT Context
)
{
    UNREFERENCED_PARAMETER(Target);
    UNREFERENCED_PARAMETER(Context);

    if (NT_SUCCESS(Params->IoStatus.Status) && Params->IoStatus.Information >= 3) {
        PVOID buffer = NULL;
        size_t length = 0;
        if (NT_SUCCESS(WdfRequestRetrieveOutputBuffer(Request, 3, &buffer, &length))) {
            T1BleStripAcSearch((PUCHAR)buffer, length);
        }
    }

    WdfRequestComplete(Request, Params->IoStatus.Status);
}

VOID T1BleEvtIoRead(
    _In_ WDFQUEUE Queue,
    _In_ WDFREQUEST Request,
    _In_ size_t Length
)
{
    WDFDEVICE device = WdfIoQueueGetDevice(Queue);
    WDFIOTARGET target = WdfDeviceGetIoTarget(device);
    BOOLEAN sent;

    UNREFERENCED_PARAMETER(Length);

    WdfRequestFormatRequestUsingCurrentType(Request);
    WdfRequestSetCompletionRoutine(Request, T1BleEvtReadComplete, WDF_NO_CONTEXT);

    sent = WdfRequestSend(Request, target, WDF_NO_SEND_OPTIONS);
    if (!sent) {
        NTSTATUS st = WdfRequestGetStatus(Request);
        WdfRequestComplete(Request, st);
    }
}

NTSTATUS T1BleEvtDeviceAdd(_In_ WDFDRIVER Driver, _Inout_ PWDFDEVICE_INIT DeviceInit)
{
    NTSTATUS status;
    WDFDEVICE device;
    WDF_OBJECT_ATTRIBUTES attributes;
    WDF_IO_QUEUE_CONFIG queueConfig;

    UNREFERENCED_PARAMETER(Driver);

    WdfFdoInitSetFilter(DeviceInit);

    WDF_OBJECT_ATTRIBUTES_INIT_CONTEXT_TYPE(&attributes, T1BLE_DEVICE_CONTEXT);
    status = WdfDeviceCreate(&DeviceInit, &attributes, &device);
    if (!NT_SUCCESS(status)) {
        return status;
    }

    WDF_IO_QUEUE_CONFIG_INIT_DEFAULT_QUEUE(&queueConfig, WdfIoQueueDispatchParallel);
    queueConfig.EvtIoRead = T1BleEvtIoRead;
    status = WdfIoQueueCreate(device, &queueConfig, WDF_NO_OBJECT_ATTRIBUTES, WDF_NO_HANDLE);
    return status;
}

NTSTATUS DriverEntry(_In_ PDRIVER_OBJECT DriverObject, _In_ PUNICODE_STRING RegistryPath)
{
    WDF_DRIVER_CONFIG config;
    WDF_DRIVER_CONFIG_INIT(&config, T1BleEvtDeviceAdd);
    return WdfDriverCreate(DriverObject, RegistryPath, WDF_NO_OBJECT_ATTRIBUTES, &config, WDF_NO_HANDLE);
}
