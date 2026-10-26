use std::collections::HashMap;
use std::net::{IpAddr, SocketAddr};

use server_core::{CoreEvent, EventValue, ServerCore};
use tracing::{info, trace};
use tracing_subscriber::filter::{LevelFilter, Targets};
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;

use samp::amx::AmxIdent;
use samp::args::Args;
use samp::prelude::*;
use samp::{exec_public, initialize_plugin, native};

mod utils;

const PORT_OFFSET: u16 = 2;
const LOG_LEVEL_ENV: &str = "SAMP_CEF_LOG_LEVEL";

fn server_log_level() -> (LevelFilter, Option<String>) {
    let Ok(value) = std::env::var(LOG_LEVEL_ENV) else {
        return (LevelFilter::INFO, None);
    };

    match value.trim().to_ascii_lowercase().parse::<LevelFilter>() {
        Ok(level) => (level, None),
        Err(_) => (LevelFilter::INFO, Some(value)),
    }
}

struct CefPlugin {
    core: ServerCore,
    events: HashMap<String, (AmxIdent, String)>,
    amx_list: Vec<AmxIdent>,
}

impl CefPlugin {
    fn new() -> Self {
        let ip: IpAddr =
            crate::utils::parse_config_field("bind").unwrap_or_else(|| "0.0.0.0".parse().unwrap());

        let port = crate::utils::parse_config_field("port").unwrap_or(7777);
        let addr = SocketAddr::from((ip, port + PORT_OFFSET));
        let core = ServerCore::new(addr).unwrap_or_else(|error| panic!("{error}"));

        info!(%addr, "CEF server bound");

        CefPlugin {
            core,
            events: HashMap::new(),
            amx_list: Vec::new(),
        }
    }

    #[native(name = "cef_on_player_connect")]
    fn on_player_connect(
        &mut self, _amx: &Amx, player_id: i32, player_ip: AmxString,
    ) -> AmxResult<bool> {
        let player_ip = player_ip.to_string();

        if let Ok(addr) = player_ip.parse::<IpAddr>() {
            self.core.allow_connection(player_id, addr);
        }

        Ok(true)
    }

    #[native(name = "cef_on_player_disconnect")]
    fn on_player_disconnect(&mut self, _: &Amx, player_id: i32) -> AmxResult<bool> {
        self.core.remove_connection(player_id);
        Ok(true)
    }

    #[native(name = "cef_create_browser")]
    fn create_browser(
        &mut self, _: &Amx, player_id: i32, browser_id: i32, url: AmxString, hidden: bool,
        focused: bool,
    ) -> AmxResult<bool> {
        Ok(self.core.create_browser(player_id, browser_id, url.to_string(), hidden, focused))
    }

    #[native(name = "cef_destroy_browser")]
    fn destroy_browser(&mut self, _: &Amx, player_id: i32, browser_id: i32) -> AmxResult<bool> {
        self.core.destroy_browser(player_id, browser_id);
        Ok(true)
    }

    #[native(name = "cef_hide_browser")]
    fn hide_browser(
        &mut self, _: &Amx, player_id: i32, browser_id: i32, hide: bool,
    ) -> AmxResult<bool> {
        self.core.hide_browser(player_id, browser_id, hide);
        Ok(true)
    }

    #[native(name = "cef_focus_browser")]
    fn browser_listen_events(
        &mut self, _: &Amx, player_id: i32, browser_id: i32, focused: bool,
    ) -> AmxResult<bool> {
        self.core.focus_browser(player_id, browser_id, focused);
        Ok(true)
    }

    #[native(name = "cef_lr_protocol_version")]
    fn protocol_version(&mut self, _: &Amx) -> AmxResult<i32> { Ok(1) }

    #[native(name = "cef_bind_game_connection")]
    fn bind_game_connection(&mut self, _: &Amx, player_id: i32, token: AmxString) -> AmxResult<bool> {
        Ok(self.core.bind_game_connection(player_id, &token.to_string()))
    }

    #[native(name = "cef_emit_event", raw)]
    fn emit_event(&mut self, _: &Amx, _args: Args) -> AmxResult<bool> { Ok(false) }

    #[native(name = "cef_emit_browser_event", raw)]
    fn emit_browser_event(&mut self, _: &Amx, args: Args) -> AmxResult<bool> {
        self.emit_browser_event_impl(args)
    }

    fn emit_browser_event_impl(&mut self, args: Args) -> AmxResult<bool> {
        if args.count() < 3 || !(args.count() - 3).is_multiple_of(2) || args.count() > 67 { return Ok(false); }
        let (Some(player_id), Some(browser_id), Some(event)) = (args.get::<i32>(0), args.get::<u32>(1), args.get::<AmxString>(2)) else { return Ok(false); };
        let mut arguments = Vec::new();
        let mut idx = 3;
        while idx < args.count() {
            let Some(ty) = args.get::<Ref<i32>>(idx) else { return Ok(false); };
            let arg = match *ty {
                0 => { let Some(v) = args.get::<AmxString>(idx + 1) else { return Ok(false); }; EventValue { string_value: Some(v.to_string().into()), float_value: None, integer_value: None } },
                1 => { let Some(v) = args.get::<Ref<i32>>(idx + 1) else { return Ok(false); }; EventValue { string_value: None, float_value: None, integer_value: Some(*v) } },
                2 => { let Some(v) = args.get::<Ref<f32>>(idx + 1) else { return Ok(false); }; if !v.is_finite() { return Ok(false); } EventValue { string_value: None, float_value: Some(*v), integer_value: None } },
                _ => return Ok(false),
            };
            arguments.push(arg); idx += 2;
        }
        Ok(self.core.emit_browser_event(player_id, browser_id, &event.to_string(), arguments))
    }

    #[native(name = "cef_always_listen_keys")]
    fn block_input(
        &mut self, _: &Amx, player_id: i32, browser_id: i32, listen: bool,
    ) -> AmxResult<bool> {
        self.core.always_listen_keys(player_id, browser_id, listen);
        Ok(true)
    }

    #[native(name = "cef_subscribe_browser")]
    fn subscribe(
        &mut self, amx: &Amx, event_name: AmxString, callback: AmxString,
    ) -> AmxResult<bool> {
        let ident = amx.ident();
        let event_name = event_name.to_string();
        let callback = callback.to_string();

        self.events.insert(event_name, (ident, callback));

        Ok(true)
    }

    #[native(name = "cef_player_has_plugin")]
    fn is_player_has_plugin(&mut self, _: &Amx, player_id: i32) -> AmxResult<bool> {
        let has_plugin = self.core.has_plugin(player_id);
        Ok(has_plugin)
    }

    #[native(name = "cef_create_ext_browser")]
    fn create_external_browser(
        &mut self, _: &Amx, player_id: i32, browser_id: i32, texture: AmxString, url: AmxString,
        scale: i32,
    ) -> AmxResult<bool> {
        let _ = (player_id, browser_id, texture, url, scale);
        Ok(false)
    }

    #[native(name = "cef_append_to_object")]
    fn append_to_object(
        &mut self, _: &Amx, player_id: i32, browser_id: i32, object_id: i32,
    ) -> AmxResult<bool> {
        self.core.append_to_object(player_id, browser_id, object_id);
        Ok(true)
    }

    #[native(name = "cef_remove_from_object")]
    fn remove_from_object(
        &mut self, _: &Amx, player_id: i32, browser_id: i32, object_id: i32,
    ) -> AmxResult<bool> {
        self.core
            .remove_from_object(player_id, browser_id, object_id);
        Ok(true)
    }

    #[native(name = "cef_toggle_dev_tools")]
    fn toggle_dev_tools(
        &mut self, _: &Amx, player_id: i32, browser_id: i32, enabled: bool,
    ) -> AmxResult<bool> {
        self.core.toggle_dev_tools(player_id, browser_id, enabled);
        Ok(true)
    }

    #[native(name = "cef_set_audio_settings")]
    fn set_audio_settings(
        &mut self, _: &Amx, player_id: i32, browser_id: u32, max_distance: f32,
        reference_distance: f32,
    ) -> AmxResult<bool> {
        self.core
            .set_audio_settings(player_id, browser_id, max_distance, reference_distance);
        Ok(true)
    }

    #[native(name = "cef_load_url")]
    fn load_url(
        &mut self, _: &Amx, player_id: i32, browser_id: u32, url: AmxString,
    ) -> AmxResult<bool> {
        let _ = (player_id, browser_id, url);
        Ok(false)
    }

    fn notify_connect(&self, player_id: i32, success: bool) {
        trace!("notify_connect({}, {})", player_id, success);

        self.amx_list.iter().for_each(|&ident| {
            samp::amx::get(ident)
                .map(|amx| exec_public!(amx, "OnCefInitialize", player_id, success));
        });
    }

    fn notify_browser_created(&self, player_id: i32, browser_id: u32, browser_generation: u32, code: i32) {
        self.amx_list.iter().for_each(|&ident| {
            samp::amx::get(ident)
                .map(|amx| exec_public!(amx, "OnCefBrowserCreatedScoped", player_id, browser_id, browser_generation, code));
        });
    }
}

impl SampPlugin for CefPlugin {
    fn on_load(&mut self) {
        info!("CEF plugin is successful loaded.");
    }

    fn on_amx_load(&mut self, amx: &Amx) {
        self.amx_list.push(amx.ident());
    }

    fn on_amx_unload(&mut self, amx: &Amx) {
        let ident = amx.ident();
        self.events.retain(|_, (owner, _)| *owner != ident);

        if let Some(position) = self.amx_list.iter().position(|&id| id == ident) {
            self.amx_list.remove(position);
        }
    }

    fn process_tick(&mut self) {
        for _ in 0..32 {
            let Some(event) = self.core.poll_event() else { break; };
            match event {
                CoreEvent::EmitEvent {
                    player_id,
                    browser_id,
                    browser_generation,
                    event,
                    arguments,
                } => {
                    trace!("process_tick::EmitEvent({}) {}", player_id, event);

                    if let Some((ident, cb)) = self.events.get(&event) {
                        samp::amx::get(*ident)
                            .map(|amx| exec_public!(amx, cb, player_id, browser_id, browser_generation, &arguments => string));
                    }
                }

                CoreEvent::PlayerInitialized { player_id, success } => {
                    trace!(
                        "process_tick::PlayerInitialized({}, {})",
                        player_id, success
                    );
                    self.notify_connect(player_id, success);
                }

                CoreEvent::BrowserCreated {
                    player_id,
                    browser_id,
                    browser_generation,
                    code,
                } => {
                    trace!("process_tick::BrowserCreated({})", player_id);
                    self.notify_browser_created(player_id, browser_id, browser_generation, code);
                }
            }
        }
    }
}

initialize_plugin!(
    natives: [
        CefPlugin::on_player_connect,
        CefPlugin::on_player_disconnect,
        CefPlugin::create_browser,
        CefPlugin::destroy_browser,
        CefPlugin::emit_event,
        CefPlugin::emit_browser_event,
        CefPlugin::protocol_version,
        CefPlugin::bind_game_connection,
        CefPlugin::subscribe,
        CefPlugin::block_input,
        CefPlugin::hide_browser,
        CefPlugin::browser_listen_events,
        CefPlugin::is_player_has_plugin,
        CefPlugin::create_external_browser,
        CefPlugin::append_to_object,
        CefPlugin::remove_from_object,
        CefPlugin::toggle_dev_tools,
        CefPlugin::set_audio_settings,
        CefPlugin::load_url,
    ],
    {
        samp::plugin::enable_process_tick();
        samp::encoding::set_default_encoding(samp::encoding::WINDOWS_1252);
        let _ = samp::plugin::logger();
        let (log_level, invalid_log_level) = server_log_level();
        let filter = Targets::new()
            .with_default(LevelFilter::WARN)
            .with_target("cef_server", log_level)
            .with_target("cef_server_core", log_level);
        let format = tracing_subscriber::fmt::layer().compact().with_ansi(false);
        if tracing_subscriber::registry()
            .with(filter)
            .with(format)
            .try_init()
            .is_ok()
        {
            tracing::info!(level = %log_level, "server logging initialized");
            if let Some(value) = invalid_log_level {
                tracing::warn!(
                    value,
                    environment = LOG_LEVEL_ENV,
                    "unknown server log level; using info"
                );
            }
        }

        CefPlugin::new()
    }
);

#[cfg(test)]
mod wire_amx_tests;
