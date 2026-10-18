//! The web UI (web/dist) is embedded into the binary. A placeholder keeps `cargo build` working
//! before the UI has been built; release builds always build the UI first (see the Makefile).
fn main() {
    let dist = std::path::Path::new("web/dist");
    if !dist.join("index.html").exists() {
        std::fs::create_dir_all(dist).expect("creating web/dist");
        std::fs::write(
            dist.join("index.html"),
            "<!doctype html><title>Parrot</title><p>The web app isn't built yet. Run <code>make web</code>.</p>",
        )
        .expect("writing placeholder");
    }
    println!("cargo:rerun-if-changed=web/dist");
}
