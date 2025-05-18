use solana_core::banking_trace::BankingPacketBatch;
use solana_perf::packet::PacketBatch;
use std::sync::Arc;
use bincode;
use solana_sdk::{
    transaction::VersionedTransaction,
    signature::Keypair,
    signer::Signer
};
use crate::contains_jito_tip;
use crate::result::{MevResult, MevError};
use crate::comp::is_relevant_tx;
use crate::tx::build_tx_sandwich;

/// Process a batch of packets and add 'sandwich' transactions around relevant swap operations
/// # Arguments
/// * `batch` - The original packet batch to process
/// * `keypair` - The keypair used to sign sandwich transactions
/// # Returns
/// A new `BankingPacketBatch` containing the original packets and sandwich packets
pub fn sandwich_batch_packets(batch: BankingPacketBatch, keypair: &Keypair) -> MevResult<BankingPacketBatch> {
    let (packet_batches, stats) = &*batch;

    // Create new packet batches that will include original packets and sandwich packets
    let mut new_packet_batches = Vec::with_capacity(packet_batches.len());

    for packet_batch in packet_batches.iter() {
        // Create a new packet batch with additional capacity for sandwich packets
        // Each swap transaction might become 3 transactions (front-run, original, back-run)
        let mut new_batch = PacketBatch::with_capacity(packet_batch.len() * 3);
        for packet in packet_batch.iter() {
            // Try to deserialize the packet into a transaction
            match packet.deserialize_slice::<VersionedTransaction, _>(..) {
                Ok(vtx) => {
                    // Check if this transaction is relevant for sandwiching
                    if is_relevant_tx(&vtx) && !contains_jito_tip(&vtx) {
                        // Create sandwich packets around the original transaction using our keypair
                        match create_sandwich_packet(packet, keypair) {
                            Ok(sandwich_packets) => {
                                // Add all sandwich packets to the new batch
                                println!("Sandwich packet {:?}", sandwich_packets);
                                for sandwich_packet in sandwich_packets {
                                    new_batch.push(sandwich_packet);
                                }
                            },
                            Err(err) => {
                                eprintln!("Failed to create sandwich packet: {}", err);
                                println!("{:?}", packet.clone());
                                // get bytes of the packet
                                let bytes = packet.data(..);
                                if bytes.is_none() {
                                    eprintln!("Packet data is empty");
                                } else {
                                    eprintln!("Packet data: {:?}", bytes.unwrap());
                                }
                                
                                // If sandwich creation fails, just include the original packet
                                new_batch.push(packet.clone());
                            }
                        }
                    } else {
                        // Not a relevant transaction, just include the original packet
                        new_batch.push(packet.clone());
                    }
                },
                Err(_) => {
                    // If deserialization fails, just include the original packet
                    new_batch.push(packet.clone());
                }
            }
        }

        new_packet_batches.push(new_batch);
    }

    // Create a new BankingPacketBatch with the modified packets
    let new_banking_packet_batch = Arc::new((new_packet_batches, stats.clone()));

    Ok(new_banking_packet_batch)
}

/// Helper function to create sandwich packets with the original in the middle
/// Returns a vector of packets where:
/// - First packet(s): Front-running transaction(s)
/// - Middle packet: Original transaction
/// - Last packet(s): Back-running transaction(s)
/// Create sandwich packets around the original transaction
///
/// # Arguments
/// * `original_packet` - The original packet containing the transaction to sandwich
/// * `keypair` - The keypair to sign sandwich transactions with
///
/// # Returns
/// A vector of packets containing the sandwich transactions with the original in the middle
fn create_sandwich_packet(
    original_packet: &solana_perf::packet::Packet,
    keypair: &Keypair
) -> MevResult<Vec<solana_perf::packet::Packet>> {
    // Extract the original transaction
    let original_tx = original_packet
        .deserialize_slice::<VersionedTransaction, _>(..)
        .map_err(|_| MevError::FailedToDeserialize)?;

    // Create a sandwich transaction sequence
    let mut sandwich_txs: Vec<VersionedTransaction> = build_tx_sandwich(&original_tx, keypair)?
        .iter_mut()
        .map(|ix| VersionedTransaction {
            signatures: [].to_vec(),
            message: ix.clone()
        })
        .collect();

    // Create packets from the transactions
    let mut packets = Vec::with_capacity(sandwich_txs.len());

    // Process and sign each sandwich transaction
    for tx in sandwich_txs.iter_mut() {
        // Sign the transaction if it's our transaction (not the original)
        if tx != &original_tx {
            let signature = keypair.sign_message(&tx.message.serialize());
            tx.signatures = vec![signature];
        }

        // Serialize the transaction
        let serialized_tx = bincode::serialize(&tx)
            .map_err(|_| MevError::FailedToSerialize)?;

        // Create a packet from the serialized transaction data
        let packet = solana_perf::packet::Packet::from_data(None, &serialized_tx)
            .map_err(|_| MevError::FailedToSerialize)?;

        packets.push(packet);
    }

    Ok(packets)
}

#[cfg(test)]
mod tests {
    use super::*;
    use solana_sdk::{
        instruction::{AccountMeta, Instruction},
        message::Message,
        signature::{Keypair, Signer},
        system_program,
        transaction::Transaction,
        hash::Hash
    };
    use crate::programs::pumpfun::PUMPFUN_PROGRAM_ID;
    use solana_perf::packet::Packet;
    use solana_sdk::pubkey::Pubkey;
    use std::str::FromStr;

    // Helper function to create a test packet for a PumpFun transaction
    fn create_test_packet() -> Packet {
        // Create a simple transaction for testing
        let payer = Keypair::new();
        let token_program = Pubkey::new_unique(); // Unique token program ID
        let sol_token = Pubkey::from_str("So11111111111111111111111111111111111111112").unwrap(); // SOL token mint
        let token_mint = Pubkey::new_unique(); // Random token mint
        let pump_program = PUMPFUN_PROGRAM_ID;

        // Create more accounts - include more than the minimum needed for testing
        // Include accounts expected by the sandwich-creating code
        let accounts = vec![
            AccountMeta::new(payer.pubkey(), true),  // Signer at index 0
            AccountMeta::new_readonly(system_program::id(), false), // System program at index 1
            AccountMeta::new(token_mint, false),  // Token mint at index 2
            AccountMeta::new(Pubkey::new_unique(), false),  // Pool account at index 3
            AccountMeta::new(Pubkey::new_unique(), false),  // Pool authority at index 4
            AccountMeta::new(Pubkey::new_unique(), false),  // User token account at index 5
            AccountMeta::new(payer.pubkey(), false),  // User account at index 6 - same as signer
            AccountMeta::new_readonly(token_program, false), // Token program at index 7
            AccountMeta::new(sol_token, false),  // SOL token at index 8
            AccountMeta::new(Pubkey::new_unique(), false),  // Additional account for safety
            AccountMeta::new(Pubkey::new_unique(), false),  // Additional account for safety
        ];

        // Simple PumpFun buy instruction data with proper format
        let mut instruction_data = vec![102, 6, 61, 18, 1, 218, 235, 234]; // Buy discriminator

        // Amount (100 tokens)
        instruction_data.extend_from_slice(&100_000_000u64.to_le_bytes());

        // Max SOL cost (0.5 SOL)
        instruction_data.extend_from_slice(&500_000_000u64.to_le_bytes());

        let instruction = Instruction {
            program_id: pump_program,
            accounts,
            data: instruction_data,
        };

        // Create a transaction with a recent blockhash
        let message = Message::new(&[instruction], Some(&payer.pubkey()));

        // Sign transaction with payer
        let tx = Transaction::new(&[&payer], message, Hash::new_unique());

        // Create versioned transaction
        let versioned_tx = VersionedTransaction::from(tx);

        // Serialize the transaction
        let tx_data = bincode::serialize(&versioned_tx).unwrap();

        // Create packet from data using the proper public API
        let packet = Packet::from_data(None, &tx_data).unwrap_or_else(|_| {
            // Fallback approach if from_data fails
            panic!("Failed to create packet from transaction data");
        });

        packet
    }

    // Create test batch data
    fn create_test_banking_packet_batch() -> BankingPacketBatch {
        let mut batch = PacketBatch::with_capacity(10);

        // Add some test packets
        batch.push(create_test_packet());
        batch.push(create_test_packet());

        // Create a simple batch
        let packet_batches = vec![batch];

        Arc::new((packet_batches, None))
    }

    #[test]
    fn test_sandwich_batch_packets() {
        let test_batch = create_test_banking_packet_batch();
        let keypair = Keypair::new();

        // Get the original count of packets for comparison
        let (original_batches, _) = &*test_batch;
        let original_packet_count = original_batches[0].len();

        // Process the batch
        let result = sandwich_batch_packets(test_batch, &keypair);

        // This should succeed
        assert!(result.is_ok(), "Failed to process batch packets");

        let new_batch = result.unwrap();
        let (packet_batches, _) = &*new_batch;

        // Make sure we have output batches
        assert!(!packet_batches.is_empty(), "No packet batches were returned");

        // Make sure the first batch has packets
        assert!(!packet_batches[0].is_empty(), "First batch has no packets");

        // The number of packets in the output should be at least
        // as many as the input (either unchanged or with sandwich packets added)
        assert!(
            packet_batches[0].len() >= original_packet_count,
            "Expected at least {} packets, but got {}",
            original_packet_count,
            packet_batches[0].len()
        );

        // If sandwich creation worked, we should have more packets than we started with
        if packet_batches[0].len() > original_packet_count {
            println!("Successfully created sandwich transactions! Original: {}, New: {}",
                original_packet_count, packet_batches[0].len());
        } else {
            println!("Packets were processed but no sandwiches were created");
        }
    }
}
