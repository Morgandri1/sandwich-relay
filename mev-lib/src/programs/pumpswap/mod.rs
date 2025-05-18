use solana_sdk::pubkey::Pubkey;

use crate::result::{MevError, MevResult};
use super::Account;

pub const PUMPSWAP_PROGRAM_ID: Pubkey = Pubkey::from_str_const("pAMMBay6oceH9fJKBRHGP5D4bD4sWpmSwMn52FMfXEA");

#[derive(Debug, PartialEq)]
pub enum ParsedPumpSwapInstructions {
    /// Buy is a LIE. base is (almost?) always WSOL. Buy === SwapBaseOut
    Buy {
        discriminator: Vec<u8>,
        base_amount_out: u64,
        max_quote_amount_in: u64,
        accounts: Vec<Account>
    },
    /// SwapBaseIn
    Sell {
        discriminator: Vec<u8>,
        base_amount_in: u64,
        min_quote_amount_out: u64,
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
        
        match bytes[..8] {
            [102, 6, 61, 18, 1, 218, 235, 234] => Ok(Self::Buy {
                discriminator: bytes[..8].to_vec(),
                max_quote_amount_in: u64::from_le_bytes(amount_in_bytes),
                base_amount_out: u64::from_le_bytes(min_out_bytes),
                accounts
            }),
            [51, 230, 133, 164, 1, 127, 131, 173] => Ok(Self::Sell { 
                discriminator: bytes[..8].to_vec(), 
                base_amount_in: u64::from_le_bytes(min_out_bytes), 
                min_quote_amount_out: u64::from_le_bytes(amount_in_bytes), 
                accounts
            }),
            _ => Err(MevError::ValueError)
        }
    }
    
    pub fn base_mint(&self, static_keys: &[Pubkey]) -> MevResult<Pubkey> {
        match self {
            Self::Buy { accounts, .. } | Self::Sell { accounts, .. } => Ok(static_keys[accounts[3].account_index as usize])
        }
    }
    
    pub fn quote_mint(&self, static_keys: &[Pubkey]) -> MevResult<Pubkey> {
        match self {
            Self::Buy { accounts, .. } | Self::Sell { accounts, .. } => Ok(static_keys[accounts[4].account_index as usize])
        }
    }
    
    pub fn mint_in(&self, static_keys: &[Pubkey]) -> MevResult<Pubkey> {
        match self {
            Self::Buy { .. } => self.quote_mint(static_keys),
            Self::Sell { .. } => self.base_mint(static_keys)
        }
    }
    
    pub fn mint_out(&self, static_keys: &[Pubkey]) -> MevResult<Pubkey> {
        match self {
            Self::Buy { .. } => self.base_mint(static_keys),
            Self::Sell { .. } => self.quote_mint(static_keys)
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
            target.base_mint(static_keys.as_slice()).unwrap().to_string().as_str(), 
            "So11111111111111111111111111111111111111112"
        );
        assert_eq!(
            target.quote_mint(static_keys.as_slice()).unwrap().to_string().as_str(), 
            "BXPwhbMYw4kYcD1d1de3mNkxA9Gk5uwh2Zfck4urFb7c"
        );
    }
    
    #[test]
    fn new_pumpswap_buy() {
        let ix = [
            102, 6, 61, 18, 1, 218, 235, 234,
            62, 162, 114, 41, 0, 0, 0, 0, 
            96, 92, 166, 246, 201, 0, 0, 0
        ].to_vec();
        let accounts = [13, 0, 14, 9, 15, 1, 2, 3, 4, 16, 5, 11, 11, 10, 8, 17, 12, 6, 18].to_vec();
        let keys: Vec<Pubkey> = [
            "Ch2AVqGPZbr9Nfa9hDpxxXm7cJGMz59oTvApvnHdrrbD", 
            "4Fy4RBAWyDFbFHnqqfX1ar7pmn72rBB3x9BFiUR7Va4w", 
            "DhfaiQHmrEAhcehzrzhhpd5ktfwMNSEzZtqtXJezVYKL",
            "6BR1Y2ihYjha1vNSU856PtnVD7RLXAgsBewJk2Y9Wyhh", 
            "2xoPJcg9dQHhVo4sgthFULiHTkMu6byLTUYqCnfFYJPq", 
            "DWWmJuB5psvteS91HTba3rZD3rBKzKLnCRXDH72P64Y6", 
            "AcGWugWDSEn1mcemBivWyc6T5MyycfWA4qL5rSZEDvuE", 
            "DttWaMuVvTiduZRnguLF7jNxTgiMBZ1hyAumKUiL2KRL", 
            "ATokenGPvbdGVxr1b2hvZbsiqW5xWH25efTNsLJA8knL", 
            "So11111111111111111111111111111111111111112", 
            "11111111111111111111111111111111", 
            "TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA", 
            "pAMMBay6oceH9fJKBRHGP5D4bD4sWpmSwMn52FMfXEA", 
            "BXVESLSEdtXn3qFL4BhiDHpZLgP81tPZvX4hVxzboTpw", 
            "ADyA8hdefvWN2dbGGWFotbzWxrAvLW83WG6QCVXvJKqw", 
            "HrJCv9sJV2587twQWqswCXLGc9oYE7QGxEiw2FVc61Hx", 
            "JCRGumoE9Qi5BBgULTgdgTLjSgkCMSbF62ZZfGs84JeU", 
            "GS4CU59F31iL7aR2Q8zVS8DRrcRnXX1yjQ66TqNVQnaR", 
            "8N3GDaZ2iwN65oxVatKTLPNooAVUJTbfiVJ1ahyqwjSk"
        ].iter().map(|k| Pubkey::from_str_const(k)).collect();
        let target = ParsedPumpSwapInstructions::from_bytes(
            ix, 
            accounts.iter().map(|i| Account::new(i, false)).collect()
        ).unwrap();
        assert_eq!(
            target,
            ParsedPumpSwapInstructions::Buy {
                discriminator: [102, 6, 61, 18, 1, 218, 235, 234].to_vec(),
                max_quote_amount_in: 867426524256,
                base_amount_out: 695378494,
                accounts: accounts.iter().map(|i| Account::new(i, false)).collect()
            }
        );
        assert_eq!(
            target.base_mint(keys.as_slice()).unwrap().to_string().as_str(), 
            "So11111111111111111111111111111111111111112"
        );
        assert_eq!(
            target.quote_mint(keys.as_slice()).unwrap().to_string().as_str(), 
            "HrJCv9sJV2587twQWqswCXLGc9oYE7QGxEiw2FVc61Hx"
        );
    }
    
    #[test]
    fn new_pumpswap_sell() {
        let ix = [
            51, 230, 133, 164, 1, 127, 131, 173, 
            51, 226, 204, 81, 0, 0, 0, 0, 
            173, 225, 227, 139, 28, 0, 0, 0
        ].to_vec();
        let accounts = [12, 0, 13, 8, 14, 1, 2, 3, 4, 15, 5, 10, 10, 9, 7, 16, 11, 6, 17].to_vec();
        let keys: Vec<Pubkey> = [
            "HJeAnGFrPyJCHhgq4Ffv7Koqm5Ka3w2VGVDcqhXRLFbW",
            "3abg88ETLCG3sNKdRz4g3q1cqC62SxeWqjiPQsuGBsB9", 
            "7QQUSJECvtbAAdwz2Gy7gLdZBC4ddbHmc2Ax61SNWbbn", 
            "3oa5WzrpkK5VkQARbwXJvu76cufBQ8MLntbLRTerdbHg", 
            "GFDqJSsVqAKH1gpH4eay2gELCdfL9pCHdAVsmrx12yor", 
            "JA5qJKsp7EsGaxwG3hSALyiVYFYYqBDrDNc35X8F8i3W", 
            "49JHbnCMLo4m9SQYgMdzZjhHHYJiUUsRAfMjEVfT6bwR", 
            "ATokenGPvbdGVxr1b2hvZbsiqW5xWH25efTNsLJA8knL", 
            "So11111111111111111111111111111111111111112", 
            "11111111111111111111111111111111", 
            "TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA", 
            "pAMMBay6oceH9fJKBRHGP5D4bD4sWpmSwMn52FMfXEA", 
            "2cv5y1hQnhfByhADjQJFLGMt2Thc56SpaUFVDN955stC", 
            "ADyA8hdefvWN2dbGGWFotbzWxrAvLW83WG6QCVXvJKqw", 
            "2Y6r9CniLauNVVThwaLoZie6P6eXAs67hcf3ZSXxEZyi", 
            "JCRGumoE9Qi5BBgULTgdgTLjSgkCMSbF62ZZfGs84JeU", 
            "GS4CU59F31iL7aR2Q8zVS8DRrcRnXX1yjQ66TqNVQnaR", 
            "8N3GDaZ2iwN65oxVatKTLPNooAVUJTbfiVJ1ahyqwjSk"
        ].iter().map(|k| Pubkey::from_str_const(k)).collect();
        let target = ParsedPumpSwapInstructions::from_bytes(
            ix, 
            accounts.iter().map(|i| Account::new(i, false)).collect()
        ).unwrap();
        assert_eq!(
            target,
            ParsedPumpSwapInstructions::Sell {
                discriminator: [51, 230, 133, 164, 1, 127, 131, 173].to_vec(),
                base_amount_in: 1372381747,
                min_quote_amount_out: 122606051757,
                accounts: accounts.iter().map(|i| Account::new(i, false)).collect()
            }
        );
        assert_eq!(
            target.base_mint(keys.as_slice()).unwrap().to_string().as_str(), 
            "So11111111111111111111111111111111111111112"
        );
        assert_eq!(
            target.quote_mint(keys.as_slice()).unwrap().to_string().as_str(), 
            "2Y6r9CniLauNVVThwaLoZie6P6eXAs67hcf3ZSXxEZyi"
        );
    }
}