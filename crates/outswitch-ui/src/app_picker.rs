use eframe::egui::{self,RichText,Vec2};
use outswitch_common::*;
use std::sync::mpsc::Receiver;
#[derive(Default)]
pub struct Picker { open:bool, search:String, apps:Vec<SavedApplication>, scan:Option<Receiver<super::app_scan::ScanResult>>, warning:String }
impl Picker {
    pub fn open(&mut self,ctx:&egui::Context,known:Vec<Application>){
        self.open=true;self.search.clear();self.apps.clear();self.warning.clear();self.scan=Some(super::app_scan::start(ctx.clone(),known));
    }
    pub fn show(&mut self,ctx:&egui::Context,state:&Snapshot,icons:&mut super::icons::Icons)->Option<Request>{
        if let Some(result)=self.scan.as_ref().and_then(|r|r.try_recv().ok()){self.apps=result.apps;self.warning=result.warning;self.scan=None;}
        if !self.open{return None;}
        let mut open=true;let mut request=None;
        egui::Window::new("Add applications").open(&mut open).default_width(590.).resizable(true).collapsible(false).show(ctx,|ui|{
            ui.label("Choose an app to add to Out-Switch. You can pin it to your dashboard afterwards.");
            ui.horizontal(|ui|{
                ui.add(egui::TextEdit::singleline(&mut self.search).hint_text("Search available apps…").desired_width(290.));
                if ui.add_enabled(self.scan.is_none(),egui::Button::new("Scan again")).clicked(){self.scan=Some(super::app_scan::start(ctx.clone(),state.applications.clone()));self.warning.clear();}
            });
            if self.scan.is_some(){ui.horizontal(|ui|{ui.spinner();ui.label("Scanning installed and running apps…");});}
            if !self.warning.is_empty(){ui.label(&self.warning);}
            ui.separator();
            let query=self.search.to_lowercase();let mut count=0;
            egui::ScrollArea::vertical().id_salt("available-apps").max_height(350.).show(ui,|ui|{
                for app in &self.apps {
                    if !app.name.to_lowercase().contains(&query)&&!app.path.to_lowercase().contains(&query){continue;}count+=1;
                    ui.push_id(&app.path,|ui|{ui.horizontal(|ui|{
                        if let Some(icon)=icons.get(ctx,&app.path){ui.image((icon.id(),Vec2::splat(28.)));}
                        ui.allocate_ui_with_layout(Vec2::new((ui.available_width()-85.).max(100.),30.),egui::Layout::left_to_right(egui::Align::Center),|ui|{ui.add(egui::Label::new(RichText::new(&app.name).strong()).truncate()).on_hover_text(&app.path);});
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center),|ui|{
                            let added=state.config.apps.contains_key(&identity(&app.path));
                            if ui.add_enabled(state.service_connected&&!added,egui::Button::new(if added{"Added"}else{"Add"})).clicked(){request=Some(Request::AddApp(app.clone()));}
                        });
                    });});ui.separator();
                }
                if count==0&&self.scan.is_none(){ui.label("No matching apps. Browse for the executable below.");}
            });
            ui.add_space(8.);ui.label("Can't find it? Select the program's .exe file.");
            if ui.add_enabled(state.service_connected,egui::Button::new("Browse files…")).clicked(){if let Some(path)=super::browse(){request=Some(Request::AddApp(SavedApplication::from_path(path)));}}
            ui.small("Scanning covers desktop shortcuts, registered apps and running network apps. Some Store apps and portable apps need Browse.");
        });self.open=open;request
    }
}
