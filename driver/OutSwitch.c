/* Minimal WFP bind callout. Policy, persistence and UI are Rust.
 * This is driver source, not a signed/validated production driver.
 * No packet payload access, injections, process hooks or global route changes.
 */
#include <ntddk.h>
#include <ndis.h>
#include <fwpsk.h>
#include <fwpmk.h>
#include <initguid.h>

DEFINE_GUID(OS_BIND4,0xf973cd10,0x96a3,0x4ed2,0x91,0x24,0x61,0xdd,0xc8,0xa7,0x63,0x01);
DEFINE_GUID(OS_BIND6,0xf973cd10,0x96a3,0x4ed2,0x91,0x24,0x61,0xdd,0xc8,0xa7,0x63,0x02);
typedef struct _OS_TARGET { UINT32 version; UINT32 family; UCHAR address[16]; } OS_TARGET;
C_ASSERT(sizeof(OS_TARGET)==24);
static PDEVICE_OBJECT Device;
static UINT32 Id4, Id6;

static NTSTATUS NTAPI Notify(FWPS_CALLOUT_NOTIFY_TYPE type,const GUID* key,FWPS_FILTER1* filter) {
    UNREFERENCED_PARAMETER(type);UNREFERENCED_PARAMETER(key);UNREFERENCED_PARAMETER(filter);return STATUS_SUCCESS;
}
static void NTAPI Classify(const FWPS_INCOMING_VALUES0* values,const FWPS_INCOMING_METADATA_VALUES0* metadata,
    void* layerData,const void* classifyContext,const FWPS_FILTER1* filter,UINT64 flowContext,FWPS_CLASSIFY_OUT0* output) {
    UINT64 handle=0; FWPS_BIND_REQUEST0* request=NULL; NTSTATUS status; OS_TARGET target;
    UNREFERENCED_PARAMETER(metadata);UNREFERENCED_PARAMETER(layerData);UNREFERENCED_PARAMETER(flowContext);
    if (!(output->rights & FWPS_RIGHT_ACTION_WRITE)) return;
    output->actionType=FWP_ACTION_CONTINUE;
    if (!filter->providerContext || !filter->providerContext->dataBuffer ||
        filter->providerContext->dataBuffer->size!=sizeof(target)) return;
    RtlCopyMemory(&target,filter->providerContext->dataBuffer->data,sizeof(target));
    if (target.version!=1 || (target.family!=AF_INET && target.family!=AF_INET6)) return;
    if ((values->layerId==FWPS_LAYER_ALE_BIND_REDIRECT_V4)!=(target.family==AF_INET)) return;
    status=FwpsAcquireClassifyHandle0((void*)classifyContext,0,&handle);
    if (!NT_SUCCESS(status)) return;
    status=FwpsAcquireWritableLayerDataPointer0(handle,filter->filterId,0,(void**)&request,output);
    if (NT_SUCCESS(status)) {
        BOOLEAN ours=FALSE; FWPS_BIND_REQUEST0* previous=request->previousVersion;
        /* Do not repeat our modification during WFP reclassification. */
        while(previous) {if(previous->modifierFilterId==filter->filterId){ours=TRUE;break;}previous=previous->previousVersion;}
        if (!ours && request->localAddressAndPort.ss_family==target.family) {
            if(target.family==AF_INET) {
                SOCKADDR_IN* addr=(SOCKADDR_IN*)&request->localAddressAndPort;
                /* Preserve explicit loopback binds; never rewrite local IPC. */
                if(((UCHAR*)&addr->sin_addr)[0]!=127) RtlCopyMemory(&addr->sin_addr,target.address,4);
            } else {
                SOCKADDR_IN6* addr=(SOCKADDR_IN6*)&request->localAddressAndPort;
                static const UCHAR loopback[16]={0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,1};
                if(RtlCompareMemory(&addr->sin6_addr,loopback,16)!=16) {
                    RtlCopyMemory(&addr->sin6_addr,target.address,16);addr->sin6_scope_id=0;
                }
            }
        }
        /* Required even if unchanged, once writable data has been acquired. */
        FwpsApplyModifiedLayerData0(handle,request,FWPS_CLASSIFY_FLAG_REAUTHORIZE_IF_MODIFIED_BY_OTHERS);
        /* Acquiring writable data cleared our action-write right. Restore it
         * after finalizing this synchronous redirection. */
        output->rights |= FWPS_RIGHT_ACTION_WRITE;
        output->actionType=FWP_ACTION_PERMIT;
    }
    FwpsReleaseClassifyHandle0(handle);
}
static void Unload(PDRIVER_OBJECT driver) {
    UNREFERENCED_PARAMETER(driver);
    if(Id6) FwpsCalloutUnregisterById0(Id6);
    if(Id4) FwpsCalloutUnregisterById0(Id4);
    if(Device) IoDeleteDevice(Device);
}
NTSTATUS DriverEntry(PDRIVER_OBJECT driver,PUNICODE_STRING registryPath) {
    FWPS_CALLOUT1 callout={0};NTSTATUS status;
    UNREFERENCED_PARAMETER(registryPath);
    driver->DriverUnload=Unload;
    /* No named device, symbolic link, dispatch interface or user IOCTL surface. */
    status=IoCreateDevice(driver,0,NULL,FILE_DEVICE_NETWORK,0,FALSE,&Device);
    if(!NT_SUCCESS(status))return status;
    callout.calloutKey=OS_BIND4;callout.classifyFn=Classify;callout.notifyFn=Notify;
    status=FwpsCalloutRegister1(Device,&callout,&Id4);
    if(NT_SUCCESS(status)){callout.calloutKey=OS_BIND6;status=FwpsCalloutRegister1(Device,&callout,&Id6);}
    if(!NT_SUCCESS(status))Unload(driver);
    return status;
}
