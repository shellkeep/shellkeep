# SPDX-FileCopyrightText: 2026 shellkeep contributors
# SPDX-License-Identifier: GPL-3.0-or-later
#
# Makefile — Convenience wrapper around CMake for common tasks.

BUILDDIR  ?= build
BUILDTYPE ?= Debug

.PHONY: build test lint clean format check setup

# --------------------------------------------------------------------------- #
# Primary targets                                                              #
# --------------------------------------------------------------------------- #

build: setup ## Build the project
	cmake --build $(BUILDDIR)

test: build ## Run all tests
	ctest --test-dir $(BUILDDIR) --output-on-failure

lint: ## Run static analysis (cppcheck + clang-format check)
	cppcheck --enable=warning,style,performance,portability \
		-I include/ --error-exitcode=1 src/
	@echo ""
	@echo "--- clang-format check ---"
	find src/ include/ tests/ -name '*.c' -o -name '*.h' -o -name '*.cpp' | \
		xargs clang-format --dry-run --Werror

clean: ## Remove build directory
	rm -rf $(BUILDDIR)

format: ## Auto-format all source files
	find src/ include/ tests/ -name '*.c' -o -name '*.h' -o -name '*.cpp' | \
		xargs clang-format -i
	@echo "All source files formatted."

check: lint test ## Run lint + test

# --------------------------------------------------------------------------- #
# Internal / setup                                                             #
# --------------------------------------------------------------------------- #

setup: ## Configure CMake build (idempotent)
	@if [ ! -f $(BUILDDIR)/build.ninja ]; then \
		cmake -B $(BUILDDIR) -G Ninja \
			-DCMAKE_BUILD_TYPE=$(BUILDTYPE) \
			-DSK_BUILD_TESTS=ON \
			-DSK_BUILD_QT_UI=ON; \
	fi

# --------------------------------------------------------------------------- #
# Help                                                                         #
# --------------------------------------------------------------------------- #

help: ## Show this help
	@grep -E '^[a-zA-Z_-]+:.*##' $(MAKEFILE_LIST) | \
		awk 'BEGIN {FS = ":.*## "}; {printf "  \033[36m%-12s\033[0m %s\n", $$1, $$2}'
