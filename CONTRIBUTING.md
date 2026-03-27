<!--
SPDX-FileCopyrightText: 2026 shellkeep contributors
SPDX-License-Identifier: GPL-3.0-or-later
-->

# Contributing to shellkeep

Thank you for your interest in contributing to shellkeep. This document
explains how to set up your development environment, run tests, and submit
changes.

## Contributor License Agreement

External contributors must sign the [Contributor License Agreement](CLA.md)
before their pull requests can be merged. The CLA is managed via
[CLA-assistant](https://cla-assistant.io/) and will prompt you automatically
when you open your first pull request.

## Development Setup

### Prerequisites

- GCC or Clang (C11 + C++17 support)
- CMake >= 3.22
- Ninja
- pkg-config
- Qt6 development libraries
- libssh >= 0.10.0 development libraries
- GLib 2.0 development libraries
- json-glib development libraries
- cmocka (for unit tests)
- Docker (for integration tests)

### Install dependencies

**Debian/Ubuntu:**

```bash
sudo apt install build-essential cmake ninja-build pkg-config \
  qt6-base-dev libgl-dev libssh-dev \
  libglib2.0-dev libjson-glib-dev \
  libcmocka-dev clang-format clang-tidy cppcheck \
  gcovr lcov
```

**macOS:**

```bash
brew install cmake ninja pkg-config qt@6 libssh glib json-glib cmocka
```

### Build

```bash
cmake -B build -G Ninja -DCMAKE_BUILD_TYPE=Debug -DSK_BUILD_TESTS=ON -DSK_BUILD_QT_UI=ON
cmake --build build
```

### Run

```bash
./build/shellkeep user@host
```

## Testing

### Unit tests

```bash
ctest --test-dir build --label-regex unit --output-on-failure
```

### Integration tests

Integration tests require Docker with an sshd + tmux container:

```bash
ctest --test-dir build --label-regex integration --output-on-failure
```

### All tests

```bash
ctest --test-dir build --output-on-failure
```

## Code Style

shellkeep follows the GNOME/GTK coding style enforced by `.clang-format`.

### Rules

- **Language:** C11
- **Naming:** `snake_case` everywhere
- **Public symbols:** prefixed with `sk_` (e.g., `sk_ssh_connect()`)
- **Indentation:** spaces (as defined in `.clang-format`)
- **Line length:** soft limit of 100 characters
- **Error handling:** every function that can fail returns a status or
  accepts `GError **`
- **Threading:** never block the UI thread; use `GTask` for blocking
  operations in the C backend, Qt signals/slots for the UI layer

### SPDX headers

Every `.c` and `.h` file must include the SPDX header:

```c
// SPDX-FileCopyrightText: 2026 shellkeep contributors
// SPDX-License-Identifier: GPL-3.0-or-later
```

### Internationalization

All user-visible strings must use gettext:

```c
_("Connected to %1$s since %2$s")
ngettext("%d active session", "%d active sessions", n)
```

### Formatting check

```bash
find src/ include/ tests/ -name '*.c' -o -name '*.h' | \
  xargs clang-format --dry-run --Werror
```

### Static analysis

```bash
cppcheck --enable=warning,style,performance,portability -I include/ src/
```

## Architecture

shellkeep uses a layered architecture. See [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md)
for details. Key dependency rules:

- `ui_qt` never includes `ssh` directly
- `ssh` never calls Qt/UI functions
- `state` never calls Qt/UI functions
- The connect layer uses `sk_ui_bridge.h` to communicate with the UI
  without toolkit-specific headers
- Each layer uses opaque types and communicates via callbacks

Source layout:

```
src/ssh/            -- SSH connections, auth, channels, reconnection (libssh)
src/session/        -- tmux interaction, create/attach/list
src/terminal_qt/    -- Qt terminal widget, search, dead session, themes
src/state/          -- Persistence, JSON, JSONL, lock
src/ui_qt/          -- Qt6 windows, tabs, tray, dialogs, stylesheet
src/connect/        -- End-to-end connection flow + UI bridge
src/config/         -- INI parsing, defaults, validation
src/log/            -- Logging, rotation, crash handling
include/shellkeep/  -- Public headers (C + C++)
```

## Requirement Traceability

When implementing a requirement from `REQUIREMENTS.md`, reference its ID in
a code comment:

```c
/* FR-CONN-01 */
status = ssh_session_is_known_server(session);
```

## Security

- Never log passwords, private keys, terminal content, environment
  variables, clipboard content, or SFTP file content
- All state files must have `0600` permissions, directories `0700`
- Never interpolate user input into shell strings
- See [SECURITY.md](SECURITY.md) for the vulnerability disclosure policy

## Pull Request Process

1. Fork the repository and create a feature branch from `main`
2. Write or update tests for your changes
3. Ensure all tests pass and code formatting is correct
4. Fill in the PR template completely
5. Wait for CI to pass and a maintainer review
6. Squash-merge will be used for most PRs

### Commit messages

Use clear, descriptive commit messages in English:

```
component: short description of the change

Longer explanation if needed. Reference requirement IDs
and issue numbers where applicable.

Fixes #123
Ref: FR-CONN-01
```

## Reporting Bugs

Use the [bug report template](https://github.com/shellkeep/shellkeep/issues/new?template=bug_report.yml).
Never include passwords, private keys, or terminal content containing secrets.

## Requesting Features

Use the [feature request template](https://github.com/shellkeep/shellkeep/issues/new?template=feature_request.yml).

## Git Hooks

shellkeep provides pre-commit hooks that check formatting and SPDX headers
before each commit. Install them once after cloning:

```bash
./scripts/install-hooks.sh
```

The hook runs `clang-format` and verifies SPDX headers on staged `.c`/`.h`
files. If formatting is wrong, run `make format` to fix it automatically.

## Makefile Convenience Targets

A Makefile wraps common Meson commands:

| Target        | Description                                |
|---------------|--------------------------------------------|
| `make build`  | Configure (if needed) and compile           |
| `make test`   | Build and run all tests                     |
| `make lint`   | Run cppcheck + clang-format check           |
| `make clean`  | Remove the build directory                  |
| `make format` | Auto-format all `.c`/`.h` files             |
| `make check`  | Run lint + test                             |

## Quick Start (One-Liner)

On a supported distribution, the fastest path from zero to building:

```bash
sudo ./scripts/setup-dev.sh && make build && make test
```

## Troubleshooting

### `cmake` fails with "dependency not found"

Ensure all development libraries are installed. The easiest way is to run
`sudo ./scripts/setup-dev.sh`, which installs every required package for
your distribution. If you prefer manual installation, double-check the
package names in the Prerequisites section above.

### `clang-format` version mismatch

The `.clang-format` file targets clang-format 14+. If your distribution
ships an older version, the pre-commit hook may produce different output.
Install a newer version:

```bash
# Ubuntu/Debian
sudo apt install clang-format-14
sudo update-alternatives --install /usr/bin/clang-format clang-format \
  /usr/bin/clang-format-14 100
```

### Pre-commit hook blocks commit

The hook checks two things: formatting and SPDX headers. Fix them:

```bash
# Fix formatting
make format
git add -u

# Fix SPDX headers — add these two lines at the top of every .c/.h file:
# // SPDX-FileCopyrightText: 2026 shellkeep contributors
# // SPDX-License-Identifier: GPL-3.0-or-later
```

### Docker permission denied

If `docker` commands fail with "permission denied", your user may not be in
the `docker` group:

```bash
sudo usermod -aG docker "$USER"
# Log out and back in, or run: newgrp docker
```

### Build fails with sanitizer errors

Debug builds enable AddressSanitizer and UndefinedBehaviorSanitizer by
default. If you hit false positives with an older compiler, try a release
build:

```bash
cmake -B build-release -G Ninja -DCMAKE_BUILD_TYPE=Release -DSK_BUILD_TESTS=ON
cmake --build build-release
```

### Tests fail with "connection refused"

Integration tests need a running Docker container with sshd + tmux. Make
sure Docker is running and you have started the test container:

```bash
sudo systemctl start docker
ctest --test-dir build --label-regex unit --output-on-failure   # unit tests only
```

## Code of Conduct

All participants in the shellkeep community are expected to follow the
[Code of Conduct](CODE_OF_CONDUCT.md).
