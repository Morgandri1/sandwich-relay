use solana_sdk::{
    message::VersionedMessage, 
    pubkey, 
    signature::Keypair, 
    signer::Signer, 
    system_transaction::transfer, 
    transaction::VersionedTransaction
};

use crate::comp::is_relevant_tx;

#[test]
fn should_recognize_raydium_swap() {
    todo!() // need raydium swap logic from ycry
}

#[test]
fn should_not_recognize_transfer() {
    let a = Keypair::new();
    let b = Keypair::new();
    let tx = transfer(&a, &b.try_pubkey().unwrap(), 10000, [0;32].into());
    let vtx = VersionedTransaction {
        message: VersionedMessage::Legacy(tx.message),
        signatures: tx.signatures
    };
    assert!(!is_relevant_tx(&vtx, &[pubkey!("675kPX9MHTjS2zt1qfr1NYHuzeLXfQM9H24wFSUt1Mp8")]))
}