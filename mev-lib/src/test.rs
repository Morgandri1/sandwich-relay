use std::str::FromStr;

use solana_client::rpc_client::RpcClient;
use solana_client::rpc_config::RpcTransactionConfig;
use solana_sdk::{
    message::VersionedMessage, 
    signature::Keypair, 
    signer::Signer, 
    system_transaction::transfer, 
    transaction::VersionedTransaction,
    signature::Signature
};
use solana_transaction_status::UiTransactionEncoding;

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
    assert!(!is_relevant_tx(&vtx))
}

#[test]
fn should_serialize_roundtrip() {
    
}

#[test]
fn should_fetch_and_deserialize_tx() {
    let hash = "66o9Dk7c1wh81CvNnABzcy4MJUZ6JKdbZqJSy9sM7go2QgithsbGjWWoqGDMR2nGuGcnFBbESzdSfWyz4ZDTMmTn";
    let client = RpcClient::new("https://api.mainnet-beta.solana.com");
    let sig = Signature::from_str(hash).unwrap();
    let tx = client.get_transaction_with_config(&sig, RpcTransactionConfig {
        encoding: Some(UiTransactionEncoding::Binary),
        commitment: None,
        max_supported_transaction_version: Some(0u8)
    }).expect("Failed to fetch transaction");
    let instruction = tx.transaction.transaction.decode().unwrap();
    eprintln!("{:?}", &instruction.message.static_account_keys());
    for ix in instruction.message.instructions() {
        eprintln!("{:?}", &ix.data);
        eprintln!("{:?}", &ix.accounts);
        eprintln!("{:?}", &ix.program_id_index);
    }
    panic!()
}