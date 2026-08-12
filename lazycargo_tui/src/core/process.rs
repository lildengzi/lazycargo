use std::ffi::OsStr;
use std::fmt::Debug;
use std::io::{self, BufRead};
use std::process::{Command, Output, Stdio};
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};

use thiserror::Error;

#[derive(Debug, Error)]
pub enum ProcessError {
    #[error("failed to run command: {source}")]
    Io { source: std::io::Error },
    #[error("already running: {command}")]
    AlreadyRunning { command: String },
}

pub enum OutputLine {
    Stdout(String),
    Stderr(String),
}

pub fn spawn_streaming<A>(
    program: &OsStr,
    args: &[A],
    envs: &[(&str, &str)],
) -> io::Result<(std::process::Child, mpsc::Receiver<OutputLine>)>
where
    A: AsRef<OsStr> + Debug,
{
    let mut cmd = Command::new(program);
    cmd.args(args).stdout(Stdio::piped()).stderr(Stdio::piped());
    for (key, value) in envs {
        cmd.env(key, value);
    }

    let mut child = cmd.spawn()?;
    let stdout = child.stdout.take().expect("stdout captured");
    let stderr = child.stderr.take().expect("stderr captured");
    let (tx, rx) = mpsc::channel();
    let stderr_tx = tx.clone();

    thread::spawn(move || {
        let reader = io::BufReader::new(stdout);
        for line in reader.lines() {
            let Ok(text) = line else {
                break;
            };
            if tx.send(OutputLine::Stdout(text)).is_err() {
                break;
            }
        }
    });

    thread::spawn(move || {
        let reader = io::BufReader::new(stderr);
        for line in reader.lines() {
            let Ok(text) = line else {
                break;
            };
            if stderr_tx.send(OutputLine::Stderr(text)).is_err() {
                break;
            }
        }
    });

    Ok((child, rx))
}

pub fn run_captured(
    program: &str,
    args: &[&str],
    timeout: Duration,
) -> Result<Option<Output>, ProcessError> {
    let mut child = Command::new(program)
        .args(args)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|source| ProcessError::Io { source })?;
    let started = Instant::now();

    loop {
        match child.try_wait().map_err(|source| ProcessError::Io { source })? {
            Some(_) => {
                return child
                    .wait_with_output()
                    .map(Some)
                    .map_err(|source| ProcessError::Io { source });
            }
            None => {}
        }

        if started.elapsed() >= timeout {
            let _ = child.kill();
            let _ = child.wait();
            return Ok(None);
        }

        thread::sleep(Duration::from_millis(100));
    }
}

pub fn split_output(bytes: &[u8]) -> Vec<String> {
    String::from_utf8_lossy(bytes)
        .lines()
        .map(str::to_owned)
        .collect()
}

pub fn extract_diagnostics(lines: &[String]) -> Vec<String> {
    lines
        .iter()
        .filter(|line| {
            let lower = line.to_lowercase();
            lower.contains("error")
                || lower.contains("warning")
                || lower.contains("failed")
                || lower.contains("unused")
        })
        .cloned()
        .collect()
}
