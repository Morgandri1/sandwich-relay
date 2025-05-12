use std::rc::Rc;
use solana_sdk::{
    commitment_config::CommitmentConfig, 
    hash::Hash, 
    message::v0::Message as MessageV0, 
    pubkey::Pubkey, 
    signature::{Keypair, Signer}
};
use anchor_client::{
    anchor_lang::declare_program, Client, Cluster
};
use spl_associated_token_account::get_associated_token_address;

use crate::result::{MevError, MevResult};

use super::{pumpfun::ParsedPumpFunInstructions, pumpswap::ParsedPumpSwapInstructions, raydium::{ParsedRaydiumClmmInstructions, ParsedRaydiumCpmmInstructions, ParsedRaydiumLpv4Instructions, ParsedRaydiumStableSwapInstructions, RAYDIUM_CPMM_PROGRAM_ID}, ParsedInstruction};

pub const MEV_PROGRAM_ID: Pubkey = Pubkey::from_str_const("XArSfgXtRWmxtyUW6dS6tTky1uYwpvaKEEq5eg93w15");

declare_program!(sandwich_swap);
use sandwich_swap::{
    client::accounts,
    client::args
};

pub enum MevInstructionBuilder {
    PumpFun(ParsedPumpFunInstructions),
    PumpSwap(ParsedPumpSwapInstructions),
    RaydiumLpv4(ParsedRaydiumLpv4Instructions),
    RaydiumCpmm(ParsedRaydiumCpmmInstructions),
    RaydiumClmm(ParsedRaydiumClmmInstructions),
    RaydiumStable(ParsedRaydiumStableSwapInstructions)
}

impl MevInstructionBuilder {
    fn derive_pda(&self) -> MevResult<(Pubkey, u64)> {
        let swap_id: u64 = rand::random();
        match Pubkey::try_find_program_address(&[&swap_id.to_le_bytes()], &MEV_PROGRAM_ID) {
            Some((key, _)) => Ok((key, swap_id)),
            None => Err(MevError::FailedToBuildTx)
        }
    }
    
    pub fn from_parsed_ix(ix: ParsedInstruction) -> MevResult<Self> {
        match ix {
            ParsedInstruction::PumpFun(i) => Ok(Self::PumpFun(i?)),
            ParsedInstruction::PumpSwap(i) => Ok(Self::PumpSwap(i?)),
            ParsedInstruction::RaydiumLpv4(i) => Ok(Self::RaydiumLpv4(i?)),
            ParsedInstruction::RaydiumClmm(i) => Ok(Self::RaydiumClmm(i?)),
            ParsedInstruction::RaydiumCpmm(i) => Ok(Self::RaydiumCpmm(i?)),
            ParsedInstruction::RaydiumStable(i) => Ok(Self::RaydiumStable(i?)),
            ParsedInstruction::Irrelevant => Err(MevError::UnknownError)
        }
    }
    
    pub fn create_sandwich_txs(
        &self, 
        signer: &Keypair, 
        target_static_accounts: &[Pubkey],
        recent_blockhash: Hash
    ) -> MevResult<(MessageV0, MessageV0)> {
        match self {
            Self::RaydiumCpmm(ix) => self.handle_cpmm(ix, signer, target_static_accounts, recent_blockhash),
            _ => Err(MevError::UnknownError)
        }
    }
    
    fn handle_cpmm(
        &self, 
        ix: &ParsedRaydiumCpmmInstructions, 
        signer: &Keypair, 
        target_static_accounts: &[Pubkey],
        recent_blockhash: Hash
    ) -> MevResult<(MessageV0, MessageV0)> {
        let program = Client::new_with_options(
            Cluster::Localnet, 
            Rc::new(signer), 
            CommitmentConfig::confirmed()
        ).program(MEV_PROGRAM_ID).map_err(|_| MevError::UnknownError)?;
        let (state_account, id) = self.derive_pda()?;
        match ix {
            ParsedRaydiumCpmmInstructions::SwapIn { amount, min_amount_out, accounts, .. } => {
                if accounts.len() < 13 {
                    return Err(MevError::ValueError)
                }
                let front_ix = program
                    .request()
                    .accounts(accounts::RaydiumCpmmFrontrunSwapBaseInput {
                        payer: signer.pubkey(),
                        cp_swap_program: RAYDIUM_CPMM_PROGRAM_ID,
                        authority: target_static_accounts[accounts[1].account_index as usize],
                        amm_config: target_static_accounts[accounts[2].account_index as usize],
                        pool_state: target_static_accounts[accounts[3].account_index as usize],
                        input_token_account: get_associated_token_address(&signer.pubkey(), &target_static_accounts[accounts[10].account_index as usize]),
                        output_token_account: get_associated_token_address(&signer.pubkey(), &target_static_accounts[accounts[11].account_index as usize]),
                        input_vault: target_static_accounts[accounts[6].account_index as usize],
                        output_vault: target_static_accounts[accounts[7].account_index as usize],
                        input_token_program: target_static_accounts[accounts[8].account_index as usize],
                        output_token_program: target_static_accounts[accounts[9].account_index as usize],
                        input_token_mint: target_static_accounts[accounts[10].account_index as usize],
                        output_token_mint: target_static_accounts[accounts[11].account_index as usize],
                        observation_state: target_static_accounts[accounts[12].account_index as usize],
                        system_program: Pubkey::from_str_const("11111111111111111111111111111111"),
                        sandwich_state: state_account
                    })
                    .args(args::RaydiumCpmmFrontrunSwapBaseInput {
                        target_amount_in: *amount,
                        target_minimum_amount_out: *min_amount_out,
                        sandwich_id: id
                    })
                    .instructions()
                    .map_err(|_| MevError::FailedToBuildTx)?;
                let back_ix = program
                    .request()
                    .accounts(accounts::RaydiumCpmmBackrunSwapBaseInput {
                        payer: signer.pubkey(),
                        cp_swap_program: RAYDIUM_CPMM_PROGRAM_ID,
                        authority: target_static_accounts[accounts[1].account_index as usize],
                        amm_config: target_static_accounts[accounts[2].account_index as usize],
                        pool_state: target_static_accounts[accounts[3].account_index as usize],
                        input_token_account: get_associated_token_address(&signer.pubkey(), &target_static_accounts[accounts[10].account_index as usize]),
                        output_token_account: get_associated_token_address(&signer.pubkey(), &target_static_accounts[accounts[11].account_index as usize]),
                        input_vault: target_static_accounts[accounts[6].account_index as usize],
                        output_vault: target_static_accounts[accounts[7].account_index as usize],
                        input_token_program: target_static_accounts[accounts[8].account_index as usize],
                        output_token_program: target_static_accounts[accounts[9].account_index as usize],
                        input_token_mint: target_static_accounts[accounts[10].account_index as usize],
                        output_token_mint: target_static_accounts[accounts[11].account_index as usize],
                        observation_state: target_static_accounts[accounts[12].account_index as usize],
                        sandwich_state: state_account
                    })
                    .args(args::RaydiumCpmmBackrunSwapBaseInput { sandwich_id: id })
                    .instructions()
                    .map_err(|_| MevError::FailedToBuildTx)?;
                Ok((
                    MessageV0::try_compile(
                        &signer.pubkey(), 
                        front_ix.as_slice(), 
                        &[], 
                        recent_blockhash
                    ).map_err(|_| MevError::FailedToBuildTx)?, 
                    MessageV0::try_compile(
                        &signer.pubkey(), 
                        back_ix.as_slice(), 
                        &[], 
                        recent_blockhash
                    ).map_err(|_| MevError::FailedToBuildTx)?
                ))
            },
            ParsedRaydiumCpmmInstructions::SwapOut { max_amount_in, amount_out, accounts, .. } => {
                if accounts.len() < 13 {
                    return Err(MevError::ValueError)
                }
                let front_ix = program
                    .request()
                    .accounts(accounts::RaydiumCpmmFrontrunSwapBaseOutput {
                        payer: signer.pubkey(),
                        cp_swap_program: RAYDIUM_CPMM_PROGRAM_ID,
                        authority: target_static_accounts[accounts[1].account_index as usize],
                        amm_config: target_static_accounts[accounts[2].account_index as usize],
                        pool_state: target_static_accounts[accounts[3].account_index as usize],
                        input_token_account: get_associated_token_address(&signer.pubkey(), &target_static_accounts[accounts[10].account_index as usize]),
                        output_token_account: get_associated_token_address(&signer.pubkey(), &target_static_accounts[accounts[11].account_index as usize]),
                        input_vault: target_static_accounts[accounts[6].account_index as usize],
                        output_vault: target_static_accounts[accounts[7].account_index as usize],
                        input_token_program: target_static_accounts[accounts[8].account_index as usize],
                        output_token_program: target_static_accounts[accounts[9].account_index as usize],
                        input_token_mint: target_static_accounts[accounts[10].account_index as usize],
                        output_token_mint: target_static_accounts[accounts[11].account_index as usize],
                        observation_state: target_static_accounts[accounts[12].account_index as usize],
                        system_program: Pubkey::from_str_const("11111111111111111111111111111111"),
                        sandwich_state: state_account
                    })
                    .args(args::RaydiumCpmmFrontrunSwapBaseOutput {
                        target_amount_out: *amount_out,
                        target_max_amount_in: *max_amount_in,
                        sandwich_id: id
                    })
                    .instructions()
                    .map_err(|_| MevError::FailedToBuildTx)?;
                let back_ix = program
                    .request()
                    .accounts(accounts::RaydiumCpmmBackrunSwapBaseOutput {
                        payer: signer.pubkey(),
                        cp_swap_program: RAYDIUM_CPMM_PROGRAM_ID,
                        authority: target_static_accounts[accounts[1].account_index as usize],
                        amm_config: target_static_accounts[accounts[2].account_index as usize],
                        pool_state: target_static_accounts[accounts[3].account_index as usize],
                        input_token_account: get_associated_token_address(&signer.pubkey(), &target_static_accounts[accounts[10].account_index as usize]),
                        output_token_account: get_associated_token_address(&signer.pubkey(), &target_static_accounts[accounts[11].account_index as usize]),
                        input_vault: target_static_accounts[accounts[6].account_index as usize],
                        output_vault: target_static_accounts[accounts[7].account_index as usize],
                        input_token_program: target_static_accounts[accounts[8].account_index as usize],
                        output_token_program: target_static_accounts[accounts[9].account_index as usize],
                        input_token_mint: target_static_accounts[accounts[10].account_index as usize],
                        output_token_mint: target_static_accounts[accounts[11].account_index as usize],
                        observation_state: target_static_accounts[accounts[12].account_index as usize],
                        sandwich_state: state_account
                    })
                    .args(args::RaydiumCpmmBackrunSwapBaseOutput { sandwich_id: id })
                    .instructions()
                    .map_err(|_| MevError::FailedToBuildTx)?;
                Ok((
                    MessageV0::try_compile(
                        &signer.pubkey(), 
                        front_ix.as_slice(), 
                        &[], 
                        recent_blockhash
                    ).map_err(|_| MevError::FailedToBuildTx)?, 
                    MessageV0::try_compile(
                        &signer.pubkey(), 
                        back_ix.as_slice(), 
                        &[], 
                        recent_blockhash
                    ).map_err(|_| MevError::FailedToBuildTx)?
                ))
            }
        }
    }
    
    fn handle_lpv4(&self, ix: ParsedRaydiumLpv4Instructions, signer: Keypair) {}
    fn handle_clmm(&self, ix: ParsedRaydiumClmmInstructions, signer: Keypair) {}
    fn handle_stable(&self, ix: ParsedRaydiumStableSwapInstructions, signer: Keypair) {}
    fn handle_pf(&self, ix: ParsedPumpFunInstructions, signer: Keypair) {}
    fn handle_ps(&self, ix: ParsedPumpSwapInstructions, signer: Keypair) {}
}

#[cfg(test)]
mod test {
    use super::*;
}