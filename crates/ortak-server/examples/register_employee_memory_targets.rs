#![deny(unsafe_code)]
//! The private namespace operator requires Unix owner/mode and descriptor checks.
#[cfg(unix)]
#[path = "register_employee_memory_targets/unix.rs"]
mod unix;

#[cfg(unix)]
fn main() {
    unix::main();
}

#[cfg(not(unix))]
fn main() {
    eprintln!("employee-target-operator: unsupported platform");
    std::process::exit(1);
}
