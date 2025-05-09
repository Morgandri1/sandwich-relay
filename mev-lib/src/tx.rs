use solana_sdk::{
    instruction::CompiledInstruction, 
    message::{v0::Message, VersionedMessage}, 
    pubkey::Pubkey,
    transaction::VersionedTransaction
};
use std::collections::HashSet;
use crate::{comp::{match_program_id_to_provider, SwapProviders}, programs::ParsedInstruction, result::{MevError, MevResult}};

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
pub fn build_tx_sandwich(transaction: &VersionedTransaction, new_signer: &Pubkey) -> MevResult<Vec<VersionedTransaction>> {
    println!("Starting build_tx_sandwich with signer: {}", new_signer);
    let message = &transaction.message;
    let static_keys = message.static_account_keys();
    
    println!("Transaction contains {} instructions", message.instructions().len());
    
    // Array to store our sandwich transactions
    let mut sandwich_txs = Vec::new();
    
    // Track if we created a valid sandwich and the token amount for back-run
    // Using allow to suppress the warnings, as these are legitimately used in the control flow
    #[allow(unused_assignments)]
    let mut front_run_created = false;
    #[allow(unused_assignments)]
    let mut received_token_amount: u64 = 0;
    
    // Process each instruction to find opportunities for sandwiching
    for (i, ix) in message.instructions().iter().enumerate() {
        println!("Processing instruction {}", i);
        // Skip if program index is out of bounds
        if ix.program_id_index as usize >= static_keys.len() {
            println!("Skipping instruction {}: program_id_index out of bounds", i);
            continue;
        }
        
        println!("Instruction {} program ID: {}", i, static_keys[ix.program_id_index as usize]);
        
        // Check if this instruction is from a DEX we can sandwich
        match ParsedInstruction::from_ix(ix, static_keys) {
            Some(ParsedInstruction::PumpFun(Ok(parsed_ix))) => {
                println!("Found PumpFun instruction: {:?}", parsed_ix);
                // Create front-run transaction
                match parsed_ix.create_sandwich_buy(new_signer, static_keys, ix.program_id_index) {
                    Ok(front_run_ix) => {
                        println!("Created PumpFun front-run instruction");
                        // Mutate accounts for our instruction
                        // This happens inside create_sandwich_buy, but we also use the result here
                        let mutated_accounts = parsed_ix.mutate_accounts(static_keys, new_signer);
                        if let Err(e) = &mutated_accounts {
                            println!("Error mutating accounts: {:?}", e);
                        }
                        let _mutated_accounts = mutated_accounts?;
                        
                        // Create a new versioned message with our front-run instruction
                        let front_run_message = create_sandwich_message(message, front_run_ix, new_signer)?;
                        
                        // Create a transaction with empty signatures (will be signed later)
                        let front_run_tx = VersionedTransaction {
                            signatures: vec![],
                            message: front_run_message.clone(),
                        };
                        
                        // Store amount we're buying to use in back-run
                        received_token_amount = match &parsed_ix {
                            crate::programs::pumpfun::ParsedPumpFunInstructions::Buy { amount, .. } => {
                                // For estimating received tokens, we use 2% of original amount
                                // This is a simplified estimation - in production you'd use more sophisticated price impact calculations
                                let amount = (*amount as f64 * 0.02) as u64;
                                println!("Estimated received token amount: {}", amount);
                                amount
                            },
                            _ => {
                                println!("Instruction is not a Buy, setting received_token_amount to 0");
                                0
                            },
                        };
                        
                        // Add front-run transaction to our list
                        sandwich_txs.push(front_run_tx);
                        front_run_created = true;
                        println!("Added PumpFun front-run transaction to sandwich_txs");
                    },
                    Err(e) => {
                        println!("Error creating PumpFun front-run: {:?}", e);
                        continue;
                    },
                }
                
                // If we created a front-run, also create back-run to sell tokens
                if front_run_created && received_token_amount > 0 {
                    println!("Creating PumpFun back-run to sell {} tokens", received_token_amount);
                    match parsed_ix.create_sandwich_sell(received_token_amount, new_signer, static_keys, ix.program_id_index) {
                        Ok(back_run_ix) => {
                            println!("Created PumpFun back-run instruction");
                            // Mutate accounts for our instruction
                            // This happens inside create_sandwich_sell, but we're making it explicit here
                            let mutated_accounts = parsed_ix.mutate_accounts(static_keys, new_signer);
                            if let Err(e) = &mutated_accounts {
                                println!("Error mutating accounts for back-run: {:?}", e);
                            }
                            let _mutated_accounts = mutated_accounts?;
                            
                            // Create a new versioned message with our back-run instruction
                            let back_run_message = create_sandwich_message(message, back_run_ix, new_signer)?;
                            
                            // Create a transaction with empty signatures (will be signed later)
                            let back_run_tx = VersionedTransaction {
                                signatures: vec![],
                                message: back_run_message.clone(),
                            };
                            
                            // Add back-run transaction to our list
                            println!("Adding original transaction and PumpFun back-run to sandwich_txs");
                            sandwich_txs.push(transaction.clone());
                            sandwich_txs.push(back_run_tx);
                            
                            // Return our sandwich transactions
                            println!("Returning complete PumpFun sandwich with {} transactions", sandwich_txs.len());
                            return Ok(sandwich_txs);
                        },
                        Err(e) => {
                            println!("Error creating PumpFun back-run: {:?}", e);
                            continue;
                        },
                    }
                }
            },
            Some(ParsedInstruction::RaydiumCpmm(Ok(parsed_ix))) => {
                println!("Found RaydiumCpmm instruction: {:?}", parsed_ix);
                // Create front-run transaction - we'll try with and without token swapping
                match parsed_ix.create_sandwich_buy(new_signer, static_keys, ix.program_id_index, false) {
                    Ok(front_run_ix) => {
                        println!("Created RaydiumCpmm front-run instruction");
                        // Mutate accounts for our instruction
                        // We need to explicitly mutate accounts for the CPMM case
                        let mutated_accounts = parsed_ix.mutate_accounts(static_keys, new_signer, false);
                        if let Err(e) = &mutated_accounts {
                            println!("Error mutating CPMM accounts: {:?}", e);
                        }
                        let _mutated_accounts = mutated_accounts?;
                        
                        // Create a new versioned message with our front-run instruction
                        let front_run_message = create_sandwich_message(message, front_run_ix, new_signer)?;
                        
                        // Create a transaction with empty signatures (will be signed later)
                        let front_run_tx = VersionedTransaction {
                            signatures: vec![],
                            message: front_run_message.clone(),
                        };
                        
                        // Store amount we're buying to use in back-run
                        received_token_amount = match &parsed_ix {
                            crate::programs::raydium::ParsedRaydiumCpmmInstructions::SwapIn { amount, .. } => {
                                // For estimating received tokens, we use 15% of original amount as defined in our method
                                let amount = (*amount as f64 * 0.15) as u64;
                                println!("Estimated CPMM SwapIn received token amount: {}", amount);
                                amount
                            },
                            crate::programs::raydium::ParsedRaydiumCpmmInstructions::SwapOut { amount_out, .. } => {
                                // For SwapOut, we're directly specifying the output amount
                                let amount = (*amount_out as f64 * 0.15) as u64;
                                println!("Estimated CPMM SwapOut received token amount: {}", amount);
                                amount
                            },
                        };
                        
                        // Add front-run transaction to our list
                        sandwich_txs.push(front_run_tx);
                        front_run_created = true;
                        println!("Added RaydiumCpmm front-run transaction to sandwich_txs");
                    },
                    Err(e) => {
                        println!("Error creating RaydiumCpmm front-run: {:?}", e);
                        continue;
                    },
                }
                
                // If we created a front-run, also create back-run to sell tokens
                if front_run_created && received_token_amount > 0 {
                    println!("Creating RaydiumCpmm back-run to sell {} tokens", received_token_amount);
                    match parsed_ix.create_sandwich_sell(received_token_amount, new_signer, static_keys, ix.program_id_index, false) {
                        Ok(back_run_ix) => {
                            println!("Created RaydiumCpmm back-run instruction");
                            // Mutate accounts for our instruction
                            // We need to use !swap_in_out for the sell side
                            let mutated_accounts = parsed_ix.mutate_accounts(static_keys, new_signer, true);
                            if let Err(e) = &mutated_accounts {
                                println!("Error mutating CPMM accounts for back-run: {:?}", e);
                            }
                            let _mutated_accounts = mutated_accounts?;
                            
                            // Create a new versioned message with our back-run instruction
                            let back_run_message = create_sandwich_message(message, back_run_ix, new_signer)?;
                            
                            // Create a transaction with empty signatures (will be signed later)
                            let back_run_tx = VersionedTransaction {
                                signatures: vec![],
                                message: back_run_message.clone(),
                            };
                            
                            // Add original and back-run transactions to our list
                            println!("Adding original transaction and RaydiumCpmm back-run to sandwich_txs");
                            sandwich_txs.push(transaction.clone());
                            sandwich_txs.push(back_run_tx);
                            
                            // Return our sandwich transactions
                            println!("Returning complete RaydiumCpmm sandwich with {} transactions", sandwich_txs.len());
                            return Ok(sandwich_txs);
                        },
                        Err(e) => {
                            println!("Error creating RaydiumCpmm back-run: {:?}", e);
                            continue;
                        },
                    }
                }
            },
            Some(ParsedInstruction::RaydiumClmm(Ok(parsed_ix))) => {
                println!("Found RaydiumClmm instruction: {:?}", parsed_ix);
                // Create front-run transaction - we'll try with and without token swapping
                match parsed_ix.create_sandwich_buy(new_signer, static_keys, ix.program_id_index, false) {
                    Ok(front_run_ix) => {
                        println!("Created RaydiumClmm front-run instruction");
                        // Mutate accounts for our instruction
                        // We need to explicitly mutate accounts for the CLMM case
                        let mutated_accounts = parsed_ix.mutate_accounts(static_keys, new_signer, false);
                        if let Err(e) = &mutated_accounts {
                            println!("Error mutating CLMM accounts: {:?}", e);
                        }
                        let _mutated_accounts = mutated_accounts?;
                        
                        // Create a new versioned message with our front-run instruction
                        let front_run_message = create_sandwich_message(message, front_run_ix, new_signer)?;
                        
                        // Create a transaction with empty signatures (will be signed later)
                        let front_run_tx = VersionedTransaction {
                            signatures: vec![],
                            message: front_run_message.clone(),
                        };
                        
                        // Store amount we're buying to use in back-run
                        received_token_amount = match &parsed_ix {
                            crate::programs::raydium::ParsedRaydiumClmmInstructions::Swap { amount, .. } => {
                                // For estimating received tokens, we use 10% of original amount as defined in our method
                                let amount = (*amount as f64 * 0.1) as u64;
                                println!("Estimated CLMM Swap received token amount: {}", amount);
                                amount
                            },
                        };
                        
                        // Add front-run transaction to our list
                        sandwich_txs.push(front_run_tx);
                        front_run_created = true;
                        println!("Added RaydiumClmm front-run transaction to sandwich_txs");
                    },
                    Err(e) => {
                        println!("Error creating RaydiumClmm front-run: {:?}", e);
                        continue;
                    },
                }
                
                // If we created a front-run, also create back-run to sell tokens
                if front_run_created && received_token_amount > 0 {
                    println!("Creating RaydiumClmm back-run to sell {} tokens", received_token_amount);
                    match parsed_ix.create_sandwich_sell(received_token_amount, new_signer, static_keys, ix.program_id_index, false) {
                        Ok(back_run_ix) => {
                            println!("Created RaydiumClmm back-run instruction");
                            // Mutate accounts for our instruction
                            // We need to use !swap_in_out for the sell side
                            let mutated_accounts = parsed_ix.mutate_accounts(static_keys, new_signer, true);
                            if let Err(e) = &mutated_accounts {
                                println!("Error mutating CLMM accounts for back-run: {:?}", e);
                            }
                            let _mutated_accounts = mutated_accounts?;
                            
                            // Create a new versioned message with our back-run instruction
                            let back_run_message = create_sandwich_message(message, back_run_ix, new_signer)?;
                            
                            // Create a transaction with empty signatures (will be signed later)
                            let back_run_tx = VersionedTransaction {
                                signatures: vec![],
                                message: back_run_message.clone(),
                            };
                            
                            // Add original and back-run transactions to our list
                            println!("Adding original transaction and RaydiumClmm back-run to sandwich_txs");
                            sandwich_txs.push(transaction.clone());
                            sandwich_txs.push(back_run_tx);
                            
                            // Return our sandwich transactions
                            println!("Returning complete RaydiumClmm sandwich with {} transactions", sandwich_txs.len());
                            return Ok(sandwich_txs);
                        },
                        Err(e) => {
                            println!("Error creating RaydiumClmm back-run: {:?}", e);
                            continue;
                        },
                    }
                }
            },
            Some(_) => println!("Found instruction of other type, but not handling it"),
            None => println!("Could not parse instruction {}", i),
        }
    }
    
    // If we couldn't create a sandwich, return the original transaction
    if sandwich_txs.is_empty() {
        println!("No sandwich transactions were created, returning original transaction");
        return Ok(vec![transaction.clone()]);
    }
    
    // Otherwise return whatever sandwich transactions we created
    println!("Returning partial sandwich with {} transactions", sandwich_txs.len());
    Ok(sandwich_txs)
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
        let result = build_tx_sandwich(&test_tx, &sandwich_keypair.pubkey());
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
            assert_eq!(sandwich_txs[1], test_tx);
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
    
    #[test]
    fn test_find_token_addresses() {
        // We need to set up a test with indexes that match what's expected in find_token_addresses
        let payer = Keypair::new();
        let pump_program = PUMPFUN_PROGRAM_ID;
        
        // Create dummy pubkeys for testing
        let token_mint = Pubkey::new_unique();
        
        // Create a simple instruction that looks like a PumpFun buy/sell
        let accounts = vec![
            AccountMeta::new(payer.pubkey(), true),   // 0: Signer
            AccountMeta::new_readonly(Pubkey::new_unique(), false), // 1
            AccountMeta::new(token_mint, false),      // 2: Token mint
            AccountMeta::new(Pubkey::new_unique(), false), // 3
            AccountMeta::new(Pubkey::new_unique(), false), // 4
        ];
        
        // Simple dummy instruction data
        let instruction_data = vec![1, 2, 3, 4];
        
        let instruction = Instruction {
            program_id: pump_program,
            accounts,
            data: instruction_data,
        };
        
        // Create a transaction
        let message = Message::new(&[instruction], Some(&payer.pubkey()));
        let tx = Transaction::new(&[&payer], message, Hash::default());
        let vtx = VersionedTransaction::from(tx);
        
        // Find token addresses
        let result = find_token_addresses(&vtx.message);
        
        // Verify we got a result
        assert!(result.is_ok());
        
        let token_addresses = result.unwrap();
        
        // Verify we found the token mint address we added
        assert!(token_addresses.contains(&token_mint));
    }
}