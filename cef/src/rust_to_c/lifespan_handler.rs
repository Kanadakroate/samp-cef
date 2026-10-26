use super::Wrapper;
use crate::handlers::lifespan::LifespanHandler;

use crate::browser::Browser;
use cef_sys::{cef_browser_t, cef_life_span_handler_t};

extern "system" fn on_after_created<I: LifespanHandler>(
    this: *mut cef_life_span_handler_t, browser: *mut cef_browser_t,
) {
    let obj: &mut Wrapper<_, I> = Wrapper::unwrap(this);

    let browser = Browser::from_raw_add_ref(browser);

    obj.interface.on_after_created(browser);
}

extern "system" fn on_before_close<I: LifespanHandler>(
    this: *mut cef_life_span_handler_t, browser: *mut cef_browser_t,
) {
    let obj: &mut Wrapper<_, I> = Wrapper::unwrap(this);

    let browser = Browser::from_raw_borrowed(browser);

    obj.interface.on_before_close(browser);
}

extern "system" fn block_popup(
    _: *mut cef_life_span_handler_t, _: *mut cef_browser_t, _: *mut cef_sys::cef_frame_t,
    _: i32, _: *const cef_sys::cef_string_t, _: *const cef_sys::cef_string_t,
    _: cef_sys::cef_window_open_disposition_t::Type, _: i32,
    _: *const cef_sys::cef_popup_features_t, _: *mut cef_sys::cef_window_info_t,
    _: *mut *mut cef_sys::cef_client_t, _: *mut cef_sys::cef_browser_settings_t,
    _: *mut *mut cef_sys::cef_dictionary_value_t, _: *mut i32,
) -> i32 { 1 }

pub fn wrap<T: LifespanHandler>(lifespan: T) -> *mut cef_life_span_handler_t {
    let mut object: cef_life_span_handler_t = unsafe { std::mem::zeroed() };
    object.on_before_popup = Some(block_popup);
    object.on_before_close = Some(on_before_close::<T>);
    object.on_after_created = Some(on_after_created::<T>);

    let wrapper = Wrapper::new(object, lifespan);

    Box::into_raw(Box::new(wrapper)) as *mut _
}
