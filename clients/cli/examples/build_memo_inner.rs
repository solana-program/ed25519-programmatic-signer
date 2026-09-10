//! Build an instruction and export the same message dump as `--sign-only`.
use {
    anyhow::Result,
    base64::{Engine as _, engine::general_purpose::STANDARD},
    clap::Parser,
    serde_json::json,
    solana_address::Address,
    solana_hash::Hash,
    solana_instruction::{AccountMeta, Instruction},
    solana_message::legacy::Message,
};

#[derive(Parser)]
struct Args {
    pda: Address,
    nonce: Hash,
    memo: String,
}

fn main() -> Result<()> {
    let args = Args::parse();
    let instruction = Instruction {
        program_id: "MemoSq4gqABAXKb96qnH8TysNcWxMyWCqXgDLGmfcHr".parse()?,
        accounts: vec![AccountMeta::new_readonly(args.pda, true)],
        data: args.memo.into_bytes(),
    };
    let message = Message::new_with_blockhash(&[instruction], Some(&args.pda), &args.nonce);
    println!(
        "{}",
        json!({
            "blockhash": args.nonce.to_string(),
            "message": STANDARD.encode(message.serialize()),
            "absent": [args.pda.to_string()],
        })
    );
    Ok(())
}
