use solana_sdk::{instruction::CompiledInstruction, pubkey::Pubkey};

use crate::{result::{MevError, MevResult}, rpc::get_mint_of_account};
use super::super::Account;

pub const STABLE_SWAP_PROGRAM_ID: Pubkey = Pubkey::from_str_const("5quBtoiQqxF9Jv6KYKctB59NT3gtJD2Y65kdnB1Uev3h");

#[derive(Debug, PartialEq)]
pub enum ParsedRaydiumStableSwapInstructions {
    /// 0
    Swap {
        instruction: u8,
        amount_in: u64,
        minimum_amount_out: u64,
        accounts: Vec<Account>
    }
}

impl ParsedRaydiumStableSwapInstructions {
    pub fn from_bytes(bytes: Vec<u8>, accounts: Vec<Account>) -> MevResult<Self> {
        if bytes.len() <= 16 {
            return Err(crate::result::MevError::FailedToDeserialize);
        };
        let mut amount_in_bytes = [0u8; 8];
        let mut min_out_bytes = [0u8; 8];
        
        // Copy the bytes into properly sized arrays for conversion
        amount_in_bytes[..8].copy_from_slice(&bytes[1..9]);
        min_out_bytes[..8].copy_from_slice(&bytes[9..17]);
        
        return Ok(Self::Swap {
            instruction: bytes[0],
            amount_in: u64::from_le_bytes(amount_in_bytes),
            minimum_amount_out: u64::from_le_bytes(min_out_bytes),
            accounts
        })
    }
    
    pub fn to_compiled_instruction(&self, program_id: u8) -> MevResult<CompiledInstruction> {
        match self {
            Self::Swap { amount_in, minimum_amount_out, accounts, instruction } => {
                let mut instruction_data = [*instruction].to_vec();
                instruction_data.extend_from_slice(&amount_in.to_le_bytes());
                instruction_data.extend_from_slice(&minimum_amount_out.to_le_bytes());
                return Ok(CompiledInstruction { 
                    program_id_index: program_id, 
                    accounts: accounts.iter().map(|a| a.account_index).collect(), 
                    data: instruction_data
                })
            },
            _ => Err(MevError::ValueError)
        }
    }
    
    pub fn mutate_accounts(&self, static_keys: &[Pubkey], new_sender: &Pubkey, swap_in_out: bool) -> MevResult<Vec<Pubkey>> {
        match self {
            Self::Swap { accounts, .. } => {
                let mint_in = get_mint_of_account(&static_keys[accounts[5].account_index as usize])?;
                let mint_out = get_mint_of_account(&static_keys[accounts[6].account_index as usize])?;
                let mut i: Vec<Pubkey> = static_keys
                    .iter()
                    .map(|k| {
                        if k == &static_keys[0] { // swap signer
                            return *new_sender
                        } else if k == &static_keys[accounts[15].account_index as usize] { // swap input account
                            return spl_associated_token_account::get_associated_token_address(
                                new_sender, 
                                &mint_in
                            )
                        } else if k == &static_keys[accounts[16].account_index as usize] { // swap output account
                            return spl_associated_token_account::get_associated_token_address(
                                new_sender, 
                                &mint_out
                            )
                        } else {
                            return *k
                        }
                    })
                    .collect();
                if swap_in_out {
                    i.swap(5, 6); // swap pool token accounts
                    i.swap(12, 13); // swap sereum market accounts
                }
                Ok(i)
            }
        }
    }
}

#[cfg(test)]
mod test {
    use crate::programs::Account;
    use super::ParsedRaydiumStableSwapInstructions;

    #[test]
    fn deserialize_stableswap_instruction() {
        let sample_ix = [9, 160, 134, 1, 0, 0, 0, 0, 0, 231, 50, 1, 0, 0, 0, 0, 0].to_vec();
        let key_i: Vec<u8> = [16, 3, 20, 4, 5, 6, 7, 21, 8, 9, 10, 11, 12, 13, 22, 2, 14, 1].to_vec();
        assert_eq!(
            ParsedRaydiumStableSwapInstructions::from_bytes(
                sample_ix, 
                key_i.iter().map(|i| Account::new(i, false)).collect()
            ).unwrap(),
            ParsedRaydiumStableSwapInstructions::Swap {
                instruction: 9,
                amount_in: 100000,
                minimum_amount_out: 78567,
                accounts: key_i.iter().map(|i| Account::new(i, false)).collect()
            }
        )
    }
}