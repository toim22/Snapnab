// Shared by the build.rs of Snapnab.exe and SnapnabEditor.exe (include!): fills resources/app.rc.in and links the
// compiled resources (icon, manifest, version information) into the executable.

fn embed_app_resources(name: &str, icon: &str, description: &str) {
    let dir = std::path::PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap());
    let res = dir.join("../resources").canonicalize().expect("rust/resources");
    let slash = |p: std::path::PathBuf| p.display().to_string().trim_start_matches(r"\\?\").replace('\\', "/");
    let template = res.join("app.rc.in");
    for f in [&template, &res.join(icon), &res.join("app.manifest")] {
        println!("cargo:rerun-if-changed={}", f.display());
    }
    let version = std::env::var("CARGO_PKG_VERSION").unwrap();
    let mut parts: Vec<String> = version.split('.').map(|p| p.split('-').next().unwrap_or("0").to_string()).collect();
    parts.resize(4, "0".into());
    let rc = std::fs::read_to_string(&template)
        .unwrap()
        .replace("@ICON@", &slash(res.join(icon)))
        .replace("@MANIFEST@", &slash(res.join("app.manifest")))
        .replace("@VERSION_RC@", &parts.join(","))
        .replace("@VERSION@", &version)
        .replace("@NAME@", name)
        .replace("@DESCRIPTION@", description);
    let out = std::path::PathBuf::from(std::env::var("OUT_DIR").unwrap()).join(format!("{name}.rc"));
    std::fs::write(&out, rc).unwrap();
    embed_resource::compile(&out, embed_resource::NONE).manifest_required().unwrap();
}
