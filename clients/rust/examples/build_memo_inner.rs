use {
    base64::{Engine as _, engine::general_purpose::STANDARD},
    serde_json::json,
    solana_address::Address,
    solana_hash::Hash,
    solana_message::{
        VersionedMessage, compiled_instruction::CompiledInstruction, legacy::Message,
    },
    std::{env, process},
};

const MEMO_PROGRAM: &str = "MemoSq4gqABAXKb96qnH8TysNcWxMyWCqXgDLGmfcHr";

fn main() {
    if let Err(error) = run() {
        eprintln!("{error}");
        process::exit(1);
    }
}

fn run() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = env::args().skip(1);
    let Some(first) = args.next() else {
        return Err("usage: build_memo_inner <PDA> <NONCE> <MEMO>".into());
    };
    if first == "-h" || first == "--help" {
        println!("usage: build_memo_inner <PDA> <NONCE> <MEMO>");
        return Ok(());
    }

    let pda = first.parse::<Address>()?;
    let nonce = args
        .next()
        .ok_or("usage: build_memo_inner <PDA> <NONCE> <MEMO>")?
        .parse::<Hash>()?;
    let memo = args.collect::<Vec<_>>().join(" ");
    if memo.is_empty() {
        return Err("usage: build_memo_inner <PDA> <NONCE> <MEMO>".into());
    }

    let memo_program = MEMO_PROGRAM.parse::<Address>()?;
    let message = VersionedMessage::Legacy(Message::new_with_compiled_instructions(
        1, // PDA is the only required signer.
        0, // The PDA signer is fee-payer-style writable.
        1, // The memo program is read-only.
        vec![pda, memo_program],
        nonce,
        vec![CompiledInstruction::new_from_raw_parts(
            1,
            memo.into_bytes(),
            vec![0],
        )],
    ));
    let message = STANDARD.encode(message.serialize());
    println!(
        "{}",
        serde_json::to_string(&json!({
            "blockhash": nonce.to_string(),
            "message": message,
            "absent": [pda.to_string()],
        }))?
    );

    Ok(())
}
