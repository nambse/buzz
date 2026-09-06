#![deny(unsafe_code)]
//! Internal selected-input reader; no environment or path arguments are read.
fn main() {
    if let Err(code) = ortak_server::workspace_reader::main() {
        eprintln!("ortak-workspace-reader: {code}");
        std::process::exit(1);
    }
}
