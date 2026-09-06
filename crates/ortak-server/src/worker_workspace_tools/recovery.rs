use super::*;

/// One bounded read-only recovery of a previously owned expired reader. Exact
/// executable/hash/UID/token absence is required; expiry or a reused PID alone
/// is not accepted. This never signals or modifies an unrelated process.
pub async fn recover_reader(control: &PgControlPlane, scope: &CompanyScope) -> Result<bool> {
    use ortak_runtime::postgres::workspace_tools::{confirm_reader_absence, unresolved_reader};
    let Some(reader) = unresolved_reader(control, scope).await? else {
        return Ok(false);
    };
    let identity = reader.identity.as_ref().ok_or_else(unavailable)?;
    if identity.uid != rustix::process::getuid().as_raw() {
        return Err(unavailable().into());
    }
    verify_executable(
        std::path::Path::new(&identity.executable),
        &identity.sha256,
        identity.uid,
    )?;
    if !reader_absent(identity, reader.execution_token).await? {
        return Ok(false);
    }
    confirm_reader_absence(control, scope, &reader).await
}

async fn reader_absent(
    identity: &WorkspaceReaderIdentity,
    token: uuid::Uuid,
) -> std::result::Result<bool, RuntimeError> {
    use tokio::io::AsyncReadExt;
    // pgrep selects only the exact owning UID and immutable argv marker; its
    // output is bounded PID digits, never command lines or other process data.
    let escape = |value: &str| {
        value.chars().fold(String::new(), |mut out, ch| {
            if ".^$*+?()[]{}|\\".contains(ch) {
                out.push('\\');
            }
            out.push(ch);
            out
        })
    };
    let pattern = format!(
        "^{} --ortak-workspace-child={token}$",
        escape(&identity.executable)
    );
    let mut child = tokio::process::Command::new("/usr/bin/pgrep")
        .env_clear()
        .current_dir("/")
        .args(["-u", &identity.uid.to_string(), "-f", &pattern])
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .kill_on_drop(true)
        .spawn()
        .map_err(|_| unavailable())?;
    let stdout = child.stdout.take().ok_or_else(unavailable)?;
    let mut bounded = stdout.take(1025);
    let mut bytes = Vec::new();
    let operation = async {
        let (read, status) = tokio::join!(bounded.read_to_end(&mut bytes), child.wait());
        read.map_err(|_| unavailable())?;
        let status = status.map_err(|_| unavailable())?;
        if bytes.len() > 1024 {
            return Err(unavailable());
        }
        match status.code() {
            Some(1) if bytes.is_empty() => Ok(true),
            Some(0)
                if !bytes.is_empty() && bytes.iter().all(|b| b.is_ascii_digit() || *b == b'\n') =>
            {
                Ok(false)
            }
            _ => Err(unavailable()),
        }
    };
    match tokio::time::timeout(Duration::from_secs(2), operation).await {
        Ok(result) => result,
        Err(_) => {
            child.start_kill().map_err(|_| unavailable())?;
            tokio::time::timeout(Duration::from_secs(1), child.wait())
                .await
                .map_err(|_| unavailable())?
                .map_err(|_| unavailable())?;
            Err(unavailable())
        }
    }
}
