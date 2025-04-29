use result::{MevError, MevResult};
use jito_relayer::relayer::RelayerPacketBatches;
use solana_sdk::{pubkey::Pubkey, transaction::VersionedTransaction};
use solana_perf::packet::PacketBatch;
use std::sync::Arc;
use bincode;

mod result;

/// Checks if a transaction is a swap that should be sandwiched
/// # Arguments
/// * `transaction` - The transaction to check
/// * `relevant_programs` - List of program IDs that are considered swap programs (e.g., Jupiter, Raydium)
/// # Returns
/// `true` if the transaction involves any of the relevant programs, `false` otherwise
pub fn is_relevant_swap(transaction: &VersionedTransaction, relevant_programs: &[Pubkey]) -> bool {
    let keys = transaction.message.static_account_keys();
    let instruction = transaction.message.instructions();
    for ix in instruction {
        let index: usize = ix.program_id_index.into();
        let program_id = keys[index];
        if relevant_programs.contains(&program_id) {
            return true
        };
    };
    false
}

/// Process a batch of packets and add 'sandwich' transactions around relevant swap operations
/// # Arguments
/// * `batch` - The original packet batch to process
/// * `relevant_programs` - List of program IDs that should be targeted for sandwiching
/// # Returns
/// A new `RelayerPacketBatches` containing the original packets and sandwich packets
pub fn sandwich_batch_packets(batch: &mut RelayerPacketBatches, relevant_programs: &[Pubkey]) -> MevResult<RelayerPacketBatches> {
    let banking_packet_batch = batch.banking_packet_batch.clone();
    let (packet_batches, stats) = &*banking_packet_batch;
    
    // Create new packet batches that will include original packets and sandwich packets
    let mut new_packet_batches = Vec::with_capacity(packet_batches.len());
    
    for packet_batch in packet_batches.iter() {
        // Create a new packet batch with additional capacity for sandwich packets
        let mut new_batch = PacketBatch::with_capacity(packet_batch.len() * 3);

        for packet in packet_batch.iter() {
            let vtx: VersionedTransaction = packet
                .deserialize_slice::<VersionedTransaction, _>(..)
                .map_err(|_| MevError::FailedToDeserialize)?;
                
            if is_relevant_swap(&vtx, relevant_programs) {
                // Create and add sandwich packet
                new_batch.push(create_sandwich_packet(packet)?);
            } else {
                new_batch.push(packet.clone());
            }
        }
        new_packet_batches.push(new_batch);
    }
    
    // Create a new BankingPacketBatch and RelayerPacketBatches with the modified packets
    let new_banking_packet_batch = Arc::new((new_packet_batches, stats.clone()));
    
    // Create a new RelayerPacketBatches with the original timestamp but updated banking_packet_batch
    Ok(RelayerPacketBatches {
        stamp: batch.stamp,
        banking_packet_batch: new_banking_packet_batch
    })
}

/// Helper function to create a new packet for sandwiching
/// This would be used to create front-running or back-running packets
#[allow(dead_code)]
fn create_sandwich_packet(
    original_packet: &solana_perf::packet::Packet, 
) -> MevResult<solana_perf::packet::Packet> {
    // Create a new packet based on the original
    let mut new_packet = original_packet.clone();
    
    // Extract the original transaction
    let original_tx = original_packet
        .deserialize_slice::<VersionedTransaction, _>(..)
        .map_err(|_| MevError::FailedToDeserialize)?;
    
    // Create a sandwich transaction - either front-run or back-run
    let sandwich_txs = build_tx_sandwich(&original_tx)?;
    
    // Serialize the sandwich transactions into the new packet
    for sandwich_tx in &sandwich_txs {
        let serialized_tx = bincode::serialize(sandwich_tx)
            .map_err(|_| MevError::FailedToSerialize)?;
        
        // Copy the serialized transaction data into the packet
        // Need to ensure the packet buffer is correct size
        // and update the meta field appropriately
        new_packet.buffer_mut()[..serialized_tx.len()].copy_from_slice(&serialized_tx);
        new_packet.meta_mut().size = serialized_tx.len();
    }
    
    Ok(new_packet)
}

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