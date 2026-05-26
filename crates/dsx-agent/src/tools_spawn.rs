use std::io::{BufRead, BufReader, Write};
use std::process::{Child, Command, Stdio};

use dsx_proto::{self, AgentToTools, ToolsToAgent};

/// Spawn dsx as a tools subprocess (dsx.exe tools).
/// Returns (child, reader, writer) for IPC init.
pub fn spawn_process(exe: &std::path::Path) -> (Child, Box<dyn BufRead + Send>, Box<dyn Write + Send>) {
    let mut child = Command::new(exe)
        .arg("tools")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .expect("dsx-agent: failed to spawn tools subprocess");

    let reader: Box<dyn BufRead + Send> = Box::new(BufReader::new(child.stdout.take().unwrap()));
    let writer: Box<dyn Write + Send> = Box::new(child.stdin.take().unwrap());
    (child, reader, writer)
}

/// Respawn the tools subprocess.
pub fn respawn(child: &mut Option<Child>) -> bool {
    if let Some(mut c) = child.take() {
        let _ = c.kill();
        let _ = c.wait();
    }

    let exe = match std::env::current_exe() {
        Ok(e) => e,
        Err(_) => return false,
    };
    let (mut new_child, mut reader, mut writer) = spawn_process(&exe);

    let init = AgentToTools::Init {
        allowed_tools: vec![], session_seed: "pipe".into(), auto_mode: false,
    };
    let _ = dsx_proto::write_frame(&mut writer, &init);
    let ready = dsx_proto::read_frame::<ToolsToAgent>(&mut reader).ok().flatten();

    match ready {
        Some(ToolsToAgent::Ready { tools }) => {
            let defs: Vec<_> = tools.iter().cloned().collect();
            crate::tools::init_tools_ipc(reader, writer, defs);
            *child = Some(new_child);
            eprintln!("dsx-agent: tools respawned");
            true
        }
        _ => {
            eprintln!("dsx-agent: tools respawn failed (no Ready)");
            let _ = new_child.kill();
            let _ = new_child.wait();
            false
        }
    }
}
