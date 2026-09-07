//! Prompt local process shutdown without changing durable remote-run ownership.

use std::{future::Future, io, time::Duration};

/// Completion of work raced against a process shutdown request.
#[derive(Debug, PartialEq, Eq)]
pub enum Outcome<T> {
    /// Work completed before shutdown was observed.
    Completed(T),
    /// Shutdown won; the in-flight work future has been dropped.
    Interrupted,
}

/// Completion after new requests have been stopped and a drain deadline applied.
#[derive(Debug, PartialEq, Eq)]
pub enum DrainOutcome<T> {
    /// The operation completed, either normally or during the drain window.
    Completed(T),
    /// The drain deadline elapsed; completion must not be claimed.
    TimedOut,
}

/// Eagerly installed, sticky process signal receivers; owns no background task.
pub struct Shutdown {
    requested: bool,
    #[cfg(unix)]
    interrupt: tokio::signal::unix::Signal,
    #[cfg(unix)]
    terminate: tokio::signal::unix::Signal,
    #[cfg(windows)]
    interrupt: tokio::signal::windows::CtrlC,
}

impl Shutdown {
    /// Installs handlers before configuration, connection or worker operations.
    ///
    /// Unix observes both SIGINT and SIGTERM. Windows observes Ctrl-C. Handler
    /// installation failure propagates rather than running without shutdown.
    pub fn install() -> io::Result<Self> {
        #[cfg(unix)]
        {
            use tokio::signal::unix::{signal, SignalKind};
            Ok(Self {
                requested: false,
                interrupt: signal(SignalKind::interrupt())?,
                terminate: signal(SignalKind::terminate())?,
            })
        }
        #[cfg(windows)]
        {
            Ok(Self {
                requested: false,
                interrupt: tokio::signal::windows::ctrl_c()?,
            })
        }
        #[cfg(not(any(unix, windows)))]
        {
            Err(io::Error::new(
                io::ErrorKind::Unsupported,
                "process signals unavailable",
            ))
        }
    }

    /// Waits for shutdown once; subsequent calls complete immediately.
    pub async fn wait(&mut self) -> io::Result<()> {
        if self.requested {
            return Ok(());
        }
        #[cfg(unix)]
        let received = tokio::select! {
            biased;
            value = self.terminate.recv() => value,
            value = self.interrupt.recv() => value,
        };
        #[cfg(windows)]
        let received = self.interrupt.recv().await;
        #[cfg(not(any(unix, windows)))]
        let received: Option<()> = None;
        self.requested = true;
        received.ok_or_else(|| io::Error::other("process signal stream closed"))
    }

    /// Polls shutdown before work, including when both become ready together.
    ///
    /// Interruption drops work immediately. Callers must use durable leases,
    /// idempotency keys and journals for external effects; interruption neither
    /// rolls those effects back nor acknowledges that remote execution stopped.
    pub async fn until<F: Future>(&mut self, work: F) -> io::Result<Outcome<F::Output>> {
        race(self.wait(), work).await
    }

    /// On shutdown, stops admission first and bounds completion of existing work.
    ///
    /// `stop_accepting` must synchronously notify the listener's graceful-stop
    /// future. No task is spawned; on timeout the remaining future is dropped.
    pub async fn drain<F: Future>(
        &mut self,
        work: F,
        stop_accepting: impl FnOnce(),
        grace: Duration,
    ) -> io::Result<DrainOutcome<F::Output>> {
        drain_on_signal(self.wait(), work, stop_accepting, grace).await
    }
}

async fn race<S: Future<Output = io::Result<()>>, F: Future>(
    signal: S,
    work: F,
) -> io::Result<Outcome<F::Output>> {
    tokio::select! {
        biased;
        result = signal => {
            result?;
            Ok(Outcome::Interrupted)
        },
        result = work => Ok(Outcome::Completed(result)),
    }
}

async fn drain_on_signal<S: Future<Output = io::Result<()>>, F: Future>(
    signal: S,
    work: F,
    stop_accepting: impl FnOnce(),
    grace: Duration,
) -> io::Result<DrainOutcome<F::Output>> {
    tokio::pin!(work);
    match race(signal, work.as_mut()).await? {
        Outcome::Completed(result) => Ok(DrainOutcome::Completed(result)),
        Outcome::Interrupted => {
            stop_accepting();
            match tokio::time::timeout(grace, work).await {
                Ok(result) => Ok(DrainOutcome::Completed(result)),
                Err(_) => Ok(DrainOutcome::TimedOut),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    };

    struct Dropped(Arc<AtomicBool>);
    impl Drop for Dropped {
        fn drop(&mut self) {
            self.0.store(true, Ordering::SeqCst);
        }
    }

    #[tokio::test]
    async fn ready_shutdown_precedes_ready_work() {
        for _ in 0..64 {
            let work = async { panic!("shutdown must be polled before ready admission") };
            assert_eq!(
                race(async { Ok(()) }, work).await.unwrap(),
                Outcome::Interrupted
            );
        }
    }

    #[tokio::test]
    async fn shutdown_drops_an_in_flight_operation_without_waiting_for_completion() {
        let dropped = Arc::new(AtomicBool::new(false));
        let (tx, rx) = tokio::sync::oneshot::channel();
        let work = async {
            let _guard = Dropped(dropped.clone());
            tx.send(()).unwrap();
            std::future::pending::<()>().await;
        };
        let signal = async {
            rx.await.unwrap();
            Ok(())
        };
        let result = tokio::time::timeout(Duration::from_secs(1), race(signal, work))
            .await
            .unwrap();
        assert_eq!(result.unwrap(), Outcome::Interrupted);
        assert!(dropped.load(Ordering::SeqCst));
    }

    #[tokio::test]
    async fn graceful_stop_notifies_acceptor_and_allows_completion() {
        let (tx, rx) = tokio::sync::oneshot::channel();
        let result = drain_on_signal(
            async { Ok(()) },
            async {
                rx.await.unwrap();
                7
            },
            || tx.send(()).unwrap(),
            Duration::from_secs(1),
        )
        .await
        .unwrap();
        assert_eq!(result, DrainOutcome::Completed(7));
    }

    #[tokio::test]
    async fn stalled_drain_is_bounded_and_drops_remaining_work() {
        let dropped = Arc::new(AtomicBool::new(false));
        let stopped = Arc::new(AtomicBool::new(false));
        let work = async {
            let _guard = Dropped(dropped.clone());
            std::future::pending::<()>().await;
        };
        let result = drain_on_signal(
            async { Ok(()) },
            work,
            || stopped.store(true, Ordering::SeqCst),
            Duration::from_millis(20),
        )
        .await
        .unwrap();
        assert_eq!(result, DrainOutcome::TimedOut);
        assert!(stopped.load(Ordering::SeqCst));
        assert!(dropped.load(Ordering::SeqCst));
    }

    #[tokio::test]
    async fn signal_error_propagates_without_polling_new_work() {
        let signal = async { Err(io::Error::other("fixture signal failure")) };
        let work = async { panic!("failed shutdown handling must not admit work") };
        assert!(race(signal, work).await.is_err());
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn installed_sigint_and_sigterm_interrupt_only_owned_child_processes() {
        use tokio::{io::AsyncReadExt, process::Command};
        for signal in ["-INT", "-TERM"] {
            let mut child = Command::new(std::env::current_exe().unwrap())
                .args([
                    "--exact",
                    "shutdown::tests::signal_child",
                    "--ignored",
                    "--nocapture",
                ])
                .env_clear()
                .env("ORTAK_SHUTDOWN_TEST_CHILD", "1")
                .stdin(std::process::Stdio::null())
                .stdout(std::process::Stdio::piped())
                .stderr(std::process::Stdio::null())
                .kill_on_drop(true)
                .spawn()
                .unwrap();
            let pid = child.id().unwrap();
            let mut stdout = child.stdout.take().unwrap();
            let ready = tokio::time::timeout(Duration::from_secs(5), async {
                let mut received = Vec::new();
                let mut chunk = [0; 256];
                loop {
                    let size = stdout.read(&mut chunk).await.unwrap();
                    assert!(size > 0, "child exited before signal registration");
                    received.extend_from_slice(&chunk[..size]);
                    assert!(received.len() <= 4096, "child output exceeded bound");
                    if received
                        .windows(b"SHUTDOWN_REGISTERED".len())
                        .any(|text| text == b"SHUTDOWN_REGISTERED")
                    {
                        break;
                    }
                }
            })
            .await;
            assert!(ready.is_ok(), "child signal registration timed out");
            let delivered = tokio::time::timeout(
                Duration::from_secs(5),
                Command::new("/bin/kill")
                    .env_clear()
                    .args([signal, &pid.to_string()])
                    .stdin(std::process::Stdio::null())
                    .stdout(std::process::Stdio::null())
                    .stderr(std::process::Stdio::null())
                    .kill_on_drop(true)
                    .status(),
            )
            .await
            .unwrap()
            .unwrap();
            assert!(delivered.success());
            let status = tokio::time::timeout(Duration::from_secs(5), child.wait())
                .await
                .unwrap()
                .unwrap();
            assert!(
                status.success(),
                "child must observe signal and exit normally"
            );
        }
    }

    #[cfg(unix)]
    #[tokio::test]
    #[ignore = "owned subprocess fixture, invoked only by the parent signal test"]
    async fn signal_child() {
        use std::io::Write;
        assert_eq!(
            std::env::var("ORTAK_SHUTDOWN_TEST_CHILD").as_deref(),
            Ok("1")
        );
        let mut shutdown = Shutdown::install().unwrap();
        println!("SHUTDOWN_REGISTERED");
        std::io::stdout().flush().unwrap();
        assert_eq!(
            shutdown.until(std::future::pending::<()>()).await.unwrap(),
            Outcome::Interrupted
        );
        // The request remains latched; no subsequent work is polled.
        assert_eq!(
            shutdown
                .until(async { panic!("latched shutdown admitted work") })
                .await
                .unwrap(),
            Outcome::Interrupted
        );
    }
}
