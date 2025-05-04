use solana_sdk::{pubkey::Pubkey, transaction::VersionedTransaction};

use crate::{programs::{pumpfun::PUMP_FUN_PROGRAM_ID, pumpswap::PUMP_SWAP_PROGRAM_ID, raydium::{LPV4_SWAP, RAYDIUM_AMM_PROGRAM_ID}, ProgramInstruction}, result::{MevError, MevResult}};

pub enum SwapProviders {
    Raydium,
    RaydiumLegacy,
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
        if ProgramInstruction::from_ix(ix, keys).is_some() {
            return true
        };
    };
    return false
}

pub fn match_program_id_to_provider(program_id: &Pubkey) -> Option<SwapProviders> {
    match program_id {
        &RAYDIUM_AMM_PROGRAM_ID => Some(SwapProviders::Raydium),
        &LPV4_SWAP => Some(SwapProviders::RaydiumLegacy),
        &PUMP_FUN_PROGRAM_ID => Some(SwapProviders::PumpFun),
        &PUMP_SWAP_PROGRAM_ID => Some(SwapProviders::PumpSwap),
        _ => None
    }
}