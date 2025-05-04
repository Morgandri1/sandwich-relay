use std::io::Read;
use std::str::FromStr;

use solana_client::rpc_client::RpcClient;
use solana_client::rpc_config::RpcTransactionConfig;
use solana_sdk::{
    message::VersionedMessage, 
    pubkey, 
    signature::Keypair, 
    signer::Signer, 
    system_transaction::transfer, 
    transaction::VersionedTransaction,
    signature::Signature
};
use solana_transaction_status::{EncodedTransaction, UiTransactionEncoding};

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

use crate::tx_types::{GenericValue, TxInstructions};

#[test]
fn should_deserialize_set_compute_unit_price_ix() {
    // Example hex provided: 03d590f70400000000 -> SetComputeUnitPrice with 83333333
    let hex_str = "03d590f70400000000";
    let bytes = hex::decode(hex_str).expect("Failed to decode hex");
    
    let instruction = TxInstructions::from_raw_bytes(&bytes).expect("Failed to deserialize bytes");
    
    match instruction {
        TxInstructions::SetComputeUnitPrice { discriminator, micro_lamports } => {
            assert_eq!(discriminator.data, 3);
            assert_eq!(discriminator._type, "u8");
            assert_eq!(micro_lamports.data, 83333333);
            assert_eq!(micro_lamports._type, "u64");
        },
        _ => panic!("Wrong instruction type deserialized"),
    }
}

#[test]
fn should_serialize_roundtrip() {
    // Create a SetComputeUnitPrice instruction
    let instruction = TxInstructions::SetComputeUnitPrice {
        discriminator: GenericValue::new(3, "u8"),
        micro_lamports: GenericValue::new(83333333, "u64"),
    };
    
    // Convert to hex
    let hex_str = instruction.to_hex().expect("Failed to convert to hex");
    
    // Should match our example
    assert_eq!(hex_str, "03d590f70400000000");
    
    // Convert back from hex
    let parsed = TxInstructions::from_hex(&hex_str).expect("Failed to parse from hex");
    
    // Verify it's the same instruction
    match parsed {
        TxInstructions::SetComputeUnitPrice { discriminator, micro_lamports } => {
            assert_eq!(discriminator.data, 3);
            assert_eq!(micro_lamports.data, 83333333);
        },
        _ => panic!("Wrong instruction type after roundtrip"),
    }
}

#[test]
fn should_fetch_and_deserialize_tx() {
    let hash = "66AvQR5BvjkwHT6mtqfvHpzJtbP2bMveN89oEzNaPf3EKGwh3p4qU8yBjkbTkNuLfzi68zu6o3QMNS1LMCwUhuw9";
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