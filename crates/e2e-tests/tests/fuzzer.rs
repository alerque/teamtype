// SPDX-FileCopyrightText: 2024 blinry <mail@blinry.org>
// SPDX-FileCopyrightText: 2024 zormit <nt4u@kpvn.de>
// SPDX-FileCopyrightText: 2026 Caleb Maclennan <caleb@alerque.com>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

use std::collections::HashMap;
use std::path::PathBuf;

use anyhow::{Context, Result};

use e2e_tests::actors::{Actor, Neovim};
use futures::future::join_all;
use pretty_assertions::assert_eq;
use rand::RngExt;
use teamtype::config::{BaseDir, Config, Peer};
use teamtype::daemon::{Daemon, TEST_FILE_PATH};
use teamtype::logging::{self, LoggingDisplay};
use teamtype::sandbox;
use teamtype::traits::Interactions;
use teamtype::types::UserInterface;
use tempfile::tempdir;
use tokio::time::{Duration, sleep, timeout};
use tracing::{debug, info, warn};

async fn perform_random_edits(name: &str, actor: &mut (impl Actor + ?Sized)) {
    for i in 1..500 {
        // Bound each single edit, so that we learn *which* actor got stuck.
        timeout(Duration::from_secs(60), actor.apply_random_delta())
            .await
            .unwrap_or_else(|_| panic!("{name} stopped responding while applying edit #{i}"));

        let random_millis = rand::rng().random_range(10..20);
        sleep(Duration::from_millis(random_millis)).await;
    }
}

fn initialize_directory() -> (BaseDir, PathBuf) {
    let dir = tempdir().expect("Failed to create temp directory");
    let base_dir = BaseDir::Temporary(dir);
    let teamtype_dir = base_dir.join(".teamtype");
    sandbox::create_dir(&base_dir, &teamtype_dir).expect("Failed to create .teamtype directory");

    let file = base_dir.join(TEST_FILE_PATH);

    (base_dir, file)
}

struct FuzzerInteractions {}

impl Interactions for FuzzerInteractions {
    fn confirm(&self, question: &str) -> Result<bool> {
        debug!("Fuzzer asked for a confirmation '{question}', answering with 'false'");
        Ok(false)
    }

    fn log(&self, message: &str) {
        debug!(message);
    }

    fn inform(&self, message: &str) {
        info!(message);
    }

    fn warn(&self, message: &str) {
        warn!(message);
    }
}

/// How long we give each phase of the fuzzer before we consider it stuck.
///
/// Every wait in this test is bounded, so that a stall shows up as a loud failure with a
/// diagnostic message instead of a CI job that hangs forever.
const PHASE_TIMEOUT: Duration = Duration::from_secs(5 * 60);

/// How long a single `content()` call may take.
///
/// A healthy actor answers in milliseconds, so this is a very generous bound. It exists so that a
/// single stuck actor is reported *by name*, instead of taking the whole phase down with it.
const CONTENT_TIMEOUT: Duration = Duration::from_secs(10);

/// How long we let the daemons and editors keep retrying until their contents agree.
const CONVERGENCE_TIMEOUT: Duration = Duration::from_secs(3 * 60);

/// Read the content of every actor, bounding each read separately.
///
/// Reading them one by one (rather than as one group) means that a stuck actor is named in the
/// error message, which is the single most useful thing to know when this test hangs.
async fn collect_contents(
    actors: &mut HashMap<String, Box<dyn Actor>>,
) -> Result<HashMap<String, String>> {
    let mut contents = HashMap::new();
    for (name, actor) in actors.iter_mut() {
        let content = timeout(CONTENT_TIMEOUT, actor.content())
            .await
            .with_context(|| format!("Timeout while reading the content of {name}"))?;
        contents.insert(name.clone(), content);
    }
    Ok(contents)
}

#[tokio::main]
async fn main() -> Result<()> {
    let default_panic = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        default_panic(info);
        std::process::exit(1);
    }));

    logging::initialize(LoggingDisplay::Pretty)?;

    let ui = &UserInterface::new(FuzzerInteractions {});

    // Set up files in shared directories. The directories will get cleaned up automatically when
    // the handle goes out of scope. We don't *use* the handle but we do need to keep it in scope.
    let (base_dir1, file1) = initialize_directory();
    let (base_dir2, file2) = initialize_directory();

    // Seed an empty starting file to what will be the sharing daemon side of the test session.
    sandbox::write_file(&base_dir1, &file1, b"").expect("Failed to create file in temp directory");

    // Set up the actors.
    let config1 = Config {
        base_dir: base_dir1,
        ..Default::default()
    };
    ui.log("Starting the first daemon");
    let daemon1 = Daemon::new(config1, true, false, ui).await?;

    // Wait until iroh's DNS discovery (hopefully) works.
    sleep(Duration::from_millis(1000)).await;

    ui.log("Starting the first Neovim");
    let nvim1 = timeout(PHASE_TIMEOUT, Neovim::new(Some(file1)))
        .await
        .context("Timeout while starting the first Neovim")?;

    let config2 = Config {
        base_dir: base_dir2,
        peer: Some(Peer::SecretAddress(daemon1.secret_address().to_string())),
        ..Default::default()
    };
    ui.log("Starting the second daemon");
    let daemon2 = Daemon::new(config2, false, false, ui).await?;

    // Wait until file2 appears, i.e. until the two daemons have synced with each other.
    timeout(PHASE_TIMEOUT, async {
        while !file2.exists() {
            debug!("{file2:?} doesn't exist yet, sleeping");
            sleep(Duration::from_millis(500)).await;
        }
    })
    .await
    .context("Timeout while waiting for the two daemons to sync their initial file")?;

    ui.log("Starting the second Neovim");
    let nvim2 = timeout(PHASE_TIMEOUT, Neovim::new(Some(file2)))
        .await
        .context("Timeout while starting the second Neovim")?;

    // Give the second Neovim time to process the "open" call.
    sleep(Duration::from_millis(1000)).await;

    let mut actors: HashMap<String, Box<dyn Actor>> = HashMap::new();
    actors.insert("daemon1".to_string(), Box::new(daemon1));
    actors.insert("nvim1".to_string(), Box::new(nvim1));
    actors.insert("daemon2".to_string(), Box::new(daemon2));
    actors.insert("nvim2".to_string(), Box::new(nvim2));

    ui.log("Performing edits");

    let handles = actors
        .iter_mut()
        .map(|(name, actor)| perform_random_edits(name, actor.as_mut()));
    timeout(PHASE_TIMEOUT, join_all(handles))
        .await
        .context("Timeout while performing random edits")?;

    ui.log("Waiting for all contents to be equal");

    // If the actors don't agree in time, we don't give up here: we carry on to the final read
    // below. That read is bounded per actor, so it names whichever actor is stuck, and the
    // comparison afterwards prints the differing contents. Failing here instead would swallow
    // the most useful part of the diagnosis.
    let converged = timeout(CONVERGENCE_TIMEOUT, async {
        loop {
            let contents = collect_contents(&mut actors).await?;

            // If all contents are equal already, we have succeeded!
            let first = contents.values().next().expect("No contents found");
            if contents.values().all(|content| content == first) {
                break;
            }
            sleep(Duration::from_millis(1000)).await;
        }
        Ok::<_, anyhow::Error>(())
    })
    .await;

    match converged {
        Ok(Ok(())) => {}
        Ok(Err(error)) => ui.warn(&format!(
            "Failed to read all contents while waiting for convergence: {error:#}"
        )),
        Err(_) => ui.warn(&format!(
            "Timeout after {CONVERGENCE_TIMEOUT:?} while waiting for all contents to be equal"
        )),
    }

    // Get all contents. Every read is bounded separately, so that a stuck actor fails the test
    // with a message naming it, instead of hanging.
    let contents = collect_contents(&mut actors)
        .await
        .context("Timeout while getting the final contents of all actors")?;

    // Print all contents.
    for (name, content) in &contents {
        println!(
            r#"
{name} content:
---------------------------------
{content}
---------------------------------
"#
        );
    }

    // Check that all contents are identical.
    let first = contents.values().next().expect("No contents found");
    let first_name = contents.keys().next().expect("No content keys found");
    for (name, content) in &contents {
        assert_eq!(
            first, content,
            "Content of {} differs from {}",
            first_name, name
        );
    }

    println!("SUCCESS! 🥳");

    // Quit immediately, so that we don't run into cleanup issues, which would make our CI fail...
    // TODO: Handle shutdown more gracefully.
    std::process::exit(0);
}
