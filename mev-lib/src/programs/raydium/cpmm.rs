use solana_sdk::{instruction::CompiledInstruction, pubkey::Pubkey};

use crate::{programs::Account, result::{MevError, MevResult}};

pub const RAYDIUM_CPMM_PROGRAM_ID: Pubkey = Pubkey::from_str_const("CPMMoo8L3F4NbTegBCKVNunggL7H1ZpdTHKxQB5qKP1C");

pub enum RaydiumCpmmInstructions {
    
}

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