pub mod pumpfun;
pub mod pumpswap;
pub mod raydium;
pub mod mev;

use pumpfun::{ParsedPumpFunInstructions, PUMPFUN_PROGRAM_ID};
use pumpswap::{ParsedPumpSwapInstructions, PUMPSWAP_PROGRAM_ID};
use raydium::{
    ParsedRaydiumClmmInstructions, 
    ParsedRaydiumCpmmInstructions, 
    ParsedRaydiumLpv4Instructions, 
    ParsedRaydiumStableSwapInstructions, 
    LPV4_SWAP, 
    RAYDIUM_CLMM_PROGRAM_ID, 
    RAYDIUM_CPMM_PROGRAM_ID, 
    STABLE_SWAP_PROGRAM_ID
};

use solana_sdk::{
    instruction::CompiledInstruction, 
    pubkey::Pubkey
};

use crate::result::MevResult;

#[derive(Debug, PartialEq, Clone)]
pub struct Account {
    pub account_index: u8,
    pub is_writable: bool
}

impl Account {
    pub fn new(index: &u8, writable: bool) -> Self {
        Self { account_index: *index, is_writable: writable }
    }
    
    pub fn from_account_map(i: Vec<u8>) -> Vec<Self> {
        i.iter().map(|index| Self::new(index, false)).collect()
    }
}

pub enum ParsedInstruction {
    #[allow(unused)]
    RaydiumLpv4(MevResult<ParsedRaydiumLpv4Instructions>),
    // RaydiumRouter(MevResult<ParsedRaydiumRouterInstructions>),
    RaydiumClmm(MevResult<ParsedRaydiumClmmInstructions>),
    #[allow(unused)]
    RaydiumStable(MevResult<ParsedRaydiumStableSwapInstructions>),
    RaydiumCpmm(MevResult<ParsedRaydiumCpmmInstructions>),
    PumpFun(MevResult<ParsedPumpFunInstructions>),
    PumpSwap(MevResult<ParsedPumpSwapInstructions>),
    Irrelevant
}

impl ParsedInstruction {
    pub fn from_ix(ix: &CompiledInstruction, accounts: &[Pubkey]) -> Option<Self> {
        let program_id = accounts[ix.program_id_index as usize];
        let accounts = Account::from_account_map(ix.accounts.clone());
        let bytes = ix.data.clone();
        let res = match (program_id, ix.data[0]) {
            (LPV4_SWAP, 0) => Self::RaydiumLpv4(ParsedRaydiumLpv4Instructions::from_bytes(bytes, accounts)),
            // (ROUTER_PROGRAM_ID, 0) => Self::RaydiumRouter(ParsedRaydiumRouterInstructions::from_bytes(bytes, accounts)),
            (STABLE_SWAP_PROGRAM_ID, 9) => Self::RaydiumStable(ParsedRaydiumStableSwapInstructions::from_bytes(bytes, accounts)),
            (RAYDIUM_CLMM_PROGRAM_ID, 1) => Self::RaydiumClmm(ParsedRaydiumClmmInstructions::from_bytes(bytes, accounts)),
            (RAYDIUM_CPMM_PROGRAM_ID, 143 | 55) => Self::RaydiumCpmm(ParsedRaydiumCpmmInstructions::from_bytes(bytes, accounts)),
            (PUMPFUN_PROGRAM_ID, 102 | 51) => Self::PumpFun(ParsedPumpFunInstructions::from_bytes(bytes, accounts)),
            (PUMPSWAP_PROGRAM_ID, 102 | 51) => Self::PumpSwap(ParsedPumpSwapInstructions::from_bytes(bytes, accounts)),
            _ => Self::Irrelevant
        };
        return Some(res)
        // match res {
            
        // }
    }
}