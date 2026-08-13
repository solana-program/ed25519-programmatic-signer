use {
    solana_address::Address,
    solana_hash::Hash,
    spl_nonce_interface::state::Nonce,
    spl_programmatic_signer_rust::{
        SignOnlyTransaction, inspect, transaction_from_sign_only_checked, verify::verify,
    },
};

#[test]
fn parses_agave_solana_transfer_sign_only_fixture() {
    assert_sign_only_fixture(
        include_str!("fixtures/agave_solana_transfer_sign_only.json"),
        1,
    );
}

#[test]
fn parses_spl_token_transfer_sign_only_fixture() {
    assert_sign_only_fixture(
        include_str!("fixtures/spl_token_transfer_sign_only.json"),
        2,
    );
}

#[test]
fn parses_spl_token_multisig_transfer_sign_only_fixture() {
    assert_sign_only_fixture(
        include_str!("fixtures/spl_token_multisig_transfer_sign_only.json"),
        3,
    );
}

fn assert_sign_only_fixture(json: &str, expected_required_signatures: u8) {
    let sign_only = SignOnlyTransaction::from_json(json).expect("parse sign-only json");

    assert!(sign_only.bad_sig.is_empty());
    assert_eq!(
        sign_only.absent.len(),
        usize::from(expected_required_signatures)
    );

    let message = sign_only
        .message()
        .expect("decode dumped transaction message");
    assert_eq!(message.recent_blockhash().to_string(), sign_only.blockhash);
    assert_eq!(
        message.header().num_required_signatures,
        expected_required_signatures
    );

    let submit_signers = sign_only
        .absent
        .iter()
        .map(|signer| signer.parse::<Address>().expect("parse absent signer"))
        .collect::<Vec<_>>();
    let nonce_account = Address::new_unique();
    let nonce = Nonce {
        nonce: *message.recent_blockhash(),
        authority: submit_signers[0],
    };
    let transaction = transaction_from_sign_only_checked(
        &sign_only,
        nonce_account,
        &nonce,
        &[Address::new_unique()],
        &submit_signers,
        genesis_hash(),
    )
    .expect("build transaction from sign-only fixture");

    verify(&transaction, &nonce, &nonce_account, &genesis_hash())
        .expect("verify imported transaction");
    let summary = inspect(&transaction).expect("inspect imported transaction");
    assert_eq!(summary.genesis_hash, genesis_hash());
    assert_eq!(summary.nonce_account, nonce_account);
    assert_eq!(summary.inner_required_signers, submit_signers);
}

fn genesis_hash() -> Hash {
    Hash::new_from_array([250; 32])
}
