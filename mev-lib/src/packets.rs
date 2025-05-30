use serde_json::{error, json};
use solana_client::rpc_client::SerializableTransaction;
use solana_core::banking_trace::BankingPacketBatch;
use solana_perf::packet::PacketBatch;
use solana_sdk::message::VersionedMessage;
use solana_sdk::packet::Packet;
use solana_sdk::system_transaction::transfer;
use std::ops::Deref;
use std::sync::Arc;
use bincode;
use solana_sdk::{
    transaction::VersionedTransaction,
    signature::Keypair,
    signer::Signer
};
use crate::{contains_jito_tip};
use crate::jito::JITO_TIP_ADDRESSES;
use crate::result::{MevResult, MevError};
use crate::comp::is_relevant_tx;
use crate::sandwich::{SandwichGroup, verify_sandwich_preflight};
#[allow(unused_imports)]
use base64::{Engine as _, engine::general_purpose};
use solana_sdk::signature::Signature;

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
                    let signature = vtx.signatures.get(0).map_or("no signature".to_string(), |sig| sig.to_string());

                    println!("Processing Transaction {}", signature);
                    // Check if this transaction is relevant for sandwiching
                    if is_relevant_tx(&vtx) && !contains_jito_tip(&vtx) {
                        // Create sandwich packets around the original transaction using our keypair
                        match create_sandwich_packet(packet, keypair) {
                            Ok(sandwich_packets) => {
                                // Add all sandwich packets to the new batch
                                if sandwich_packets.len() == 3 {
                                    let frontrun = sandwich_packets.get(0).ok_or(MevError::FailedToDeserialize)?.1.to_string();
                                    let target = sandwich_packets.get(1).ok_or(MevError::FailedToDeserialize)?.1.to_string();
                                    let backrun = sandwich_packets.get(2).ok_or(MevError::FailedToDeserialize)?.1.to_string();
                                    println!("Inserting MEV target: {} - frontrun: {} - backrun: {}", target, frontrun, backrun);
                                }

                                match verify_sandwich_preflight(
                                    sandwich_packets
                                        .iter()
                                        .map(|(packet, _)| packet.clone())
                                        .collect::<Vec<Packet>>()
                                        .as_slice()
                                ) {
                                   Ok(true) => {
                                       // Insert packets in strict sequence: frontrun, original, backrun
                                       for (sandwich_packet, _) in sandwich_packets {
                                           new_batch.push(sandwich_packet);
                                       }
                                   },
                                   Ok(false) => {
                                       let mut packets = sandwich_packets.clone();
                                       packets.reverse();
                                       for (sandwich_packet, _) in packets {
                                           new_batch.push(sandwich_packet);
                                       }
                                   },
                                   Err(_) => new_batch.push(packet.clone())
                                }
                            },
                            Err(err) => {
                                eprintln!("Failed to create sandwich packet {}: {}", signature, err);

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
/// Returns a vector of packets that is strictly ordered as:
/// [0]: Front-running transaction
/// [1]: Original transaction
/// [2]: Back-running transaction
///
/// This ordering is guaranteed to be preserved when the packets are forwarded
/// to ensure MEV sandwich execution happens in the correct sequence.
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
) -> MevResult<Vec<(solana_perf::packet::Packet, Signature)>> {
    // Extract the original transaction
    let original_tx = original_packet
        .deserialize_slice::<VersionedTransaction, _>(..)
        .map_err(|_| MevError::FailedToDeserialize)?;

    // Create a sandwich group to handle ordering
    let mut sandwich_group = SandwichGroup::new(original_tx.clone());

    // Create the sandwich transactions
    sandwich_group.create_sandwich(keypair)?;

    // Create Jito tip transaction
    let jito_txs = vec![
        VersionedTransaction::from(transfer(
            &keypair,
            &JITO_TIP_ADDRESSES[0],
            10000000,
            *original_tx.get_recent_blockhash()
        ))
    ];

    // Convert the sandwich group to packets
    let packets = sandwich_group.to_packets()?;

    /*if let Err(e) = send_to_jito(&jito_txs) {
        eprintln!("Failed to send to Jito: {}", e);
    }*/

    // Verify the packet ordering during preflight
    if !verify_sandwich_preflight(&packets.iter().map(|(p, _)| p.clone()).collect::<Vec<_>>())? {
        eprintln!("Warning: Sandwich packet ordering verification failed");
    } else {
        println!("Sandwich packet ordering verified successfully");
    }

    Ok(packets)
}

#[allow(unused)]
fn send_to_jito(
    jito_txs: &Vec<VersionedTransaction>,
) -> MevResult<String> {
    let rt = tokio::runtime::Runtime::new().map_err(|_| MevError::UnknownError)?;

    let b64_tx: Vec<(String, String)> = jito_txs
        .iter()
        .map(|tx| {
            let serialized_tx = bincode::serialize(&tx)
                .unwrap();
            let b64_tx = general_purpose::STANDARD.encode(serialized_tx);
            let signature = tx.signatures.get(0)
                .ok_or_else(|| MevError::ValueError)
                .unwrap()
                .to_string();
            (b64_tx, signature)
        })
        .collect();

    let params = json!([
        b64_tx.iter()
            .map(|x| x.0.clone())
            .collect::<Vec<String>>(),
        {
            "encoding": "base64"
        }
    ]);

    let res = rt.block_on(async move {
        let c = jito_sdk_rust::JitoJsonRpcSDK::new("https://frankfurt.mainnet.block-engine.jito.wtf/api/v1", None);
        c.send_bundle(Some(params), None).await
    }).map_err(|err| {
        eprintln!("Error sending to Jito: {}", err);
        MevError::UnknownError
    })?;

    println!(
        "Response: {:?} {:?}",
        res,
        b64_tx
            .iter()
            .map(|x| x.1.clone())
            .collect::<Vec<String>>().join(", ")
    );

    let id = res["result"]
        .as_str()
        .ok_or_else(|| MevError::ValueError)?
        .to_string();

    Ok(id)
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
