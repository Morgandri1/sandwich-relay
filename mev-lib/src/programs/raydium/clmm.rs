use solana_sdk::{instruction::CompiledInstruction, pubkey::Pubkey};

use crate::{programs::Account, result::MevResult};

pub const RAYDIUM_AMM_PROGRAM_ID: Pubkey = Pubkey::from_str_const("CPMMoo8L3F4NbTegBCKVNunggL7H1ZpdTHKxQB5qKP1C");

pub enum RaydiumClmmInstructions {
    
}

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
        if bytes.len() < 33 {
            return Err(crate::result::MevError::FailedToDeserialize);
        };
        let mut amount_in_bytes = [0u8; 8];
        let mut min_out_bytes = [0u8; 8];
        let mut sqrt_thing_bytes = [0u8; 16];
        
        // Copy the bytes into properly sized arrays for conversion
        min_out_bytes[..8].copy_from_slice(&bytes[1..9]);
        amount_in_bytes[..8].copy_from_slice(&bytes[9..17]);
        sqrt_thing_bytes[..16].copy_from_slice(&bytes[17..33]);
        
        return Ok(Self::Swap {
            amount: u64::from_le_bytes(amount_in_bytes),
            other_amount_threshold: u64::from_le_bytes(min_out_bytes),
            sqrt_price_limit_64: u128::from_le_bytes(sqrt_thing_bytes),
            is_base_input: bytes[32] == 1,
            accounts
        })
    }
    
    pub fn to_compiled_instruction(&self, program_id: u8) -> MevResult<CompiledInstruction> {
        match self {
            Self::Swap { amount, other_amount_threshold, accounts, sqrt_price_limit_64, is_base_input } => {
                let mut instruction_data = [0u8].to_vec();
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
    
    pub fn mint_in(&self, static_keys: &[Pubkey], swap_in_out: bool) -> Pubkey {
        match self {
            Self::Swap { accounts, .. } => {
                match swap_in_out {
                    true => static_keys[accounts[9].account_index as usize],
                    false => static_keys[accounts[8].account_index as usize]
                }
            }
        }
    }
    
    pub fn mint_out(&self, static_keys: &[Pubkey], swap_in_out: bool) -> Pubkey {
        match self {
            Self::Swap { accounts, .. } => {
                match swap_in_out {
                    true => static_keys[accounts[8].account_index as usize],
                    false => static_keys[accounts[9].account_index as usize]
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
                        } else if swap_in_out && k == &static_keys[accounts[8].account_index as usize] { // swap mint in
                            return static_keys[accounts[9].account_index as usize]
                        } else if swap_in_out && k == &static_keys[accounts[9].account_index as usize] { // swap mint out
                            return static_keys[accounts[8].account_index as usize]
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
    use crate::programs::{raydium::ParsedRaydiumClmmInstructions, Account};

    #[test]
    fn deserialize_clmm_instruction() {
        let ix = [
            1, 132, 183, 67, 17, 0, 0, 0, // amount
            0, 94, 175, 176, 155, 27, 0, 0, // otherAmountThreshold
            0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, // sqrtPriceLimitX64
            0 // isBaseInput
        ].to_vec();
        let accounts = [17, 20, 18, 19, 0, 16, 1, 3, 2, 4, 5, 6, 7, 8, 9, 15, 10, 11, 12];
        assert_eq!(
            ParsedRaydiumClmmInstructions::from_bytes(ix, accounts.iter().map(|i| Account::new(i, false)).collect()).unwrap(),
            ParsedRaydiumClmmInstructions::Swap { 
                amount: 118576164702, 
                other_amount_threshold: 289650564, 
                accounts: accounts.iter().map(|i| Account::new(i, false)).collect(), 
                sqrt_price_limit_64: 0, 
                is_base_input: false
            }
        )
    }
}