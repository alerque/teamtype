// SPDX-FileCopyrightText: 2026 Caleb Maclennan <caleb@alerque.com>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

use std::path::{Path, PathBuf};

use anyhow::bail;
use anyhow::{Context, Result};
use docstr::docstr;
use microxdg::XdgApp;
use tempfile::{TempDir, tempdir_in};

use crate::config::{self};
use crate::sandbox;
use crate::types::UserInterface;

pub fn setup_teamtype_directory(
    directory: &Path,
    temporary_directory: Option<&TempDir>,
    ui: &UserInterface,
) -> Result<()> {
    if !has_teamtype_directory(directory) {
        let teamtype_dir = directory.join(config::CONFIG_DIR);
        let directory_is_temporary_directory = temporary_directory.is_some();
        if directory_is_temporary_directory {
            ui.log(&format!(
                "'{}' is the temporary directory that is used as a Teamtype directory.",
                directory.display()
            ));
            sandbox::create_dir(directory, &teamtype_dir)?;
        } else if ui.confirm(&docstr!(format!
            /// '{}' hasn't been used as a Teamtype directory before.
            ///
            /// Do you want to enable live collaboration here? (This will create a {}/ directory.)
            directory.display(),
            config::CONFIG_DIR
        ))? {
            sandbox::create_dir(directory, &teamtype_dir)?;
            ui.log("Created! Resuming launch.");
        } else {
            bail!("Aborting launch. Teamtype needs a .teamtype/ directory to function");
        }
    }
    Ok(())
}

fn has_teamtype_directory(dir: &Path) -> bool {
    let teamtype_dir = dir.join(config::CONFIG_DIR);
    // Using the sandbox method here is technically unnecessary,
    // but we want to really run all path operations through the sandbox module.
    sandbox::exists(dir, &teamtype_dir).expect("Failed to check") && teamtype_dir.is_dir()
}

pub fn setup_temporary_directory() -> Result<TempDir> {
    let parent_dir = get_app_cache_dir()?;
    tempdir_in(&parent_dir).with_context(|| {
        format!(
            "Failed to create a temporary directory in the directory {}",
            parent_dir.display()
        )
    })
}

fn get_app_cache_dir() -> Result<PathBuf> {
    let xdg = XdgApp::new("teamtype")?;
    let app_cache_dir = xdg.app_cache()?;
    let app_cache_dir_parent = app_cache_dir.parent().with_context(|| {
        format!(
            "Failed to get parent directory of the directory {}",
            app_cache_dir.display()
        )
    })?;
    // Using the sandbox method here is technically unnecessary,
    // but we want to really run all path operations through the sandbox module.
    sandbox::create_dir(app_cache_dir_parent, &app_cache_dir)?;
    Ok(app_cache_dir)
}
