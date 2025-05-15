use solana_sdk::{
    hash::Hash, message::{v0::Message, VersionedMessage}, pubkey::Pubkey, signature::{Keypair, Signature}, signer::Signer, system_transaction::transfer, transaction::VersionedTransaction
};

use crate::{comp::is_relevant_tx, programs::raydium::ParsedRaydiumLpv4Instructions};

#[test]
fn should_recognize_raydium_swap() {
    let t = ParsedRaydiumLpv4Instructions::Swap { 
        is_base_in: true,
        amount_in: 1000, 
        minimum_amount_out: 10, 
        accounts: [].to_vec()
    };
    assert!(is_relevant_tx(&VersionedTransaction { 
        signatures: [].to_vec(), 
        message: VersionedMessage::V0(Message {
            header: solana_sdk::message::MessageHeader { 
                num_required_signatures: 0, 
                num_readonly_signed_accounts: 0, 
                num_readonly_unsigned_accounts: 0
            },
            account_keys: [
                Pubkey::from_str_const("675kPX9MHTjS2zt1qfr1NYHuzeLXfQM9H24wFSUt1Mp8")
            ].to_vec(),
            recent_blockhash: Hash::new_unique(),
            address_table_lookups: [].to_vec(),
            instructions: [t.to_compiled_instruction(0).unwrap()].to_vec()
        }) 
    }))
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