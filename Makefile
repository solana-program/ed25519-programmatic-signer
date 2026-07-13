RUST_TOOLCHAIN_NIGHTLY = nightly-2026-01-22
SOLANA_CLI_VERSION = 3.1.8

nightly = +${RUST_TOOLCHAIN_NIGHTLY}

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
	cargo $(nightly) clippy --package $* \
	  --all-targets \
	  --all-features \
		-- \
		--deny=warnings \
		--deny=clippy::default_trait_access \
		--deny=clippy::arithmetic_side_effects \
		--deny=clippy::manual_let_else \
		--deny=clippy::used_underscore_binding $(ARGS)

format-check-%:
	cargo $(nightly) fmt --check --package $* $(ARGS)

powerset-%:
	cargo $(nightly) hack check --feature-powerset --all-targets --package $* $(ARGS)

format-rust:
	cargo $(nightly) fmt --all $(ARGS)

build-sbf-%:
	cargo build-sbf $(ARGS) -- --package $*

build-doc-%:
	RUSTDOCFLAGS="--cfg docsrs -D warnings" cargo $(nightly) doc --all-features --no-deps --package $* $(ARGS)

test-doc-%:
	cargo $(nightly) test --doc --all-features --package $* $(ARGS)

test-%:
	SBF_OUT_DIR=$(PWD)/target/deploy cargo $(nightly) test --package $* $(ARGS)

generate-clients:
	@echo "No clients to generate yet"

check-no-std-core-%:
	cargo $(nightly) hack check \
		--target bpfel-unknown-none \
		--each-feature \
		--package $* \
		-Zbuild-std=core \
		$(ARGS)

check-no-std-alloc-%:
	cargo $(nightly) hack check \
		--target bpfel-unknown-none \
		--each-feature \
		--package $* \
		-Zbuild-std=alloc,core \
		$(ARGS)
