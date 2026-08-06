// SPDX-FileCopyrightText: 2026 Caleb Maclennan <caleb@alerque.com>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

use anyhow::Result;

use crate::traits::Interactions;

// These tests don't look at UI output, they look at what happens to files. This dummy UI
// implementation just needs to discard everything.
#[derive(Clone)]
pub struct TestInteractions {}

impl Interactions for TestInteractions {
    fn confirm(&self, _question: &str) -> Result<bool> {
        Ok(false)
    }

    fn log(&self, _message: &str) {}

    fn inform(&self, _message: &str) {}

    fn warn(&self, _message: &str) {}
}
