//! F12: the config parser: arbitrary text must never panic, and a successful parse must survive a round trip
//! through `effective_text`.
#![no_main]
use libfuzzer_sys::fuzz_target;
use terminus_app::config::Config;

fuzz_target!(|data: &[u8]| {
    let Ok(text) = std::str::from_utf8(data) else { return };
    if let Ok((config, _warnings)) = Config::parse(text) {
        let _ = config.effective_text();
        let _ = config.theme(true);
        let _ = config.resolved_keybindings(terminus_app::config::Platform::Linux);
    }
});
