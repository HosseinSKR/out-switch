//! Explicit, opt-in live test. Uses a separate executable identity and opaque DNS
//! messages over both TCP and UDP; never changes saved rules or system routes.
use anyhow::{Result,Context,ensure};
use outswitch_common::{Application,Rule,Priority};
use outswitch_core::RoutingEngine;
use std::{io::{Read,Write},net::{TcpStream,UdpSocket},time::{Duration,Instant}};
const QUERY:&[u8]=b"\x53\x57\x01\x00\x00\x01\x00\x00\x00\x00\x00\x00\x07example\x03com\x00\x00\x01\x00\x01";
fn validate(reply:&[u8])->Result<()>{ensure!(reply.len()>=12&&reply[..2]==QUERY[..2]&&reply[2]&0x80!=0&&reply[3]&15==0,"DNS probe response did not validate");Ok(())}
pub fn child()->Result<()>{
    let mut stream=TcpStream::connect_timeout(&"1.1.1.1:53".parse()?,Duration::from_secs(8))?;
    stream.set_read_timeout(Some(Duration::from_secs(8)))?;stream.set_write_timeout(Some(Duration::from_secs(8)))?;
    stream.write_all(&(QUERY.len()as u16).to_be_bytes())?;stream.write_all(QUERY)?;
    let mut length=[0;2];stream.read_exact(&mut length)?;let mut data=vec![0;u16::from_be_bytes(length)as usize];stream.read_exact(&mut data)?;validate(&data)?;
    println!("PASS: TCP response received and validated");drop(stream);
    let udp=UdpSocket::bind("0.0.0.0:0")?;udp.connect("1.1.1.1:53")?;udp.set_read_timeout(Some(Duration::from_secs(4)))?;
    let mut buf=[0;4096];let mut success=false;
    for _ in 0..3{udp.send(QUERY)?;if let Ok(n)=udp.recv(&mut buf){validate(&buf[..n])?;success=true;break;}}
    ensure!(success,"No UDP reply; DNS may be blocked on this connection");println!("PASS: UDP response received and validated");Ok(())
}
pub fn run(id:&str)->Result<()>{
    let networks=outswitch_network::scan()?;let network=networks.iter().find(|n|n.id==id&&n.connected).context("Choose a connected adapter from --ping-list")?;
    let folder=std::env::temp_dir().join(format!("OutSwitch-probe-{}-{}",std::process::id(),outswitch_common::now()));std::fs::create_dir_all(&folder)?;
    let helper=folder.join("OutSwitchTrafficProbe.exe");std::fs::copy(std::env::current_exe()?,&helper)?;
    let result=(||->Result<()>{
        let rule=Rule{path:helper.to_string_lossy().into_owned(),network:Some(id.into()),priority:Priority::High,upload_mbps:Some(1.),download_mbps:Some(1.),..Default::default()};
        let mut engine=outswitch_routing::WindowsRoutingEngine::default();engine.apply(&[rule.clone()],&networks)?;
        println!("Testing {} (interface {}) with TCP and UDP to Cloudflare DNS",network.name,network.index);
        use std::os::windows::process::CommandExt;
        let mut child=std::process::Command::new(&helper).arg("--traffic-probe").creation_flags(0x08000000).spawn()?;
        let start=Instant::now();let status=loop{
            if let Some(status)=child.try_wait()?{break status;}
            if start.elapsed()>Duration::from_secs(35){let _=child.kill();let _=child.wait();anyhow::bail!("Live routing test timed out");}
            if let Some(error)=engine.failure(){let _=child.kill();let _=child.wait();anyhow::bail!("Routing worker failed: {error}");}
            std::thread::sleep(Duration::from_millis(100));
        };
        let mut apps=[Application{path:rule.path,..Default::default()}];engine.measurements(&mut apps);engine.clear()?;
        ensure!(status.success(),"The traffic probe failed. This can be a relay fault or DNS blocking; routing is not verified.");
        let a=&apps[0];ensure!(a.uploaded.unwrap_or(0)>QUERY.len()as u64*2&&a.downloaded.unwrap_or(0)>24&&a.observed_networks.contains(&id.to_string()),"Replies did not pass through the selected-adapter relay");
        println!("PASS: selected-adapter relay forwarded {} upload bytes and {} download bytes. Driver filters closed.",a.uploaded.unwrap(),a.downloaded.unwrap());
        println!("This smoke test does not certify two-adapter isolation, bandwidth accuracy, IPv6, failover, or compatibility with every app.");Ok(())
    })();
    let _=std::fs::remove_file(&helper);let _=std::fs::remove_dir(&folder);result
}
