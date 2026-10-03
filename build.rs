//! The website is embedded from `website/dist`, which `just build-website` fills. The folder
//! has to exist for the server to compile at all, so a checkout that has not built the
//! website yet (every `cargo run` during development) gets an empty one.

fn main() {
    std::fs::create_dir_all("website/dist").expect("could not create website/dist");
    // a rebuilt website is embedded into the next build of the server
    println!("cargo:rerun-if-changed=website/dist");
}
