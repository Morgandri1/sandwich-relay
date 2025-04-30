use solana_sdk::{pubkey::Pubkey, transaction::VersionedTransaction};

use crate::result::{MevError, MevResult};

pub enum SwapProviders {
    Raydium,
    PumpSwap,
    PumpFun,
    RaydiumCPMM,
    MeteoraDLMM
}

/// Checks if a transaction is a swap that should be sandwiched
/// # Arguments
/// * `transaction` - The transaction to check
/// * `relevant_programs` - List of program IDs that are considered swap programs (e.g., Jupiter, Raydium)
/// # Returns
/// `true` if the transaction involves any of the relevant programs, `false` otherwise
pub fn is_relevant_tx(transaction: &VersionedTransaction, relevant_programs: &[Pubkey]) -> bool {
    let keys = transaction.message.static_account_keys();
    let instruction = transaction.message.instructions();
    for ix in instruction {
        let index: usize = ix.program_id_index.into();
        let program_id = keys[index];
        if relevant_programs.contains(&program_id) {
            return true
        };
    };
    return false
}

pub fn match_program_id_to_provider(program_id: &Pubkey) -> MevResult<SwapProviders> {
    match program_id.to_string().as_str() {
        "675kPX9MHTjS2zt1qfr1NYHuzeLXfQM9H24wFSUt1Mp8" => Ok(SwapProviders::Raydium),
        _ => Err(MevError::ValueError)
    }
}