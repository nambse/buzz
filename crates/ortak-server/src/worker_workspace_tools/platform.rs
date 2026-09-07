//! Platform ownership is checked before selecting any local reader process.
use super::{invalid, RuntimeError};
#[cfg(unix)]
use sha2::{Digest, Sha256};
#[cfg(unix)]
use std::io::Read;

#[cfg(unix)]
pub(super) fn current_uid() -> std::result::Result<u32, RuntimeError> {
    Ok(rustix::process::getuid().as_raw())
}
#[cfg(not(unix))]
pub(super) fn current_uid() -> std::result::Result<u32, RuntimeError> {
    Err(invalid())
}
#[cfg(unix)]
pub(super) fn verify_executable(
    binary: &std::path::Path,
    expected_hash: &str,
    uid: u32,
) -> std::result::Result<(), RuntimeError> {
    let fd = rustix::fs::open(
        binary,
        rustix::fs::OFlags::RDONLY | rustix::fs::OFlags::NOFOLLOW | rustix::fs::OFlags::CLOEXEC,
        rustix::fs::Mode::empty(),
    )
    .map_err(|_| invalid())?;
    let stat = rustix::fs::fstat(&fd).map_err(|_| invalid())?;
    if rustix::fs::FileType::from_raw_mode(stat.st_mode) != rustix::fs::FileType::RegularFile
        || stat.st_nlink != 1
        || stat.st_uid != uid
        || stat.st_mode & 0o022 != 0
        || stat.st_mode & 0o100 == 0
        || stat.st_size < 1
        || stat.st_size > 268435456
    {
        return Err(invalid());
    }
    let mut file = std::fs::File::from(fd);
    let mut digest = Sha256::new();
    let mut bytes = [0u8; 65536];
    let mut count = 0usize;
    loop {
        let read = file.read(&mut bytes).map_err(|_| invalid())?;
        if read == 0 {
            break;
        }
        count += read;
        if count > 268435456 {
            return Err(invalid());
        }
        digest.update(&bytes[..read]);
    }
    if count as i64 != stat.st_size || hex::encode(digest.finalize()) != expected_hash {
        return Err(invalid());
    }
    Ok(())
}

#[cfg(not(unix))]
pub(super) fn verify_executable(
    _binary: &std::path::Path,
    _expected_hash: &str,
    _uid: u32,
) -> std::result::Result<(), RuntimeError> {
    Err(invalid())
}
