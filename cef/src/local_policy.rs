//! LR's native URL boundary. This module has no CEF dependency so the exact
//! predicate used by browser and renderer can be tested without launching GTA.
pub fn allows_asset_url(url: &str) -> bool {
    url.starts_with("sampcef://assets/") && !url.bytes().any(|byte| byte.is_ascii_control())
}

pub fn allows_bridge(is_main: bool, url: &str) -> bool {
    is_main && allows_asset_url(url)
}

/// The finite notification player is private to the shipped toast main page.
pub fn allows_notification_sound(url: &str) -> bool {
    notification_bundle(url).is_some()
}

pub fn notification_bundle(url: &str) -> Option<&str> {
    if !allows_asset_url(url) { return None; }
    let path = url.split(['?', '#']).next()?;
    let hash = path.strip_prefix("sampcef://assets/liberty-revival/")?
        .strip_suffix("/lr_progression_toast/index.html")?;
    (hash.len() == 64 && hash.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))).then_some(hash)
}

#[cfg(test)]
mod notification_tests {
    use super::*;
    #[test]
    fn notification_access_requires_exact_versioned_local_toast() {
        let hash = "a".repeat(64);
        let allowed = format!("sampcef://assets/liberty-revival/{hash}/lr_progression_toast/index.html");
        assert_eq!(notification_bundle(&allowed),Some(hash.as_str()));
        assert!(allows_notification_sound(&(allowed.clone()+"?version=1")));
        for denied in [allowed.replace("assets/", "assets.evil/"), allowed.replace("lr_progression_toast", "lr_house_ui"),
            allowed.replace(&hash,"../private"), allowed.replace("index.html","other.html"),
            allowed.replace("sampcef:","https:"), allowed.replace(&hash,&"A".repeat(64))] {
            assert!(!allows_notification_sound(&denied));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn bridge_only_for_local_main_frame() {
        assert!(allows_bridge(true, "sampcef://assets/lr/index.html?x=1#x"));
        assert!(!allows_bridge(false, "sampcef://assets/lr/index.html"));
        for url in ["https://assets/", "http://localhost/", "file:///C:/assets/index.html",
            "javascript:alert(1)", "data:text/html,test", "about:blank",
            "sampcef://assets.evil/index.html", "sampcef://assets@evil/index.html",
            "sampcef://evil@assets/index.html", "sampcef://assets:80/index.html",
            "sampcef://assets/index.html\n"] {
            assert!(!allows_bridge(true, url), "accepted {url}");
        }
    }
}
