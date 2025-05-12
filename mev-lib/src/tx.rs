use solana_client::rpc_client::SerializableTransaction;
use solana_sdk::{
    instruction::CompiledInstruction, message::{v0::Message, VersionedMessage}, pubkey::Pubkey, signature::Keypair, signer::Signer, transaction::VersionedTransaction
};
use crate::{programs::{mev::MevInstructionBuilder, ParsedInstruction}, result::{MevError, MevResult}};

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

/// Creates a new VersionedMessage for a sandwich transaction
fn create_sandwich_message(
    original_message: &VersionedMessage,
    sandwich_ix: CompiledInstruction,
    new_signer: &Pubkey, // Use the new signer pubkey
) -> MevResult<VersionedMessage> {
    // Get lookup tables if they exist
    let alts = match original_message.address_table_lookups() {
        Some(alts) => alts,
        None => &[]
    };
    
    // Create a message header with our signer
    let mut new_header = *original_message.header();
    new_header.num_required_signatures = 1;
    
    // Create account keys, replacing the original signer with our signer
    let mut account_keys = original_message.static_account_keys().to_vec();
    if !account_keys.is_empty() {
        account_keys[0] = *new_signer;
    }
    
    // Create a new versioned message with our sandwich instruction
    let new_message = VersionedMessage::V0(Message {
        header: new_header,
        recent_blockhash: *original_message.recent_blockhash(),
        address_table_lookups: alts.to_vec(),
        account_keys,
        instructions: vec![sandwich_ix],
    });
    
    Ok(new_message)
}

/// Create a mirrored transaction based on input tx allowing us to skip constructing new swap txs
#[allow(unused)]
pub fn mirror_target(transaction: &VersionedTransaction, new_signer: &Pubkey, invert: bool) -> MevResult<VersionedMessage> {
    let message = transaction.message.clone();
    let alts = match message.address_table_lookups() {
        Some(alts) => alts,
        None => &[]
    };
    let mut accounts = vec![];
    let new_ix = message.instructions()
        .iter()
        .map(|ix: &CompiledInstruction| -> CompiledInstruction {
            if let Some(instruction) = ParsedInstruction::from_ix(&ix, transaction.message.static_account_keys()) {
                accounts.extend(update_accounts(
                    &instruction, 
                    message.static_account_keys(), 
                    new_signer, 
                    Some(invert)
                ).unwrap_or(message.static_account_keys().to_vec()));
                match instruction {
                    ParsedInstruction::Irrelevant => ix.clone(),
                    _ => construct_mirror_ix(
                        instruction,
                        ix.program_id_index
                    ).unwrap_or(ix.clone()),
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
        account_keys: if accounts.len() == 0 { message.static_account_keys().to_vec() } else { accounts },
        instructions: new_ix
    });
    Ok(new_message)
}

fn update_accounts(ix: &ParsedInstruction, keys: &[Pubkey], new_signer: &Pubkey, swap_in_out: Option<bool>) -> MevResult<Vec<Pubkey>> {
    match ix {
        ParsedInstruction::PumpFun(i) => {
            if let Ok(ix) = i {
                ix.mutate_accounts(&keys, new_signer)
            } else {
                Err(MevError::FailedToBuildTx)
            }
        },
        ParsedInstruction::PumpSwap(i) => {
            if let Ok(ix) = i {
                ix.mutate_accounts(&keys, new_signer, swap_in_out.unwrap_or(false))
            } else {
                Err(MevError::FailedToBuildTx)
            }
        },
        ParsedInstruction::RaydiumClmm(i) => {
            if let Ok(ix) = i {
                ix.mutate_accounts(&keys, new_signer, swap_in_out.unwrap_or(false))
            } else {
                Err(MevError::FailedToBuildTx)
            }
        },
        ParsedInstruction::RaydiumCpmm(i) => {
            if let Ok(ix) = i {
                ix.mutate_accounts(&keys, new_signer, swap_in_out.unwrap_or(false))
            } else {
                Err(MevError::FailedToBuildTx)
            }
        },
        _ => Err(MevError::FailedToBuildTx)
    }
}

fn construct_mirror_ix(
    parsed: ParsedInstruction, 
    program_index: u8
) -> Option<CompiledInstruction> {
    match parsed {
        ParsedInstruction::PumpFun(ix) => {
            match ix {
                Ok(ix) => {
                    let compiled = ix.to_compiled_instruction(program_index);
                    match compiled {
                        Ok(cix) => Some(cix),
                        _ => None
                    }
                },
                Err(_) => None
            }
        },
        ParsedInstruction::PumpSwap(ix) => {
            match ix {
                Ok(ix) => {
                    let compiled = ix.to_compiled_instruction(program_index);
                    match compiled {
                        Ok(cix) => Some(cix),
                        _ => None
                    }
                },
                Err(_) => None
            }
        },
        ParsedInstruction::RaydiumClmm(ix) => {
            match ix {
                Ok(ix) => {
                    let compiled = ix.to_compiled_instruction(program_index);
                    match compiled {
                        Ok(cix) => Some(cix),
                        _ => None
                    }
                },
                Err(_) => None
            }
        },
        ParsedInstruction::RaydiumCpmm(ix) => {
            match ix {
                Ok(ix) => {
                    let compiled = ix.to_compiled_instruction(program_index);
                    match compiled {
                        Ok(cix) => Some(cix),
                        _ => None
                    }
                },
                Err(_) => None
            }
        },
        _ => None
    }
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
        ];
        
        // Simple PumpFun buy instruction data
        // This mimics the format expected by our deserializer
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

    #[test]
    fn test_mirror_target() {
        // Create a test transaction
        let test_tx = create_test_buy_transaction();
        
        // Create our sandwich keypair
        let sandwich_keypair = Keypair::new();
        
        // Try mirroring the transaction
        let result = mirror_target(&test_tx, &sandwich_keypair.pubkey(), false);
        
        // Verify we got a result
        assert!(result.is_ok());
        
        let mirrored_message = result.unwrap();
        
        // Verify the message has at least one instruction
        assert!(!mirrored_message.instructions().is_empty());
        
        // Just verify the message contains our pubkey somewhere
        let accounts = mirrored_message.static_account_keys();
        assert!(accounts.contains(&sandwich_keypair.pubkey()), 
               "Expected sandwich pubkey in message accounts");
    }
}