use crate::{
    comp::{match_program_id_to_provider, SwapProviders},
    programs::ParsedInstruction,
    result::{MevError, MevResult},
};
use solana_sdk::{
    instruction::CompiledInstruction,
    message::{v0::Message, VersionedMessage},
    pubkey::Pubkey,
    transaction::VersionedTransaction,
};
use std::collections::HashSet;

// Well-known program IDs
pub const TOKEN_PROGRAM_ID: &str = "TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA";
pub const ASSOCIATED_TOKEN_PROGRAM_ID: &str = "ATokenGPvbdGVxr1b2hvZbsiqW5xWH25efTNsLJA8knL";
pub const PUMP_FUN_PROGRAM_ID: &str = "DSRCj2mWaSbQyBEG8BQxHBy7vCDk5Hafy6qcYw1i1yus"; // PumpFun DEX program
pub const JUPITER_PROGRAM_ID: &str = "JUP6LkbZbjS1jKKwapdHNy74zcZ3tLUZoi5QNyVTaV4"; // Jupiter aggregator
pub const MIN_PROFIT_THRESHOLD : u64 = 100_000; // Minimum profit threshold for sandwiching
pub const PRIORITY_FEE_MICRO_LAMPORTS: u64 = 1_000_000; // Priority fee in micro lamports

/// Builds sandwich transactions for a given swap transaction
/// # Arguments
/// * `transaction` - The original swap transaction to build sandwiching transactions for
/// * `new_signer` - The public key of the sandwich trader (who will execute the sandwich)
/// # Returns
/// A vector containing transactions to execute before and after the original transaction
/// to extract MEV value from the swap
pub fn build_tx_sandwich(
    transaction: &VersionedTransaction,
    new_signer: &Pubkey,
) -> MevResult<Vec<VersionedTransaction>> {
    let message = &transaction.message;
    let static_keys = message.static_account_keys();

    // return if no accounts
    if static_keys.is_empty() {
        return Err(MevError::InvalidTransaction);
    }

    // Array to store our sandwich transactions
    let mut sandwich_txs = Vec::new();
    let mut profit_estimate = 0u64;

    // Add priority fee instruction to all transactions
    let priority_ix = ComputeBudgetInstruction::set_compute_unit_price(PRIORITY_FEE_MICRO_LAMPORTS);

    // Process each instruction to find opportunities for sandwiching
    for ix in message.instructions() {
        // Skip if program index is out of bounds
        if ix.program_id_index as usize >= static_keys.len() {
            continue;
        }

        let program_id = static_keys[ix.program_id_index as usize];
        let dex_type = match_program_id_to_provider(&program_id);

        // Skip non-DEX instructions early
        let parsed_ix = match ParsedInstruction::from_ix(ix, static_keys) {
            Some(parsed) => parsed,
            None => continue,
        };

        // Dynamic sizing based on pool liquidity
        let (front_run_ix, received_amount) = match dex_type {
            SwapProviders::PumpFun => {
                let liquidity = fetch_pool_liquidity(program_id, current_slot)?;
                let amount = calculate_dynamic_amount(ix, liquidity, 0.5, 2.0)?;
                let mut ix =
                    parsed_ix.create_sandwich_buy(new_signer, static_keys, ix.program_id_index)?;
                ix.instructions.push(priority_ix.clone());
                (ix, amount)
            }
            SwapProviders::RaydiumCpmm => {
                let liquidity = fetch_pool_liquidity(program_id, current_slot)?;
                let amount = calculate_dynamic_amount(ix, liquidity, 5.0, 15.0)?;
                let mut ix = parsed_ix.create_sandwich_buy(
                    new_signer,
                    static_keys,
                    ix.program_id_index,
                    false,
                )?;
                ix.instructions.push(priority_ix.clone());
                (ix, amount)
            }
            SwapProviders::RaydiumClmm => {
                let liquidity = fetch_pool_liquidity(program_id, current_slot)?;
                let amount = calculate_dynamic_amount(ix, liquidity, 3.0, 10.0)?;
                let mut ix = parsed_ix.create_sandwich_buy(
                    new_signer,
                    static_keys,
                    ix.program_id_index,
                    false,
                )?;
                ix.instructions.push(priority_ix.clone());
                (ix, amount)
            }
            _ => continue,
        };

        // Build front-run transaction
        let front_run_message = create_sandwich_message(message, front_run_ix, new_signer)?;
        let front_run_tx = VersionedTransaction {
            signatures: vec![],
            message: front_run_message,
        };

        // Simulate before proceeding
        if let Err(e) = simulate_transaction(&front_run_tx) {
            log::warn!("Front-run simulation failed: {:?}", e);
            continue;
        }

        // Build back-run transaction
        let back_run_ix = match dex_type {
            SwapProviders::PumpFun => parsed_ix.create_sandwich_sell(
                received_amount,
                new_signer,
                static_keys,
                ix.program_id_index,
            )?,
            SwapProviders::RaydiumCpmm => parsed_ix.create_sandwich_sell(
                received_amount,
                new_signer,
                static_keys,
                ix.program_id_index,
                true,
            )?,
            SwapProviders::RaydiumClmm => parsed_ix.create_sandwich_sell(
                received_amount,
                new_signer,
                static_keys,
                ix.program_id_index,
                true,
            )?,
            _ => continue,
        };

        let back_run_message = create_sandwich_message(message, back_run_ix, new_signer)?;
        let back_run_tx = VersionedTransaction {
            signatures: vec![],
            message: back_run_message,
        };

        // Profitability check
        profit_estimate = estimate_profit(&front_run_tx, transaction, &back_run_tx)?;
        if profit_estimate < MIN_PROFIT_THRESHOLD {
            continue;
        }

        // Only add to bundle if profitable
        sandwich_txs.push(front_run_tx);
        sandwich_txs.push(transaction.clone());
        sandwich_txs.push(back_run_tx);

        // Add slight delay between transactions
        add_jitter_delay();

        return Ok(sandwich_txs);
    }

    Ok(vec![])
}

/// Dynamic amount calculation based on pool liquidity
fn calculate_dynamic_amount(
    ix: &CompiledInstruction,
    pool_liquidity: u64,
    min_pct: f64,
    max_pct: f64,
) -> MevResult<u64> {
    let base_amount = parse_swap_amount(ix)?;
    let liquidity_ratio = base_amount as f64 / pool_liquidity as f64;

    // Scale percentage based on liquidity impact
    let dynamic_pct = if liquidity_ratio > 0.1 {
        min_pct // Small percentage for large swaps
    } else {
        min_pct + (max_pct - min_pct) * (1.0 - liquidity_ratio * 10.0)
    };

    let amount = (base_amount as f64 * dynamic_pct / 100.0) as u64;
    Ok(amount.max(1)) // Ensure at least 1 lamport
}

/// Add random delay to avoid detection
fn add_jitter_delay() {
    use rand::Rng;
    let delay_ms = rand::thread_rng().gen_range(10..100);
    std::thread::sleep(std::time::Duration::from_millis(delay_ms));
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
        None => &[],
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
pub fn mirror_target(
    transaction: &VersionedTransaction,
    new_signer: &Pubkey,
    invert: bool,
) -> MevResult<VersionedMessage> {
    let message = transaction.message.clone();
    let alts = match message.address_table_lookups() {
        Some(alts) => alts,
        None => &[],
    };
    let mut accounts = vec![];
    let new_ix = message
        .instructions()
        .iter()
        .map(|ix: &CompiledInstruction| -> CompiledInstruction {
            if let Some(instruction) =
                ParsedInstruction::from_ix(&ix, transaction.message.static_account_keys())
            {
                accounts.extend(
                    update_accounts(
                        &instruction,
                        message.static_account_keys(),
                        new_signer,
                        Some(invert),
                    )
                    .unwrap_or(message.static_account_keys().to_vec()),
                );
                match instruction {
                    ParsedInstruction::Irrelevant => ix.clone(),
                    _ => {
                        construct_mirror_ix(instruction, ix.program_id_index).unwrap_or(ix.clone())
                    }
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
        account_keys: if accounts.len() == 0 {
            message.static_account_keys().to_vec()
        } else {
            accounts
        },
        instructions: new_ix,
    });
    Ok(new_message)
}

fn update_accounts(
    ix: &ParsedInstruction,
    keys: &[Pubkey],
    new_signer: &Pubkey,
    swap_in_out: Option<bool>,
) -> MevResult<Vec<Pubkey>> {
    match ix {
        ParsedInstruction::PumpFun(i) => {
            if let Ok(ix) = i {
                ix.mutate_accounts(&keys, new_signer)
            } else {
                Err(MevError::FailedToBuildTx)
            }
        }
        ParsedInstruction::PumpSwap(i) => {
            if let Ok(ix) = i {
                ix.mutate_accounts(&keys, new_signer, swap_in_out.unwrap_or(false))
            } else {
                Err(MevError::FailedToBuildTx)
            }
        }
        ParsedInstruction::RaydiumClmm(i) => {
            if let Ok(ix) = i {
                ix.mutate_accounts(&keys, new_signer, swap_in_out.unwrap_or(false))
            } else {
                Err(MevError::FailedToBuildTx)
            }
        }
        ParsedInstruction::RaydiumCpmm(i) => {
            if let Ok(ix) = i {
                ix.mutate_accounts(&keys, new_signer, swap_in_out.unwrap_or(false))
            } else {
                Err(MevError::FailedToBuildTx)
            }
        }
        _ => Err(MevError::FailedToBuildTx),
    }
}

fn construct_mirror_ix(
    parsed: ParsedInstruction,
    program_index: u8,
) -> Option<CompiledInstruction> {
    match parsed {
        ParsedInstruction::PumpFun(ix) => match ix {
            Ok(ix) => {
                let compiled = ix.to_compiled_instruction(program_index);
                match compiled {
                    Ok(cix) => Some(cix),
                    _ => None,
                }
            }
            Err(_) => None,
        },
        ParsedInstruction::PumpSwap(ix) => match ix {
            Ok(ix) => {
                let compiled = ix.to_compiled_instruction(program_index);
                match compiled {
                    Ok(cix) => Some(cix),
                    _ => None,
                }
            }
            Err(_) => None,
        },
        ParsedInstruction::RaydiumClmm(ix) => match ix {
            Ok(ix) => {
                let compiled = ix.to_compiled_instruction(program_index);
                match compiled {
                    Ok(cix) => Some(cix),
                    _ => None,
                }
            }
            Err(_) => None,
        },
        ParsedInstruction::RaydiumCpmm(ix) => match ix {
            Ok(ix) => {
                let compiled = ix.to_compiled_instruction(program_index);
                match compiled {
                    Ok(cix) => Some(cix),
                    _ => None,
                }
            }
            Err(_) => None,
        },
        _ => None,
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
            }
            Some(SwapProviders::PumpFun) => {
                token_addresses.insert(account_keys[ix.accounts[2] as usize]);
            }
            Some(SwapProviders::PumpSwap) => {
                token_addresses.insert(account_keys[ix.accounts[3] as usize]);
                token_addresses.insert(account_keys[ix.accounts[4] as usize]);
            }
            _ => continue,
        }
    }

    Ok(token_addresses.into_iter().collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::programs::pumpfun::PUMPFUN_PROGRAM_ID;
    use crate::programs::raydium::RAYDIUM_CPMM_PROGRAM_ID;
    use solana_sdk::{
        hash::Hash,
        instruction::{AccountMeta, Instruction},
        message::Message,
        signature::{Keypair, Signer},
        system_program,
        transaction::Transaction,
    };

    // Helper function to create a simple buy transaction for testing
    fn create_test_buy_transaction() -> VersionedTransaction {
        let payer = Keypair::new();
        let token_program = Pubkey::new_from_array([0; 32]); // Dummy token program ID
        let pump_program = PUMPFUN_PROGRAM_ID;

        // Create a simple instruction that looks like a PumpFun buy
        let accounts = vec![
            AccountMeta::new(payer.pubkey(), true), // Signer
            AccountMeta::new_readonly(system_program::id(), false),
            AccountMeta::new(Pubkey::new_unique(), false), // Token mint
            AccountMeta::new(Pubkey::new_unique(), false), // Pool account
            AccountMeta::new(Pubkey::new_unique(), false), // Pool authority
            AccountMeta::new(Pubkey::new_unique(), false), // User token account
            AccountMeta::new(payer.pubkey(), false),       // User account
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
        assert!(
            accounts.contains(&sandwich_keypair.pubkey()),
            "Expected sandwich pubkey in message accounts"
        );
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
            AccountMeta::new(payer.pubkey(), true), // 0: Signer
            AccountMeta::new_readonly(Pubkey::new_unique(), false), // 1
            AccountMeta::new(token_mint, false),    // 2: Token mint
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
