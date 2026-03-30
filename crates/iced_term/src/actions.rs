// SPDX-FileCopyrightText: 2026 shellkeep contributors
// SPDX-License-Identifier: MIT

#[derive(Debug, Clone, PartialEq, Default)]
pub enum Action {
    Shutdown,
    ChangeTitle(String),
    #[default]
    Ignore,
}
