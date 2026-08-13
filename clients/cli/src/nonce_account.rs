use {
    crate::runtime::rpc::RpcAccount,
    anyhow::{Result, bail},
    spl_nonce_interface::state::Nonce,
    spl_programmatic_signer_rust::nonce::decode,
};

pub(crate) fn decode_nonce_account(account: &RpcAccount) -> Result<Nonce> {
    if account.owner != spl_nonce_interface::id() {
        bail!(
            "account owner {} is not the SPL Nonce program {}",
            account.owner,
            spl_nonce_interface::id()
        );
    }
    Ok(decode(&account.data)?)
}
