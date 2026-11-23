use std::io::Read;
use std::process::{Command, ExitStatus, Stdio};
use std::thread;
use std::time::{Duration, Instant};

use xmedia_core::domain::error::{DomainError, DomainResult};

pub struct BoundedProcessOutput {
    pub status: ExitStatus,
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
}

fn read_bounded<R: Read>(mut reader: R, max_bytes: usize) -> Vec<u8> {
    let mut buf = Vec::new();
    let mut chunk = [0u8; 8192];
    loop {
        match reader.read(&mut chunk) {
            Ok(0) => break,
            Ok(n) => {
                if buf.len() < max_bytes {
                    let take = (max_bytes - buf.len()).min(n);
                    buf.extend_from_slice(&chunk[..take]);
                }
            }
            Err(_) => break,
        }
    }
    buf
}

/// Runs an external command with `Stdio::null()` stdin, capped stdout/stderr buffers,
/// and a hard wall-clock timeout (killing and reaping the child process if exceeded).
pub fn run_command_bounded(
    program: &str,
    args: &[&str],
    timeout: Duration,
    max_output_bytes: usize,
) -> DomainResult<BoundedProcessOutput> {
    let mut child = Command::new(program)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| DomainError::Storage(format!("spawn {program} failed: {e}")))?;

    let stdout_handle = child
        .stdout
        .take()
        .map(|s| thread::spawn(move || read_bounded(s, max_output_bytes)));
    let stderr_handle = child
        .stderr
        .take()
        .map(|s| thread::spawn(move || read_bounded(s, max_output_bytes)));

    let deadline = Instant::now() + timeout;
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) => {
                if Instant::now() >= deadline {
                    let _ = child.kill();
                    let _ = child.wait();
                    if let Some(h) = stdout_handle {
                        let _ = h.join();
                    }
                    if let Some(h) = stderr_handle {
                        let _ = h.join();
                    }
                    return Err(DomainError::Storage(format!(
                        "{program} timed out after {}s",
                        timeout.as_secs()
                    )));
                }
                thread::sleep(Duration::from_millis(20));
            }
            Err(e) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(DomainError::Storage(format!(
                    "waiting for {program} failed: {e}"
                )));
            }
        }
    };

    let stdout = stdout_handle
        .and_then(|h| h.join().ok())
        .unwrap_or_default();
    let stderr = stderr_handle
        .and_then(|h| h.join().ok())
        .unwrap_or_default();

    Ok(BoundedProcessOutput {
        status,
        stdout,
        stderr,
    })
}
