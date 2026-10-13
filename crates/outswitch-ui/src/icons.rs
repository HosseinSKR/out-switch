use eframe::egui::{self, ColorImage, TextureHandle};
use std::{collections::{BTreeMap,BTreeSet},sync::mpsc::{self,Receiver,Sender}};
use windows_sys::Win32::{Graphics::Gdi::*,UI::{Shell::ExtractIconExW,WindowsAndMessaging::*}};

pub struct Icons {
    requests:Sender<String>, results:Receiver<(String,Option<ColorImage>)>,
    requested:BTreeSet<String>, textures:BTreeMap<String,TextureHandle>,
}
impl Icons {
    pub fn new(ctx:egui::Context)->Self {
        let (requests,rx)=mpsc::channel::<String>();let (tx,results)=mpsc::channel();
        std::thread::spawn(move||{while let Ok(path)=rx.recv(){
            let image=extract(&path);if tx.send((path,image)).is_err(){break;}ctx.request_repaint();
        }});
        Self {requests,results,requested:BTreeSet::new(),textures:BTreeMap::new()}
    }
    pub fn get(&mut self,ctx:&egui::Context,path:&str)->Option<TextureHandle> {
        while let Ok((path,image))=self.results.try_recv(){if let Some(image)=image{
            self.textures.insert(path.clone(),ctx.load_texture(format!("app-icon:{path}"),image,egui::TextureOptions::LINEAR));
        }}
        if self.requested.len()<512 && self.requested.insert(path.to_owned()){let _=self.requests.send(path.to_owned());}
        self.textures.get(path).cloned()
    }
}

/// Render on both black and white to reconstruct alpha for old masked icons
/// as well as modern 32-bit icons. All GDI objects are released on every path.
fn extract(path:&str)->Option<ColorImage> { unsafe {
    if path.contains('\0') || !std::path::Path::new(path).is_file(){return None;}
    let wide:Vec<u16>=path.encode_utf16().chain(Some(0)).collect();
    let mut icon=std::ptr::null_mut();
    if ExtractIconExW(wide.as_ptr(),0,&mut icon,std::ptr::null_mut(),1)==0 || icon.is_null(){return None;}
    let result=(||{
        let black=render(icon,0)?;let white=render(icon,255)?;
        let mut rgba=Vec::with_capacity(32*32*4);
        for (b,w) in black.chunks_exact(4).zip(white.chunks_exact(4)) {
            let transparency=(0..3).map(|i|w[i].saturating_sub(b[i]) as u32).sum::<u32>()/3;
            let alpha=(255-transparency) as u8;
            for i in [2,1,0] {rgba.push(if alpha==0{0}else{(b[i] as u32*255/alpha as u32).min(255) as u8});}
            rgba.push(alpha);
        }
        Some(ColorImage::from_rgba_unmultiplied([32,32],&rgba))
    })();
    DestroyIcon(icon);result
}}
unsafe fn render(icon:HICON,background:u8)->Option<Vec<u8>> {
    let dc=CreateCompatibleDC(std::ptr::null_mut());if dc.is_null(){return None;}
    let mut info:BITMAPINFO=std::mem::zeroed();
    info.bmiHeader.biSize=std::mem::size_of::<BITMAPINFOHEADER>() as u32;
    info.bmiHeader.biWidth=32;info.bmiHeader.biHeight=-32;info.bmiHeader.biPlanes=1;info.bmiHeader.biBitCount=32;info.bmiHeader.biCompression=BI_RGB;
    let mut data=std::ptr::null_mut();
    let bitmap=CreateDIBSection(dc,&info,DIB_RGB_COLORS,&mut data,std::ptr::null_mut(),0);
    if bitmap.is_null() || data.is_null(){if !bitmap.is_null(){DeleteObject(bitmap);}DeleteDC(dc);return None;}
    let old=SelectObject(dc,bitmap);
    let pixels=std::slice::from_raw_parts_mut(data.cast::<u8>(),32*32*4);pixels.fill(background);
    let drawn=DrawIconEx(dc,0,0,icon,32,32,0,std::ptr::null_mut(),DI_NORMAL);
    GdiFlush();
    let result=(drawn!=0).then(||pixels.to_vec());
    SelectObject(dc,old);DeleteObject(bitmap);DeleteDC(dc);result
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn extracts_native_windows_executable_icon() {
        let path=std::path::PathBuf::from(std::env::var_os("WINDIR").unwrap()).join("explorer.exe");
        let image=extract(&path.to_string_lossy()).expect("Windows Explorer has an executable icon");
        assert_eq!(image.size,[32,32]);
        assert!(image.pixels.iter().any(|p|p.a()>0));
        assert!(image.pixels.iter().any(|p|p.a()==0));
        assert!(extract("Z:\\does-not-exist\\missing.exe").is_none());
    }
}
