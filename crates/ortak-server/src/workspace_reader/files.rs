use ortak_control::workspace::{WorkspaceFile, WorkspaceGrant};
use rustix::fs::{self, FileType, Mode, OFlags};
use sha2::{Digest, Sha256};
use std::{
    fs::File,
    io::{Read, Write},
    os::fd::OwnedFd,
};
use uuid::Uuid;

pub(super) fn check_directory(fd: &OwnedFd, sealed: bool) -> Result<(), &'static str> {
    let s = fs::fstat(fd).map_err(|_| "store_unavailable")?;
    let permissions = s.st_mode & 0o777;
    if FileType::from_raw_mode(s.st_mode) != FileType::Directory
        || s.st_uid != rustix::process::getuid().as_raw()
        || !(permissions == 0o500 || !sealed && permissions == 0o700)
    {
        return Err("store_unavailable");
    }
    Ok(())
}
pub(super) fn directory(parent: &OwnedFd, name: &str) -> Result<OwnedFd, &'static str> {
    let fd = fs::openat(
        parent,
        name,
        OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC,
        Mode::empty(),
    )
    .map_err(|_| "store_unavailable")?;
    check_directory(&fd, false)?;
    Ok(fd)
}
pub(super) fn ensure_directory(parent: &OwnedFd, name: &str) -> Result<OwnedFd, &'static str> {
    match fs::mkdirat(parent, name, Mode::RUSR | Mode::WUSR | Mode::XUSR) {
        Ok(()) => fs::fsync(parent).map_err(|_| "store_unavailable")?,
        Err(rustix::io::Errno::EXIST) => (),
        Err(_) => return Err("store_unavailable"),
    }
    directory(parent, name)
}
fn check_file(fd: &OwnedFd, maximum: usize, writable: bool) -> Result<usize, &'static str> {
    let s = fs::fstat(fd).map_err(|_| "file_unavailable")?;
    if FileType::from_raw_mode(s.st_mode) != FileType::RegularFile
        || s.st_uid != rustix::process::getuid().as_raw()
        || s.st_nlink != 1
        || s.st_size < 0
        || s.st_size as u64 > maximum as u64
        || s.st_mode & 0o777 != if writable { 0o600 } else { 0o400 }
    {
        return Err("file_unavailable");
    }
    Ok(s.st_size as usize)
}
pub(super) fn read_private(
    parent: &OwnedFd,
    name: &str,
    maximum: usize,
) -> Result<Vec<u8>, &'static str> {
    let fd = fs::openat(
        parent,
        name,
        OFlags::RDONLY | OFlags::NONBLOCK | OFlags::NOFOLLOW | OFlags::CLOEXEC,
        Mode::empty(),
    )
    .map_err(|_| "file_unavailable")?;
    let expected = check_file(&fd, maximum, false)?;
    let mut file = File::from(fd);
    let mut bytes = Vec::new();
    (&mut file)
        .take(maximum as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| "file_unavailable")?;
    let after = fs::fstat(&file).map_err(|_| "file_unavailable")?;
    if bytes.len() != expected
        || after.st_size != expected as i64
        || after.st_nlink != 1
        || after.st_mode & 0o777 != 0o400
    {
        return Err("input_changed");
    }
    Ok(bytes)
}
pub(super) fn read_selected(
    parent: &OwnedFd,
    file: &WorkspaceFile,
) -> Result<Vec<u8>, &'static str> {
    let bytes = read_private(parent, &file.file_id.to_string(), 16384)?;
    if bytes.len() != file.bytes as usize
        || hex::encode(Sha256::digest(&bytes)) != file.sha256
        || bytes.contains(&0)
        || std::str::from_utf8(&bytes).is_err()
    {
        return Err("input_changed");
    }
    Ok(bytes)
}
pub(super) fn verify_manifest(
    parent: &OwnedFd,
    grant: &WorkspaceGrant,
) -> Result<(), &'static str> {
    let bytes = read_private(parent, "manifest.json", 16384)?;
    let value = serde_json::to_value(grant).map_err(|_| "invalid_selection")?;
    if bytes != serde_json::to_vec(&value).map_err(|_| "invalid_selection")? {
        return Err("input_changed");
    }
    Ok(())
}
pub(super) fn verify_run(parent: &OwnedFd, grant: &WorkspaceGrant) -> Result<(), &'static str> {
    verify_manifest(parent, grant)?;
    for file in &grant.files {
        read_selected(parent, file)?;
    }
    Ok(())
}
pub(super) fn lock_run(parent: &OwnedFd, run: Uuid) -> Result<OwnedFd, &'static str> {
    let fd = fs::openat(
        parent,
        format!("{run}.lock"),
        OFlags::RDWR | OFlags::CREATE | OFlags::NOFOLLOW | OFlags::CLOEXEC,
        Mode::RUSR | Mode::WUSR,
    )
    .map_err(|_| "store_busy")?;
    check_file(&fd, 0, true)?;
    fs::flock(&fd, fs::FlockOperation::NonBlockingLockExclusive).map_err(|_| "store_busy")?;
    Ok(fd)
}
pub(super) fn write_once(parent: &OwnedFd, name: &str, bytes: &[u8]) -> Result<(), &'static str> {
    match fs::openat(
        parent,
        name,
        OFlags::RDONLY | OFlags::NONBLOCK | OFlags::NOFOLLOW | OFlags::CLOEXEC,
        Mode::empty(),
    ) {
        Ok(fd) => {
            check_file(&fd, bytes.len(), false)?;
            drop(fd);
            if read_private(parent, name, bytes.len())? != bytes {
                return Err("input_changed");
            }
            return Ok(());
        }
        Err(rustix::io::Errno::NOENT) => (),
        Err(_) => return Err("store_unavailable"),
    }
    let temporary = format!("{name}.partial");
    // Under the stable per-run flock, only this exact owned partial file may
    // be discarded after a crash. Other names and final files are untouched.
    match fs::openat(
        parent,
        &temporary,
        OFlags::RDONLY | OFlags::NONBLOCK | OFlags::NOFOLLOW | OFlags::CLOEXEC,
        Mode::empty(),
    ) {
        Ok(fd) => {
            if check_file(&fd, 131072, true).is_err() {
                check_file(&fd, 131072, false)?;
            }
            fs::unlinkat(parent, &temporary, fs::AtFlags::empty())
                .map_err(|_| "store_unavailable")?;
        }
        Err(rustix::io::Errno::NOENT) => (),
        Err(_) => return Err("store_unavailable"),
    }
    let fd = fs::openat(
        parent,
        &temporary,
        OFlags::WRONLY | OFlags::CREATE | OFlags::EXCL | OFlags::NOFOLLOW | OFlags::CLOEXEC,
        Mode::RUSR | Mode::WUSR,
    )
    .map_err(|_| "store_unavailable")?;
    check_file(&fd, 0, true)?;
    let mut file = File::from(fd);
    file.write_all(bytes).map_err(|_| "store_unavailable")?;
    fs::fsync(&file).map_err(|_| "store_unavailable")?;
    fs::fchmod(&file, Mode::RUSR).map_err(|_| "store_unavailable")?;
    fs::fsync(&file).map_err(|_| "store_unavailable")?;
    fs::renameat_with(parent, &temporary, parent, name, fs::RenameFlags::NOREPLACE)
        .map_err(|_| "store_unavailable")?;
    fs::fsync(parent).map_err(|_| "store_unavailable")?;
    Ok(())
}
