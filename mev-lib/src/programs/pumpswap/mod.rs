use solana_sdk::{instruction::CompiledInstruction, pubkey::Pubkey};

use crate::result::{MevError, MevResult};
use super::Account;

pub const PUMPSWAP_PROGRAM_ID: Pubkey = Pubkey::from_str_const("pAMMBay6oceH9fJKBRHGP5D4bD4sWpmSwMn52FMfXEA");

#[derive(Debug, PartialEq)]
pub enum ParsedPumpSwapInstructions {
    /// 0
    Buy {
        discriminator: Vec<u8>,
        base_amount_out: u64,
        max_quote_amount_in: u64,
        accounts: Vec<Account>
    }
}

impl ParsedPumpSwapInstructions {
    pub fn from_bytes(bytes: Vec<u8>, accounts: Vec<Account>) -> MevResult<Self> {
        if bytes.len() < 24 {
            return Err(crate::result::MevError::FailedToDeserialize);
        };
        let mut amount_in_bytes = [0u8; 8];
        let mut min_out_bytes = [0u8; 8];
        
        // Copy the bytes into properly sized arrays for conversion
        min_out_bytes[..8].copy_from_slice(&bytes[8..16]);
        amount_in_bytes[..8].copy_from_slice(&bytes[16..]);
        
        return Ok(Self::Buy {
            discriminator: bytes[..8].to_vec(),
            max_quote_amount_in: u64::from_le_bytes(amount_in_bytes),
            base_amount_out: u64::from_le_bytes(min_out_bytes),
            accounts
        })
    }
    
    pub fn to_compiled_instruction(&self, program_id: u8) -> MevResult<CompiledInstruction> {
        match self {
            Self::Buy { base_amount_out, max_quote_amount_in, accounts, discriminator } => {
                let mut instruction_data = discriminator.clone();
                instruction_data.extend_from_slice(&base_amount_out.to_le_bytes());
                instruction_data.extend_from_slice(&max_quote_amount_in.to_le_bytes());
                return Ok(CompiledInstruction { 
                    program_id_index: program_id, 
                    accounts: accounts.iter().map(|a| a.account_index).collect(), 
                    data: instruction_data
                })
            },
            _ => Err(MevError::ValueError)
        }
    }
    
    pub fn mint_in(&self, static_keys: &[Pubkey]) -> MevResult<Pubkey> {
        match self {
            Self::Buy { accounts, .. } => Ok(static_keys[accounts[3].account_index as usize])
        }
    }
    
    pub fn mint_out(&self, static_keys: &[Pubkey]) -> MevResult<Pubkey> {
        match self {
            Self::Buy { accounts, .. } => Ok(static_keys[accounts[4].account_index as usize])
        }
    }
    
    pub fn mutate_accounts(&self, static_keys: &[Pubkey], new_sender: &Pubkey, swap_in_out: bool) -> MevResult<Vec<Pubkey>> {
        match self {
            Self::Buy { accounts, .. } => {
                Ok(static_keys
                    .iter()
                    .map(|k| {
                        if k == &static_keys[0] { // swap signer
                            return *new_sender
                        } else if k == &static_keys[accounts[6].account_index as usize] { // swap output account
                            return spl_associated_token_account::get_associated_token_address(
                                new_sender, 
                                &static_keys[accounts[4].account_index as usize]
                            )
                        } else if k == &static_keys[accounts[5].account_index as usize] { // swap input account
                            return spl_associated_token_account::get_associated_token_address(
                                new_sender, 
                                &static_keys[accounts[3].account_index as usize]
                            )
                        } else if swap_in_out && k == &static_keys[accounts[3].account_index as usize] { // swap mint in
                            return static_keys[accounts[4].account_index as usize]
                        } else if swap_in_out && k == &static_keys[accounts[4].account_index as usize] { // swap mint out
                            return static_keys[accounts[3].account_index as usize]
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

    use crate::programs::Account;
    use super::ParsedPumpSwapInstructions;

    #[test]
    fn deserialize_pumpswap_buy_instruction() {
        let sample_ix = [
            102, 6, 61, 18, 1, 218, 235, 234, // discriminator?
            177, 121, 106, 44, 0, 0, 0, 0, 
            162, 191, 206, 130, 28, 0, 0, 0
        ].to_vec();
        let key_i: Vec<u8> = [11, 0, 12, 7, 13, 1, 2, 3, 4, 14, 5, 9, 9, 8, 6, 15, 10].to_vec();
        let target = ParsedPumpSwapInstructions::from_bytes(
            sample_ix, 
            key_i.iter().map(|i| Account::new(i, false)).collect()
        ).unwrap();
        let static_keys: Vec<Pubkey> = [
            "4nVJwya7ZGAphcTHsME7DMCsiNS66evAAgzo1KjJ3xQX", 
            "B19esomyECurxPNErq1fwrE9sQuuiKkeovzjecnB8f6T", 
            "E5n2CW7bKG993iKx7TLQzbDxFPFwxF4bFzbZEagySR4c", 
            "2jUKvTjyT8jxjjUkj6zeAQcZvfVSEhr5PT3qnr2QRhLB", 
            "9LLsBUwHCUvD1ARN9wDeXMb76JRs5UZeneNm1ebDbR5X", 
            "2Qh3YSmraSJAJUCvHs4xHN2QHfAFpBE7qonpmY5zCNje", 
            "ATokenGPvbdGVxr1b2hvZbsiqW5xWH25efTNsLJA8knL", 
            "So11111111111111111111111111111111111111112", 
            "11111111111111111111111111111111", 
            "TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA", 
            "pAMMBay6oceH9fJKBRHGP5D4bD4sWpmSwMn52FMfXEA", 
            "Y7KL4eshwpxkNSAUaoMBnE1QyMVuzX6pyogmAhcoVUp", 
            "ADyA8hdefvWN2dbGGWFotbzWxrAvLW83WG6QCVXvJKqw", 
            "BXPwhbMYw4kYcD1d1de3mNkxA9Gk5uwh2Zfck4urFb7c", 
            "FWsW1xNtWscwNmKv6wVsU1iTzRN6wmmk3MjxRP5tT7hz", 
            "GS4CU59F31iL7aR2Q8zVS8DRrcRnXX1yjQ66TqNVQnaR"
        ].iter().map(|k| Pubkey::from_str_const(k)).collect();
        assert_eq!(
            target,
            ParsedPumpSwapInstructions::Buy {
                discriminator: [102, 6, 61, 18, 1, 218, 235, 234].to_vec(),
                max_quote_amount_in: 122453671842,
                base_amount_out: 745175473,
                accounts: key_i.iter().map(|i| Account::new(i, false)).collect()
            }
        );
        assert_eq!(
            target.mint_in(static_keys.as_slice()).unwrap().to_string().as_str(), 
            "So11111111111111111111111111111111111111112"
        );
        assert_eq!(
            target.mint_out(static_keys.as_slice()).unwrap().to_string().as_str(), 
            "BXPwhbMYw4kYcD1d1de3mNkxA9Gk5uwh2Zfck4urFb7c"
        );
        let mutated = target.mutate_accounts(
            static_keys.as_slice(), 
            &Pubkey::from_str_const("11111111111111111111111111111111"), 
            true
        ).unwrap();
        assert_eq!(
            target.mint_in(mutated.as_slice()).unwrap(),
            Pubkey::from_str_const("BXPwhbMYw4kYcD1d1de3mNkxA9Gk5uwh2Zfck4urFb7c")
        );
        assert_eq!(
            mutated[0],
            Pubkey::from_str_const("11111111111111111111111111111111")
        )
    }
}