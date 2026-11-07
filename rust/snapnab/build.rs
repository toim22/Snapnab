//! Embeds the icon, the manifest (per-monitor DPI awareness v2, Common Controls 6, asInvoker) and the version
//! information in Snapnab.exe (resources/app.rc.in).

include!("../resources/build_rc.rs");

fn main() {
    embed_app_resources("Snapnab", "Snapnab.ico", "Snapnab — screen capture and recording");
}
