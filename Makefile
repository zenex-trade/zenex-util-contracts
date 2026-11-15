.PHONY: default test build release fmt clean

# The CLI is pinned to the version the release workflow builds with
# (.github/workflows/release.yml), which CI installs too. The `stellar-cli`
# target checks that the `stellar` on PATH is that version, so a different
# install fails the build instead of emitting different artifacts.
STELLAR_CLI_VERSION ?= 27.0.0
# Exported, not spliced into the recipe, so the value never reaches the shell
# as text.
export STELLAR_CLI_VERSION

default: build

# Fails unless the `stellar` on PATH is the pinned version.
.PHONY: stellar-cli
stellar-cli:
	@stellar --version | grep -q "^stellar $$STELLAR_CLI_VERSION " || \
		{ echo "stellar-cli $$STELLAR_CLI_VERSION is required, found: $$(stellar --version | head -1)" >&2; exit 1; }

# `--tests` selects every target with `test = true`, the in-crate unit suites
# included. `--lib` is named alongside it so the intent reads explicitly. The
# tests need no built WASM: the only WASM they load is committed testdata.
test:
	cargo test --locked --all --lib --tests

build: stellar-cli
	stellar contract build --locked --optimize

# Rebuilds the release WASMs into `wasm/`, which git ignores, the way the
# release workflow builds them: the same Stellar CLI, the pinned Rust toolchain
# and the same contract meta. At a release tag the output must equal the
# published release assets byte for byte. A changed or untracked file could
# reach the build, so the target fails unless `git status` is clean. The two
# cargo variables hold the workspace's release profile against a strip or debug
# setting in a user-level cargo config, which would change every hash.
RELEASE_REPO ?= github:zenex-trade/zenex-util-contracts
RELEASE_HOME_DOMAIN ?= app.zenex.trade
release: stellar-cli
	@test -z "$$(git status --porcelain)" || { echo "release needs a clean checkout; commit, stash or remove changed and untracked files" >&2; exit 1; }
	rm -f wasm/*.wasm
	CARGO_PROFILE_RELEASE_STRIP=symbols CARGO_PROFILE_RELEASE_DEBUG=0 \
		stellar contract build --locked --optimize --out-dir wasm \
		--meta source_repo=$(RELEASE_REPO) --meta home_domain=$(RELEASE_HOME_DOMAIN)

fmt:
	cargo fmt --all

clean:
	cargo clean
