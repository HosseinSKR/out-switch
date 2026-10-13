use std::{collections::BTreeSet,sync::{Mutex,atomic::{AtomicBool,AtomicU64,Ordering}},time::{Duration,Instant}};
use outswitch_common::{Priority,Rule};
pub struct Traffic{pub uploaded:AtomicU64,pub downloaded:AtomicU64,pub observed:Mutex<BTreeSet<String>>,up:Mutex<Instant>,down:Mutex<Instant>}
impl Default for Traffic{fn default()->Self{Self{uploaded:AtomicU64::new(0),downloaded:AtomicU64::new(0),observed:Mutex::new(BTreeSet::new()),up:Mutex::new(Instant::now()),down:Mutex::new(Instant::now())}}}
fn reserve(previous:Instant,now:Instant,bytes:usize,mbps:f64)->Instant{
    previous.max(now)+Duration::from_secs_f64(bytes as f64/(mbps*125_000.))
}
impl Traffic{
    pub fn reset_waits(&self){*self.up.lock().unwrap()=Instant::now();*self.down.lock().unwrap()=Instant::now();self.observed.lock().unwrap().clear();}
    // One clock per executable and direction, shared by every connection and PID.
    // Waiting happens on bounded relay workers, never on the packet capture thread.
    pub fn wait(&self,rule:&Rule,download:bool,bytes:usize,stop:&AtomicBool,closed:&AtomicBool)->bool{
        let limit=if download{rule.download_mbps}else{rule.upload_mbps};
        let until=if let Some(rate)=limit{
            let mut clock=if download{self.down.lock().unwrap()}else{self.up.lock().unwrap()};
            *clock=reserve(*clock,Instant::now(),bytes,rate);*clock
        }else{Instant::now()};
        loop{if stop.load(Ordering::Relaxed)||closed.load(Ordering::Relaxed){return false;}
            let Some(wait)=until.checked_duration_since(Instant::now())else{return true;};std::thread::sleep(wait.min(Duration::from_millis(20)));
        }
    }
    pub fn count(&self,download:bool,bytes:usize){if download{&self.downloaded}else{&self.uploaded}.fetch_add(bytes as u64,Ordering::Relaxed);}
}
pub fn marking(priority:&Priority)->u8{match priority{Priority::Critical=>46,Priority::High=>34,Priority::Normal=>0,Priority::Low=>10,Priority::Background=>8}}
#[cfg(test)]mod tests{
    use super::*;
    #[test]fn aggregate_limit_reserves_across_connections_and_resets_after_idle(){let now=Instant::now();let first=reserve(now,now,125_000,1.);let second=reserve(first,now,125_000,1.);assert_eq!(second.duration_since(now),Duration::from_secs(2));let idle=now+Duration::from_secs(10);assert_eq!(reserve(second,idle,125_000,1.).duration_since(idle),Duration::from_secs(1));}
    #[test]fn cancellation_interrupts_a_low_bandwidth_wait(){let traffic=Traffic::default();let rule=Rule{upload_mbps:Some(0.001),..Default::default()};let stop=AtomicBool::new(true);assert!(!traffic.wait(&rule,false,65535,&stop,&AtomicBool::new(false)));assert_eq!(traffic.uploaded.load(Ordering::Relaxed),0);}
}
