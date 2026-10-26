//! Standalone native notification-bridge audit. Never loads GTA,
//! account data, microphone or system loopback. Captures only its own document.
//! Copy into client/examples/ and build against the pinned local samp-cef source.
#[path="../src/notification_sound.rs"] mod notification_sound;
use notification_sound::{NotificationSession,Voice};
#[path="../src/browser/assets_scheme.rs"] mod assets_scheme;
mod utils {pub fn assets_dir()->std::path::PathBuf{std::env::current_exe().unwrap().parent().unwrap().join("assets")}}
use std::{sync::{Arc, Mutex, atomic::{AtomicBool, Ordering}}, time::{Duration, Instant}, io::Write};
use cef::{app::App, browser::{Browser, BrowserHost, Frame, ContextMenuParams, MenuModel}, client::Client,
    command_line::CommandLine, types::string::{CefString, into_cef_string},
    handlers::{audio::AudioHandler, lifespan::LifespanHandler, load::LoadHandler,
    render::{RenderHandler, DirtyRects, PaintElement}, permission::PermissionHandler,
    context_menu::ContextMenuHandler, render_process::RenderProcessHandler,
    browser_process::BrowserProcessHandler}};
use cef_sys::{cef_rect_t, cef_audio_parameters_t};

struct Stream { id: i32, rate: i32, channels: i32, start: u128, stop: u128, samples: Vec<f32> }
struct Mixed { voice: Voice, samples: Vec<f32>, done: bool }
struct State { session: Mutex<NotificationSession>, mixed: Mutex<Vec<Mixed>>, results: Mutex<Vec<String>>, began: Instant, browser: Mutex<Option<Browser>>, streams: Mutex<Vec<Stream>>, closed: AtomicBool }
#[derive(Clone)] struct Probe(Arc<State>);
impl Client for Probe {
    type LifespanHandler=Self; type RenderHandler=Self; type ContextMenuHandler=Self;
    type LoadHandler=Self; type AudioHandler=Self; type PermissionHandler=Self;
    fn lifespan_handler(&self)->Option<Self>{Some(self.clone())}
    fn render_handler(&self)->Option<Self>{Some(self.clone())}
    fn load_handler(&self)->Option<Self>{Some(self.clone())}
    fn audio_handler(&self)->Option<Self>{Some(self.clone())}
    fn on_process_message(&self,_:Browser,frame:Frame,source:cef::ProcessId,msg:cef::process_message::ProcessMessage)->bool {
        if !matches!(source,cef::ProcessId::Renderer) || !frame.is_main(){return false;}
        let url=frame.url(); let Some(bundle)=cef::local_policy::notification_bundle(&url) else{return false;};
        let args=msg.argument_list();
        if msg.name().to_string()=="emit_event" {
            if args.string(0).to_string()=="probe_result" { self.0.results.lock().unwrap().push(args.string(1).to_string()); }
            return true;
        }
        if msg.name().to_string()!="lr_notification_sound" || args.len()!=2{return false;}
        let action=args.string(0).to_string();let token=args.string(1).to_string();
        let status=self.0.session.lock().unwrap().request(&action,&token,bundle,utils::assets_dir(),true,|voice|{
            self.0.mixed.lock().unwrap().push(Mixed{voice,samples:Vec::new(),done:false});true
        });
        let reply=cef::process_message::ProcessMessage::create("trigger_event");
        let values=cef::types::list::List::new(); values.set_string(0,&CefString::new(&token));values.set_string(1,&CefString::new(status));
        reply.argument_list().set_string(0,&CefString::new("lr:notification-sound"));reply.argument_list().set_list(1,values);
        frame.send_process_message(cef::ProcessId::Renderer,reply);true
    }
}
impl LifespanHandler for Probe {
    fn on_after_created(&self,b:Browser){*self.0.browser.lock().unwrap()=Some(b);}
    fn on_before_close(&self,_:Browser){self.0.browser.lock().unwrap().take();self.0.closed.store(true,Ordering::SeqCst);}
}
impl LoadHandler for Probe {
    fn on_load_error(&self,_:Browser,_:Frame,code:i32){eprintln!("LOAD_ERROR {code}");}
    fn on_load_end(&self,_:Browser,_:Frame,status:i32){eprintln!("LOAD {status}");}
}
impl ContextMenuHandler for Probe {fn on_before_context_menu(&self,_:Browser,_:Frame,_:ContextMenuParams,_:MenuModel){}}
impl PermissionHandler for Probe {}
impl RenderHandler for Probe {
    fn view_rect(&self,_:Browser,r:&mut cef_rect_t){*r=cef_rect_t{x:0,y:0,width:320,height:200};}
    fn on_popup_show(&self,_:Browser,_:bool){}
    fn on_popup_size(&self,_:Browser,_:&cef_rect_t){}
    fn on_paint(&self,_:Browser,_:PaintElement,_:DirtyRects,_:&[u8],_:usize,_:usize){}
}
impl AudioHandler for Probe {
    fn get_audio_parameters(&self,_:Browser,_:&mut cef_audio_parameters_t)->bool{true}
    fn on_audio_stream_started(&self,_:Browser,id:i32,channels:i32,_:i32,rate:i32,_:i32){
        assert!((1..=2).contains(&channels));
        let mut streams=self.0.streams.lock().unwrap();assert!(streams.len()<16);
        streams.push(Stream{id,rate,channels,start:self.0.began.elapsed().as_micros(),stop:0,samples:Vec::with_capacity(44100*2*28)});
    }
    fn on_audio_stream_packet(&self,_:Browser,id:i32,data:*mut *const f32,frames:i32,_:i64){
        if frames<=0 || data.is_null(){return;}
        let mut streams=self.0.streams.lock().unwrap();
        if let Some(s)=streams.iter_mut().rev().find(|s|s.id==id && s.stop==0){
            if s.samples.len()+frames as usize*s.channels as usize>44100*2*28{return;}
            for i in 0..frames as usize { for c in 0..s.channels as usize {
                s.samples.push(unsafe{*(*data.add(c)).add(i)});
            }}
        }
    }
    fn on_audio_stream_stopped(&self,_:Browser,id:i32){
        if let Some(s)=self.0.streams.lock().unwrap().iter_mut().rev().find(|s|s.id==id && s.stop==0){s.stop=self.0.began.elapsed().as_micros();}
    }
    fn on_audio_stream_error(&self,_:Browser,error:String){eprintln!("AUDIO_ERROR {error}");}
}
#[derive(Clone)] struct ProbeApp;
impl RenderProcessHandler for ProbeApp{}
impl BrowserProcessHandler for ProbeApp{}
impl App for ProbeApp{
    type RenderProcessHandler=Self;type BrowserProcessHandler=Self;
    fn on_register_custom_schemes(&self,registrar:*mut cef_sys::cef_scheme_registrar_t){unsafe{assets_scheme::register_custom_scheme(registrar);}}
    fn on_before_command_line_processing(&self,_:CefString,cmd:CommandLine){
        cmd.append_switch_with_value("autoplay-policy","no-user-gesture-required");
        cmd.append_switch("disable-gpu");cmd.append_switch("disable-background-networking");
    }
}
#[link(name="kernel32")] unsafe extern "system" {fn GetModuleHandleW(name:*const u16)->*mut std::ffi::c_void;}
fn main(){
    let args=cef_sys::cef_main_args_t{instance:unsafe{GetModuleHandleW(std::ptr::null())}.cast()};
    let result=cef::execute_process(&args,Some(ProbeApp));if result>=0{std::process::exit(result);}
    let options:Vec<String>=std::env::args().collect();assert_eq!(options.len(),4,"probe <runtime> <fixture-url> <output>");
    let runtime=std::path::Path::new(&options[1]);let output=std::path::Path::new(&options[3]);
    std::fs::create_dir_all(output).unwrap();
    let mut settings:cef_sys::cef_settings_t=unsafe{std::mem::zeroed()};settings.size=std::mem::size_of_val(&settings);
    settings.browser_subprocess_path=into_cef_string(&runtime.join("cef-renderer.exe").to_string_lossy());
    settings.no_sandbox=1;settings.windowless_rendering_enabled=1;
    settings.root_cache_path=into_cef_string(&output.join("profile").to_string_lossy());
    settings.resources_dir_path=into_cef_string(&runtime.to_string_lossy());
    settings.locales_dir_path=into_cef_string(&runtime.join("locales").to_string_lossy());
    settings.log_file=into_cef_string(&output.join("cef.log").to_string_lossy());
    assert_ne!(cef::initialize(Some(&args),&settings,Some(ProbeApp)),0);
    assets_scheme::register_scheme_handler_factory();
    let state=Arc::new(State{session:Mutex::default(),mixed:Mutex::new(Vec::new()),results:Mutex::new(Vec::new()),began:Instant::now(),browser:Mutex::new(None),streams:Mutex::new(Vec::new()),closed:AtomicBool::new(false)});
    let mut window:cef_sys::cef_window_info_t=unsafe{std::mem::zeroed()};window.size=std::mem::size_of_val(&window);window.windowless_rendering_enabled=1;
    let mut bs:cef_sys::cef_browser_settings_t=unsafe{std::mem::zeroed()};bs.size=std::mem::size_of_val(&bs);bs.windowless_frame_rate=30;
    assert_ne!(BrowserHost::create_browser(&window,Some(Probe(state.clone())),&CefString::new(&options[2]),&bs),0);
    let mut closing=false; let mut mixed_frames=0usize;
    while state.began.elapsed()<Duration::from_secs(28) && !state.closed.load(Ordering::SeqCst){
        cef::do_message_loop_work();
        let target=(state.began.elapsed().as_secs_f64()*44100.) as usize;
        while mixed_frames+256<=target {
            for m in state.mixed.lock().unwrap().iter_mut().filter(|m|!m.done) {
                let mut output=[(0.,0.);256];m.done=!m.voice.render(44100,1.,&mut output);
                for (l,r) in output {m.samples.push(l);m.samples.push(r);}
            }
            mixed_frames+=256;
        }
        if !closing && state.began.elapsed()>Duration::from_secs(24){
            let b=state.browser.lock().unwrap().clone();if let Some(b)=b{b.host().close_browser(true);closing=true;}
        }
        std::thread::sleep(Duration::from_millis(2));
    }
    assert!(state.closed.load(Ordering::SeqCst),"CEF close timed out");
    cef::shutdown();
    let streams=state.streams.lock().unwrap();let mut meta=std::fs::File::create(output.join("streams.jsonl")).unwrap();
    for (index,s) in streams.iter().enumerate(){
        let mut file=std::io::BufWriter::new(std::fs::File::create(output.join(format!("stream-{index}.f32"))).unwrap());
        for sample in &s.samples{file.write_all(&sample.to_le_bytes()).unwrap();}file.flush().unwrap();
        writeln!(meta,"{{\"index\":{index},\"id\":{},\"rate\":{},\"channels\":{},\"startUs\":{},\"stopUs\":{},\"frames\":{}}}",s.id,s.rate,s.channels,s.start,s.stop,s.samples.len()/s.channels as usize).unwrap();
    }
    let mixed=state.mixed.lock().unwrap();
    for (index,m) in mixed.iter().enumerate(){
        assert!(m.done,"unfinished native voice");
        let mut file=std::io::BufWriter::new(std::fs::File::create(output.join(format!("native-{index}.f32"))).unwrap());
        for sample in &m.samples{file.write_all(&sample.to_le_bytes()).unwrap();}
    }
    let results=state.results.lock().unwrap();
    std::fs::write(output.join("results.jsonl"),results.join("\n")).unwrap();
    assert_eq!(mixed.len(),4);assert_eq!(results.len(),4);assert_eq!(streams.len(),0);
    println!("COMPLETE {} native sounds, {} CEF capture streams, {} renderer completions",mixed.len(),streams.len(),results.len());
}
