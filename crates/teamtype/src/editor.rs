// SPDX-FileCopyrightText: 2024 blinry <mail@blinry.org>
// SPDX-FileCopyrightText: 2024 zormit <nt4u@kpvn.de>
// SPDX-FileCopyrightText: 2026 Caleb Maclennan <caleb@alerque.com>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

//! This module is all about daemon to editor communication.

use std::os::unix::{fs::PermissionsExt, net::UnixStream as StdUnixStream};
use std::path::{Path, PathBuf};
use std::{env, fs};

use anyhow::bail;
use anyhow::{Context, Result};
use futures::{SinkExt, StreamExt};
use tokio::{
    net::{UnixListener, UnixStream},
    sync::mpsc,
    sync::mpsc::error::SendError,
};
use tokio_util::{
    bytes::BytesMut,
    codec::{Decoder, Encoder, FramedRead, FramedWrite, LinesCodec},
};
use tracing::debug;

use crate::daemon::{DocMessage, DocumentActorHandle};
use crate::editor_protocol::{
    EditorProtocolMessageError, IncomingMessage, JSONRPCResponse, OutgoingMessage,
};
use crate::sandbox;
use crate::types::UserInterface;

pub type EditorId = usize;

/// How many messages we tolerate queuing up for a single editor before we warn about it.
const EDITOR_BACKLOG_WARN_THRESHOLD: usize = 10_000;

/// A handle to send messages to one connected editor.
///
/// Sending never blocks the caller. The actual socket write happens in a dedicated task, so a
/// single editor that stops reading (because it is busy, wedged, or simply too slow) can never
/// stall the rest of the daemon. Without this, one lagging editor would block the document actor
/// on a full socket buffer, which in turn blocks *everything* else: peer sync, the file watcher,
/// and any request for the current content.
#[derive(Clone, Debug)]
pub struct EditorWriter {
    message_tx: mpsc::UnboundedSender<OutgoingMessage>,
}

impl EditorWriter {
    /// Queue a message for the editor.
    ///
    /// Fails only if the writer task is gone, i.e. the editor connection is already broken.
    pub fn send(&self, message: OutgoingMessage) -> Result<(), SendError<OutgoingMessage>> {
        self.message_tx.send(message)
    }
}

#[derive(Debug)]
pub struct OutgoingProtocolCodec;

impl Encoder<OutgoingMessage> for OutgoingProtocolCodec {
    type Error = anyhow::Error;

    fn encode(&mut self, item: OutgoingMessage, dst: &mut BytesMut) -> Result<(), Self::Error> {
        let payload = item.to_jsonrpc()?;
        dst.extend_from_slice(format!("{payload}\n").as_bytes());
        Ok(())
    }
}

#[derive(Debug)]
pub struct IncomingProtocolCodec;

impl Decoder for IncomingProtocolCodec {
    type Error = anyhow::Error;
    type Item = IncomingMessage;

    fn decode(&mut self, src: &mut BytesMut) -> Result<Option<Self::Item>, Self::Error> {
        LinesCodec::new()
            .decode(src)?
            .map(|line| IncomingMessage::from_jsonrpc(&line))
            .transpose()
    }
}

fn is_user_readable_only(socket_path: &Path) -> Result<()> {
    let parent_dir = socket_path
        .parent()
        .context("The socket path should not be the root directory")?;
    let current_permissions = fs::metadata(parent_dir)
        .with_context(|| {
            format!(
                "Expected to have access to metadata of the socket's parent directory: {}",
                parent_dir.display()
            )
        })?
        .permissions()
        .mode();
    // Group and others should not have any permissions.
    let allowed_permissions = 0o77700u32;
    if current_permissions | allowed_permissions != allowed_permissions {
        bail!(
            "For security reasons, the parent directory of the socket must only be accessible by the current user. Please run `chmod go-rwx {}`",
            parent_dir.display()
        );
    }
    Ok(())
}

pub fn strip_current_dir(path: &Path) -> PathBuf {
    let Ok(cwd) = env::current_dir() else {
        return path.to_path_buf();
    };
    path.strip_prefix(&cwd)
        .map_or_else(|_| path.to_path_buf(), Path::to_path_buf)
}

/// # Panics
///
/// Will panic if we fail to listen on the socket, or if we fail to accept an incoming connection.
pub fn spawn_socket_listener(
    socket_path: &Path,
    document_handle: DocumentActorHandle,
    ui: &UserInterface,
) -> Result<()> {
    // Make sure the parent directory of the socket is only accessible by the current user.
    if let Err(description) = is_user_readable_only(socket_path) {
        bail!("{description}");
    }

    // Using the sandbox method here is technically unnecessary,
    // but we want to really run all path operations through the sandbox module.
    // TODO: Use correct directory as guard.
    if sandbox::exists(Path::new("/"), Path::new(&socket_path))
        .expect("Failed to check existence of path")
    {
        // If there's an existing socket, try to connect to it as a client. If that fails, we assume
        // there's no other daemon running and we can delete the socket.
        if StdUnixStream::connect(strip_current_dir(socket_path)).is_ok() {
            bail!(
                "Detected an existing daemon running for this directory. Rejecting to start another one."
            );
        }
        ui.warn(
            "An existing socket was found for this directory, but since the daemon seems to be defunct it is being removed."
        );
        sandbox::remove_file(Path::new("/"), socket_path).expect("Could not remove socket");
    }

    // The std library function used to create sockets requires a path shorter than SUN_LEN, but the
    // length that matters is only the segment it is asked to handle. If passed an absolute path
    // here we can potentially be run in a path that exceeds the maximum (~100 chars). Passing it a
    // relative path effectively sidesteps this limitation. Stripping the leading path segments will
    // result in a relative path that won't have a long cumbersome prefix that fails safety checks.
    // The extra song and dance to change into the parent directory first is not needed by our CLI
    // (which already changes to that location) but it will make this API usable when linked as a
    // library without changing the parent thread's location for keeps.
    let previous_cwd = env::current_dir()?;
    env::set_current_dir(
        socket_path
            .parent()
            .context("Invalid socket creation location")?,
    )?;
    let listener = UnixListener::bind(strip_current_dir(socket_path))?;
    env::set_current_dir(previous_cwd)?;
    debug!("Listening on UNIX socket: {}", socket_path.display());

    tokio::spawn({
        let ui = ui.clone();
        async move {
            loop {
                match listener.accept().await {
                    Ok((stream, _addr)) => {
                        let id = document_handle.clone().next_editor_id();
                        let document_handle_clone = document_handle.clone();
                        tokio::spawn({
                            let ui = ui.clone();
                            async move {
                                handle_editor_connection(
                                    stream,
                                    document_handle_clone.clone(),
                                    id,
                                    &ui,
                                )
                                .await;
                            }
                        })
                    }
                    Err(err) => {
                        panic!("Error while accepting socket connection: {err}");
                    }
                };
            }
        }
    });

    Ok(())
}

async fn handle_editor_connection(
    stream: UnixStream,
    document_handle: DocumentActorHandle,
    editor_id: EditorId,
    ui: &UserInterface,
) {
    let (stream_read, stream_write) = tokio::io::split(stream);
    let mut reader = FramedRead::new(stream_read, IncomingProtocolCodec);

    // Writing to the editor happens in its own task, so that a slow editor can not block the
    // document actor (and thereby the whole daemon) on a full socket buffer.
    let (message_tx, mut message_rx) = mpsc::unbounded_channel();
    let writer_task = tokio::spawn({
        let ui = ui.clone();
        async move {
            let mut writer = FramedWrite::new(stream_write, OutgoingProtocolCodec);
            let mut warned_about_backlog = false;
            while let Some(message) = message_rx.recv().await {
                if !warned_about_backlog && message_rx.len() > EDITOR_BACKLOG_WARN_THRESHOLD {
                    ui.warn(&format!(
                        "Editor #{editor_id} is not reading from its socket fast enough. There are \
                         more than {EDITOR_BACKLOG_WARN_THRESHOLD} messages queued up for it."
                    ));
                    warned_about_backlog = true;
                }
                if let Err(e) = writer.send(message).await {
                    // The editor is gone. Stop the writer task; the reader loop below will notice
                    // the closed connection and clean up the editor.
                    ui.warn(&format!("Failed to write to editor #{editor_id}: {e}"));
                    return;
                }
            }
        }
    });

    let editor_writer = EditorWriter { message_tx };
    document_handle
        .send_message(DocMessage::NewEditorConnection(
            editor_id,
            editor_writer.clone(),
        ))
        .await;
    ui.log(&format!("Editor #{editor_id} connected."));

    while let Some(message) = reader.next().await {
        match message {
            Ok(message) => {
                document_handle
                    .send_message(DocMessage::FromEditor(editor_id, message))
                    .await;
            }
            Err(e) => {
                let response = JSONRPCResponse::RequestError {
                    id: None,
                    error: EditorProtocolMessageError {
                        code: -32700,
                        message: format!("Invalid request: {e}"),
                        data: None,
                    },
                };
                ui.warn(&format!("Error for JSON-RPC request: {response:?}"));
                let message = OutgoingMessage::Response(response);
                if editor_writer.send(message).is_err() {
                    break;
                }
            }
        }
    }
    // Err(e) => {
    // }

    // The connection is going away. The writer task holds the write half of the socket, so stop
    // it instead of leaving it (potentially blocked on a write to a dead peer) behind.
    drop(editor_writer);
    writer_task.abort();

    document_handle
        .send_message(DocMessage::CloseEditorConnection(editor_id))
        .await;
    ui.log(&format!("Editor #{editor_id} disconnected."));
}
