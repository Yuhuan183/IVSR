//! Running external tools with cancellation and streamed output.

use std::collections::VecDeque;
use std::ffi::OsStr;
use std::io::{BufReader, Read};
use std::process::{Child, Command, ExitStatus, Stdio};
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};

use crate::{CancelToken, Error, Result};

const TICK: Duration = Duration::from_millis(100);
const TAIL_LINES: usize = 12;

/// Creates a command that never opens a console window on Windows.
pub fn command(program: impl AsRef<OsStr>) -> Command {
    #[allow(unused_mut)]
    let mut cmd = Command::new(program);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    cmd
}

#[derive(Debug)]
pub struct Finished {
    pub status: ExitStatus,
    /// Last lines of output, for error messages.
    pub tail: String,
}

impl Finished {
    /// Converts a non-zero exit into `Error::Tool` with the output tail.
    pub fn into_result(self, tool: &str) -> Result<Self> {
        if self.status.success() {
            Ok(self)
        } else {
            let code = self.status.code().map_or_else(|| "signal".to_string(), |c| c.to_string());
            let detail = if self.tail.is_empty() { String::new() } else { format!(":\n{}", self.tail) };
            Err(Error::tool(tool, format!("exited with status {code}{detail}")))
        }
    }
}

/// Spawns `cmd` and pumps its stdout/stderr line by line (`\n` or `\r`
/// delimited) into `on_line` until it exits. `on_tick` runs roughly every
/// 100 ms. Cancellation kills the process and returns `Error::Cancelled`.
pub fn run(
    cmd: &mut Command,
    tool: &str,
    cancel: &CancelToken,
    mut on_line: impl FnMut(&str),
    mut on_tick: impl FnMut(),
) -> Result<Finished> {
    cancel.check()?;
    cmd.stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped());
    let mut child = cmd.spawn().map_err(|e| Error::tool(tool, format!("failed to start: {e}")))?;
    let (tx, rx) = mpsc::channel::<String>();
    let readers = [
        child.stdout.take().map(|s| spawn_reader(s, tx.clone())),
        child.stderr.take().map(|s| spawn_reader(s, tx.clone())),
    ];
    drop(tx);

    let mut guard = KillOnDrop(Some(child));
    let mut tail: VecDeque<String> = VecDeque::with_capacity(TAIL_LINES);
    let mut push = |line: String, on_line: &mut dyn FnMut(&str)| {
        on_line(&line);
        if tail.len() == TAIL_LINES {
            tail.pop_front();
        }
        tail.push_back(line);
    };
    // Cancellation and ticks run on a timer, not only when output pauses:
    // engines can print progress continuously for minutes.
    let mut last_tick = Instant::now();
    loop {
        match rx.recv_timeout(TICK) {
            Ok(line) => push(line, &mut on_line),
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            // Both pipes closed: the process has exited or is about to.
            Err(mpsc::RecvTimeoutError::Disconnected) => break,
        }
        if last_tick.elapsed() >= TICK {
            if cancel.is_cancelled() {
                guard.kill();
                return Err(Error::Cancelled);
            }
            on_tick();
            last_tick = Instant::now();
        }
    }
    for reader in readers.into_iter().flatten() {
        let _ = reader.join();
    }
    let status = guard.wait().map_err(|e| Error::tool(tool, format!("failed to wait: {e}")))?;
    if cancel.is_cancelled() {
        return Err(Error::Cancelled);
    }
    Ok(Finished { status, tail: tail.into_iter().collect::<Vec<_>>().join("\n") })
}

/// Runs a short-lived command and returns its stdout. Used for probing.
pub fn output(cmd: &mut Command, tool: &str) -> Result<String> {
    let out = cmd
        .stdin(Stdio::null())
        .output()
        .map_err(|e| Error::tool(tool, format!("failed to start: {e}")))?;
    if !out.status.success() {
        let stderr = String::from_utf8_lossy(&out.stderr);
        return Err(Error::tool(tool, stderr.trim().to_string()));
    }
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}

fn spawn_reader(stream: impl Read + Send + 'static, tx: mpsc::Sender<String>) -> thread::JoinHandle<()> {
    thread::spawn(move || {
        let mut reader = BufReader::new(stream);
        let mut buf = Vec::with_capacity(256);
        loop {
            buf.clear();
            // Split on both '\n' and '\r' so carriage-return progress lines arrive live.
            let mut byte = [0u8; 1];
            let mut eof = false;
            loop {
                match reader.read(&mut byte) {
                    Ok(0) => {
                        eof = true;
                        break;
                    }
                    Ok(_) if byte[0] == b'\n' || byte[0] == b'\r' => break,
                    Ok(_) => buf.push(byte[0]),
                    Err(_) => {
                        eof = true;
                        break;
                    }
                }
            }
            let line = String::from_utf8_lossy(&buf).trim().to_string();
            if !line.is_empty() && tx.send(line).is_err() {
                return;
            }
            if eof {
                return;
            }
        }
    })
}

/// Owns a child process and kills it unless it was waited on.
pub struct KillOnDrop(Option<Child>);

impl KillOnDrop {
    pub fn new(child: Child) -> Self {
        Self(Some(child))
    }

    pub fn kill(&mut self) {
        if let Some(mut child) = self.0.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }

    pub fn wait(&mut self) -> std::io::Result<ExitStatus> {
        match self.0.take() {
            Some(mut child) => child.wait(),
            None => Err(std::io::Error::other("process already reaped")),
        }
    }
}

impl Drop for KillOnDrop {
    fn drop(&mut self) {
        self.kill();
    }
}

#[cfg(all(test, unix))]
mod tests {
    use std::time::{Duration, Instant};

    use super::*;

    #[test]
    fn cancellation_is_observed_while_the_child_streams_output() {
        // Prints a progress line every 20 ms and never exits on its own.
        let mut cmd = command("sh");
        cmd.args(["-c", "while true; do echo 12.50%; sleep 0.02; done"]);
        let cancel = CancelToken::new();
        let trigger = cancel.clone();
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(200));
            trigger.cancel();
        });
        let started = Instant::now();
        let mut ticks = 0;
        let result = run(&mut cmd, "sh", &cancel, |_| {}, || ticks += 1);
        assert!(matches!(result, Err(Error::Cancelled)), "{result:?}");
        assert!(started.elapsed() < Duration::from_secs(2), "took {:?}", started.elapsed());
        assert!(ticks > 0, "ticks must run even when output never pauses");
    }
}
