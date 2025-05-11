use solana_sdk::{instruction::CompiledInstruction, pubkey::Pubkey};

use crate::{programs::Account, result::{MevError, MevResult}};

pub const RAYDIUM_CPMM_PROGRAM_ID: Pubkey = Pubkey::from_str_const("CPMMoo8L3F4NbTegBCKVNunggL7H1ZpdTHKxQB5qKP1C");

#[derive(Debug, PartialEq)]
pub enum ParsedRaydiumCpmmInstructions {
    SwapIn {
        extra: Vec<u8>,
        amount: u64,
        min_amount_out: u64,
        accounts: Vec<Account>,
    },
    SwapOut {
        extra: Vec<u8>,
        max_amount_in: u64,
        amount_out: u64,
        accounts: Vec<Account>
    }
}

impl ParsedRaydiumCpmmInstructions {
    pub fn from_bytes(bytes: Vec<u8>, accounts: Vec<Account>) -> MevResult<Self> {
        if bytes.len() < 24 {
            return Err(crate::result::MevError::FailedToDeserialize);
        };
        let mut amount_in_bytes = [0u8; 8];
        let mut min_out_bytes = [0u8; 8];
        
        // Copy the bytes into properly sized arrays for conversion
        min_out_bytes[..8].copy_from_slice(&bytes[16..24]);
        amount_in_bytes[..8].copy_from_slice(&bytes[8..16]);

        // Print byte values for debugging
        println!("Parsing bytes: {:?}", bytes);
        println!("Amount in bytes: {:?}", &bytes[8..16]);
        println!("Min out bytes: {:?}", &bytes[16..24]);
        println!("Amount value: {}", u64::from_le_bytes(amount_in_bytes));
        println!("Min out value: {}", u64::from_le_bytes(min_out_bytes));
        
        match bytes[0] {
            143 => Ok(Self::SwapIn {
                extra: bytes[0..8].to_vec(),
                amount: u64::from_le_bytes(amount_in_bytes),
                min_amount_out: u64::from_le_bytes(min_out_bytes),
                accounts
            }),
            55 => Ok(Self::SwapOut {
                extra: bytes[0..8].to_vec(),
                max_amount_in: u64::from_le_bytes(amount_in_bytes),
                amount_out: u64::from_le_bytes(min_out_bytes),
                accounts
            }),
            _ => Err(MevError::FailedToDeserialize)
        }
    }
    
    pub fn to_compiled_instruction(&self, program_id: u8) -> MevResult<CompiledInstruction> {
        match self {
            Self::SwapIn { amount, min_amount_out, accounts, extra } => {
                let mut instruction_data = [].to_vec();
                instruction_data.extend(extra);
                instruction_data.extend_from_slice(&amount.to_le_bytes());
                instruction_data.extend_from_slice(&min_amount_out.to_le_bytes());
                return Ok(CompiledInstruction { 
                    program_id_index: program_id, 
                    accounts: accounts.iter().map(|a| a.account_index).collect(), 
                    data: instruction_data
                })
            }
            Self::SwapOut { extra, max_amount_in, amount_out, accounts } => {
                let mut instruction_data = [].to_vec();
                instruction_data.extend(extra);
                instruction_data.extend_from_slice(&max_amount_in.to_le_bytes());
                instruction_data.extend_from_slice(&amount_out.to_le_bytes());
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
    /// * `program_id_index` - Index of the Raydium CPMM program in the static keys
    /// * `swap_in_out` - Whether to swap the input and output tokens
    /// 
    /// # Returns
    /// * `MevResult<CompiledInstruction>` - The compiled sandwich buy instruction
    pub fn create_sandwich_buy(&self, new_sender: &Pubkey, static_keys: &[Pubkey], program_id_index: u8, swap_in_out: bool) -> MevResult<CompiledInstruction> {
        match self {
            Self::SwapIn { extra, amount, min_amount_out, accounts } => {
                // Calculate optimal sandwich size based on price analysis
                // Higher slippage means more profitable opportunity
                let price_ratio = (*min_amount_out as f64) / (*amount as f64);
                
                // Use 15% of the original amount as our sandwich size
                let slippage_factor = 0.15;
                let sandwich_amount = (*amount as f64 * slippage_factor) as u64;
                
                // Set a very low min_amount_out to ensure our transaction executes
                // We accept 90% of the theoretical output to ensure execution
                let min_output_factor = 0.9;
                let sandwich_min_out = (sandwich_amount as f64 * price_ratio * min_output_factor) as u64;
                
                // Create a new Vec of accounts for sandwich instruction
                let sandwich_accounts: Vec<Account> = accounts.iter().map(|a| {
                    Account {
                        account_index: a.account_index,
                        is_writable: a.is_writable
                    }
                }).collect();
                
                // Create a new SwapIn instruction with our parameters
                let sandwich_swap = Self::SwapIn {
                    extra: extra.clone(),
                    amount: sandwich_amount,
                    min_amount_out: sandwich_min_out,
                    accounts: sandwich_accounts,
                };
                
                // Get updated account list with our address
                let _mutated_accounts = self.mutate_accounts(static_keys, new_sender, swap_in_out)?;
                
                // Convert to a compiled instruction
                let ix = sandwich_swap.to_compiled_instruction(program_id_index)?;
                
                Ok(ix)
            },
            Self::SwapOut { extra, max_amount_in, amount_out, accounts } => {
                // Calculate price ratio
                let price_ratio = (*max_amount_in as f64) / (*amount_out as f64);
                
                // Use 15% of the original amount as our sandwich size
                let slippage_factor = 0.15;
                let sandwich_out = (*amount_out as f64 * slippage_factor) as u64;
                
                // Set max_amount_in with some extra margin (10%) to ensure transaction executes
                let sandwich_max_in = (sandwich_out as f64 * price_ratio * 1.1) as u64;
                
                // Create a new Vec of accounts for sandwich instruction
                let sandwich_accounts: Vec<Account> = accounts.iter().map(|a| {
                    Account {
                        account_index: a.account_index,
                        is_writable: a.is_writable
                    }
                }).collect();
                
                // Create a new SwapOut instruction with our parameters
                let sandwich_swap = Self::SwapOut {
                    extra: extra.clone(),
                    max_amount_in: sandwich_max_in,
                    amount_out: sandwich_out,
                    accounts: sandwich_accounts,
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
    /// * `program_id_index` - Index of the Raydium CPMM program in the static keys
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
            Self::SwapIn { extra, amount, min_amount_out, accounts } => {
                // Calculate the original price ratio
                let original_price = (*min_amount_out as f64) / (*amount as f64);
                
                // When selling, we want to maximize our output while ensuring execution
                // Accept 90% of the theoretical value
                let min_output_factor = 0.9;
                let min_out = (token_amount as f64 * original_price * min_output_factor) as u64;
                
                // Create a new Vec of accounts for sandwich instruction, but we need to reverse the direction
                let sandwich_accounts: Vec<Account> = accounts.iter().map(|a| {
                    Account {
                        account_index: a.account_index,
                        is_writable: a.is_writable
                    }
                }).collect();
                
                // Create a new SwapIn instruction with our parameters - direction is reversed from buy
                let sandwich_swap = Self::SwapIn {
                    extra: extra.clone(),
                    amount: token_amount,
                    min_amount_out: min_out,
                    accounts: sandwich_accounts,
                };
                
                // Get updated account list with our address
                // Note: We use !swap_in_out to reverse the direction compared to our buy
                let _mutated_accounts = self.mutate_accounts(static_keys, new_sender, !swap_in_out)?;
                
                // Convert to a compiled instruction
                let ix = sandwich_swap.to_compiled_instruction(program_id_index)?;
                
                Ok(ix)
            },
            Self::SwapOut { extra, max_amount_in, amount_out, accounts } => {
                // Calculate the original price ratio
                let original_price = (*max_amount_in as f64) / (*amount_out as f64);
                
                // For our sell, we want to get a specific amount out while keeping the max in reasonable
                // We'll add 10% margin to ensure execution
                let max_in_factor = 1.1;
                let max_in = (token_amount as f64 * original_price * max_in_factor) as u64;
                
                // Create a new Vec of accounts for sandwich instruction
                let sandwich_accounts: Vec<Account> = accounts.iter().map(|a| {
                    Account {
                        account_index: a.account_index,
                        is_writable: a.is_writable
                    }
                }).collect();
                
                // Create a new SwapOut instruction with our parameters - direction is reversed from buy
                let sandwich_swap = Self::SwapOut {
                    extra: extra.clone(),
                    max_amount_in: max_in,
                    amount_out: token_amount,
                    accounts: sandwich_accounts,
                };
                
                // Get updated account list with our address
                // Note: We use !swap_in_out to reverse the direction compared to our buy
                let _mutated_accounts = self.mutate_accounts(static_keys, new_sender, !swap_in_out)?;
                
                // Convert to a compiled instruction
                let ix = sandwich_swap.to_compiled_instruction(program_id_index)?;
                
                Ok(ix)
            }
        }
    }
    
    pub fn mint_in(&self, static_keys: &[Pubkey], swap_in_out: bool) -> Pubkey {
        match self {
            Self::SwapIn { accounts, .. } | Self::SwapOut { accounts, .. } => {
                match swap_in_out {
                    true => static_keys[accounts[11].account_index as usize],
                    false => static_keys[accounts[10].account_index as usize]
                }
            }
        }
    }
    
    pub fn mint_out(&self, static_keys: &[Pubkey], swap_in_out: bool) -> Pubkey {
        match self {
            Self::SwapIn { accounts, .. } | Self::SwapOut { accounts, .. } => {
                match swap_in_out {
                    true => static_keys[accounts[10].account_index as usize],
                    false => static_keys[accounts[11].account_index as usize]
                }
            }
        }
    }
    
    pub fn mutate_accounts(&self, static_keys: &[Pubkey], new_sender: &Pubkey, swap_in_out: bool) -> MevResult<Vec<Pubkey>> {
        match self {
            Self::SwapIn { accounts, .. } | Self::SwapOut { accounts, .. } => {
                Ok(static_keys
                    .iter()
                    .map(|k| {
                        if k == &static_keys[0] { // update signer
                            return *new_sender
                        } else if k == &static_keys[accounts[4].account_index as usize] { // update input account
                            return spl_associated_token_account::get_associated_token_address(
                                new_sender, 
                                &self.mint_in(static_keys, swap_in_out)
                            )
                        } else if k == &static_keys[accounts[5].account_index as usize] { // swap output account
                            return spl_associated_token_account::get_associated_token_address(
                                new_sender, 
                                &self.mint_out(static_keys, swap_in_out)
                            )
                        } else if swap_in_out && k == &static_keys[accounts[10].account_index as usize] { // swap mint in
                            return static_keys[accounts[11].account_index as usize]
                        } else if swap_in_out && k == &static_keys[accounts[11].account_index as usize] { // swap mint out
                            return static_keys[accounts[10].account_index as usize]
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

    use super::ParsedRaydiumCpmmInstructions;
    use super::super::super::Account;

    #[test]
    fn deserialize_cpmm_in_instruction() {
        let ix = [143, 190, 90, 218, 196, 30, 51, 222, 95, 135, 138, 119, 80, 172, 1, 0, 173, 117, 123, 11, 0, 0, 0, 0].to_vec();
        let accounts = [0, 10, 11, 1, 2, 3, 4, 5, 12, 12, 13, 14, 6];
        let static_keys: Vec<Pubkey> = [
            "9ma2VmxyRQBJYu7BJJuuTs4yakAvMqpKRHGeokt6ktcf", 
            "3YjqrC6qvT6FVoKxGtSHgFVzuJ6pQR94KyFXG39TUqGz", 
            "GU69LEWmJh6Bp8XtUS3KKhGaCxhyVapWCQEcmU5bZBwq", 
            "7LiVoHWxHDmQa3xVRcpQ28fMM4bDQLYwx3j5DFvPAo8h", 
            "CHW9Y1iGxHUcsXviaNKKtEoDTXHKyAbRU3fzmJx9tczs", 
            "7Zf48GqhRgp2zQmUZeToLN8AtHzwjEZnkwtnigKh6Jin", 
            "Cwpve1YxWABByKTSmqgDn6ZA4P7E4A957woRnH9sa1xF", 
            "9yMwSPk9mrXSN7yDHUuZurAh1sjbJsfpUqjZ7SvVtdco", 
            "ComputeBudget111111111111111111111111111111", 
            "CPMMoo8L3F4NbTegBCKVNunggL7H1ZpdTHKxQB5qKP1C", 
            "GpMZbSM2GgvTKHJirzeGfMFoaZ8UR2X7F4v8vHTvxFbL", 
            "G95xxie3XbkCqtE39GgQ9Ggc7xBC8Uceve7HFDEFApkc", 
            "TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA", 
            "BQwWyj3ukK6EE4nJXc9HGtjngfJmFWdSk66e4zvboop", 
            "So11111111111111111111111111111111111111112", 
            "troY36YiPGqMyAYCNbEqYCdN2tb91Zf7bHcQt7KUi61", 
            "jitodontfront111111111111111119657491111111", 
            "11111111111111111111111111111111"
        ].iter().map(|k| Pubkey::from_str_const(k)).collect();
        let target = ParsedRaydiumCpmmInstructions::from_bytes(ix, accounts.iter().map(|i| Account::new(i, false)).collect()).unwrap();
        assert_eq!(
            target,
            ParsedRaydiumCpmmInstructions::SwapIn { 
                extra: [143, 190, 90, 218, 196, 30, 51, 222].to_vec(),
                amount: 470936579639135, 
                min_amount_out: 192640429,
                accounts: accounts.iter().map(|i| Account::new(i, false)).collect(), 
            }
        );
        let input_mint = target.mint_in(&static_keys, false);
        assert_eq!(
            input_mint, 
            Pubkey::from_str_const("BQwWyj3ukK6EE4nJXc9HGtjngfJmFWdSk66e4zvboop"),
            "SwapIn input mint should be BQwWyj3ukK6EE4nJXc9HGtjngfJmFWdSk66e4zvboop"
        );
        
        // With swapping
        let swapped_input_mint = target.mint_in(&static_keys, true);
        assert_eq!(
            swapped_input_mint, 
            Pubkey::from_str_const("So11111111111111111111111111111111111111112"),
            "SwapIn swapped input mint should be SOL"
        );
    }
    
    #[test]
    fn deserialize_cpmm_out_instruction() {
        let ix = [
            55, 217, 98, 86, 163, 74, 180, 173,
            188, 77, 0, 0, 0, 0, 0, 0, 
            222, 163, 133, 148, 3, 0, 0, 0
        ].to_vec();
        let accounts = [1, 13, 14, 3, 2, 4, 5, 6, 9, 15, 10, 16, 7];
        let static_keys: Vec<Pubkey> = [
            "FaXGzwdFzLNWnzWWx9tSeBMuY9w4mNkZHxyzuFaLomdk", 
            "EgX31zDChrxDVr8asxE9Gpn3pArMk3rDPXyoUyQmaPZv", 
            "CeHMMW3SnhjNSX1gCwP3XaNqyELH3MxHbHeLNCcizvM5", 
            "4ZmGmYqnno7VDpo1igbA1n7nLWmkKZ4x6dcE6yCYuGsR", 
            "8SThroVVEwF9FWrqF2sVUNEigyfX4DHZm2vXX6c52ngX", 
            "HXyVaZ9DBrxqTTG5Q1oMdxEn4G4XrVaCoQQXYdGrGVh3", 
            "4V9kShWisjWA2DNcqqc9mgEsuwws9bnWX2caqb74QVGD",
            "DBMUzT2Ndw3g3SsUcrw5in7uWp6kCksZSqYu2gh9sBAs", 
            "11111111111111111111111111111111", 
            "TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA",
            "So11111111111111111111111111111111111111112", 
            "SysvarRent111111111111111111111111111111111", 
            "CPMMoo8L3F4NbTegBCKVNunggL7H1ZpdTHKxQB5qKP1C",
            "GpMZbSM2GgvTKHJirzeGfMFoaZ8UR2X7F4v8vHTvxFbL",
            "2fGXL8uhqxJ4tpgtosHZXT4zcQap6j62z3bMDxdkMvy5", 
            "TokenzQdBNbLqP5VEhdkAS6EPFLC1PHnBqCXEpPxuEb", 
            "5rX6WRhezyszRDQKSAAKMHDdH83GTYeyWESrQ8AyZREV"
        ].iter().map(|k| Pubkey::from_str_const(k)).collect();
        let target = ParsedRaydiumCpmmInstructions::from_bytes(ix, accounts.iter().map(|i| Account::new(i, false)).collect()).unwrap();
        assert_eq!(
            target,
            ParsedRaydiumCpmmInstructions::SwapOut { 
                extra: [55, 217, 98, 86, 163, 74, 180, 173].to_vec(),
                max_amount_in: 19900, 
                amount_out: 15376688094,
                accounts: accounts.iter().map(|i| Account::new(i, false)).collect(), 
            }
        );
        let input_mint = target.mint_in(&static_keys, false);
        assert_eq!(
            input_mint, 
            Pubkey::from_str_const("So11111111111111111111111111111111111111112"),
            "SwapOut input mint should be SOL"
        );
        
        // With swapping
        let swapped_input_mint = target.mint_in(&static_keys, true);
        assert_eq!(
            swapped_input_mint, 
            Pubkey::from_str_const("5rX6WRhezyszRDQKSAAKMHDdH83GTYeyWESrQ8AyZREV"),
            "SwapOut swapped input mint should be 5rX6WRhezyszRDQKSAAKMHDdH83GTYeyWESrQ8AyZREV"
        );
    }
}