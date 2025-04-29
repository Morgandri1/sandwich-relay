use solana_sdk::transaction::VersionedTransaction;
use crate::result::MevResult;


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