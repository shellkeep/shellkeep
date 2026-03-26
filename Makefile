# SPDX-FileCopyrightText: 2026 shellkeep contributors
# SPDX-License-Identifier: GPL-3.0-or-later
#
# Makefile — Convenience wrapper around Meson for common tasks.

BUILDDIR  ?= build
BUILDTYPE ?= debug

.PHONY: build test lint clean format check setup

# --------------------------------------------------------------------------- #
# Primary targets                                                              #
# --------------------------------------------------------------------------- #

build: setup ## Build the project
	meson compile -C $(BUILDDIR)

test: build ## Run all tests
	meson test -C $(BUILDDIR) --print-errorlogs

lint: ## Run static analysis (cppcheck + clang-tidy)
	cppcheck --enable=warning,style,performance,portability \
		-I include/ --error-exitcode=1 src/
	@echo ""
	@echo "--- clang-format check ---"
	find src/ include/ tests/ -name '*.c' -o -name '*.h' | \
		xargs clang-format --dry-run --Werror

clean: ## Remove build directory
	rm -rf $(BUILDDIR)

format: ## Auto-format all source files
	find src/ include/ tests/ -name '*.c' -o -name '*.h' | \
		xargs clang-format -i
	@echo "All source files formatted."

check: lint test ## Run lint + test

# --------------------------------------------------------------------------- #
# Internal / setup                                                             #
# --------------------------------------------------------------------------- #

setup: ## Configure Meson build (idempotent)
	@if [ ! -f $(BUILDDIR)/build.ninja ]; then \
		meson setup $(BUILDDIR) --buildtype=$(BUILDTYPE) -Dtests=true; \
	fi

# --------------------------------------------------------------------------- #
# Help                                                                         #
# --------------------------------------------------------------------------- #

help: ## Show this help
	@grep -E '^[a-zA-Z_-]+:.*##' $(MAKEFILE_LIST) | \
		awk 'BEGIN {FS = ":.*## "}; {printf "  \033[36m%-12s\033[0m %s\n", $$1, $$2}'
