<!--
SPDX-FileCopyrightText: 2026 shellkeep contributors
SPDX-License-Identifier: GPL-3.0-or-later
-->

# Security Policy

## Supported Versions

| Version | Supported |
|---|---|
| Latest release | Yes |
| Previous release | Security fixes only |
| Older releases | No |

## Reporting a Vulnerability

**Do NOT report security vulnerabilities through public GitHub issues.**

If you discover a security vulnerability in shellkeep, please report it
responsibly by emailing:

**security@shellkeep.org**

### What to include

- Description of the vulnerability
- Steps to reproduce (if possible)
- Potential impact
- Suggested fix (if you have one)

### Response timeline

- **Acknowledgment:** within 48 hours of receipt
- **Initial assessment:** within 7 days
- **Fix target:** within 90 days of confirmed vulnerability
- **Disclosure:** coordinated with the reporter after fix is released

### What to expect

1. You will receive an acknowledgment email within 48 hours confirming
   receipt of your report.
2. A maintainer will assess the severity and validity of the report.
3. We will work on a fix and coordinate a release timeline with you.
4. Once a fix is released, we will publicly disclose the vulnerability
   with credit to you (unless you prefer to remain anonymous).

## Security Design Principles

shellkeep is designed with security as a core concern:

- **Host key verification** is mandatory before any connection operation
- **Explicit algorithm lists** for ciphers, MACs, and key exchange
  (no "accept anything libssh supports")
- **No password storage** -- passwords are never written to disk
- **File permissions** -- all state files are `0600`, directories `0700`,
  verified and corrected at startup
- **No sensitive data in logs** -- terminal content, passwords, keys,
  environment variables, and clipboard content are never logged
- **No telemetry** -- crash reporting is local-only
- **Core dumps disabled** via `prctl(PR_SET_DUMPABLE, 0)`
- **Memory safety** -- `mlock()` for cryptographic material,
  `explicit_bzero()` after use
- **Input sanitization** -- client-id, session names, and UUIDs are
  validated before use in file paths or commands

## Scope

The following are considered security vulnerabilities:

- Authentication bypass or credential exposure
- Host key verification bypass
- Sensitive data appearing in logs or crash dumps
- Path traversal via session names, client-id, or UUIDs
- Remote code execution
- Privilege escalation
- Denial of service via malformed state files
- Use of deprecated or weak cryptographic algorithms

The following are NOT considered security vulnerabilities:

- Bugs that require local root access to exploit
- Issues in third-party dependencies (report those upstream)
- Social engineering attacks
- Physical access attacks

## PGP Key

A PGP key for encrypted vulnerability reports will be published at
[shellkeep.org/.well-known/security.txt](https://shellkeep.org/.well-known/security.txt)
once the project reaches its first stable release.
