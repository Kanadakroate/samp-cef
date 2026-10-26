use samp::cell::{AmxCell, AmxString, Buffer, Ref, string::put_in_buffer};
use quick_protobuf::{serialize_into_vec, deserialize_from_slice};

#[test]
fn protobuf_json_to_actual_sdk_amx_cells_preserves_cp1252_and_json_escapes() {
    samp::encoding::set_default_encoding(samp::encoding::WINDOWS_1252);
    let cases = [
        "{\"text\":\"Gr\u{00fc}\u{00df}e: \u{00e4}\u{00f6}\u{00fc}\u{00c4}\u{00d6}\u{00dc}\u{00df} \u{20ac}\",\"path\":\"C:\\\\Haus\",\"quote\":\"\\\"Fenster\\\"\"}",
        r#"{"text":"Gr\u00fc\u00dfe: \u00e4\u00f6\u00fc\u00c4\u00d6\u00dc\u00df \u20ac","path":"C:\\Haus","quote":"\"Fenster\""}"#,
    ];
    let mut fixtures = Vec::new();
    for json in cases {
        let packet = messages::packets::EmitEvent {
            event_name: "lr:test:encoding".into(), args: Some(json.into()), arguments: Vec::new(),
            browser_id: Some(62404), browser_generation: Some(42),
        };
        let wire = serialize_into_vec(&packet).unwrap();
        let received: messages::packets::EmitEvent = deserialize_from_slice(&wire).unwrap();
        let json_after_wire = received.args.unwrap();
        assert_eq!(json_after_wire, json, "Protobuf must preserve the single string exactly");
        let mut cells = vec![0x7fffffff; 1024];
        {
            // Real pinned samp-sdk AMX encoder; storage is an isolated 32-bit cell array.
            let reference = unsafe { Ref::new(0, cells.as_mut_ptr()) };
            let mut buffer = Buffer::new(reference, cells.len());
            put_in_buffer(&mut buffer, &json_after_wire).unwrap();
        }
        let bytes: Vec<u8> = cells.iter().take_while(|c| **c != 0).map(|c| {
            assert!((0..=255).contains(c)); *c as u8
        }).collect();
        let (decoded, _, errors) = samp::encoding::WINDOWS_1252.decode(&bytes);
        assert!(!errors);
        assert_eq!(decoded, json, "No extra quoting or escape rewriting at AMX boundary");
        let parsed: serde_json::Value = serde_json::from_str(&decoded).unwrap();
        assert_eq!(parsed["text"], "Gr\u{00fc}\u{00df}e: \u{00e4}\u{00f6}\u{00fc}\u{00c4}\u{00d6}\u{00dc}\u{00df} \u{20ac}");
        assert_eq!(parsed["path"], r"C:\Haus");
        assert_eq!(parsed["quote"], "\"Fenster\"");
        // Exercise the actual native-argument decoder over real AMX-layout cells,
        // then the typed outbound protobuf path used by cef_emit_browser_event.
        let mut header: samp::raw::types::AMX_HEADER = unsafe { std::mem::zeroed() };
        extern "C" fn callback(_: *mut samp::raw::types::AMX, _: i32, _: *mut i32, _: *mut i32) -> i32 { unreachable!() }
        extern "C" fn debug(_: *mut samp::raw::types::AMX) -> i32 { unreachable!() }
        let mut raw = samp::raw::types::AMX {
            base: std::ptr::null_mut(), data: std::ptr::null_mut(), callback, debug,
            cip: 0, frm: 0, hea: 0, hlw: 0, stk: 0, stp: 0, flags: 0,
            usertags: [0; 4], userdata: [std::ptr::null_mut(); 4], error: 0,
            paramcount: 0, pri: 0, alt: 0, reset_stk: 0, reset_hea: 0, sysreq_d: 0,
        };
        raw.base = (&mut header as *mut samp::raw::types::AMX_HEADER).cast();
        raw.data = cells.as_mut_ptr().cast();
        raw.hea = (cells.len() * 4) as i32; raw.stk = raw.hea; raw.stp = raw.hea;
        let amx = samp::amx::Amx::new(&mut raw, 0);
        let argument = AmxString::from_raw(&amx, 0).unwrap().to_string();
        assert_eq!(argument, json, "Actual AMX input decoder must use Windows-1252");
        let outbound = messages::packets::EmitEvent {
            event_name: "lr:house:state".into(), args: None,
            arguments: vec![messages::packets::EventValue { string_value: Some(argument.into()), integer_value: None, float_value: None }],
            browser_id: Some(62404), browser_generation: Some(42),
        };
        let wire = serialize_into_vec(&outbound).unwrap();
        let delivered: messages::packets::EmitEvent = deserialize_from_slice(&wire).unwrap();
        let ui_json = delivered.arguments[0].string_value.as_ref().unwrap();
        assert_eq!(ui_json, json, "CP1252 AMX -> UTF-8 protobuf JSON must be lossless");
        let ui_value: serde_json::Value = serde_json::from_str(ui_json).unwrap();
        assert_eq!(ui_value, parsed);
        fixtures.push(serde_json::json!({ "utf8_json": json, "amx_bytes": bytes, "browser_id": 62404, "browser_generation": 42 }));
    }
    assert!(fixtures[0]["amx_bytes"].as_array().unwrap().iter().any(|n| n.as_u64() == Some(252)));
    assert!(fixtures[1]["amx_bytes"].as_array().unwrap().iter().all(|n| n.as_u64().unwrap() < 128));
    if let Some(path) = std::env::var_os("SAMP_CEF_AMX_FIXTURE_OUTPUT") {
        std::fs::write(path, serde_json::to_vec_pretty(&fixtures).unwrap()).unwrap();
    }
}
