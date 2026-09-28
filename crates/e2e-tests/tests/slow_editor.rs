// SPDX-FileCopyrightText: 2026 Caleb Maclennan <caleb@alerque.com>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

//! Regression tests for editors which do not (or no longer) read from their socket.

use std::path::{Path, PathBuf};

use serial_test::serial;
use teamtype::config::{BaseDir, Config};
use teamtype::daemon::{Daemon, TEST_FILE_PATH};
use teamtype::sandbox;
use teamtype::traits::Interactions;
use teamtype::types::UserInterface;
use tempfile::tempdir;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::UnixStream;
use tokio::time::{Duration, timeout};

struct SilentInteractions {}

impl Interactions for SilentInteractions {
    fn confirm(&self, _question: &str) -> anyhow::Result<bool> {
        Ok(false)
    }

    fn log(&self, _message: &str) {}

    fn inform(&self, _message: &str) {}

    fn warn(&self, message: &str) {
        eprintln!("daemon warning: {message}");
    }
}

fn setup() -> (BaseDir, PathBuf) {
    let dir = tempdir().expect("Failed to create temp directory");
    let base_dir = BaseDir::Temporary(dir);
    let teamtype_dir = base_dir.join(".teamtype");
    sandbox::create_dir(&base_dir, &teamtype_dir).expect("Failed to create .teamtype directory");
    let file = base_dir.join(TEST_FILE_PATH);
    (base_dir, file)
}

/// Connects an editor to the daemon, tells it that we opened the test file, and then never reads
/// another byte from the socket. This simulates a Neovim (or other editor) that is too busy, or
/// wedged, to keep up with the deltas the daemon is sending it.
async fn connect_stubborn_editor(socket_path: &Path, file: &Path) -> UnixStream {
    let mut stream = UnixStream::connect(socket_path)
        .await
        .expect("Could not connect to daemon socket");
    let open = format!(
        r#"{{"jsonrpc":"2.0","id":1,"method":"open","params":{{"uri":"file://{}","content":""}}}}"#,
        file.display()
    );
    stream
        .write_all(open.as_bytes())
        .await
        .expect("Could not write open message");
    stream
        .write_all(b"\n")
        .await
        .expect("Could not write newline");
    stream.flush().await.expect("Could not flush");

    // Reading the response to our "open" request tells us that the daemon has registered this
    // editor and is now sending it deltas. From here on we don't read a single byte again.
    let mut response = vec![0u8; 4096];
    let read = timeout(Duration::from_secs(10), stream.read(&mut response[..]))
        .await
        .expect("Daemon did not respond to the 'open' request")
        .expect("Could not read the 'open' response");
    assert_eq!(
        String::from_utf8_lossy(&response[..read]),
        "{\"id\":1,\"jsonrpc\":\"2.0\",\"result\":\"success\"}\n",
        "Unexpected response to the 'open' request"
    );
    stream
}

#[tokio::test]
#[serial]
async fn non_reading_editor_does_not_freeze_daemon() {
    let ui = UserInterface::new(SilentInteractions {});

    let (base_dir, file) = setup();
    sandbox::write_file(&base_dir, &file, b"").expect("Failed to create file in temp directory");

    let config = Config {
        base_dir: base_dir.clone(),
        ..Default::default()
    };
    let daemon = Daemon::new(config, true, false, &ui)
        .await
        .expect("Failed to start daemon");

    let socket_path = base_dir.join(".teamtype").join("socket");
    let _stubborn = connect_stubborn_editor(&socket_path, &file).await;

    // Give the daemon a moment to register the editor.
    tokio::time::sleep(Duration::from_millis(200)).await;

    // Now generate edits without ever reading from the editor socket. This floods the socket
    // buffers, and would block the document actor on a full buffer if writing to editors was
    // done inline (as it used to be).
    let editor = tokio::spawn({
        let handle = daemon.document_handle.clone();
        async move {
            for _ in 0..5_000 {
                handle.apply_random_delta().await;
            }
        }
    });

    // The daemon must stay responsive throughout, no matter how far behind the editor is.
    for _ in 0..20 {
        let result = timeout(Duration::from_secs(5), daemon.document_handle.content()).await;
        assert!(
            result.is_ok(),
            "The document actor stopped responding because an editor is not reading from its \
             socket."
        );
    }

    editor.abort();
}

/// A second, well-behaved editor must keep receiving deltas even while another editor is stuck.
#[tokio::test]
#[serial]
async fn stuck_editor_does_not_starve_other_editors() {
    let ui = UserInterface::new(SilentInteractions {});

    let (base_dir, file) = setup();
    sandbox::write_file(&base_dir, &file, b"").expect("Failed to create file in temp directory");

    let config = Config {
        base_dir: base_dir.clone(),
        ..Default::default()
    };
    let daemon = Daemon::new(config, true, false, &ui)
        .await
        .expect("Failed to start daemon");

    let socket_path = base_dir.join(".teamtype").join("socket");
    let _stuck = connect_stubborn_editor(&socket_path, &file).await;

    // A second editor which reads everything it gets.
    let mut eager = connect_stubborn_editor(&socket_path, &file).await;

    // Generators of edits, none of which read their own deltas.
    let editors = tokio::spawn({
        let handle = daemon.document_handle.clone();
        async move {
            for _ in 0..5_000 {
                handle.apply_random_delta().await;
            }
        }
    });

    // The eager editor should still see new deltas arriving.
    let mut received = Vec::new();
    for _ in 0..10 {
        let mut buffer = vec![0u8; 64 * 1024];
        let read = timeout(Duration::from_secs(5), eager.read(&mut buffer[..]))
            .await
            .expect("The well-behaved editor stopped receiving deltas")
            .expect("Could not read from the editor socket");
        assert!(read > 0, "The well-behaved editor's socket was closed");
        received.extend_from_slice(&buffer[..read]);
    }

    editors.abort();

    let messages = received
        .split(|byte| *byte == b'\n')
        .filter(|chunk| !chunk.is_empty())
        .count();
    assert!(
        messages >= 10,
        "Expected the well-behaved editor to receive many deltas, got {messages}."
    );
}
