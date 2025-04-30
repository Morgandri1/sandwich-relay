use solana_sdk::transaction::VersionedTransaction;
use crate::{raydium::subscribe::PoolKeys, result::MevResult};


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

/// Create Pool Keys from target transaction
/// configured for raydium; may need abstraction
pub fn get_pool_keys_from_target(transaction: &VersionedTransaction) -> MevResult<PoolKeys> {
    let keys = transaction.message.static_account_keys();
    Ok(PoolKeys {
        // Swap Program
        program_id: keys[0],
        // AMM Pool
        id: keys[2],
        authority: keys[3],
        open_orders: keys[4],
        target_orders: keys[5],
        quote_vault: keys[6], // check
        base_vault: keys[7], // check
        // Market
        market_program_id: keys[8],
        market_id: keys[9],
        market_bids: keys[10],
        market_asks: keys[11],
        market_event_queue: keys[12],
        market_quote_vault: keys[13], // check
        market_base_vault: keys[14], // check
        market_authority: keys[15],
    })
}