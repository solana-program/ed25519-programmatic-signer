RUST_TOOLCHAIN_NIGHTLY = nightly-2026-01-22
SOLANA_CLI_VERSION = 3.1.8
SBF_ERROR_PATTERN = 'Error: Function|Stack offset|overwrites values in the frame'

nightly = +${RUST_TOOLCHAIN_NIGHTLY}

# This is a bit tricky -- findstring returns the found string, so we're looking
# for "directory-", returning that, and replacing "-" with "/" to change the
# first "-" to a "/". But if it isn't found, we replace "" with "", which works
# in the case where there is no subdirectory.
pattern-dir = $(firstword $(subst -, ,$1))
find-pattern-dir = $(findstring $(call pattern-dir,$1)-,$1)
make-path = $(subst $(call find-pattern-dir,$1),$(subst -,/,$(call find-pattern-dir,$1)),$1)

rust-toolchain-nightly:
	@echo ${RUST_TOOLCHAIN_NIGHTLY}

solana-cli-version:
	@echo ${SOLANA_CLI_VERSION}

cargo-nightly:
	cargo $(nightly) $(ARGS)

audit:
	cargo audit \
		--ignore RUSTSEC-2022-0093 \
		--ignore RUSTSEC-2024-0344 \
		$(ARGS)

spellcheck:
	cargo spellcheck --code 1 $(ARGS)

clippy-%:
	cargo $(nightly) clippy --manifest-path $(call make-path,$*)/Cargo.toml \
	  --all-targets \
	  --all-features \
		-- \
		--deny=warnings \
		--deny=clippy::default_trait_access \
		--deny=clippy::arithmetic_side_effects \
		--deny=clippy::manual_let_else \
		--deny=clippy::used_underscore_binding $(ARGS)

format-check-%:
	cargo $(nightly) fmt --check --manifest-path $(call make-path,$*)/Cargo.toml $(ARGS)

powerset-%:
	cargo $(nightly) hack check --feature-powerset --all-targets --manifest-path $(call make-path,$*)/Cargo.toml $(ARGS)

format-rust:
	cargo $(nightly) fmt --all $(ARGS)

build-sbf-%:
	@mkdir -p target
	@bash -o pipefail -c 'cargo build-sbf --manifest-path $(call make-path,$*)/Cargo.toml $(ARGS) 2>&1 | tee target/build-sbf-$*.log'
	@! grep -E $(SBF_ERROR_PATTERN) target/build-sbf-$*.log

build-sbf-program-falcon:
	@mkdir -p target
	@bash -o pipefail -c 'cargo build-sbf --manifest-path program/Cargo.toml -- --features falcon $(ARGS) 2>&1 | tee target/build-sbf-program-falcon.log'
	@! grep -E $(SBF_ERROR_PATTERN) target/build-sbf-program-falcon.log

build-doc-%:
	RUSTDOCFLAGS="--cfg docsrs -D warnings" cargo $(nightly) doc --all-features --no-deps --manifest-path $(call make-path,$*)/Cargo.toml $(ARGS)

test-doc-%:
	cargo $(nightly) test --doc --all-features --manifest-path $(call make-path,$*)/Cargo.toml $(ARGS)

test-%:
	SBF_OUT_DIR=$(PWD)/target/deploy cargo $(nightly) test --manifest-path $(call make-path,$*)/Cargo.toml $(ARGS)

test-program-falcon:
	SBF_OUT_DIR=$(PWD)/target/deploy cargo $(nightly) test --manifest-path program/Cargo.toml --features falcon --test test_falcon_submit $(ARGS)

generate-clients:
	@echo "No clients to generate yet"

check-no-std-core-%:
	cargo $(nightly) hack check \
		--target bpfel-unknown-none \
		--each-feature \
		--manifest-path $(call make-path,$*)/Cargo.toml \
		-Zbuild-std=core \
		$(ARGS)

check-no-std-alloc-%:
	cargo $(nightly) hack check \
		--target bpfel-unknown-none \
		--each-feature \
		--manifest-path $(call make-path,$*)/Cargo.toml \
		-Zbuild-std=alloc,core \
		$(ARGS)
