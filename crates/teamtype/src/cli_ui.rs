// SPDX-FileCopyrightText: 2026 Caleb Maclennan <caleb@alerque.com>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

use anyhow::{Context, Result};
use inquire::Confirm;
use nu_ansi_term::Color;
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

    fn inform(&self, message: &str) {
        debug!("UI inform event: {message}");
        println!("{message}");
    }

    fn warn(&self, message: &str) {
        let yellow = Color::Yellow;
        debug!("UI warn event: {message}");
        eprintln!("{}", yellow.paint(message));
    }
}
