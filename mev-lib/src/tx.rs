use solana_sdk::{
    instruction::CompiledInstruction, 
    message::{v0::Message, VersionedMessage}, 
    pubkey::Pubkey,
    transaction::VersionedTransaction
};
use std::collections::HashSet;
use crate::{comp::{match_program_id_to_provider, SwapProviders}, result::{MevError, MevResult}, tx_types::TxInstructions};

// Well-known program IDs
pub const TOKEN_PROGRAM_ID: &str = "TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA";
pub const ASSOCIATED_TOKEN_PROGRAM_ID: &str = "ATokenGPvbdGVxr1b2hvZbsiqW5xWH25efTNsLJA8knL";
pub const PUMP_FUN_PROGRAM_ID: &str = "DSRCj2mWaSbQyBEG8BQxHBy7vCDk5Hafy6qcYw1i1yus"; // PumpFun DEX program
pub const JUPITER_PROGRAM_ID: &str = "JUP6LkbZbjS1jKKwapdHNy74zcZ3tLUZoi5QNyVTaV4";  // Jupiter aggregator

/// Builds sandwich transactions for a given swap transaction
/// # Arguments
/// * `transaction` - The original swap transaction to build sandwiching transactions for
/// # Returns
/// A vector containing transactions to execute before and after the original transaction
/// to extract MEV value from the swap
pub fn build_tx_sandwich(transaction: &VersionedTransaction) -> MevResult<Vec<VersionedTransaction>> {
    // Front-run transaction - clone of original for now
    let front_run = transaction.to_owned();

    // Back-run transaction - clone of original for now
    let back_run = transaction.to_owned();
    
    // Return both front-run and back-run transactions
    Ok(vec![front_run, transaction.to_owned(), back_run])
}

/// Create a mirrored transaction based on input tx allowing us to skip constructing new swap txs
#[allow(unused)]
pub fn mirror_target(transaction: &VersionedTransaction) -> MevResult<VersionedMessage> {
    let message = transaction.message.clone();
    let alts = match message.address_table_lookups() {
        Some(alts) => alts,
        None => &[]
    };
    let new_ix = message.instructions()
        .iter()
        .map(|ix| -> CompiledInstruction {
            if let Ok(instruction) = TxInstructions::from_raw_bytes(&ix.data) {
                match instruction {
                    _ => ix.clone()
                }
            } else {
                ix.clone()
            }
        })
        .collect();
    let new_message = VersionedMessage::V0(Message {
        header: *message.header(),
        recent_blockhash: *message.recent_blockhash(),
        address_table_lookups: alts.to_vec(),
        account_keys: message.static_account_keys().to_vec(),
        instructions: new_ix
    });
    Ok(new_message)
}

/// Find token mint addresses being interacted with in a VersionedMessage
/// 
/// This function analyzes a transaction message to identify token mint addresses
/// that are being interacted with through token transfers, swaps, or other operations.
/// 
/// # Arguments
/// * `message` - The VersionedMessage to analyze
/// 
/// # Returns
/// A Result containing a vector of Pubkeys for token mint addresses
pub fn find_token_addresses(message: &VersionedMessage) -> MevResult<Vec<Pubkey>> {
    let mut token_addresses = HashSet::new();
    
    let account_keys = message.static_account_keys();
    
    // Process each instruction to find token addresses
    for ix in message.instructions() {
        // Get the program ID for this instruction
        let program_id = if ix.program_id_index as usize >= account_keys.len() {
            continue;
        } else {
            account_keys[ix.program_id_index as usize]
        };
        
        match match_program_id_to_provider(&program_id) {
            Some(SwapProviders::Raydium) => {
                token_addresses.insert(account_keys[ix.accounts[17] as usize]);
                token_addresses.insert(account_keys[ix.accounts[18] as usize]);
            },
            Some(SwapProviders::PumpFun) => {
                token_addresses.insert(account_keys[ix.accounts[2] as usize]);
            },
            Some(SwapProviders::PumpSwap) => {
                token_addresses.insert(account_keys[ix.accounts[3] as usize]);
                token_addresses.insert(account_keys[ix.accounts[4] as usize]);
            },
            _ => continue
        }
    }
    
    Ok(token_addresses.into_iter().collect())
}