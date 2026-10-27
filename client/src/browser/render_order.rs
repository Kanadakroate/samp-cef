/// Native browser IDs are stable; HashMap insertion order must not choose
/// overlay stacking. Keyboard focus preserves upstream's final draw position.
pub fn key(id: u32, focused: Option<u32>) -> (bool, bool, u32) {
    // The passive LR objective card is below every interactive overlay.
    (focused == Some(id), id != 62405, id)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn browser_ids_order_stably_and_focus_stays_last() {
        let mut ids = vec![2400, 1200, 900, 1300];
        ids.sort_unstable_by_key(|id| key(*id, None));
        assert_eq!(ids, vec![900, 1200, 1300, 2400]);
        ids.sort_unstable_by_key(|id| key(*id, Some(1200)));
        assert_eq!(ids, vec![900, 1300, 2400, 1200]);
    }
    #[test]
    fn objective_is_below_menus_even_without_keyboard_focus() {
        let mut ids = vec![62406,62405,62404,62403,62402];
        ids.sort_unstable_by_key(|id| key(*id, None));
        assert_eq!(ids, vec![62405,62402,62403,62404,62406]);
        ids.sort_unstable_by_key(|id| key(*id, Some(62402)));
        assert_eq!(ids.last(), Some(&62402));
    }
}
