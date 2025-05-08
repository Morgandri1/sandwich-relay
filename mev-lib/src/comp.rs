use solana_sdk::transaction::VersionedTransaction;

use crate::{jito::JITO_TIP_ADDRESSES, programs::ParsedInstruction};

pub fn contains_jito_tip(transaction: &VersionedTransaction) -> bool {
    let keys = transaction.message.static_account_keys();
    for key in JITO_TIP_ADDRESSES.iter() {
        if keys.contains(key) {
            return true
        }
    }
    return false
}

/// Checks if a transaction is a swap that should be sandwiched
/// # Arguments
/// * `transaction` - The transaction to check
/// * `relevant_programs` - List of program IDs that are considered swap programs (e.g., Jupiter, Raydium)
/// # Returns
/// `true` if the transaction involves any of the relevant programs, `false` otherwise
pub fn is_relevant_tx(transaction: &VersionedTransaction) -> bool {
    let keys = transaction.message.static_account_keys();
    let instruction = transaction.message.instructions();
    for ix in instruction {
        match ParsedInstruction::from_ix(ix, keys) {
            Some(ParsedInstruction::Irrelevant) | None => continue,
            _ => return true
        }
    };
    return false
}