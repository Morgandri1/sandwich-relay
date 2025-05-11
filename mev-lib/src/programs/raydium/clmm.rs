use solana_sdk::{instruction::CompiledInstruction, pubkey::Pubkey};

use crate::{programs::Account, result::MevResult};

pub const RAYDIUM_CLMM_PROGRAM_ID: Pubkey = Pubkey::from_str_const("CAMMCzo5YL8w4VFF8KVHrK22GGUsp5VTaW7grrKgrWqK");

#[derive(Debug, PartialEq)]
pub enum ParsedRaydiumClmmInstructions {
    Swap {
        amount: u64,
        other_amount_threshold: u64,
        accounts: Vec<Account>,
        sqrt_price_limit_64: u128,
        is_base_input: bool
    }
}

impl ParsedRaydiumClmmInstructions {
    pub fn from_bytes(bytes: Vec<u8>, accounts: Vec<Account>) -> MevResult<Self> {
        if bytes.len() < 41 {
            return Err(crate::result::MevError::FailedToDeserialize);
        };
        let mut amount_in_bytes = [0u8; 8];
        let mut min_out_bytes = [0u8; 8];
        let mut sqrt_thing_bytes = [0u8; 16];
        
        // Copy the bytes into properly sized arrays for conversion
        min_out_bytes[..8].copy_from_slice(&bytes[8..16]);
        amount_in_bytes[..8].copy_from_slice(&bytes[16..24]);
        sqrt_thing_bytes[..16].copy_from_slice(&bytes[24..40]);
        
        return Ok(Self::Swap {
            amount: u64::from_le_bytes(min_out_bytes),
            other_amount_threshold: u64::from_le_bytes(amount_in_bytes),
            sqrt_price_limit_64: u128::from_le_bytes(sqrt_thing_bytes),
            is_base_input: bytes[40] == 1,
            accounts
        })
    }
    
    pub fn to_compiled_instruction(&self, program_id: u8) -> MevResult<CompiledInstruction> {
        match self {
            Self::Swap { amount, other_amount_threshold, accounts, sqrt_price_limit_64, is_base_input } => {
                let mut instruction_data = [43, 4, 237, 11, 26, 201, 30, 98].to_vec();
                instruction_data.extend_from_slice(&amount.to_le_bytes());
                instruction_data.extend_from_slice(&other_amount_threshold.to_le_bytes());
                instruction_data.extend_from_slice(&sqrt_price_limit_64.to_le_bytes());
                instruction_data.extend_from_slice(if *is_base_input {&[1]} else {&[0]});
                return Ok(CompiledInstruction { 
                    program_id_index: program_id, 
                    accounts: accounts.iter().map(|a| a.account_index).collect(), 
                    data: instruction_data
                })
            }
        }
    }
    
    /// Creates a sandwich buy instruction that takes advantage of slippage in the original transaction.
    /// This method is intended to be used before the target transaction to impact the price.
    /// 
    /// # Arguments
    /// * `new_sender` - Public key of the new transaction sender (your address)
    /// * `static_keys` - Original transaction account keys
    /// * `program_id_index` - Index of the Raydium CLMM program in the static keys
    /// * `swap_in_out` - Whether to swap the input and output tokens
    /// 
    /// # Returns
    /// * `MevResult<CompiledInstruction>` - The compiled sandwich buy instruction
    pub fn create_sandwich_buy(&self, new_sender: &Pubkey, static_keys: &[Pubkey], program_id_index: u8, swap_in_out: bool) -> MevResult<CompiledInstruction> {
        match self {
            Self::Swap { amount, other_amount_threshold, accounts, sqrt_price_limit_64, is_base_input } => {
                // Calculate the original price ratio to understand slippage opportunity
                let price_ratio = (*other_amount_threshold as f64) / (*amount as f64);
                
                // Use 10% of the original amount for our sandwich trade
                let slippage_factor = 0.1;
                let sandwich_amount = (*amount as f64 * slippage_factor) as u64;
                
                // Set a conservative threshold to ensure our transaction goes through
                // For CLMM pools, we might need a more conservative amount to prevent reverts
                let threshold_factor = 0.85; // Accept 85% of the theoretical output
                let sandwich_threshold = (sandwich_amount as f64 * price_ratio * threshold_factor) as u64;
                
                // Create a new Vec of accounts for sandwich instruction
                let sandwich_accounts: Vec<Account> = accounts.iter().map(|a| {
                    Account {
                        account_index: a.account_index,
                        is_writable: a.is_writable
                    }
                }).collect();
                
                // Create a new Swap instruction with our parameters
                let sandwich_swap = Self::Swap {
                    amount: sandwich_amount,
                    other_amount_threshold: sandwich_threshold,
                    accounts: sandwich_accounts,
                    sqrt_price_limit_64: *sqrt_price_limit_64, // Keep the same price limit
                    is_base_input: *is_base_input, // Keep the same direction
                };
                
                // Get updated account list with our address
                let _mutated_accounts = self.mutate_accounts(static_keys, new_sender, swap_in_out)?;
                
                // Convert to a compiled instruction
                let ix = sandwich_swap.to_compiled_instruction(program_id_index)?;
                
                Ok(ix)
            }
        }
    }
    
    /// Creates a sandwich sell instruction to sell tokens acquired from a front-running trade.
    /// This method is intended to be used after the target transaction to complete the sandwich.
    /// 
    /// # Arguments
    /// * `token_amount` - Amount of token to sell (should be the amount received from the sandwich buy)
    /// * `new_sender` - Public key of the new transaction sender (your address)
    /// * `static_keys` - Original transaction account keys
    /// * `program_id_index` - Index of the Raydium CLMM program in the static keys
    /// * `swap_in_out` - Whether to swap the input and output tokens
    /// 
    /// # Returns
    /// * `MevResult<CompiledInstruction>` - The compiled sandwich sell instruction
    pub fn create_sandwich_sell(
        &self, 
        token_amount: u64,
        new_sender: &Pubkey,
        static_keys: &[Pubkey],
        program_id_index: u8,
        swap_in_out: bool
    ) -> MevResult<CompiledInstruction> {
        match self {
            Self::Swap { amount, other_amount_threshold, accounts, sqrt_price_limit_64, is_base_input } => {
                // Calculate the original price ratio to determine fair value
                let original_price = (*other_amount_threshold as f64) / (*amount as f64);
                
                // When selling, we want to maximize our output amount
                // Accept 90% of the theoretical value to ensure execution
                let threshold_factor = 0.9;
                let sandwich_threshold = (token_amount as f64 * original_price * threshold_factor) as u64;
                
                // Create a new Vec of accounts for sandwich instruction
                let sandwich_accounts: Vec<Account> = accounts.iter().map(|a| {
                    Account {
                        account_index: a.account_index,
                        is_writable: a.is_writable
                    }
                }).collect();
                
                // Create a new Swap instruction with our parameters, but we need to flip is_base_input
                // to reverse the direction of the swap for the sell side of the sandwich
                let sandwich_swap = Self::Swap {
                    amount: token_amount,
                    other_amount_threshold: sandwich_threshold,
                    accounts: sandwich_accounts,
                    sqrt_price_limit_64: *sqrt_price_limit_64, // Keep the same price limit
                    is_base_input: !(*is_base_input), // Flip the direction for the sell
                };
                
                // Get updated account list with our address
                // Use !swap_in_out to reverse the input/output direction
                let _mutated_accounts = self.mutate_accounts(static_keys, new_sender, !swap_in_out)?;
                
                // Convert to a compiled instruction
                let ix = sandwich_swap.to_compiled_instruction(program_id_index)?;
                
                Ok(ix)
            }
        }
    }
    
    pub fn mint_in(&self, static_keys: &[Pubkey], swap_in_out: bool) -> Pubkey {
        match self {
            Self::Swap { accounts, .. } => {
                match swap_in_out {
                    true => static_keys[accounts[12].account_index as usize],
                    false => static_keys[accounts[11].account_index as usize]
                }
            }
        }
    }
    
    pub fn mint_out(&self, static_keys: &[Pubkey], swap_in_out: bool) -> Pubkey {
        match self {
            Self::Swap { accounts, .. } => {
                match swap_in_out {
                    true => static_keys[accounts[11].account_index as usize],
                    false => static_keys[accounts[12].account_index as usize]
                }
            }
        }
    }
    
    pub fn mutate_accounts(&self, static_keys: &[Pubkey], new_sender: &Pubkey, swap_in_out: bool) -> MevResult<Vec<Pubkey>> {
        match self {
            Self::Swap { accounts, .. } => {
                Ok(static_keys
                    .iter()
                    .map(|k| {
                        if k == &static_keys[0] { // update signer
                            return *new_sender
                        } else if k == &static_keys[accounts[6].account_index as usize] { // update input account
                            return spl_associated_token_account::get_associated_token_address(
                                new_sender, 
                                &self.mint_in(static_keys, swap_in_out)
                            )
                        } else if k == &static_keys[accounts[7].account_index as usize] { // swap output account
                            return spl_associated_token_account::get_associated_token_address(
                                new_sender, 
                                &self.mint_out(static_keys, swap_in_out)
                            )
                        } else if swap_in_out && k == &self.mint_in(static_keys, false) { // swap mint in
                            return self.mint_in(static_keys, true)
                        } else if swap_in_out && k == &self.mint_out(static_keys, false) { // swap mint out
                            return self.mint_out(static_keys, true)
                        } else {
                            return *k
                        }
                    })
                    .collect())
            }
        }
    }
}

#[cfg(test)]
mod test {
    use solana_sdk::pubkey::Pubkey;

    use crate::programs::{raydium::ParsedRaydiumClmmInstructions, Account};

    #[test]
    fn deserialize_clmm_instruction() {
        let ix = [
            43, 4, 237, 11, 26, 201, 30, 98, 
            157, 49, 166, 180, 0, 0, 0, 0,
            1, 0, 0, 0, 0, 0, 0, 0, 
            154, 87, 105, 78, 169, 26, 92, 132, 177, 196, 254, 255, 0, 0, 0, 0,
            1
        ].to_vec();
        let accounts = [0, 10, 1, 2, 3, 4, 5, 6, 11, 12, 13, 14, 15, 7, 8, 9];
        let target = ParsedRaydiumClmmInstructions::from_bytes(ix, accounts.iter().map(|i| Account::new(i, false)).collect()).unwrap();
        let static_keys: Vec<Pubkey> = [
            "CWZH15GdUAMRVrJogDgLzYzaSMhVtqqZjMaEKSVyxAN", 
            "2vu8uC6Dd47KT4Pjv34KzDaRKZq6nuwpjxfgbtiGKm5e", 
            "98PG8fGr8WBN5Tqmic4UU8K9v8ZKsvnAQdUdgtrNGpGg", 
            "DNd6iKDkeZyMLyqFm4sCGDtiGZmJPd6HsSQGdS2WQqQH",
            "54hPh6Btj8RTt4AKQX3KEYV3rxmYEQMrF5fxVuDyd7Ka", 
            "AH7oyTfUvM3vN47kd9w25rHNGGy7pMGdh4KMUGxnwEfj", 
            "14sFgtoPYf2fj1LYgQEJN7iM1c9rZ3RcuqA9q9RhFRM9", 
            "4xsKyvEJV5Prww5phxHdcERqBg6WjEVVeNgqFM4w5Fxz", 
            "3BtkUDGg5Cvs7fYzwUmfTHGsWe6Fyvd4uvRCJsakw2Qd", 
            "7HC2yNBGpjf4QqL2yfjGh8wTwNLYMFyLL2J35nxD5Hnq", 
            "9iFER3bpjf1PTTCQCfTRu17EJgvsxo9pVyA9QWwEuX4x", 
            "TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA", 
            "TokenzQdBNbLqP5VEhdkAS6EPFLC1PHnBqCXEpPxuEb", 
            "MemoSq4gqABAXKb96qnH8TysNcWxMyWCqXgDLGmfcHr", 
            "rq1nFfHyk65DGjUYHScFQTzx3cJd79RA7k7aPCNpump", 
            "So11111111111111111111111111111111111111112", 
            "ComputeBudget111111111111111111111111111111", 
            "CAMMCzo5YL8w4VFF8KVHrK22GGUsp5VTaW7grrKgrWqK"
        ].iter().map(|k| Pubkey::from_str_const(k)).collect();
        assert_eq!(
            target, 
            ParsedRaydiumClmmInstructions::Swap { 
                amount: 3030790557, 
                other_amount_threshold: 1, 
                accounts: accounts.iter().map(|i| Account::new(i, false)).collect(), 
                sqrt_price_limit_64: 79226673521066979257578248090, 
                is_base_input: true
            }
        );
        assert_eq!(
            target.mint_out(static_keys.as_slice(), true),
            Pubkey::from_str_const("rq1nFfHyk65DGjUYHScFQTzx3cJd79RA7k7aPCNpump")
        );
    }
    
    #[test]
    fn deserialize_2() {
        let ix = [
            43, 4, 237, 11, 26, 201, 30, 98, 
            99, 84, 101, 3, 126, 5, 0, 0, 
            72, 174, 230, 22, 0, 0, 0, 0, 
            0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
            1
        ].to_vec();
        let accounts = [0, 10, 1, 2, 3, 4, 5, 6, 11, 12, 13, 14, 15, 7, 8, 9].to_vec();
        let target = ParsedRaydiumClmmInstructions::from_bytes(ix, accounts.iter().map(|i| Account::new(i, false)).collect()).unwrap();
        let static_keys: Vec<Pubkey> = [
            "CWZH15GdUAMRVrJogDgLzYzaSMhVtqqZjMaEKSVyxAN", 
            "2vu8uC6Dd47KT4Pjv34KzDaRKZq6nuwpjxfgbtiGKm5e", 
            "98PG8fGr8WBN5Tqmic4UU8K9v8ZKsvnAQdUdgtrNGpGg", 
            "DNd6iKDkeZyMLyqFm4sCGDtiGZmJPd6HsSQGdS2WQqQH",
            "54hPh6Btj8RTt4AKQX3KEYV3rxmYEQMrF5fxVuDyd7Ka", 
            "AH7oyTfUvM3vN47kd9w25rHNGGy7pMGdh4KMUGxnwEfj", 
            "14sFgtoPYf2fj1LYgQEJN7iM1c9rZ3RcuqA9q9RhFRM9", 
            "4xsKyvEJV5Prww5phxHdcERqBg6WjEVVeNgqFM4w5Fxz", 
            "3BtkUDGg5Cvs7fYzwUmfTHGsWe6Fyvd4uvRCJsakw2Qd", 
            "7HC2yNBGpjf4QqL2yfjGh8wTwNLYMFyLL2J35nxD5Hnq", 
            "9iFER3bpjf1PTTCQCfTRu17EJgvsxo9pVyA9QWwEuX4x", 
            "TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA", 
            "TokenzQdBNbLqP5VEhdkAS6EPFLC1PHnBqCXEpPxuEb", 
            "MemoSq4gqABAXKb96qnH8TysNcWxMyWCqXgDLGmfcHr", 
            "rq1nFfHyk65DGjUYHScFQTzx3cJd79RA7k7aPCNpump", 
            "So11111111111111111111111111111111111111112", 
            "ComputeBudget111111111111111111111111111111", 
            "CAMMCzo5YL8w4VFF8KVHrK22GGUsp5VTaW7grrKgrWqK"
        ].iter().map(|k| Pubkey::from_str_const(k)).collect();
        assert_eq!(
            target, 
            ParsedRaydiumClmmInstructions::Swap { 
                amount: 6038780990563, 
                other_amount_threshold: 384216648, 
                accounts: accounts.iter().map(|i| Account::new(i, false)).collect(), 
                sqrt_price_limit_64: 0, 
                is_base_input: true
            }
        );
        assert_eq!(
            target.mint_out(static_keys.as_slice(), true),
            Pubkey::from_str_const("rq1nFfHyk65DGjUYHScFQTzx3cJd79RA7k7aPCNpump")
        );
    }
}