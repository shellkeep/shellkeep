#!/usr/bin/env bash
# SPDX-FileCopyrightText: 2026 shellkeep contributors
# SPDX-License-Identifier: GPL-3.0-or-later
#
# setup-dev.sh — Install all build dependencies for shellkeep development.
# Supports: Ubuntu 22.04/24.04, Debian 12, Fedora 40+, Arch Linux.

set -euo pipefail

readonly SCRIPT_NAME="$(basename "$0")"

info()  { printf '\033[1;34m[info]\033[0m  %s\n' "$*"; }
ok()    { printf '\033[1;32m[ok]\033[0m    %s\n' "$*"; }
warn()  { printf '\033[1;33m[warn]\033[0m  %s\n' "$*"; }
die()   { printf '\033[1;31m[error]\033[0m %s\n' "$*" >&2; exit 1; }

detect_distro() {
  if [[ ! -f /etc/os-release ]]; then
    die "Cannot detect distribution — /etc/os-release not found."
  fi
  # shellcheck disable=SC1091
  . /etc/os-release
  DISTRO_ID="${ID:-unknown}"
  DISTRO_VERSION="${VERSION_ID:-unknown}"
  info "Detected: ${PRETTY_NAME:-${DISTRO_ID} ${DISTRO_VERSION}}"
}

# System packages needed for iced (GPU UI framework) and linking
APT_PACKAGES=(
  libxkbcommon-dev
  libwayland-dev
  libvulkan-dev
  libfontconfig1-dev
  git
  curl
  pkg-config
)

DNF_PACKAGES=(
  libxkbcommon-devel
  wayland-devel
  vulkan-loader-devel
  fontconfig-devel
  git
  curl
  pkgconf-pkg-config
)

PACMAN_PACKAGES=(
  libxkbcommon
  wayland
  vulkan-icd-loader
  fontconfig
  git
  curl
  pkgconf
)

install_rust() {
  if command -v rustc &>/dev/null; then
    ok "Rust already installed ($(rustc --version))."
  else
    info "Installing Rust via rustup..."
    curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y
    # shellcheck disable=SC1091
    source "$HOME/.cargo/env"
    ok "Rust installed ($(rustc --version))."
  fi
}

main() {
  info "shellkeep developer setup — ${SCRIPT_NAME}"
  detect_distro

  case "${DISTRO_ID}" in
    ubuntu|debian)
      sudo apt-get update -qq
      sudo apt-get install -y --no-install-recommends "${APT_PACKAGES[@]}"
      ;;
    fedora)
      sudo dnf install -y "${DNF_PACKAGES[@]}"
      ;;
    arch)
      sudo pacman -Syu --noconfirm --needed "${PACMAN_PACKAGES[@]}"
      ;;
    *)
      warn "Unsupported distribution: ${DISTRO_ID}. Install deps manually."
      ;;
  esac

  install_rust

  echo ""
  ok "Development environment ready!"
  info "Next steps:"
  info "  cargo build --release"
  info "  cargo test"
  info "  cargo clippy -- -D warnings"
}

main "$@"
