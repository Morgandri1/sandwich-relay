use solana_sdk::{instruction::CompiledInstruction, pubkey::Pubkey};

use crate::{programs::Account, result::{MevError, MevResult}};

pub const ROUTER_PROGRAM_ID: Pubkey = Pubkey::from_str_const("routeUGWgWzqBWFcrCfv8tritsqukccJPu3q5GPP3xS");

#[repr(u8)]
pub enum RaydiumRouterInstructions {
    Route = 0,
    Cleanup = 6
}

#[derive(Debug, PartialEq)]
pub enum ParsedRaydiumRouterInstructions {
    /// Discriminator: 0
    Route {
        amount_in: u64,
        minimum_amount_out: u64,
        accounts: Vec<Account>
    }
}

impl ParsedRaydiumRouterInstructions {
    pub fn from_bytes(bytes: Vec<u8>, accounts: Vec<Account>) -> MevResult<Self> {
        if bytes.len() < 17 {
            return Err(crate::result::MevError::FailedToDeserialize);
        };
        let mut amount_in_bytes = [0u8; 8];
        let mut min_out_bytes = [0u8; 8];
        
        // Copy the bytes into properly sized arrays for conversion
        amount_in_bytes[..8].copy_from_slice(&bytes[1..9]);
        min_out_bytes[..8].copy_from_slice(&bytes[9..]);
        
        return Ok(Self::Route {
            amount_in: u64::from_le_bytes(amount_in_bytes),
            minimum_amount_out: u64::from_le_bytes(min_out_bytes),
            accounts
        })
    }
    
    pub fn to_compiled_instruction(&self, program_id: u8) -> MevResult<CompiledInstruction> {
        match self {
            Self::Route { amount_in, minimum_amount_out, accounts } => {
                let mut instruction_data = [0u8].to_vec();
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
}