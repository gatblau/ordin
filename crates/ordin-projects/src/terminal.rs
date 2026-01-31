//! The engineer's terminal (ADR-022, SA-TERM-01; Internal Logic 13). Output bytes go to the
//! `StreamSink` only: no MCP tool, agent, journal entry or log line receives them.

use std::collections::HashMap;
use std::io::{Read, Write};
use std::sync::{Arc, Mutex};

use ordin_types::ProjectId;
use portable_pty::{native_pty_system, ChildKiller, CommandBuilder, MasterPty, PtySize};

use crate::error::ProjectError;
use crate::service::{lock, ProjectService};
use crate::types::{StreamId, StreamPayload, StreamSink};

struct Terminal {
    master: Box<dyn MasterPty + Send>,
    writer: Box<dyn Write + Send>,
    killer: Box<dyn ChildKiller + Send + Sync>,
}

#[derive(Default)]
pub(crate) struct Terminals {
    open: Mutex<HashMap<StreamId, Arc<Mutex<Terminal>>>>,
}

fn size(cols: u16, rows: u16) -> PtySize {
    PtySize {
        rows,
        cols,
        pixel_width: 0,
        pixel_height: 0,
    }
}

/// `$SHELL`, else `/bin/zsh` on macOS and `/bin/bash` on Linux; `%ComSpec%` on Windows.
fn person_shell() -> String {
    if cfg!(windows) {
        std::env::var("ComSpec").unwrap_or_else(|_| "cmd.exe".to_owned())
    } else {
        std::env::var("SHELL").unwrap_or_else(|_| {
            if cfg!(target_os = "macos") {
                "/bin/zsh".to_owned()
            } else {
                "/bin/bash".to_owned()
            }
        })
    }
}

fn relay(mut reader: Box<dyn Read + Send>, stream: StreamId, sink: Arc<dyn StreamSink>) {
    let mut buffer = [0u8; 8192];
    loop {
        match reader.read(&mut buffer) {
            Ok(0) | Err(_) => break,
            Ok(read) => sink.send(
                stream,
                StreamPayload::TerminalBytes(buffer[..read].to_vec()),
            ),
        }
    }
    sink.send(
        stream,
        StreamPayload::Closed {
            reason: "terminal_closed".to_owned(),
        },
    );
}

impl ProjectService {
    /// Internal Logic 13.1.
    // spec: ordin-desktop/components/ProjectService#ADR-022
    // spec: ordin-desktop/components/ProjectService#SA-TERM-01
    pub async fn terminal_open(
        &self,
        project_id: ProjectId,
        cols: u16,
        rows: u16,
    ) -> Result<StreamId, ProjectError> {
        let row = self.open_row(project_id).await?;
        let folder = self.place_folder(row.project.place_id).await?;
        let shell = lock(&self.tunables)
            .shell
            .clone()
            .unwrap_or_else(person_shell);
        let sink = self.streams.clone();
        let stream = StreamId(uuid::Uuid::now_v7());
        let terminal = tokio::task::spawn_blocking(move || {
            let failed = |_| ProjectError::TerminalFailed;
            let pair = native_pty_system()
                .openpty(size(cols, rows))
                .map_err(failed)?;
            let mut command = CommandBuilder::new(shell);
            command.cwd(folder);
            for variable in crate::CREDENTIAL_VARIABLES {
                command.env_remove(variable);
            }
            let mut child = pair.slave.spawn_command(command).map_err(failed)?;
            drop(pair.slave);
            let killer = child.clone_killer();
            let reader = pair.master.try_clone_reader().map_err(failed)?;
            let writer = pair.master.take_writer().map_err(failed)?;
            std::thread::spawn(move || relay(reader, stream, sink));
            std::thread::spawn(move || {
                let _ = child.wait();
            });
            Ok::<_, ProjectError>(Terminal {
                master: pair.master,
                writer,
                killer,
            })
        })
        .await
        .map_err(|_| ProjectError::TerminalFailed)??;
        lock(&self.terminals.open).insert(stream, Arc::new(Mutex::new(terminal)));
        Ok(stream)
    }

    fn terminal(&self, stream: StreamId) -> Result<Arc<Mutex<Terminal>>, ProjectError> {
        lock(&self.terminals.open)
            .get(&stream)
            .cloned()
            .ok_or(ProjectError::TerminalFailed)
    }

    /// Accepted only from the Service API (the screens), never from the MCP Front.
    pub async fn terminal_input(
        &self,
        terminal: StreamId,
        bytes: Vec<u8>,
    ) -> Result<(), ProjectError> {
        let terminal = self.terminal(terminal)?;
        tokio::task::spawn_blocking(move || {
            let mut terminal = lock(&terminal);
            terminal
                .writer
                .write_all(&bytes)
                .and_then(|_| terminal.writer.flush())
                .map_err(|_| ProjectError::TerminalFailed)
        })
        .await
        .map_err(|_| ProjectError::TerminalFailed)?
    }

    pub async fn terminal_resize(
        &self,
        terminal: StreamId,
        cols: u16,
        rows: u16,
    ) -> Result<(), ProjectError> {
        let terminal = self.terminal(terminal)?;
        let resized = lock(&terminal).master.resize(size(cols, rows));
        resized.map_err(|_| ProjectError::TerminalFailed)
    }

    /// Closing the tab kills the shell.
    pub async fn terminal_close(&self, terminal: StreamId) {
        let removed = lock(&self.terminals.open).remove(&terminal);
        if let Some(terminal) = removed {
            let _ = lock(&terminal).killer.kill();
        }
    }

    pub(crate) fn close_all_terminals(&self) {
        let open: Vec<_> = lock(&self.terminals.open).drain().collect();
        for (_, terminal) in open {
            let _ = lock(&terminal).killer.kill();
        }
    }
}
