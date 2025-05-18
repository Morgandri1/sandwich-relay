use solana_sdk::pubkey::Pubkey;

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
    
    pub fn mint_in(&self, static_keys: &[Pubkey]) -> MevResult<Pubkey> {
        match self {
            Self::Swap { accounts, .. } => Ok(static_keys[accounts[11].account_index as usize])
        }
    }

    pub fn mint_out(&self, static_keys: &[Pubkey]) -> MevResult<Pubkey> {
        match self {
            Self::Swap { accounts, .. } => Ok(static_keys[accounts[12].account_index as usize])
        }
    }
}

#[cfg(test)]
mod test {
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
    }
}