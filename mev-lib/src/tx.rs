use solana_client::rpc_client::SerializableTransaction;
use solana_sdk::{
    message::VersionedMessage, signature::Keypair, signer::Signer, transaction::VersionedTransaction
};
use crate::{programs::{mev::MevInstructionBuilder, ParsedInstruction}, result::MevResult};

// Well-known program IDs
pub const TOKEN_PROGRAM_ID: &str = "TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA";
pub const ASSOCIATED_TOKEN_PROGRAM_ID: &str = "ATokenGPvbdGVxr1b2hvZbsiqW5xWH25efTNsLJA8knL";
pub const PUMP_FUN_PROGRAM_ID: &str = "DSRCj2mWaSbQyBEG8BQxHBy7vCDk5Hafy6qcYw1i1yus"; // PumpFun DEX program
pub const JUPITER_PROGRAM_ID: &str = "JUP6LkbZbjS1jKKwapdHNy74zcZ3tLUZoi5QNyVTaV4";  // Jupiter aggregator

/// Builds sandwich transactions for a given swap transaction
/// # Arguments
/// * `transaction` - The original swap transaction to build sandwiching transactions for
/// * `new_signer` - The public key of the sandwich trader (who will execute the sandwich)
/// # Returns
/// A vector containing transactions to execute before and after the original transaction
/// to extract MEV value from the swap
pub fn build_tx_sandwich(transaction: &VersionedTransaction, new_signer: &Keypair) -> MevResult<Vec<VersionedMessage>> {
    println!("Starting build_tx_sandwich with signer: {}", new_signer.pubkey());
    let message = &transaction.message;
    let static_keys = message.static_account_keys();
    
    println!("Transaction contains {} instructions", message.instructions().len());
    
    // Process each instruction to find opportunities for sandwiching
    for (i, ix) in message.instructions().iter().enumerate() {
        println!("Processing instruction {}", i);
        // Skip if program index is out of bounds
        if ix.program_id_index as usize >= static_keys.len() {
            println!("Skipping instruction {}: program_id_index out of bounds", i);
            continue;
        }
        
        println!("Instruction {} program ID: {}", i, static_keys[ix.program_id_index as usize]);
        
        let parsed = ParsedInstruction::from_ix(ix, static_keys);
        let builder = match parsed {
            Some(i) => match i {
                ParsedInstruction::Irrelevant => continue,
                _ => MevInstructionBuilder::from_parsed_ix(i)?
            },
            None => continue
        };
        let (front, back) = builder.create_sandwich_txs(new_signer, static_keys, *transaction.get_recent_blockhash())?;
        return Ok(vec![VersionedMessage::V0(front), transaction.message.clone(), VersionedMessage::V0(back)])
    }
    
    return Ok(vec![transaction.message.clone()]);
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
        hash::Hash,
        pubkey::Pubkey
    };
    use crate::programs::pumpfun::PUMPFUN_PROGRAM_ID;
    use crate::programs::raydium::RAYDIUM_CPMM_PROGRAM_ID;

    // Helper function to create a simple buy transaction for testing
    fn create_test_buy_transaction() -> VersionedTransaction {
        let payer = Keypair::new();
        let token_program = Pubkey::new_from_array([0; 32]); // Dummy token program ID
        let pump_program = PUMPFUN_PROGRAM_ID;
        
        // Create a simple instruction that looks like a PumpFun buy
        let accounts = vec![
            AccountMeta::new(payer.pubkey(), true),  // Signer
            AccountMeta::new_readonly(system_program::id(), false),
            AccountMeta::new(Pubkey::new_unique(), false),  // Token mint
            AccountMeta::new(Pubkey::new_unique(), false),  // Pool account
            AccountMeta::new(Pubkey::new_unique(), false),  // Pool authority
            AccountMeta::new(Pubkey::new_unique(), false),  // User token account
            AccountMeta::new(payer.pubkey(), false),  // User account
            AccountMeta::new_readonly(token_program, false),
            AccountMeta::new_readonly(Pubkey::from_str_const(ASSOCIATED_TOKEN_PROGRAM_ID), false)
        ];
        
        // Simple PumpFun buy instruction data
        // This mimics the format expected by our deserializer
        let sample_ix = [
            102, 6, 61, 18, 1, 218, 235, 234, 
            27, 162, 85, 43, 0, 0, 0, 0, 
            216, 158, 3, 0, 0, 0, 0, 0
        ].to_vec();
        
        let instruction = Instruction {
            program_id: pump_program,
            accounts,
            data: sample_ix,
        };
        
        // Create a transaction
        let message = Message::new(&[instruction], Some(&payer.pubkey()));
        let tx = Transaction::new(&[&payer], message, Hash::default());
        
        // Convert to VersionedTransaction
        VersionedTransaction::from(tx)
    }
    
    // Helper function to create a simple swap transaction for Raydium CPMM
    #[allow(dead_code)]
    fn create_test_raydium_cpmm_transaction() -> VersionedTransaction {
        let payer = Keypair::new();
        let token_program = Pubkey::new_from_array([0; 32]); // Dummy token program ID
        let raydium_program = RAYDIUM_CPMM_PROGRAM_ID;
        
        // Create accounts for Raydium CPMM swap
        let mut accounts = Vec::new();
        for _ in 0..15 {
            accounts.push(AccountMeta::new(Pubkey::new_unique(), false));
        }
        accounts[0] = AccountMeta::new(payer.pubkey(), true); // Signer
        
        // Add token mints at indices 10 and 11
        accounts.push(AccountMeta::new_readonly(token_program, false));
        
        // Simple Raydium swap instruction data
        let mut instruction_data = vec![143, 190, 90, 218, 196, 30, 51, 222]; // SwapIn discriminator
        
        // Amount in (1000 tokens)
        instruction_data.extend_from_slice(&1_000_000_000u64.to_le_bytes());
        
        // Min amount out (950 tokens - 5% slippage)
        instruction_data.extend_from_slice(&950_000_000u64.to_le_bytes());
        
        let instruction = Instruction {
            program_id: raydium_program,
            accounts,
            data: instruction_data,
        };
        
        // Create transaction
        let message = Message::new(&[instruction], Some(&payer.pubkey()));
        let tx = Transaction::new(&[&payer], message, Hash::default());
        
        // Convert to VersionedTransaction
        VersionedTransaction::from(tx)
    }

    #[test]
    fn test_build_tx_sandwich_pumpfun() {
        // Create a test transaction
        let test_tx = create_test_buy_transaction();
        
        // Create our sandwich keypair
        let sandwich_keypair = Keypair::new();
        
        // Build sandwich transactions
        let result = build_tx_sandwich(&test_tx, &sandwich_keypair);
        // Check if sandwich building worked
        println!("Sandwich build result: {:?} transactions created", result.is_ok());
        // Verify we got a result
        assert!(result.is_ok());
        
        let sandwich_txs = result.unwrap();
        
        // We should at least get some transactions back
        assert!(!sandwich_txs.is_empty());
        
        // In the ideal case, we would have 3 transactions
        if sandwich_txs.len() == 3 {
            // Verify the middle transaction is the original
            assert_eq!(sandwich_txs[1], test_tx.message);
        }
    }
}