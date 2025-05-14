use solana_sdk::pubkey::Pubkey;

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
}

#[cfg(test)]
mod test {
    use super::ParsedRaydiumCpmmInstructions;
    use super::super::super::Account;

    #[test]
    fn deserialize_cpmm_in_instruction() {
        let ix = [143, 190, 90, 218, 196, 30, 51, 222, 95, 135, 138, 119, 80, 172, 1, 0, 173, 117, 123, 11, 0, 0, 0, 0].to_vec();
        let accounts = [0, 10, 11, 1, 2, 3, 4, 5, 12, 12, 13, 14, 6];
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
    }
    
    #[test]
    fn deserialize_cpmm_out_instruction() {
        let ix = [
            55, 217, 98, 86, 163, 74, 180, 173,
            188, 77, 0, 0, 0, 0, 0, 0, 
            222, 163, 133, 148, 3, 0, 0, 0
        ].to_vec();
        let accounts = [1, 13, 14, 3, 2, 4, 5, 6, 9, 15, 10, 16, 7];
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
    }
}