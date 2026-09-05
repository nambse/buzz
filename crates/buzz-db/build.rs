// On stable Rust, SQLx's migration macro tracks existing SQL files but not new
// directory entries. Rebuild its embedded migrator when a migration is added.
fn main() {
    println!("cargo:rerun-if-changed=../../migrations");
}
