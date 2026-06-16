//! Agent subprocess management: spawn + IPC channels.
//!
//! Uses a thread-local for the channel handles so that AppState methods
//! can send/receive without carrying the channel through every function.

use deepx_proto::{Agent2Ui, Ui2Agent};
use std::cell::RefCell;
use std::sync::mpsc;

// Thread-local channel to the agent child process.
// Tuple: (tx → stdin writer thread, rx ← stdout reader thread, Child handle).
thread_local! {
    pub(crate) static CH: RefCell<Option<(
        mpsc::Sender<Ui2Agent>,
        mpsc::Receiver<Agent2Ui>,
        std::process::Child,
    )>> = const { RefCell::new(None) };
}

/// Spawn the `deepx agent` subprocess, start stdin/stdout bridge threads,
/// return channels for UI-to-agent communication.
pub(crate) fn spawn_agent() -> Result<
    (
        mpsc::Sender<Ui2Agent>,
        mpsc::Receiver<Agent2Ui>,
        std::process::Child,
    ),
    String,
> {
    use std::io::{BufRead, BufReader, Write};
    use std::process::{Command, Stdio};

    let dir = std::env::current_exe()
        .map_err(|e| format!("{e}"))?
        .parent()
        .ok_or("no parent")?
        .to_path_buf();
    let exe = ["deepx.exe", "deepx"]
        .iter()
        .find_map(|n| {
            let p = dir.join(n);
            p.exists().then_some(p)
        })
        .ok_or("deepx not found")?;
    let mut child = Command::new(&exe)
        .arg("agent")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .spawn()
        .map_err(|e| format!("spawn: {e}"))?;
    let stdin = child.stdin.take().ok_or("no stdin")?;
    let stdout = child.stdout.take().ok_or("no stdout")?;
    let (tx, urx) = mpsc::channel::<Ui2Agent>();
    let (atx, arx) = mpsc::channel::<Agent2Ui>();

    // Writer thread: Ui2Agent → stdin (JSON-LP)
    std::thread::spawn(move || {
        let mut s = stdin;
        while let Ok(f) = urx.recv() {
            if let Ok(j) = serde_json::to_string(&f) {
                if writeln!(s, "{j}").is_err() || s.flush().is_err() {
                    break;
                }
            }
        }
    });

    // Reader thread: stdout (JSON-LP) → Agent2Ui
    std::thread::spawn(move || {
        for l in BufReader::new(stdout).lines() {
            if let Ok(l) = l {
                if let Ok(e) = serde_json::from_str::<Agent2Ui>(&l) {
                    if atx.send(e).is_err() {
                        break;
                    }
                }
            }
        }
    });

    Ok((tx, arx, child))
}
