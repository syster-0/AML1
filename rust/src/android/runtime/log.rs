//! IPC channel toward the launcher (Phase 4).
//!
//! The game connects to the launcher's named Unix-domain socket (path from
//! the launch manifest) and sends NDJSON frames: `hello` (pid), `log` lines,
//! and `exit`. The launcher turns log frames into live UI logs; the game also
//! mirrors each line to logcat directly, so `:game` output stays visible in
//! device logs even before the launcher processes a frame. We never touch
//! fd 1/2, so ART's process stdio is left intact.
//!
//! Writes are serialized by a mutex; the socket is owned for the process
//! lifetime.

use std::io::Write;
use std::os::unix::net::UnixStream;
use std::sync::Mutex;

use super::super::launch::GameFrame;
use super::RuntimeError;

static SENDER: Mutex<Option<UnixStream>> = Mutex::new(None);

/// Connect to the launcher socket and announce this game process.
pub(super) fn connect(socket_path: &str) -> Result<(), RuntimeError> {
    let stream = UnixStream::connect(socket_path)
        .map_err(|e| RuntimeError::Log(format!("connect {socket_path}: {e}")))?;
    {
        let mut slot = SENDER
            .lock()
            .map_err(|_| RuntimeError::Log("sender lock poisoned".into()))?;
        *slot = Some(stream);
        // Drop the guard before calling `send` (which locks SENDER again).
    }
    send(GameFrame::Hello {
        pid: std::process::id(),
    });
    Ok(())
}

/// Mirror a line to logcat and forward it as a `log` frame.
pub(crate) fn write_line(line: &str) {
    super::super::android_log("aml-game", line);
    send(GameFrame::Log {
        line: line.to_string(),
    });
}

/// Tell the launcher the game VM has stopped, then drop the connection so
/// the launcher's read loop returns.
pub(crate) fn send_exit(code: i32) {
    send(GameFrame::Exit { code });
    if let Ok(mut slot) = SENDER.lock() {
        *slot = None;
    }
}

fn send(frame: GameFrame) {
    let payload = match serde_json::to_vec(&frame) {
        Ok(mut bytes) => {
            bytes.push(b'\n');
            bytes
        }
        Err(_) => return,
    };
    let mut slot = match SENDER.lock() {
        Ok(slot) => slot,
        Err(_) => return,
    };
    if let Some(stream) = slot.as_mut() {
        let _ = stream.write_all(&payload);
        let _ = stream.flush();
    }
}
