use eframe::egui::{self, Color32, RichText, Vec2};
use outswitch_common::Network;
use outswitch_network::ping::{self, Reply, Summary, TARGETS};
use std::{collections::BTreeMap, sync::mpsc::{self, Receiver, Sender}, time::{Duration, Instant}};

enum Update { Reply(Reply), Finished(String) }
#[derive(Default)]
struct Test {
    hidden: bool, selected: usize, tested_target: Option<usize>, source: String, replies: Vec<Reply>,
    receiver: Option<Receiver<Update>>, cancel: Option<Sender<()>>, status: String,
}
#[derive(Default)]
pub struct PingPanel { tests: BTreeMap<String,Test> }
fn milliseconds(value:Option<f64>) -> String { value.map(|v|format!("{v:.1} ms")).unwrap_or_else(||"—".into()) }
impl PingPanel {
    #[cfg(debug_assertions)]
    pub fn layout_example(&mut self,id:&str) {
        self.tests.entry(id.into()).or_insert_with(||Test {
            tested_target:Some(0),source:"192.0.2.1 (example)".into(),status:"Layout example · 10 probes".into(),
            replies:[Some(8),Some(11),None,Some(10),Some(20),Some(12),Some(9),Some(11),Some(10),Some(9)]
                .into_iter().enumerate().map(|(i,ms)|Reply {sequence:i as u32+1,ms,ttl:ms.map(|_|58),bytes:32,
                    from:ms.map(|_|"1.1.1.1".parse().unwrap()),error:if ms.is_none(){"Timed out after 1500 ms".into()}else{String::new()}}).collect(),
            ..Default::default()
        });
    }
    pub fn has_test(&self,id:&str)->bool {self.tests.get(id).is_some_and(|t|t.tested_target.is_some())}
    pub fn poll(&mut self) {
        for test in self.tests.values_mut() {
            let updates:Vec<_>=test.receiver.as_ref().map(|r|r.try_iter().collect()).unwrap_or_default();
            for update in updates { match update {
                Update::Reply(reply)=>test.replies.push(reply),
                Update::Finished(status)=>{test.status=status;test.receiver=None;test.cancel=None;}
            }}
        }
    }
    pub fn show(&mut self,ui:&mut egui::Ui,network:&Network,accent:Color32) {
        let test=self.tests.entry(network.id.clone()).or_default();
        ui.separator();ui.horizontal(|ui|{
            if test.hidden {if ui.small_button("Show ping test").clicked(){test.hidden=false;}}
            else{ui.label(RichText::new("PING TEST").strong());if ui.small_button("Hide").clicked(){test.hidden=true;}}
            if test.hidden&&test.receiver.is_some(){ui.spinner();if ui.small_button("Stop").clicked(){if let Some(cancel)=&test.cancel{let _=cancel.send(());}}}
        });
        if test.hidden{return;}
        let running=test.receiver.is_some();
        ui.add_enabled_ui(!running,|ui|{
            egui::ComboBox::from_id_salt(("ping-target",&network.id)).width(ui.available_width()-8.)
                .selected_text(format!("{} · {}",TARGETS[test.selected].0,TARGETS[test.selected].1))
                .show_ui(ui,|ui|{ for (i,(name,ip)) in TARGETS.iter().enumerate(){ui.selectable_value(&mut test.selected,i,format!("{name} · {ip}"));} });
        });
        ui.horizontal(|ui|{
            if running {
                ui.spinner();ui.label(format!("{} / 10 replies processed",test.replies.len()));
                if ui.button("Stop").clicked(){if let Some(cancel)=&test.cancel{let _=cancel.send(());}test.status="Stopping…".into();}
            } else if ui.add_enabled(network.connected,egui::Button::new("▶ Test ping")).clicked() {
                test.replies.clear();test.tested_target=Some(test.selected);test.status.clear();test.source.clear();
                match ping::source(network) {
                    Err(error)=>test.status=error.to_string(),
                    Ok(source)=>{
                        test.source=source.to_string();
                        let target=TARGETS[test.selected].1.parse().expect("fixed IPv4 target");
                        let network=network.clone();let repaint=ui.ctx().clone();
                        let (tx,rx)=mpsc::channel();let (cancel,stop)=mpsc::channel();
                        test.receiver=Some(rx);test.cancel=Some(cancel);test.status="Testing this network…".into();
                        std::thread::spawn(move||{
                            let mut status="Complete · 10 probes".to_string();
                            for sequence in 1..=10 {
                                if !matches!(stop.try_recv(),Err(mpsc::TryRecvError::Empty)){status="Stopped".into();break;}
                                let started=Instant::now();
                                match ping::probe(&network,source,target,sequence) {
                                    Ok(reply)=>{if tx.send(Update::Reply(reply)).is_err(){return;}repaint.request_repaint();},
                                    Err(error)=>{status=error.to_string();break;}
                                }
                                if sequence<10 {
                                    let delay=Duration::from_secs(1).saturating_sub(started.elapsed());
                                    if !matches!(stop.recv_timeout(delay),Err(mpsc::RecvTimeoutError::Timeout)){status="Stopped".into();break;}
                                }
                            }
                            let _=tx.send(Update::Finished(status));repaint.request_repaint();
                        });
                    }
                }
            }
        });
        if let Some(target)=test.tested_target {
            ui.label(RichText::new(format!("{} → {}",test.source,TARGETS[target].1)).small());
            ui.label(RichText::new(&test.status).small());
            if !network.connected {ui.colored_label(Color32::from_rgb(242,170,90),"Disconnected · showing the last test");}
            let summary=Summary::from_replies(&test.replies);
            if summary.sent>0 {
                let latest=test.replies.last().and_then(|r|r.ms).map(f64::from);
                ui.horizontal(|ui|{
                    ui.label(RichText::new(milliseconds(latest)).size(24.).strong().color(accent));
                    ui.label(format!("{:.0}% loss",summary.loss_percent()));
                });
                egui::Grid::new(("ping-summary",&network.id)).num_columns(2).spacing([14.,4.]).show(ui,|ui|{
                    for (title,value) in [("Minimum",summary.min),("Average",summary.average),("Maximum",summary.max),("Jitter",summary.jitter)] {
                        ui.label(title);let response=ui.label(milliseconds(value));
                        if title=="Jitter" {response.on_hover_text("Mean latency change between consecutive successful replies. A timeout breaks the sequence.");}ui.end_row();
                    }
                });
                ui.label(format!("Sent {} · Received {} · Lost {}",summary.sent,summary.received,summary.lost));
                let (rect,_)=ui.allocate_exact_size(Vec2::new(ui.available_width(),54.),egui::Sense::hover());
                let painter=ui.painter_at(rect);let peak=summary.max.unwrap_or(1.).max(1.) as f32;
                let step=rect.width()/10.;
                for (i,reply) in test.replies.iter().enumerate(){
                    let x=rect.left()+step*(i as f32+0.5);
                    if let Some(ms)=reply.ms {
                        let height=(ms as f32/peak*(rect.height()-5.)).max(3.);
                        painter.rect_filled(egui::Rect::from_min_max(egui::pos2(x-step*0.3,rect.bottom()-height),egui::pos2(x+step*0.3,rect.bottom())),2.,accent);
                    } else {painter.text(egui::pos2(x,rect.center().y),egui::Align2::CENTER_CENTER,"×",egui::FontId::proportional(18.),Color32::from_rgb(242,124,124));}
                }
                egui::CollapsingHeader::new("Reply details").id_salt(("ping-details",&network.id)).show(ui,|ui|{
                    for reply in &test.replies {
                        ui.label(RichText::new(if let Some(ms)=reply.ms {
                            format!("#{} · {} · {ms} ms · {} bytes · TTL {}",reply.sequence,reply.from.map(|a|a.to_string()).unwrap_or_default(),reply.bytes,reply.ttl.unwrap_or(0))
                        } else {format!("#{} · {}",reply.sequence,reply.error)}).small());
                    }
                });
                if summary.lost>0 {ui.label(RichText::new("Some networks or targets block ping. Timeouts alone do not prove an internet outage.").small().color(Color32::from_gray(150)));}
            }
        } else {ui.label(RichText::new("10 ICMP probes through this adapter. Results appear here.").small().color(Color32::from_gray(150)));}
    }
}
