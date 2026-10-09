//! Shared plumbing for backends that drive an external helper process.
//!
//! Replaces QSpectrumAnalyzer's `subprocess.py` and the duplicated
//! `process_start` / `process_stop` / `run` methods on every `PowerThread`.
//!
//! Differences from the Python version, both deliberate:
//!
//! * the child's **stderr is captured** and forwarded as [`SourceEvent::Log`],
//!   so backend complaints show up in the application instead of on a console
//!   the user may never see;
//! * stopping sends a plain kill rather than `CTRL_BREAK_EVENT` on Windows.
//!   The Python build only used the console-control path for `soapy_power` and
//!   `terminate()` for everything else; a kill loses at most the sweep in
//!   flight, and avoids having to allocate a console and a process group.

use std::io::{BufRead, BufReader, Read};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;

use super::{EventSink, SourceError, SourceSession};
use crate::util::split_args;

/// Hide the console window a child would otherwise pop up on Windows.
#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

fn build_command(cmdline: &[String]) -> Result<Command, SourceError> {
    let (program, args) = cmdline
        .split_first()
        .ok_or_else(|| SourceError::InvalidConfig("no executable configured".to_owned()))?;

    let mut cmd = Command::new(program);
    cmd.args(args);
    cmd.stdin(Stdio::null());

    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }

    Ok(cmd)
}

/// Run `<executable> <args>` and return whatever it prints.
///
/// Both streams are merged, because `-h` output lands on either depending on
/// the tool. `COLUMNS=125` reproduces the width the Python version forced so
/// that argparse help does not wrap into an unreadable column.
pub fn capture_help(executable: &str, args: &[&str]) -> String {
    let mut cmdline = split_args(executable);
    if cmdline.is_empty() {
        return "No executable configured.".to_owned();
    }
    cmdline.extend(args.iter().map(|s| (*s).to_owned()));

    let mut cmd = match build_command(&cmdline) {
        Ok(c) => c,
        Err(e) => return e.to_string(),
    };
    cmd.env("COLUMNS", "125");

    match cmd.output() {
        Ok(out) => {
            let mut text = String::from_utf8_lossy(&out.stdout).into_owned();
            let err = String::from_utf8_lossy(&out.stderr);
            if !err.trim().is_empty() {
                if !text.is_empty() && !text.ends_with('\n') {
                    text.push('\n');
                }
                text.push_str(&err);
            }
            if text.trim().is_empty() {
                format!("'{}' produced no output.", cmdline.join(" "))
            } else {
                text
            }
        }
        Err(e) => format!("{} executable not found! ({e})", cmdline[0]),
    }
}

/// Like [`capture_help`] but keeps stdout only, for `--detect` / `--info`
/// style queries whose stderr is noise.
pub fn capture_stdout(executable: &str, args: &[&str]) -> String {
    let mut cmdline = split_args(executable);
    if cmdline.is_empty() {
        return "No executable configured.".to_owned();
    }
    cmdline.extend(args.iter().map(|s| (*s).to_owned()));

    let mut cmd = match build_command(&cmdline) {
        Ok(c) => c,
        Err(e) => return e.to_string(),
    };
    cmd.env("COLUMNS", "125");

    match cmd.output() {
        Ok(out) => String::from_utf8_lossy(&out.stdout).into_owned(),
        Err(e) => format!("{} executable not found! ({e})", cmdline[0]),
    }
}

/// Consumes the child's stdout one text line at a time.
///
/// This is the shape of `BasePowerThread.parse_output()` for the backends whose
/// output is line based.
pub trait LineParser: Send {
    /// Handle one line, newline already stripped.
    fn feed(&mut self, line: &str, sink: &EventSink);

    /// Called once after the last line. The line-oriented backends all emit
    /// their final sweep from `feed`, so the default does nothing.
    fn finish(&mut self, sink: &EventSink) {
        let _ = sink;
    }
}

/// Consumes the child's stdout as a byte stream, for the framed binary
/// protocols (`hackrf_sweep`, `soapy_power`).
pub trait ByteParser: Send {
    /// Pump `stdout` until it ends or `alive` is cleared.
    fn run(&mut self, stdout: &mut dyn BufRead, sink: &EventSink, alive: &AtomicBool);
}

/// A running helper process plus the threads reading it.
pub struct ChildSession {
    child: Arc<Mutex<Option<Child>>>,
    alive: Arc<AtomicBool>,
    readers: Vec<JoinHandle<()>>,
}

impl ChildSession {
    fn kill_child(&self) {
        if let Ok(mut guard) = self.child.lock() {
            if let Some(child) = guard.as_mut() {
                // `kill` on an already-exited process is an error we don't care
                // about; `wait` is what actually reaps it.
                let _ = child.kill();
                let _ = child.wait();
            }
            *guard = None;
        }
    }
}

impl SourceSession for ChildSession {
    fn stop(&mut self) {
        self.alive.store(false, Ordering::SeqCst);
        self.kill_child();
        for handle in self.readers.drain(..) {
            let _ = handle.join();
        }
    }
}

impl Drop for ChildSession {
    fn drop(&mut self) {
        self.stop();
    }
}

/// A freshly spawned child together with the two streams taken from it.
type SpawnedChild = (Child, Box<dyn Read + Send>, Box<dyn Read + Send>);

/// Spawn `cmdline`, logging the command the way the Python version printed
/// "Starting backend:".
fn spawn_inner(cmdline: &[String], sink: &EventSink) -> Result<SpawnedChild, SourceError> {
    let mut cmd = build_command(cmdline)?;
    cmd.stdout(Stdio::piped());
    cmd.stderr(Stdio::piped());

    sink.log(format!("Starting backend: {}", cmdline.join(" ")));

    let mut child = cmd.spawn().map_err(|e| SourceError::ExecutableNotFound {
        executable: cmdline[0].clone(),
        detail: e.to_string(),
    })?;

    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| SourceError::Io("child has no stdout".to_owned()))?;
    let stderr = child
        .stderr
        .take()
        .ok_or_else(|| SourceError::Io("child has no stderr".to_owned()))?;

    Ok((child, Box::new(stdout), Box::new(stderr)))
}

/// Forward the child's stderr to the log, a line at a time.
fn spawn_stderr_reader(stderr: Box<dyn Read + Send>, sink: EventSink) -> JoinHandle<()> {
    std::thread::Builder::new()
        .name("spectroscope-backend-stderr".into())
        .spawn(move || {
            let reader = BufReader::new(stderr);
            for line in reader.lines() {
                match line {
                    Ok(l) if !l.trim().is_empty() => {
                        if !sink.log(l) {
                            break;
                        }
                    }
                    Ok(_) => {}
                    Err(_) => break,
                }
            }
        })
        .expect("spawn stderr reader")
}

/// Common tail of both spawners: emit `Started`, run the reader, emit `Stopped`.
fn finish_spawn(
    child: Child,
    stderr: Box<dyn Read + Send>,
    sink: EventSink,
    hops: usize,
    body: impl FnOnce(&EventSink, &AtomicBool) + Send + 'static,
) -> Result<Box<dyn SourceSession>, SourceError> {
    let alive = Arc::new(AtomicBool::new(true));
    let child = Arc::new(Mutex::new(Some(child)));

    let stderr_handle = spawn_stderr_reader(stderr, sink.clone());

    let main_handle = {
        let alive = Arc::clone(&alive);
        let child = Arc::clone(&child);
        std::thread::Builder::new()
            .name("spectroscope-backend".into())
            .spawn(move || {
                sink.started(hops);
                body(&sink, &alive);

                // The stream ended on its own (single shot, or the child died):
                // reap the child so `stop()` has nothing left to do.
                if let Ok(mut guard) = child.lock() {
                    if let Some(c) = guard.as_mut() {
                        let _ = c.kill();
                        match c.wait() {
                            Ok(status) if !status.success() && alive.load(Ordering::SeqCst) => {
                                sink.log(format!("Backend exited with {status}"));
                            }
                            _ => {}
                        }
                    }
                    *guard = None;
                }

                alive.store(false, Ordering::SeqCst);
                sink.stopped();
            })
            .map_err(|e| SourceError::Io(e.to_string()))?
    };

    Ok(Box::new(ChildSession {
        child,
        alive,
        readers: vec![main_handle, stderr_handle],
    }))
}

/// Spawn a helper process and feed its stdout to `parser` line by line.
pub fn spawn_lines(
    cmdline: Vec<String>,
    sink: EventSink,
    mut parser: Box<dyn LineParser>,
    hops: usize,
) -> Result<Box<dyn SourceSession>, SourceError> {
    let (child, stdout, stderr) = spawn_inner(&cmdline, &sink)?;

    finish_spawn(child, stderr, sink, hops, move |sink, alive| {
        let reader = BufReader::new(stdout);
        for line in reader.lines() {
            if !alive.load(Ordering::SeqCst) {
                break;
            }
            match line {
                // Backends emit ASCII; a stray invalid byte should not kill the
                // run, and `lines()` already replaced it.
                Ok(l) => parser.feed(&l, sink),
                Err(e) => {
                    if alive.load(Ordering::SeqCst) {
                        sink.log(format!("Error reading backend output: {e}"));
                    }
                    break;
                }
            }
        }
        parser.finish(sink);
    })
}

/// Spawn a helper process and hand its stdout to `parser` as raw bytes.
pub fn spawn_bytes(
    cmdline: Vec<String>,
    sink: EventSink,
    mut parser: Box<dyn ByteParser>,
    hops: usize,
) -> Result<Box<dyn SourceSession>, SourceError> {
    let (child, stdout, stderr) = spawn_inner(&cmdline, &sink)?;

    finish_spawn(child, stderr, sink, hops, move |sink, alive| {
        // 1 MiB: a single hackrf_sweep record is a few kB and soapy_power
        // frames can be hundreds of kB, so this keeps syscalls off the hot path.
        let mut reader = BufReader::with_capacity(1 << 20, stdout);
        parser.run(&mut reader, sink, alive);
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sources::{Frame, SourceEvent};
    use std::sync::Arc as StdArc;

    fn sink() -> (EventSink, crossbeam_channel::Receiver<SourceEvent>) {
        let (tx, rx) = crossbeam_channel::unbounded();
        (EventSink::new(tx, None), rx)
    }

    #[test]
    fn missing_executable_reports_not_found() {
        let (s, _rx) = sink();
        struct Nop;
        impl LineParser for Nop {
            fn feed(&mut self, _l: &str, _s: &EventSink) {}
        }
        let result = spawn_lines(
            vec!["definitely-not-a-real-program-xyzzy".to_owned()],
            s,
            Box::new(Nop),
            0,
        );
        assert!(
            matches!(result, Err(SourceError::ExecutableNotFound { .. })),
            "expected ExecutableNotFound, got {:?}",
            result.err()
        );
    }

    #[test]
    fn empty_cmdline_is_rejected() {
        let (s, _rx) = sink();
        struct Nop;
        impl LineParser for Nop {
            fn feed(&mut self, _l: &str, _s: &EventSink) {}
        }
        let result = spawn_lines(Vec::new(), s, Box::new(Nop), 0);
        assert!(
            matches!(result, Err(SourceError::InvalidConfig(_))),
            "expected InvalidConfig, got {:?}",
            result.err()
        );
    }

    #[test]
    fn capture_help_on_missing_executable_says_so() {
        let text = capture_help("definitely-not-a-real-program-xyzzy", &["-h"]);
        assert!(text.contains("not found"), "{text}");
    }

    #[test]
    fn capture_help_with_no_executable() {
        assert!(capture_help("", &["-h"]).contains("No executable"));
        assert!(capture_stdout("   ", &["-h"]).contains("No executable"));
    }

    /// End-to-end: drive a real child process and check the event sequence.
    #[test]
    fn line_parser_sees_child_output_and_run_terminates() {
        // `cargo test` guarantees a working rustc toolchain but not a shell, so
        // use the one interpreter we know is on PATH in every environment this
        // builds in: the test binary itself is not reusable, so fall back to
        // skipping when no suitable helper exists.
        let helper = ["python3", "python"].into_iter().find(|p| {
            Command::new(p)
                .arg("--version")
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status()
                .map(|s| s.success())
                .unwrap_or(false)
        });
        let Some(python) = helper else {
            eprintln!("no python on PATH; skipping subprocess round-trip test");
            return;
        };

        struct Collect {
            seen: StdArc<Mutex<Vec<String>>>,
        }
        impl LineParser for Collect {
            fn feed(&mut self, line: &str, sink: &EventSink) {
                self.seen.lock().unwrap().push(line.to_owned());
                if line == "two" {
                    sink.frame(Frame {
                        timestamp: 1.0,
                        x: std::sync::Arc::new(vec![0.0]),
                        y: vec![-1.0],
                    });
                }
            }
        }

        let seen = StdArc::new(Mutex::new(Vec::new()));
        let (s, rx) = sink();
        let mut session = spawn_lines(
            vec![
                python.to_owned(),
                "-c".to_owned(),
                "print('one'); print('two')".to_owned(),
            ],
            s,
            Box::new(Collect {
                seen: StdArc::clone(&seen),
            }),
            3,
        )
        .expect("spawn");

        // Wait for the run to finish by draining until Stopped.
        let mut started = false;
        let mut frames = 0;
        let mut stopped = false;
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
        while std::time::Instant::now() < deadline {
            match rx.recv_timeout(std::time::Duration::from_millis(500)) {
                Ok(SourceEvent::Started { hops }) => {
                    assert_eq!(hops, 3);
                    started = true;
                }
                Ok(SourceEvent::Frame(_)) => frames += 1,
                Ok(SourceEvent::Stopped) => {
                    stopped = true;
                    break;
                }
                Ok(_) => {}
                Err(_) => {}
            }
        }

        session.stop();
        assert!(started, "never saw Started");
        assert!(stopped, "never saw Stopped");
        assert_eq!(frames, 1);
        assert_eq!(
            *seen.lock().unwrap(),
            vec!["one".to_owned(), "two".to_owned()]
        );
    }

    #[test]
    fn stop_is_idempotent() {
        struct Nop;
        impl LineParser for Nop {
            fn feed(&mut self, _l: &str, _s: &EventSink) {}
        }
        let (s, _rx) = sink();
        if let Ok(mut session) = spawn_lines(
            vec!["cmd".to_owned(), "/c".to_owned(), "echo hi".to_owned()],
            s,
            Box::new(Nop),
            0,
        ) {
            session.stop();
            session.stop();
        }
    }
}
