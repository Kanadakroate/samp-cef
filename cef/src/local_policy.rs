//! LR's native URL boundary. This module has no CEF dependency so the exact
//! predicate used by browser and renderer can be tested without launching GTA.
pub fn allows_asset_url(url: &str) -> bool {
    url.starts_with("sampcef://assets/") && !url.bytes().any(|byte| byte.is_ascii_control())
}

pub fn allows_bridge(is_main: bool, url: &str) -> bool {
    is_main && allows_asset_url(url)
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
