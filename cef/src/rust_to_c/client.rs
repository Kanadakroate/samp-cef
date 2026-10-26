use super::Wrapper;
use crate::client::Client;

use std::ptr::null_mut;

use crate::ProcessId;
use crate::browser::{Browser, Frame};
use crate::process_message::ProcessMessage;
use cef_sys::{
    cef_audio_handler_t, cef_browser_t, cef_client_t, cef_context_menu_handler_t,
    cef_dialog_handler_t, cef_display_handler_t, cef_download_handler_t, cef_drag_handler_t,
    cef_find_handler_t, cef_focus_handler_t, cef_frame_t, cef_jsdialog_handler_t,
    cef_keyboard_handler_t, cef_life_span_handler_t, cef_load_handler_t, cef_permission_handler_t,
    cef_process_message_t, cef_render_handler_t, cef_request_handler_t,
};

// audio

extern "system" fn audio<I: Client>(this: *mut cef_client_t) -> *mut cef_audio_handler_t {
    let obj: &mut Wrapper<_, I> = Wrapper::unwrap(this);

    if let Some(handler) = obj.interface.audio_handler() {
        super::audio_handler::wrap(handler)
    } else {
        null_mut()
    }
}

// context menu

#[inline(never)]
extern "system" fn context_menu<I: Client>(
    this: *mut cef_client_t,
) -> *mut cef_context_menu_handler_t {
    let obj: &mut Wrapper<_, I> = Wrapper::unwrap(this);

    if let Some(handler) = obj.interface.context_menu_handler() {
        super::context_menu_handler::wrap(handler)
    } else {
        null_mut()
    }
}

// dialog

extern "system" fn dialog(_this: *mut cef_client_t) -> *mut cef_dialog_handler_t {
    println!("dialog");
    null_mut()
}

// display

extern "system" fn display(_this: *mut cef_client_t) -> *mut cef_display_handler_t {
    println!("display");
    null_mut()
}

// download

extern "system" fn download(_this: *mut cef_client_t) -> *mut cef_download_handler_t {
    println!("download");
    null_mut()
}

// drag

extern "system" fn drag(_this: *mut cef_client_t) -> *mut cef_drag_handler_t {
    println!("drag");
    null_mut()
}

// find

extern "system" fn find(_this: *mut cef_client_t) -> *mut cef_find_handler_t {
    println!("find");
    null_mut()
}

// focus

extern "system" fn focus(_this: *mut cef_client_t) -> *mut cef_focus_handler_t {
    println!("focus");
    null_mut()
}

// jsdialog

extern "system" fn jsdialog(_this: *mut cef_client_t) -> *mut cef_jsdialog_handler_t {
    println!("jsdialog");
    null_mut()
}

// keyboard

extern "system" fn keyboard(_this: *mut cef_client_t) -> *mut cef_keyboard_handler_t {
    println!("keyboard");
    null_mut()
}

// load

extern "system" fn load<I: Client>(this: *mut cef_client_t) -> *mut cef_load_handler_t {
    let obj: &mut Wrapper<_, I> = Wrapper::unwrap(this);

    if let Some(handler) = obj.interface.load_handler() {
        super::load_handler::wrap(handler)
    } else {
        null_mut()
    }
}

// render

extern "system" fn render<I: Client>(this: *mut cef_client_t) -> *mut cef_render_handler_t {
    let obj: &mut Wrapper<_, I> = Wrapper::unwrap(this);

    if let Some(handler) = obj.interface.render_handler() {
        super::render_handler::wrap(handler)
    } else {
        null_mut()
    }
}

// request

extern "system" fn request(_this: *mut cef_client_t) -> *mut cef_request_handler_t {
    let mut object: cef_request_handler_t = unsafe { std::mem::zeroed() };
    object.on_before_browse = Some(local_before_browse);
    object.on_open_urlfrom_tab = Some(block_open_url);
    object.get_resource_request_handler = Some(local_resources);
    Box::into_raw(Box::new(Wrapper::new(object, ()))) as *mut _
}

fn local_url(request: *mut cef_sys::cef_request_t) -> bool {
    if request.is_null() { return false; }
    let url = unsafe { (*request).get_url.map(|get| get(request)) };
    let Some(url) = url else { return false; };
    let url = crate::types::string::CefString::from(url).to_string();
    crate::local_policy::allows_asset_url(&url)
}

extern "system" fn local_before_browse(
    _: *mut cef_request_handler_t, _: *mut cef_browser_t, frame: *mut cef_frame_t,
    request: *mut cef_sys::cef_request_t, _: i32, _: i32,
) -> i32 {
    // LR has no subframes, popups or external navigation. A rejected URL never
    // receives a privileged renderer context, even via redirects or links.
    let main = !frame.is_null() && unsafe { (*frame).is_main.map(|f| f(frame) == 1).unwrap_or(false) };
    if main && local_url(request) { 0 } else { 1 }
}

extern "system" fn block_open_url(
    _: *mut cef_request_handler_t, _: *mut cef_browser_t, _: *mut cef_frame_t,
    _: *const cef_sys::cef_string_t, _: cef_sys::cef_window_open_disposition_t::Type, _: i32,
) -> i32 { 1 }

extern "system" fn local_resources(
    _: *mut cef_request_handler_t, _: *mut cef_browser_t, _: *mut cef_frame_t,
    _: *mut cef_sys::cef_request_t, _: i32, _: i32, _: *const cef_sys::cef_string_t, _: *mut i32,
) -> *mut cef_sys::cef_resource_request_handler_t {
    let mut object: cef_sys::cef_resource_request_handler_t = unsafe { std::mem::zeroed() };
    object.on_before_resource_load = Some(local_before_resource);
    Box::into_raw(Box::new(Wrapper::new(object, ()))) as *mut _
}

extern "system" fn local_before_resource(
    _: *mut cef_sys::cef_resource_request_handler_t, _: *mut cef_browser_t,
    _: *mut cef_frame_t, request: *mut cef_sys::cef_request_t, _: *mut cef_sys::cef_callback_t,
) -> cef_sys::cef_return_value_t::Type {
    if local_url(request) { cef_sys::cef_return_value_t::RV_CONTINUE }
    else { cef_sys::cef_return_value_t::RV_CANCEL }
}

extern "system" fn permission<I: Client>(this: *mut cef_client_t) -> *mut cef_permission_handler_t {
    let obj: &mut Wrapper<_, I> = Wrapper::unwrap(this);

    if let Some(handler) = obj.interface.permission_handler() {
        super::permission_handler::wrap(handler)
    } else {
        null_mut()
    }
}

// message received

extern "system" fn on_process_message<I: Client>(
    this: *mut cef_client_t, browser: *mut cef_browser_t, frame: *mut cef_frame_t,
    source_process: cef_sys::cef_process_id_t::Type, message: *mut cef_process_message_t,
) -> i32 {
    let obj: &mut Wrapper<_, I> = Wrapper::unwrap(this);

    let browser = Browser::from_raw_add_ref(browser);
    let frame = Frame::from_raw_add_ref(frame);
    let process_id: ProcessId = ProcessId::from(source_process);
    let message = ProcessMessage::from_raw_add_ref(message);

    let result = obj
        .interface
        .on_process_message(browser, frame, process_id, message);

    if result { 1 } else { 0 }
}

// lifespan handler

extern "system" fn lifespan<I: Client>(this: *mut cef_client_t) -> *mut cef_life_span_handler_t {
    let obj: &mut Wrapper<_, I> = Wrapper::unwrap(this);

    if let Some(handler) = obj.interface.lifespan_handler() {
        super::lifespan_handler::wrap::<I::LifespanHandler>(handler)
    } else {
        null_mut()
    }
}

pub fn wrap<T: Client>(client: T) -> *mut cef_client_t {
    let mut object: cef_client_t = unsafe { std::mem::zeroed() };

    object.get_life_span_handler = Some(lifespan::<T>);
    object.on_process_message_received = Some(on_process_message::<T>);
    object.get_request_handler = Some(request);
    object.get_permission_handler = Some(permission::<T>);
    object.get_render_handler = Some(render::<T>);
    object.get_load_handler = Some(load::<T>);
    object.get_keyboard_handler = Some(keyboard);
    object.get_jsdialog_handler = Some(jsdialog);
    object.get_focus_handler = Some(focus);
    object.get_find_handler = Some(find);
    object.get_drag_handler = Some(drag);
    object.get_download_handler = Some(download);
    object.get_display_handler = Some(display);
    object.get_dialog_handler = Some(dialog);
    object.get_context_menu_handler = Some(context_menu::<T>);
    object.get_audio_handler = Some(audio::<T>);

    let wrapper = Wrapper::new(object, client);

    Box::into_raw(Box::new(wrapper)) as *mut _
}
