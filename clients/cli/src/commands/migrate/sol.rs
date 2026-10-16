use {
    super::{RequiredSigners, execution_message, new_owner_signer},
    crate::{client::Client, output::OutputFormat},
    anyhow::{Result, ensure},
    base64::{Engine, prelude::BASE64_STANDARD},
    clap::Args,
    serde::{Deserialize, Serialize},
    solana_address::Address,
    solana_native_token::{Sol, sol_str_to_lamports},
    solana_system_interface::instruction::transfer,
    spl_memo_interface::instruction::build_memo,
    std::fmt,
};

#[derive(Debug, Args)]
pub(crate) struct SolCommand {
    /// Current owner of the SOL, a System Program account with no data.
    /// If transferring the full balance, it must not also pay the
    /// relay transaction's fee, since that comes out of the same balance.
    #[clap(long)]
    owner: Address,

    /// Authority of the new owner. The SOL moves to its programmatic signer. Pass the authority
    /// itself, not its programmatic signer.
    #[clap(long)]
    new_owner: Address,

    /// Amount to move, in SOL. Moves the whole balance when omitted, as read when the message is
    /// built, which closes the owner's account. Moving part of it must leave the owner rent
    /// exempt.
    #[clap(long, value_parser = parse_sol)]
    amount: Option<u64>,
}

fn parse_sol(amount: &str) -> Result<u64, String> {
    sol_str_to_lamports(amount).ok_or_else(|| format!("invalid amount {amount}"))
}

pub(super) async fn run(
    command: SolCommand,
    client: &Client,
    output: OutputFormat,
) -> Result<String> {
    let new_owner_signer = new_owner_signer(&command.new_owner)?;
    ensure!(
        command.owner != new_owner_signer,
        "owner {} is already the new owner's programmatic signer",
        command.owner
    );
    let balance = client.system_account(&command.owner).await?.lamports;
    // Accounts without data, like both of these.
    let rent = client.minimum_balance_for_rent_exemption(0).await?;

    let lamports = command.amount.unwrap_or(balance);
    ensure!(lamports > 0, "amount must be greater than zero");
    // The runtime rejects leaving an account with a balance below the rent-exempt minimum.
    if lamports != balance {
        let max_partial = balance.saturating_sub(rent);
        ensure!(
            lamports <= max_partial,
            "account {} holds {}. Move all of it, or at most {} to leave it rent exempt",
            command.owner,
            Sol(balance),
            Sol(max_partial)
        );
    }
    let new_owner_balance = client
        .optional_account(&new_owner_signer)
        .await?
        .map_or(0, |account| account.lamports);
    ensure!(
        new_owner_balance.saturating_add(lamports) >= rent,
        "the new owner's programmatic signer {new_owner_signer} must end up rent exempt, so move \
         at least {}",
        Sol(rent.saturating_sub(new_owner_balance))
    );

    let instructions = [
        transfer(&command.owner, &new_owner_signer, lamports),
        // A transfer doesn't need the new owner's approval, but requiring it confirms the new
        // owner can sign before any SOL moves.
        build_memo(
            &spl_memo_interface::v4::id(),
            b"migrate sol",
            &[&new_owner_signer],
        ),
    ];
    let execution_message = execution_message(&new_owner_signer, &instructions)?;

    output.render(&SolMigrationOutput {
        owner: command.owner.to_string(),
        lamports,
        closes_account: lamports == balance,
        new_owner: new_owner_signer.to_string(),
        new_owner_authority: command.new_owner.to_string(),
        required_signers: RequiredSigners::new(&execution_message, &[command.new_owner]),
        execution_message: BASE64_STANDARD.encode(execution_message.serialize()),
    })
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SolMigrationOutput {
    pub owner: String,
    pub lamports: u64,
    /// Whether the migration moves the owner's whole balance, closing its account.
    pub closes_account: bool,
    pub new_owner: String,
    pub new_owner_authority: String,
    #[serde(flatten)]
    pub required_signers: RequiredSigners,
    pub execution_message: String,
}

impl fmt::Display for SolMigrationOutput {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f, "From: {}", self.owner)?;
        if self.closes_account {
            writeln!(
                f,
                "Amount: {} (the whole balance, closing the account)",
                Sol(self.lamports)
            )?;
        } else {
            writeln!(f, "Amount: {}", Sol(self.lamports))?;
        }
        writeln!(
            f,
            "New owner: {} (authority: {})",
            self.new_owner, self.new_owner_authority
        )?;
        writeln!(f)?;
        writeln!(f, "{}", self.required_signers)?;
        writeln!(f)?;
        write!(f, "Execution message (base64):\n{}", self.execution_message)
    }
}
