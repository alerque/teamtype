// SPDX-FileCopyrightText: 2026 Caleb Maclennan <caleb@alerque.com>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

/// Defines the ways Teamtype expects to be able to interact with an end user. This includes
/// printing messages through *to* the user and receiving confirmations *from* them. When using the
/// CLI interface, these interactions are provided as console functions that interact with the
/// `STDIN` and `STDOUT` streams.
///
/// Editors or editor plugins directly linking to Teamtype will need to wire these up to whatever
/// UI mechanism is available in their context.
pub trait Interactions: Send + Sync {}
