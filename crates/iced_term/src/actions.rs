// SPDX-FileCopyrightText: 2026 shellkeep contributors
// SPDX-License-Identifier: MIT

//! Terminal action types returned by the backend event loop.

#[derive(Debug, Clone, PartialEq, Default)]
pub enum Action {
    Shutdown,
    ChangeTitle(String),
    #[default]
    Ignore,
}
