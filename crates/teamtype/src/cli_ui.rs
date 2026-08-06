// SPDX-FileCopyrightText: 2026 Caleb Maclennan <caleb@alerque.com>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

use anyhow::{Context, Result};
use inquire::Confirm;
use teamtype::traits::Interactions;
use tracing::debug;

#[derive(Clone)]
pub struct ConsoleInteractions {}

impl Interactions for ConsoleInteractions {
    fn confirm(&self, question: &str) -> Result<bool> {
        debug!("UI confirm event: {question}");
        Confirm::new(question)
            .with_default(false)
            .prompt()
            .context("Failed to read answer to y/n prompt")
    }
}
