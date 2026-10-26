//! Exact wire shape for native renderer events; no JS-supplied identity fields.
pub enum Argument { Text(String), Integer(i32), Unsupported }
pub fn serialize(args: &[Argument]) -> Option<String> {
    let text = match args {
        [] => String::new(),
        [Argument::Text(text)] => text.clone(),
        _ if args.len() <= 8 && args.iter().all(|a| matches!(a, Argument::Integer(_))) => {
            args.iter().map(|a| match a { Argument::Integer(n) => n.to_string(), _ => unreachable!() }).collect::<Vec<_>>().join(" ")
        }
        _ => return None,
    };
    (text.len() <= 4096 && !text.contains('\0')).then_some(text)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test] fn zero_argument_ready_and_abort_preserve_empty_payload() {
        for _event in ["lr:test:ready", "lr:test:abort"] { assert_eq!(serialize(&[]), Some(String::new())); }
    }
    #[test] fn one_json_string_is_exact_and_legacy_integers_are_unambiguous() {
        let json = r#"{"x":"quoted \\"text\\" and \\u00fc"}"#;
        assert_eq!(serialize(&[Argument::Text(json.into())]), Some(json.into()));
        assert_eq!(serialize(&[Argument::Integer(-1), Argument::Integer(2147483647)]).as_deref(), Some("-1 2147483647"));
        assert!(serialize(&[Argument::Text("a".into()), Argument::Text("b".into())]).is_none());
        assert!(serialize(&[Argument::Unsupported]).is_none());
        assert!(serialize(&[Argument::Text("x".repeat(4097))]).is_none());
        assert!(serialize(&[Argument::Text("a\0b".into())]).is_none());
        assert!(serialize(&(0..9).map(Argument::Integer).collect::<Vec<_>>()).is_none());
    }
}
