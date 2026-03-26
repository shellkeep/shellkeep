#!/usr/bin/env bash
# SPDX-FileCopyrightText: 2026 shellkeep contributors
# SPDX-License-Identifier: GPL-3.0-or-later
#
# setup-dev.sh — Install all build dependencies for shellkeep development.
# Supports: Ubuntu 22.04/24.04, Debian 12, Fedora 40, Arch Linux.

set -euo pipefail

readonly SCRIPT_NAME="$(basename "$0")"

# --------------------------------------------------------------------------- #
# Helpers                                                                      #
# --------------------------------------------------------------------------- #

info()  { printf '\033[1;34m[info]\033[0m  %s\n' "$*"; }
ok()    { printf '\033[1;32m[ok]\033[0m    %s\n' "$*"; }
warn()  { printf '\033[1;33m[warn]\033[0m  %s\n' "$*"; }
die()   { printf '\033[1;31m[error]\033[0m %s\n' "$*" >&2; exit 1; }

need_root() {
  if [[ $EUID -ne 0 ]]; then
    die "This script must be run as root (or via sudo)."
  fi
}

# --------------------------------------------------------------------------- #
# Distro detection                                                             #
# --------------------------------------------------------------------------- #

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

# --------------------------------------------------------------------------- #
# Package lists                                                                #
# --------------------------------------------------------------------------- #

APT_PACKAGES=(
  build-essential
  meson
  ninja-build
  pkg-config
  libgtk-3-dev
  libvte-2.91-dev
  libssh-dev
  libayatana-appindicator3-dev
  libjson-glib-dev
  libcmocka-dev
  clang-format
  clang-tidy
  cppcheck
  gcovr
  lcov
  git
  curl
)

DNF_PACKAGES=(
  gcc
  meson
  ninja-build
  pkgconf-pkg-config
  gtk3-devel
  vte291-devel
  libssh-devel
  libayatana-appindicator-gtk3-devel
  json-glib-devel
  libcmocka-devel
  clang-tools-extra
  cppcheck
  gcovr
  lcov
  git
  curl
)

PACMAN_PACKAGES=(
  base-devel
  meson
  ninja
  pkgconf
  gtk3
  vte3
  libssh
  libayatana-appindicator
  json-glib
  cmocka
  clang
  cppcheck
  gcovr
  lcov
  git
  curl
)

# --------------------------------------------------------------------------- #
# Install functions                                                            #
# --------------------------------------------------------------------------- #

install_apt() {
  info "Updating package index..."
  apt-get update -qq
  info "Installing build dependencies via apt..."
  apt-get install -y --no-install-recommends "${APT_PACKAGES[@]}"
  ok "APT packages installed."
}

install_dnf() {
  info "Installing build dependencies via dnf..."
  dnf install -y "${DNF_PACKAGES[@]}"
  ok "DNF packages installed."
}

install_pacman() {
  info "Installing build dependencies via pacman..."
  pacman -Syu --noconfirm --needed "${PACMAN_PACKAGES[@]}"
  ok "Pacman packages installed."
}

# --------------------------------------------------------------------------- #
# Docker setup                                                                 #
# --------------------------------------------------------------------------- #

setup_docker() {
  if command -v docker &>/dev/null; then
    ok "Docker is already installed ($(docker --version))."
  else
    info "Docker not found — installing..."
    case "${DISTRO_ID}" in
      ubuntu|debian)
        apt-get install -y --no-install-recommends \
          ca-certificates gnupg lsb-release
        install -m 0755 -d /etc/apt/keyrings
        curl -fsSL "https://download.docker.com/linux/${DISTRO_ID}/gpg" \
          | gpg --dearmor -o /etc/apt/keyrings/docker.gpg
        chmod a+r /etc/apt/keyrings/docker.gpg
        echo \
          "deb [arch=$(dpkg --print-architecture) signed-by=/etc/apt/keyrings/docker.gpg] \
          https://download.docker.com/linux/${DISTRO_ID} \
          $(lsb_release -cs) stable" \
          > /etc/apt/sources.list.d/docker.list
        apt-get update -qq
        apt-get install -y --no-install-recommends \
          docker-ce docker-ce-cli containerd.io docker-buildx-plugin
        ;;
      fedora)
        dnf install -y dnf-plugins-core
        dnf config-manager --add-repo \
          https://download.docker.com/linux/fedora/docker-ce.repo
        dnf install -y docker-ce docker-ce-cli containerd.io docker-buildx-plugin
        ;;
      arch)
        pacman -S --noconfirm --needed docker
        ;;
      *)
        warn "Unsupported distro for automatic Docker install — please install manually."
        return
        ;;
    esac
    ok "Docker installed."
  fi

  # Ensure the Docker daemon is running.
  if systemctl is-active --quiet docker 2>/dev/null; then
    ok "Docker daemon is running."
  else
    info "Starting Docker daemon..."
    systemctl enable --now docker 2>/dev/null || true
    ok "Docker daemon started."
  fi

  # Add the calling user to the docker group (if not root).
  if [[ -n "${SUDO_USER:-}" ]]; then
    if id -nG "${SUDO_USER}" | grep -qw docker; then
      ok "User '${SUDO_USER}' is already in the docker group."
    else
      info "Adding '${SUDO_USER}' to the docker group..."
      usermod -aG docker "${SUDO_USER}"
      ok "User '${SUDO_USER}' added to docker group (log out/in to take effect)."
    fi
  fi
}

# --------------------------------------------------------------------------- #
# Git hooks                                                                    #
# --------------------------------------------------------------------------- #

setup_hooks() {
  local repo_root
  repo_root="$(cd "$(dirname "$0")/.." && pwd)"
  if [[ -x "${repo_root}/scripts/install-hooks.sh" ]]; then
    info "Installing git hooks..."
    bash "${repo_root}/scripts/install-hooks.sh"
    ok "Git hooks installed."
  fi
}

# --------------------------------------------------------------------------- #
# Main                                                                         #
# --------------------------------------------------------------------------- #

main() {
  info "shellkeep developer setup — ${SCRIPT_NAME}"
  need_root
  detect_distro

  case "${DISTRO_ID}" in
    ubuntu)
      case "${DISTRO_VERSION}" in
        22.04|24.04) install_apt ;;
        *) die "Unsupported Ubuntu version: ${DISTRO_VERSION}. Supported: 22.04, 24.04." ;;
      esac
      ;;
    debian)
      case "${DISTRO_VERSION}" in
        12) install_apt ;;
        *) die "Unsupported Debian version: ${DISTRO_VERSION}. Supported: 12." ;;
      esac
      ;;
    fedora)
      case "${DISTRO_VERSION}" in
        40) install_dnf ;;
        *) die "Unsupported Fedora version: ${DISTRO_VERSION}. Supported: 40." ;;
      esac
      ;;
    arch)
      install_pacman
      ;;
    *)
      die "Unsupported distribution: ${DISTRO_ID}. Supported: Ubuntu 22.04/24.04, Debian 12, Fedora 40, Arch."
      ;;
  esac

  setup_docker
  setup_hooks

  echo ""
  ok "Development environment ready!"
  info "Next steps:"
  info "  meson setup build --buildtype=debug -Dtests=true"
  info "  meson compile -C build"
  info "  meson test -C build"
}

main "$@"
