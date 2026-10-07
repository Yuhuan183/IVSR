//! A long-lived ffmpeg process that encodes PNG frames streamed over stdin.

use std::collections::VecDeque;
use std::fs::File;
use std::io::{BufRead, BufReader, Write};
use std::path::Path;
use std::process::{ChildStdin, Stdio};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;

use ivsr_core::process::{self, KillOnDrop};
use ivsr_core::{Error, FrameEncoder, Result};

pub(crate) struct PipeEncoder {
    child: KillOnDrop,
    stdin: Option<ChildStdin>,
    stderr: Arc<Mutex<VecDeque<String>>>,
    reader: Option<JoinHandle<()>>,
}

impl PipeEncoder {
    pub fn spawn(ffmpeg: &Path, args: &[String]) -> Result<Self> {
        let mut cmd = process::command(ffmpeg);
        cmd.args(args).stdin(Stdio::piped()).stdout(Stdio::null()).stderr(Stdio::piped());
        let mut child = cmd.spawn().map_err(|e| Error::tool("ffmpeg", format!("failed to start encoder: {e}")))?;
        let stdin = child.stdin.take();
        let stderr = Arc::new(Mutex::new(VecDeque::new()));
        let reader = child.stderr.take().map(|pipe| {
            let sink = stderr.clone();
            std::thread::spawn(move || {
                for line in BufReader::new(pipe).lines().map_while(|l| l.ok()) {
                    let mut tail = sink.lock().unwrap();
                    if tail.len() == 20 {
                        tail.pop_front();
                    }
                    tail.push_back(line);
                }
            })
        });
        Ok(Self { child: KillOnDrop::new(child), stdin, stderr, reader })
    }

    fn failure(&mut self, what: &str) -> Error {
        // Let the process exit and the reader drain so the message is complete.
        self.stdin.take();
        let _ = self.child.wait();
        if let Some(reader) = self.reader.take() {
            let _ = reader.join();
        }
        let tail: Vec<String> = self.stderr.lock().unwrap().iter().cloned().collect();
        Error::tool("ffmpeg", format!("{what}:\n{}", tail.join("\n")))
    }
}

impl FrameEncoder for PipeEncoder {
    fn push_frame(&mut self, png: &Path) -> Result<()> {
        let mut file = File::open(png).map_err(|e| Error::io_at("open", png, e))?;
        let stdin = self.stdin.as_mut().ok_or_else(|| Error::tool("ffmpeg", "encoder already closed"))?;
        if std::io::copy(&mut file, stdin).is_err() {
            return Err(self.failure("encoder stopped accepting frames"));
        }
        Ok(())
    }

    fn finish(mut self: Box<Self>) -> Result<()> {
        if let Some(mut stdin) = self.stdin.take() {
            let _ = stdin.flush();
        }
        let status = self.child.wait().map_err(|e| Error::tool("ffmpeg", format!("failed to wait: {e}")))?;
        if let Some(reader) = self.reader.take() {
            let _ = reader.join();
        }
        if status.success() {
            Ok(())
        } else {
            let tail: Vec<String> = self.stderr.lock().unwrap().iter().cloned().collect();
            Err(Error::tool("ffmpeg", format!("encoder exited with {status}:\n{}", tail.join("\n"))))
        }
    }
}
