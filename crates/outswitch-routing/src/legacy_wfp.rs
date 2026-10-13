use anyhow::{bail,Context,Result};
use outswitch_common::*;
use outswitch_core::{select_network,RoutingEngine,Selection};
use std::{collections::BTreeMap,net::IpAddr,ptr};
use windows_sys::{core::GUID,Win32::{Foundation::HANDLE,NetworkManagement::WindowsFilteringPlatform::*}};

const SUBLAYER:GUID=GUID::from_u128(0xf973cd10_96a3_4ed2_9124_61ddc8a76300);
const BIND4:GUID=GUID::from_u128(0xf973cd10_96a3_4ed2_9124_61ddc8a76301);
const BIND6:GUID=GUID::from_u128(0xf973cd10_96a3_4ed2_9124_61ddc8a76302);
fn wide(s:&str)->Vec<u16>{s.encode_utf16().chain(Some(0)).collect()}
fn check(code:u32)->Result<()>{if code!=0{bail!("Windows Filtering Platform error 0x{code:08X}");}Ok(())}
/// Read-only readiness check. WFP registration is verified again before applying rules.
pub fn availability_error()->Option<String>{unsafe{
    use windows_sys::Win32::System::Services::*;
    let manager=OpenSCManagerW(ptr::null(),ptr::null(),SC_MANAGER_CONNECT);
    if manager.is_null(){return Some("Cannot check the networking driver. Start or repair the installed service.".into());}
    let service=OpenServiceW(manager,wide("OutSwitchDriver").as_ptr(),SERVICE_QUERY_STATUS);
    let error=windows_sys::Win32::Foundation::GetLastError();
    CloseServiceHandle(manager);
    if service.is_null(){return Some(if error==windows_sys::Win32::Foundation::ERROR_SERVICE_DOES_NOT_EXIST {
        "Networking driver not installed. Per-app routing is unavailable in this preview; monitoring and ping tests are available.".into()
    }else{format!("Cannot inspect the networking driver (Windows error {error}).")});}
    let mut status:SERVICE_STATUS=std::mem::zeroed();let ok=QueryServiceStatus(service,&mut status);CloseServiceHandle(service);
    if ok==0 || status.dwCurrentState!=SERVICE_RUNNING {Some("Networking driver is not running. Per-app routing is unavailable.".into())}else{None}
}}
struct Session(HANDLE);
impl Drop for Session{fn drop(&mut self){unsafe{FwpmEngineClose0(self.0);}}}
struct AppId(*mut FWP_BYTE_BLOB);
impl Drop for AppId{fn drop(&mut self){unsafe{FwpmFreeMemory0((&mut self.0 as *mut *mut FWP_BYTE_BLOB).cast());}}}

pub struct WindowsRoutingEngine {
    session:Option<Session>,filters:Vec<u64>,contexts:Vec<GUID>,
    pub statuses:BTreeMap<String,String>,selected:BTreeMap<String,String>,generation:u64,
}
impl Default for WindowsRoutingEngine{fn default()->Self{Self{session:None,filters:vec![],contexts:vec![],statuses:BTreeMap::new(),selected:BTreeMap::new(),generation:0}}}
impl WindowsRoutingEngine {
    fn open(&mut self)->Result<()> {unsafe{
        if self.session.is_some(){return Ok(());}
        let mut session:FWPM_SESSION0=std::mem::zeroed();session.flags=FWPM_SESSION_FLAG_DYNAMIC;session.txnWaitTimeoutInMSec=2000;
        let mut handle=ptr::null_mut();check(FwpmEngineOpen0(ptr::null(),10,ptr::null(),&session,&mut handle)).context("Cannot open WFP; start the installed service")?;
        let owner=Session(handle);let mut name=wide("Out-Switch — session rules");
        let mut sub:FWPM_SUBLAYER0=std::mem::zeroed();sub.subLayerKey=SUBLAYER;sub.displayData.name=name.as_mut_ptr();sub.weight=0x7000;
        check(FwpmSubLayerAdd0(handle,&sub,ptr::null_mut()))?;
        for (key,layer) in [(BIND4,FWPM_LAYER_ALE_BIND_REDIRECT_V4),(BIND6,FWPM_LAYER_ALE_BIND_REDIRECT_V6)] {
            let mut callout:FWPM_CALLOUT0=std::mem::zeroed();callout.calloutKey=key;callout.applicableLayer=layer;callout.displayData.name=name.as_mut_ptr();callout.flags=FWPM_CALLOUT_FLAG_USES_PROVIDER_CONTEXT;
            check(FwpmCalloutAdd0(handle,&callout,ptr::null_mut(),ptr::null_mut()))?;
            let mut found=ptr::null_mut();check(FwpmCalloutGetByKey0(handle,&key,&mut found))?;
            let registered=(*found).flags&FWPM_CALLOUT_FLAG_REGISTERED!=0;FwpmFreeMemory0((&mut found as *mut *mut FWPM_CALLOUT0).cast());
            if !registered{bail!("Routing unavailable: the signed OutSwitchDriver is not loaded. No rules were applied.");}
        }
        self.session=Some(owner);Ok(())
    }}
    pub fn enabled(&self)->bool{self.session.is_some()}
    unsafe fn install(&self,handle:HANDLE,rule:&Rule,network:Option<&Network>,ordinal:u64,filters:&mut Vec<u64>,contexts:&mut Vec<GUID>)->Result<()> {
        rule.validate()?;
        // Canonical application identity is obtained by WFP from the complete executable path.
        let mut app=ptr::null_mut();check(FwpmGetAppIdFromFileName0(wide(&rule.path).as_ptr(),&mut app)).with_context(||format!("Cannot identify {}",rule.path))?;let app=AppId(app);
        let mut name=wide(&format!("Out-Switch: {}",rule.path));
        for (family,bind_layer,auth_layer,key) in [(2,FWPM_LAYER_ALE_BIND_REDIRECT_V4,FWPM_LAYER_ALE_AUTH_CONNECT_V4,BIND4),(23,FWPM_LAYER_ALE_BIND_REDIRECT_V6,FWPM_LAYER_ALE_AUTH_CONNECT_V6,BIND6)] {
            let ip=network.and_then(|n|n.addresses.iter().filter_map(|s|s.parse::<IpAddr>().ok()).find(|a| match a {IpAddr::V4(v)=>family==2&&!v.is_loopback()&&!v.is_unspecified()&& !v.is_link_local(),IpAddr::V6(v)=>family==23&&!v.is_loopback()&&!v.is_unspecified()&& (v.segments()[0]&0xffc0)!=0xfe80}));
            let mut conditions:Vec<FWPM_FILTER_CONDITION0>=Vec::new();
            let mut condition:FWPM_FILTER_CONDITION0=std::mem::zeroed();condition.fieldKey=FWPM_CONDITION_ALE_APP_ID;condition.matchType=FWP_MATCH_EQUAL;condition.conditionValue.r#type=FWP_BYTE_BLOB_TYPE;condition.conditionValue.Anonymous.byteBlob=app.0;conditions.push(condition);
            let mut filter:FWPM_FILTER0=std::mem::zeroed();filter.displayData.name=name.as_mut_ptr();filter.subLayerKey=SUBLAYER;
            filter.weight.r#type=FWP_UINT8;filter.weight.Anonymous.uint8=8;
            if let Some(ip)=ip {
                let context_key=GUID::from_u128(0xf973cd10_96a3_4ed2_0000_000000000000u128|((self.generation as u128 & 0xffffff)<<32)|((ordinal as u128)<<8)|family as u128);
                let mut bytes=[0u8;24];bytes[..4].copy_from_slice(&1u32.to_le_bytes());bytes[4..8].copy_from_slice(&(family as u32).to_le_bytes());
                match ip{IpAddr::V4(a)=>bytes[8..12].copy_from_slice(&a.octets()),IpAddr::V6(a)=>bytes[8..24].copy_from_slice(&a.octets())};
                let mut blob=FWP_BYTE_BLOB{size:24,data:bytes.as_mut_ptr()};let mut context:FWPM_PROVIDER_CONTEXT0=std::mem::zeroed();
                context.providerContextKey=context_key;context.displayData.name=name.as_mut_ptr();context.r#type=FWPM_GENERAL_CONTEXT;context.Anonymous.dataBuffer=&mut blob;
                check(FwpmProviderContextAdd0(handle,&context,ptr::null_mut(),ptr::null_mut()))?;contexts.push(context_key);
                filter.flags=FWPM_FILTER_FLAG_HAS_PROVIDER_CONTEXT|FWPM_FILTER_FLAG_PERMIT_IF_CALLOUT_UNREGISTERED;
                filter.Anonymous.providerContextKey=context_key;filter.layerKey=bind_layer;filter.action.r#type=FWP_ACTION_CALLOUT_TERMINATING;filter.action.Anonymous.calloutKey=key;
            } else {
                // No address of this family: block new non-loopback flows instead of silently using another adapter.
                filter.layerKey=auth_layer;filter.action.r#type=FWP_ACTION_BLOCK;
                let mut no_loopback:FWPM_FILTER_CONDITION0=std::mem::zeroed();no_loopback.fieldKey=FWPM_CONDITION_FLAGS;no_loopback.matchType=FWP_MATCH_FLAGS_NONE_SET;no_loopback.conditionValue.r#type=FWP_UINT32;no_loopback.conditionValue.Anonymous.uint32=FWP_CONDITION_FLAG_IS_LOOPBACK;conditions.push(no_loopback);
            }
            filter.numFilterConditions=conditions.len() as u32;filter.filterCondition=conditions.as_mut_ptr();let mut id=0;
            check(FwpmFilterAdd0(handle,&filter,ptr::null_mut(),&mut id))?;filters.push(id);
        }
        Ok(())
    }
}
impl RoutingEngine for WindowsRoutingEngine {
    fn apply(&mut self,rules:&[Rule],networks:&[Network])->Result<()> {unsafe{
        self.open()?;let handle=self.session.as_ref().unwrap().0;
        let mut statuses=BTreeMap::new();let mut selected=BTreeMap::new();let mut plan=Vec::new();
        for rule in rules {
            let key=identity(&rule.path);
            match select_network(rule,networks,self.selected.get(&key).map(String::as_str)) {
                Selection::Automatic=>{statuses.insert(key,"Automatic — Windows chooses".into());},
                Selection::Network(id)=>{
                    let n=networks.iter().find(|n|n.id==id).unwrap();
                    anyhow::ensure!(n.strong_host,"{} has weak-host sending enabled or unknown; cannot guarantee interface isolation",n.name);
                    for address in &n.addresses {
                        let usable=match address.parse::<IpAddr>(){Ok(IpAddr::V4(a))=>!a.is_loopback()&&!a.is_unspecified()&&!a.is_link_local(),Ok(IpAddr::V6(a))=>!a.is_loopback()&&!a.is_unspecified()&&(a.segments()[0]&0xffc0)!=0xfe80,Err(_)=>false};
                        if usable {anyhow::ensure!(!networks.iter().any(|other|other.id!=n.id&&other.connected&&other.addresses.contains(address)),"{} shares source address {} with another connected adapter; source binding cannot ensure independent routing",n.name,address);}
                    }
                    plan.push((rule,Some(n)));selected.insert(key.clone(),id);
                    statuses.insert(key,"Filters installed — new sockets only; verify observed connection".into());
                },
                Selection::Unavailable(_)=>{plan.push((rule,None));statuses.insert(key,"Selected network unavailable — new remote flows blocked".into());},
                Selection::ApprovalRequired(_)=>{plan.push((rule,None));statuses.insert(key,"Fallback needs approval — choose the fallback network to continue".into());}
            }
        }
        check(FwpmTransactionBegin0(handle,0))?;self.generation+=1;
        let mut next_filters=vec![];let mut next_contexts=vec![];
        let result=(||{
            for id in &self.filters{check(FwpmFilterDeleteById0(handle,*id))?;}
            for key in &self.contexts{check(FwpmProviderContextDeleteByKey0(handle,key))?;}
            for (i,(rule,network)) in plan.into_iter().enumerate(){self.install(handle,rule,network,i as u64,&mut next_filters,&mut next_contexts)?;}
            check(FwpmTransactionCommit0(handle))
        })();
        if result.is_err(){FwpmTransactionAbort0(handle);return result;}
        self.filters=next_filters;self.contexts=next_contexts;self.statuses=statuses;self.selected=selected;Ok(())
    }}
    fn clear(&mut self)->Result<()>{self.session.take();self.filters.clear();self.contexts.clear();self.statuses.clear();self.selected.clear();Ok(())}
}

