use std::rc::Rc;
use anchor_lang::pubkey;
use solana_sdk::{
    commitment_config::CommitmentConfig,
    compute_budget::ComputeBudgetInstruction,
    hash::Hash,
    instruction::Instruction,
    message::{v0::Message as MessageV0, VersionedMessage},
    pubkey::Pubkey,
    signature::{Keypair, Signer},
    transaction::VersionedTransaction
};
use anchor_client::{
    anchor_lang::declare_program, Client, Cluster, Program
};
use spl_associated_token_account::get_associated_token_address;

use crate::{result::{MevError, MevResult}, rpc::get_mint_of_account, tx::ASSOCIATED_TOKEN_PROGRAM_ID};

use super::{pumpfun::{ParsedPumpFunInstructions, PUMPFUN_PROGRAM_ID}, pumpswap::{ParsedPumpSwapInstructions, PUMPSWAP_PROGRAM_ID}, raydium::{ParsedRaydiumClmmInstructions, ParsedRaydiumCpmmInstructions, ParsedRaydiumLpv4Instructions, ParsedRaydiumStableSwapInstructions, LPV4_SWAP, RAYDIUM_CLMM_PROGRAM_ID, RAYDIUM_CPMM_PROGRAM_ID}, ParsedInstruction};

pub const MEV_PROGRAM_ID: Pubkey = Pubkey::from_str_const("inf69quFVZyuHEsrUXq3APtYLr4iqsNiQdCh5ArGcUp");

declare_program!(sandwich_swap);
use sandwich_swap::{
    client::accounts,
    client::args
};

const SYSTEM_PROGRAM: Pubkey = Pubkey::from_str_const("11111111111111111111111111111111");
const ASSOCIATED_TOKEN_PROGRAM: Pubkey = Pubkey::from_str_const(ASSOCIATED_TOKEN_PROGRAM_ID);
const MEMO_PROGRAM: Pubkey = Pubkey::from_str_const("MemoSq4gqABAXKb96qnH8TysNcWxMyWCqXgDLGmfcHr");
const TOKEN_PROGRAM: Pubkey = Pubkey::from_str_const("TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA");
const TOKEN22_PROGRAM: Pubkey =  Pubkey::from_str_const("TokenzQdBNbLqP5VEhdkAS6EPFLC1PHnBqCXEpPxuEb");
const COMPUTE_BUDGET_PROGRAM: Pubkey = Pubkey::from_str_const("ComputeBudget111111111111111111111111111111");

pub enum MevInstructionBuilder {
    PumpFun(ParsedPumpFunInstructions),
    PumpSwap(ParsedPumpSwapInstructions),
    RaydiumLpv4(ParsedRaydiumLpv4Instructions),
    RaydiumCpmm(ParsedRaydiumCpmmInstructions),
    RaydiumClmm(ParsedRaydiumClmmInstructions),
    #[allow(dead_code)]
    RaydiumStable(ParsedRaydiumStableSwapInstructions)
}

impl MevInstructionBuilder {
    fn derive_pda(&self) -> MevResult<(Pubkey, [u8; 16])> {
        let swap_id: uuid::Uuid = uuid::Uuid::new_v4();
        match Pubkey::try_find_program_address(&[b"sandwich", swap_id.as_bytes()], &MEV_PROGRAM_ID) {
            Some((key, _)) => Ok((key, *swap_id.as_bytes())),
            None => Err(MevError::ValueError)
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

    #[allow(dead_code)]
    pub fn create_compute_budget_instructions(
        units: Option<u32>,
        price: Option<u64>,
    ) -> Vec<Instruction> {
        let mut instructions = Vec::new();
        
        // Set compute unit limit if specified
        if let Some(compute_units) = units {
            instructions.push(
                ComputeBudgetInstruction::set_compute_unit_limit(compute_units)
            );
        }
        
        // Set compute unit price if specified
        if let Some(compute_price) = price {
            instructions.push(
                ComputeBudgetInstruction::set_compute_unit_price(compute_price)
            );
        }
        
        instructions
    }
    
    /// Creates compute budget instructions based on the target transaction.
    /// The frontrun will have 35% more compute units than the target,
    /// and the backrun will have 35% less compute units than the target.
    ///
    /// # Arguments
    ///
    /// * `target_tx` - The target transaction to base compute budget on
    /// * `prioritize_frontrun` - Optional priority boost (in micro lamports) for frontrun
    ///
    /// # Returns
    ///
    /// A tuple containing compute budget instructions for (frontrun, backrun)
    pub fn create_compute_budget_instructions_from_target(
        target_tx: &VersionedTransaction,
        prioritize_frontrun: Option<u64>,
    ) -> (Vec<Instruction>, Vec<Instruction>) {
        // Default compute unit limit if we can't determine from target
        const DEFAULT_COMPUTE_UNITS: u32 = 20_000;
        
        // Extract compute unit limit from target transaction if present
        let mut target_units = DEFAULT_COMPUTE_UNITS;
        
        // Check if target has compute budget instructions
        if let Some(instructions) = Self::get_compute_budget_from_tx(target_tx) {
            for ix in instructions {
                // Only care about the unit limit instruction for scaling
                if let Some(units) = Self::extract_compute_units(&ix) {
                    target_units = units;
                    break;
                }
            }
        }
        
        // Calculate compute units for frontrun (35% more)
        let frontrun_units = (target_units as f32 * 1.35).min(u32::MAX as f32) as u32;
        
        // Calculate compute units for backrun (35% less)
        let backrun_units = (target_units as f32 * 0.65) as u32;
        
        // Create frontrun compute budget instructions
        let mut frontrun_instructions = Vec::new();
        frontrun_instructions.push(ComputeBudgetInstruction::set_compute_unit_limit(frontrun_units));
        
        // Add priority fee to frontrun if specified
        if let Some(priority) = prioritize_frontrun {
            frontrun_instructions.push(ComputeBudgetInstruction::set_compute_unit_price(priority));
        }
        
        // Create backrun compute budget instructions
        let backrun_instructions = vec![
            ComputeBudgetInstruction::set_compute_unit_limit(backrun_units)
        ];
        
        (frontrun_instructions, backrun_instructions)
    }
    
    /// Extract compute budget instructions from a transaction
    pub fn get_compute_budget_from_tx(tx: &VersionedTransaction) -> Option<Vec<Instruction>> {
        let message = &tx.message;
        let compute_budget_program_index = message.static_account_keys().iter()
            .position(|key| *key == COMPUTE_BUDGET_PROGRAM)?;
        
        // Find all instructions using the compute budget program
        let mut compute_budget_ixs = Vec::new();
        
        // Extract instructions based on the message version
        let instructions = match &message {
            VersionedMessage::Legacy(legacy) => &legacy.instructions,
            VersionedMessage::V0(v0) => &v0.instructions,
        };
        
        // Process all instructions that use the compute budget program
        for ix in instructions {
            if ix.program_id_index as usize == compute_budget_program_index {
                // Convert compiled instruction back to instruction
                let program_id = COMPUTE_BUDGET_PROGRAM;
                let accounts = vec![]; // Compute budget instructions don't use accounts
                let data = ix.data.clone();
                
                compute_budget_ixs.push(Instruction {
                    program_id,
                    accounts,
                    data,
                });
            }
        }
        
        if compute_budget_ixs.is_empty() {
            None
        } else {
            Some(compute_budget_ixs)
        }
    }
    
    /// Extract compute unit limit from a compute budget instruction
    pub fn extract_compute_units(ix: &Instruction) -> Option<u32> {
        if ix.program_id != COMPUTE_BUDGET_PROGRAM {
            return None;
        }

        if ix.data.is_empty() || ix.data[0] != 2 {
            return None;
        }
        
        // The unit limit is encoded as a little-endian u32 in the next 4 bytes
        if ix.data.len() < 5 {
            return None;
        }
        
        let mut num = [0; 4];
        num.copy_from_slice(&ix.data[1..]);
        Some(u32::from_le_bytes(num))
    }

    pub fn create_sandwich_txs(
        &self,
        signer: &Keypair,
        target_static_accounts: &[Pubkey],
        recent_blockhash: Hash
    ) -> MevResult<(MessageV0, MessageV0)> {
        match self {
            Self::RaydiumCpmm(ix) => self.handle_cpmm(ix, signer, target_static_accounts, recent_blockhash),
            Self::RaydiumClmm(ix) => self.handle_clmm(ix, signer, target_static_accounts, recent_blockhash),
            Self::PumpSwap(ix) => self.handle_ps(ix, signer, target_static_accounts, recent_blockhash),
            Self::PumpFun(ix) => self.handle_pf(ix, signer, target_static_accounts, recent_blockhash),
            Self::RaydiumLpv4(ix) => self.handle_lpv4(ix, signer, target_static_accounts, recent_blockhash),
            _ => Err(MevError::UnknownError)
        }
    }

    fn create_client(&self, signer: Keypair) -> MevResult<Program<Rc<Keypair>>> {
        Client::new_with_options(
            Cluster::Localnet, // shouldn't ever be used in theory
            Rc::new(signer),
            CommitmentConfig::confirmed()
        ).program(MEV_PROGRAM_ID).map_err(|_| MevError::UnknownError)
    }

    pub fn is_frontrunable(&self, keys: &[Pubkey]) -> bool {
        let wsol = pubkey!("So11111111111111111111111111111111111111112");
        let def = pubkey!("11111111111111111111111111111111");
        let mint_in = match self {
            Self::PumpFun(ix) => ix.mint_in(keys),
            Self::PumpSwap(ix) => ix.mint_in(keys),
            Self::RaydiumClmm(ix) => ix.mint_in(keys),
            Self::RaydiumCpmm(ix) => ix.mint_in(keys),
            Self::RaydiumLpv4(ix) => ix.mint_in(keys),
            _ => Ok(def)
        };

        match mint_in {
            Ok(mint) => mint == wsol,
            Err(err) => {
                eprintln!("Error while checking if frontrunable: {:?}", err);
                return false;
            }
        }
    }

    fn handle_cpmm(
        &self,
        ix: &ParsedRaydiumCpmmInstructions,
        signer: &Keypair,
        target_static_accounts: &[Pubkey],
        recent_blockhash: Hash
    ) -> MevResult<(MessageV0, MessageV0)> {
        let program = self.create_client(signer.insecure_clone())?;
        let (state_account, id) = self.derive_pda()?;
        match ix {
            ParsedRaydiumCpmmInstructions::SwapIn { amount, min_amount_out, accounts, .. } => {
                if accounts.len() <= 12 {
                    return Err(MevError::ValueError)
                }

                let highest_index = accounts.iter().map(|a| a.account_index).max().unwrap_or(0);
                if highest_index as usize >= target_static_accounts.len() {
                    return Err(MevError::ValueError);
                }
                
                if target_static_accounts[accounts[10].account_index as usize] != Pubkey::from_str_const("So11111111111111111111111111111111111111112") {
                    return Err(MevError::FailedToBuildTx)
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
                        system_program: SYSTEM_PROGRAM,
                        sandwich_state: state_account
                    })
                    .args(args::RaydiumCpmmFrontrunSwapBaseInput {
                        target_amount_in: *amount,
                        target_minimum_amount_out: *min_amount_out,
                        sandwich_id: id.clone()
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
                        input_token_account: get_associated_token_address(&signer.pubkey(), &target_static_accounts[accounts[11].account_index as usize]),
                        output_token_account: get_associated_token_address(&signer.pubkey(), &target_static_accounts[accounts[10].account_index as usize]),
                        input_vault: target_static_accounts[accounts[7].account_index as usize],
                        output_vault: target_static_accounts[accounts[6].account_index as usize],
                        input_token_program: target_static_accounts[accounts[9].account_index as usize],
                        output_token_program: target_static_accounts[accounts[8].account_index as usize],
                        input_token_mint: target_static_accounts[accounts[11].account_index as usize],
                        output_token_mint: target_static_accounts[accounts[10].account_index as usize],
                        observation_state: target_static_accounts[accounts[12].account_index as usize],
                        sandwich_state: state_account
                    })
                    .args(args::RaydiumCpmmBackrunSwapBaseInput {
                        sandwich_id: id.clone()
                    })
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
                if accounts.len() <= 12 {
                    return Err(MevError::ValueError)
                }

                let highest_index = accounts.iter().map(|a| a.account_index).max().unwrap_or(0);
                if highest_index as usize >= target_static_accounts.len() {
                    return Err(MevError::ValueError);
                }
                
                if target_static_accounts[accounts[10].account_index as usize] != Pubkey::from_str_const("So11111111111111111111111111111111111111112") {
                    return Err(MevError::FailedToBuildTx)
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
                        system_program: SYSTEM_PROGRAM,
                        sandwich_state: state_account
                    })
                    .args(args::RaydiumCpmmFrontrunSwapBaseOutput {
                        target_amount_out: *amount_out,
                        target_max_amount_in: *max_amount_in,
                        sandwich_id: id.clone()
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
                        input_token_account: get_associated_token_address(&signer.pubkey(), &target_static_accounts[accounts[11].account_index as usize]),
                        output_token_account: get_associated_token_address(&signer.pubkey(), &target_static_accounts[accounts[10].account_index as usize]),
                        input_vault: target_static_accounts[accounts[7].account_index as usize],
                        output_vault: target_static_accounts[accounts[6].account_index as usize],
                        input_token_program: target_static_accounts[accounts[9].account_index as usize],
                        output_token_program: target_static_accounts[accounts[8].account_index as usize],
                        input_token_mint: target_static_accounts[accounts[11].account_index as usize],
                        output_token_mint: target_static_accounts[accounts[10].account_index as usize],
                        observation_state: target_static_accounts[accounts[12].account_index as usize],
                        sandwich_state: state_account
                    })
                    .args(args::RaydiumCpmmBackrunSwapBaseOutput {
                        sandwich_id: id
                    })
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

    fn handle_clmm(
        &self,
        ix: &ParsedRaydiumClmmInstructions,
        signer: &Keypair,
        target_static_accounts: &[Pubkey],
        recent_blockhash: Hash
    ) -> MevResult<(MessageV0, MessageV0)> {
        let program = self.create_client(signer.insecure_clone())?;
        let (state_account, id) = self.derive_pda()?;
        match ix {
            ParsedRaydiumClmmInstructions::Swap { amount, other_amount_threshold, accounts, sqrt_price_limit_64, is_base_input } => {
                if accounts.len() <= 12 {
                    return Err(MevError::ValueError);
                }
                
                let highest_index = accounts.iter().map(|a| a.account_index).max().unwrap_or(0);
                if highest_index as usize >= target_static_accounts.len() {
                    return Err(MevError::ValueError);
                }
                
                if target_static_accounts[accounts[11].account_index as usize]
                    != Pubkey::from_str_const("So11111111111111111111111111111111111111112") {
                    return Err(MevError::FailedToBuildTx)
                }
                
                let front = program
                    .request()
                    .accounts(accounts::RaydiumClmmFrontrunSwap {
                        payer: signer.pubkey(),
                        amm_config: target_static_accounts[accounts[1].account_index as usize],
                        pool_state: target_static_accounts[accounts[2].account_index as usize],
                        input_token_account: get_associated_token_address(
                            &signer.pubkey(),
                            &target_static_accounts[accounts[11].account_index as usize]
                        ),
                        output_token_account: get_associated_token_address(
                            &signer.pubkey(),
                            &target_static_accounts[accounts[12].account_index as usize]
                        ),
                        input_vault: target_static_accounts[accounts[5].account_index as usize],
                        output_vault: target_static_accounts[accounts[6].account_index as usize],
                        observation_state: target_static_accounts[accounts[7].account_index as usize],
                        token_program: TOKEN_PROGRAM,
                        token_program_2022: TOKEN22_PROGRAM,
                        memo_program: MEMO_PROGRAM,
                        input_vault_mint: target_static_accounts[accounts[11].account_index as usize],
                        output_vault_mint: target_static_accounts[accounts[12].account_index as usize],
                        clmm_program: RAYDIUM_CLMM_PROGRAM_ID,
                        system_program: SYSTEM_PROGRAM,
                        sandwich_state: state_account
                    })
                    .args(args::RaydiumClmmFrontrunSwap {
                        target_amount: *amount,
                        target_is_base_input: *is_base_input,
                        target_other_amount_threshold: *other_amount_threshold,
                        target_sqrt_price_limit_x64: *sqrt_price_limit_64,
                        sandwich_id: id.clone()
                    })
                    .instructions()
                    .map_err(|_| MevError::FailedToBuildTx)?;

                let back = program
                    .request()
                    .accounts(accounts::RaydiumClmmBackrunSwap {
                        payer: signer.pubkey(),
                        amm_config: target_static_accounts[accounts[1].account_index as usize],
                        pool_state: target_static_accounts[accounts[2].account_index as usize],
                        input_token_account: get_associated_token_address(&signer.pubkey(), &target_static_accounts[accounts[12].account_index as usize]),
                        output_token_account: get_associated_token_address(&signer.pubkey(), &target_static_accounts[accounts[11].account_index as usize]),
                        input_vault: target_static_accounts[accounts[6].account_index as usize],
                        output_vault: target_static_accounts[accounts[5].account_index as usize],
                        observation_state: target_static_accounts[accounts[7].account_index as usize],
                        token_program: TOKEN_PROGRAM,
                        token_program_2022: TOKEN22_PROGRAM,
                        memo_program: MEMO_PROGRAM,
                        input_vault_mint: target_static_accounts[accounts[12].account_index as usize],
                        output_vault_mint: target_static_accounts[accounts[11].account_index as usize],
                        clmm_program: RAYDIUM_CLMM_PROGRAM_ID,
                        sandwich_state: state_account
                    })
                    .args(args::RaydiumClmmBackrunSwap {
                        sandwich_id: id
                    })
                    .instructions()
                    .map_err(|_| MevError::FailedToBuildTx)?;

                Ok((
                    MessageV0::try_compile(
                        &signer.pubkey(),
                        front.as_slice(),
                        &[],
                        recent_blockhash
                    ).map_err(|_| MevError::FailedToBuildTx)?,
                    MessageV0::try_compile(
                        &signer.pubkey(),
                        back.as_slice(),
                        &[],
                        recent_blockhash
                    ).map_err(|_| MevError::FailedToBuildTx)?
                ))
            }
        }
    }

    fn handle_ps(
        &self,
        ix: &ParsedPumpSwapInstructions,
        signer: &Keypair,
        target_static_accounts: &[Pubkey],
        recent_blockhash: Hash
    ) -> MevResult<(MessageV0, MessageV0)> {
        let program = self.create_client(signer.insecure_clone())?;
        let (state_account, id) = self.derive_pda()?;
        match ix {
            ParsedPumpSwapInstructions::Buy { base_amount_out, max_quote_amount_in, accounts, .. } => {
                if accounts.len() <= 18 {
                    return Err(MevError::ValueError);
                }

                let highest_index = accounts.iter().map(|a| a.account_index).max().unwrap_or(0);
                if highest_index as usize >= target_static_accounts.len() {
                    return Err(MevError::ValueError);
                }
                
                if target_static_accounts[accounts[4].account_index as usize]
                    != Pubkey::from_str_const("So11111111111111111111111111111111111111112") {
                    return Err(MevError::FailedToBuildTx)
                }
                let front = program
                    .request()
                    .accounts(accounts::PumpFrontrunBuy {
                        pool: target_static_accounts[accounts[0].account_index as usize],
                        user: signer.pubkey(),
                        global_config: target_static_accounts[accounts[2].account_index as usize],
                        base_mint: target_static_accounts[accounts[3].account_index as usize],
                        quote_mint: target_static_accounts[accounts[4].account_index as usize],
                        user_base_token_account: get_associated_token_address(&signer.pubkey(), &target_static_accounts[accounts[3].account_index as usize]),
                        user_quote_token_account: get_associated_token_address(&signer.pubkey(), &target_static_accounts[accounts[4].account_index as usize]),
                        pool_base_token_account: target_static_accounts[accounts[7].account_index as usize],
                        pool_quote_token_account: target_static_accounts[accounts[8].account_index as usize],
                        protocol_fee_recipient: target_static_accounts[accounts[9].account_index as usize],
                        protocol_fee_recipient_token_account: target_static_accounts[accounts[10].account_index as usize],
                        base_token_program: target_static_accounts[accounts[11].account_index as usize],
                        quote_token_program: target_static_accounts[accounts[12].account_index as usize],
                        system_program: SYSTEM_PROGRAM,
                        associated_token_program: ASSOCIATED_TOKEN_PROGRAM,
                        event_authority: target_static_accounts[accounts[15].account_index as usize],
                        program: PUMPSWAP_PROGRAM_ID,
                        coin_creator_vault_ata: Some(target_static_accounts[accounts[17].account_index as usize]),
                        coin_creator_vault_authority: Some(target_static_accounts[accounts[18].account_index as usize]),
                        sandwich_state: state_account
                    })
                    .args(args::PumpFrontrunBuy {
                        max_quote_amount_in: *max_quote_amount_in,
                        base_amount_out: *base_amount_out,
                        sandwich_id: id
                    })
                    .instructions()
                    .map_err(|_| MevError::FailedToBuildTx)?;

                let back = program
                    .request()
                    .accounts(accounts::PumpBackrunBuy {
                        pool: target_static_accounts[accounts[0].account_index as usize],
                        user: signer.pubkey(),
                        global_config: target_static_accounts[accounts[2].account_index as usize],
                        base_mint: target_static_accounts[accounts[3].account_index as usize],
                        quote_mint: target_static_accounts[accounts[4].account_index as usize],
                        user_base_token_account: get_associated_token_address(&signer.pubkey(), &target_static_accounts[accounts[3].account_index as usize]),
                        user_quote_token_account: get_associated_token_address(&signer.pubkey(), &target_static_accounts[accounts[4].account_index as usize]),
                        pool_base_token_account: target_static_accounts[accounts[7].account_index as usize],
                        pool_quote_token_account: target_static_accounts[accounts[8].account_index as usize],
                        protocol_fee_recipient: target_static_accounts[accounts[9].account_index as usize],
                        protocol_fee_recipient_token_account: target_static_accounts[accounts[10].account_index as usize],
                        base_token_program: target_static_accounts[accounts[11].account_index as usize],
                        quote_token_program: target_static_accounts[accounts[12].account_index as usize],
                        system_program: SYSTEM_PROGRAM,
                        associated_token_program: ASSOCIATED_TOKEN_PROGRAM,
                        event_authority: target_static_accounts[accounts[15].account_index as usize],
                        program: PUMPSWAP_PROGRAM_ID,
                        coin_creator_vault_ata: Some(target_static_accounts[accounts[17].account_index as usize]),
                        coin_creator_vault_authority: Some(target_static_accounts[accounts[18].account_index as usize]),
                        sandwich_state: state_account
                    })
                    .args(args::PumpBackrunBuy {
                        sandwich_id: id
                    })
                    .instructions()
                    .map_err(|_| MevError::FailedToBuildTx)?;

                Ok((
                    MessageV0::try_compile(
                        &signer.pubkey(),
                        &front,
                        &[],
                        recent_blockhash
                    ).map_err(|_| MevError::FailedToBuildTx)?,
                    MessageV0::try_compile(
                        &signer.pubkey(),
                        &back,
                        &[],
                        recent_blockhash
                    ).map_err(|_| MevError::FailedToBuildTx)?
                ))
            },
            ParsedPumpSwapInstructions::Sell { .. } => {
                Err(MevError::FailedToBuildTx)
            }
        }
    }

    fn handle_lpv4(
        &self,
        ix: &ParsedRaydiumLpv4Instructions,
        signer: &Keypair,
        target_static_accounts: &[Pubkey],
        recent_blockhash: Hash
    ) -> MevResult<(MessageV0, MessageV0)> {
        let program = self.create_client(signer.insecure_clone())?;
        let (state_account, id) = self.derive_pda()?;
        match ix {
            ParsedRaydiumLpv4Instructions::Swap { amount_in, minimum_amount_out, accounts, .. } => {
                if accounts.len() <= 16 {
                    return Err(MevError::ValueError)
                }

                let highest_index = accounts.iter().map(|a| a.account_index).max().unwrap_or(0);
                if highest_index as usize >= target_static_accounts.len() {
                    return Err(MevError::ValueError);
                }
                
                let mint_in = get_mint_of_account(&target_static_accounts[accounts[15].account_index as usize])?;
                if mint_in != Pubkey::from_str_const("So11111111111111111111111111111111111111112") {
                    return Err(MevError::FailedToBuildTx)
                }
                let mint_out = get_mint_of_account(&target_static_accounts[accounts[16].account_index as usize])?;

                let front = program
                    .request()
                    .accounts(accounts::RaydiumFrontrunAmmSwapBaseIn {
                        token_program: TOKEN_PROGRAM,
                        amm: target_static_accounts[accounts[1].account_index as usize],
                        amm_authority: target_static_accounts[accounts[2].account_index as usize],
                        amm_open_orders: target_static_accounts[accounts[3].account_index as usize],
                        amm_target_orders: target_static_accounts[accounts[4].account_index as usize],
                        pool_coin_token_account: target_static_accounts[accounts[5].account_index as usize],
                        pool_pc_token_account: target_static_accounts[accounts[6].account_index as usize],
                        serum_program: target_static_accounts[accounts[7].account_index as usize],
                        serum_market: target_static_accounts[accounts[8].account_index as usize],
                        serum_bids: target_static_accounts[accounts[9].account_index as usize],
                        serum_asks: target_static_accounts[accounts[10].account_index as usize],
                        serum_event_queue: target_static_accounts[accounts[11].account_index as usize],
                        serum_coin_vault_account: target_static_accounts[accounts[12].account_index as usize],
                        serum_pc_vault_account: target_static_accounts[accounts[13].account_index as usize],
                        serum_vault_signer: target_static_accounts[accounts[14].account_index as usize],
                        user_source_token_account: get_associated_token_address(
                            &signer.pubkey(),
                            &mint_in
                        ),
                        user_target_token_account: get_associated_token_address(
                            &signer.pubkey(),
                            &mint_out
                        ),
                        base_mint: mint_in,
                        sandwich_state: state_account,
                        user_source_owner: signer.pubkey(),
                        associated_token_program: ASSOCIATED_TOKEN_PROGRAM,
                        system_program: SYSTEM_PROGRAM,
                        amm_program: LPV4_SWAP
                    })
                    .args(args::RaydiumFrontrunAmmSwapBaseIn {
                        target_amount_in: *amount_in,
                        target_minimum_amount_out: *minimum_amount_out,
                        sandwich_id: id.clone()
                    })
                    .instructions()
                    .map_err(|_| MevError::FailedToBuildTx)?;
                let back = program
                    .request()
                    .accounts(accounts::BackrunRaydiumAmmSwapBaseIn {
                        token_program: target_static_accounts[accounts[0].account_index as usize],
                        amm: target_static_accounts[accounts[1].account_index as usize],
                        amm_authority: target_static_accounts[accounts[2].account_index as usize],
                        amm_open_orders: target_static_accounts[accounts[3].account_index as usize],
                        amm_target_orders: target_static_accounts[accounts[4].account_index as usize],
                        pool_coin_token_account: target_static_accounts[accounts[5].account_index as usize],
                        pool_pc_token_account: target_static_accounts[accounts[6].account_index as usize],
                        serum_program: target_static_accounts[accounts[7].account_index as usize],
                        serum_market: target_static_accounts[accounts[8].account_index as usize],
                        serum_bids: target_static_accounts[accounts[9].account_index as usize],
                        serum_asks: target_static_accounts[accounts[10].account_index as usize],
                        serum_event_queue: target_static_accounts[accounts[11].account_index as usize],
                        serum_coin_vault_account: target_static_accounts[accounts[12].account_index as usize],
                        serum_pc_vault_account: target_static_accounts[accounts[13].account_index as usize],
                        serum_vault_signer: target_static_accounts[accounts[14].account_index as usize],
                        user_source_token_account: get_associated_token_address(
                            &signer.pubkey(),
                            &mint_in
                        ),
                        user_target_token_account: get_associated_token_address(
                            &signer.pubkey(),
                            &mint_out
                        ),
                        base_mint: mint_in,
                        sandwich_state: state_account,
                        user_source_owner: signer.pubkey(),
                        amm_program: LPV4_SWAP
                    })
                    .args(args::BackrunRaydiumAmmSwapBaseIn {
                        sandwich_id: id
                    })
                    .instructions()
                    .map_err(|_| MevError::FailedToBuildTx)?;

                Ok((
                    MessageV0::try_compile(
                        &signer.pubkey(),
                        &front,
                        &[],
                        recent_blockhash
                    ).map_err(|_| MevError::FailedToBuildTx)?,
                    MessageV0::try_compile(
                        &signer.pubkey(),
                        &back,
                        &[],
                        recent_blockhash
                    ).map_err(|_| MevError::FailedToBuildTx)?
                ))
            }
        }
    }

    fn handle_pf(
        &self,
        ix: &ParsedPumpFunInstructions,
        signer: &Keypair,
        target_static_accounts: &[Pubkey],
        recent_blockhash: Hash
    ) -> MevResult<(MessageV0, MessageV0)> {
        let program = self.create_client(signer.insecure_clone())?;
        let (state_account, id) = self.derive_pda()?;
        match ix {
            ParsedPumpFunInstructions::Buy { amount, max_sol_cost, accounts, .. } => {
                if accounts.len() <= 10 {
                    return Err(MevError::ValueError);
                }
                
                let highest_index = accounts.iter().map(|a| a.account_index).max().unwrap_or(0);
                if highest_index as usize >= target_static_accounts.len() {
                    return Err(MevError::ValueError);
                }
                
                let front = program
                    .request()
                    .accounts(accounts::PumpfunFrontrunBuy {
                        global: target_static_accounts[accounts[0].account_index as usize],
                        protocol_fee_recipient: target_static_accounts[accounts[1].account_index as usize],
                        mint: target_static_accounts[accounts[2].account_index as usize],
                        bonding_curve: target_static_accounts[accounts[3].account_index as usize],
                        bonding_curve_ata: target_static_accounts[accounts[4].account_index as usize],
                        user_ata: get_associated_token_address(
                            &signer.pubkey(),
                            &target_static_accounts[accounts[2].account_index as usize]
                        ),
                        user: signer.pubkey(),
                        system_program: SYSTEM_PROGRAM,
                        token_program: TOKEN_PROGRAM,
                        creator_fee_vault: target_static_accounts[accounts[9].account_index as usize],
                        event_authority: target_static_accounts[accounts[10].account_index as usize],
                        pump_program: PUMPFUN_PROGRAM_ID,
                        associated_token_program: ASSOCIATED_TOKEN_PROGRAM,
                        sandwich_state: state_account
                    })
                    .args(args::PumpfunFrontrunBuy {
                        target_base_amount_out: *amount,
                        target_max_quote_amount_in: *max_sol_cost,
                        sandwich_id: id.clone()
                    })
                    .instructions()
                    .map_err(|_| MevError::FailedToBuildTx)?;

                let back = program
                    .request()
                    .accounts(accounts::PumpfunBackrunBuy {
                        global: target_static_accounts[accounts[0].account_index as usize],
                        protocol_fee_recipient: target_static_accounts[accounts[1].account_index as usize],
                        mint: target_static_accounts[accounts[2].account_index as usize],
                        bonding_curve: target_static_accounts[accounts[3].account_index as usize],
                        bonding_curve_ata: target_static_accounts[accounts[4].account_index as usize],
                        user_ata: get_associated_token_address(
                            &signer.pubkey(),
                            &target_static_accounts[accounts[2].account_index as usize]
                        ),
                        user: signer.pubkey(),
                        system_program: SYSTEM_PROGRAM,
                        token_program: TOKEN_PROGRAM,
                        creator_fee_vault: target_static_accounts[accounts[9].account_index as usize],
                        event_authority: target_static_accounts[accounts[10].account_index as usize],
                        pump_program: PUMPFUN_PROGRAM_ID,
                        sandwich_state: state_account
                    })
                    .args(args::PumpfunBackrunBuy {
                        sandwich_id: id
                    })
                    .instructions()
                    .map_err(|_| MevError::FailedToBuildTx)?;

                Ok((
                    MessageV0::try_compile(
                        &signer.pubkey(),
                        &front,
                        &[],
                        recent_blockhash
                    ).map_err(|_| MevError::FailedToBuildTx)?,
                    MessageV0::try_compile(
                        &signer.pubkey(),
                        &back,
                        &[],
                        recent_blockhash
                    ).map_err(|_| MevError::FailedToBuildTx)?
                ))
            },
            ParsedPumpFunInstructions::Sell { .. } => {
                Err(MevError::FailedToBuildTx)
            }
        }
    }
}

#[cfg(test)]
mod test {
    use crate::programs::{pumpfun::ParsedPumpFunInstructions, Account, ParsedInstruction};
    use solana_sdk::{compute_budget::{ComputeBudgetInstruction, ID as COMPUTE_BUDGET_PROGRAM}, hash::Hash, instruction::Instruction, message::{Message, VersionedMessage}, transaction::VersionedTransaction, pubkey::Pubkey};
    use super::MevInstructionBuilder;

    #[test]
    fn should_generate_swap_uuid_and_pda() {
        let sample_ix = [
            102, 6, 61, 18, 1, 218, 235, 234,
            27, 162, 85, 43, 0, 0, 0, 0,
            216, 158, 3, 0, 0, 0, 0, 0
        ].to_vec();
        let key_i: Vec<u8> = [7, 1, 8, 2, 3, 4, 0, 9, 10, 11, 12, 6].to_vec();
        let target = ParsedPumpFunInstructions::from_bytes(
            sample_ix,
            key_i.iter().map(|i| Account::new(i, false)).collect()
        );
        let builder = MevInstructionBuilder::from_parsed_ix(ParsedInstruction::PumpFun(target)).unwrap();
        let (_, id) = builder.derive_pda().unwrap();
        println!("{:?}", id);
    }

    #[test]
    fn should_create_compute_budget_instructions() {
        // Test with both parameters specified
        let instructions = MevInstructionBuilder::create_compute_budget_instructions(
            Some(300_000),
            Some(1_000)
        );
        
        assert_eq!(instructions.len(), 2);
        assert_eq!(instructions[0].program_id, COMPUTE_BUDGET_PROGRAM);
        assert_eq!(instructions[1].program_id, COMPUTE_BUDGET_PROGRAM);
        
        // Verify first instruction is set_compute_unit_limit with correct data
        let expected_unit_limit = ComputeBudgetInstruction::set_compute_unit_limit(300_000);
        assert_eq!(instructions[0].data, expected_unit_limit.data);
        
        // Verify second instruction is set_compute_unit_price with correct data
        let expected_unit_price = ComputeBudgetInstruction::set_compute_unit_price(1_000);
        assert_eq!(instructions[1].data, expected_unit_price.data);
        
        // Test with only units parameter
        let instructions_units_only = MevInstructionBuilder::create_compute_budget_instructions(
            Some(200_000),
            None
        );
        
        assert_eq!(instructions_units_only.len(), 1);
        assert_eq!(instructions_units_only[0].program_id, COMPUTE_BUDGET_PROGRAM);
        
        // Test with only price parameter
        let instructions_price_only = MevInstructionBuilder::create_compute_budget_instructions(
            None,
            Some(500)
        );
        
        assert_eq!(instructions_price_only.len(), 1);
        assert_eq!(instructions_price_only[0].program_id, COMPUTE_BUDGET_PROGRAM);
        
        // Test with no parameters
        let empty_instructions = MevInstructionBuilder::create_compute_budget_instructions(
            None,
            None
        );
        
        assert_eq!(empty_instructions.len(), 0);
    }
    
    #[test]
    fn should_create_compute_budget_instructions_from_target() {        
        // Create a mock instruction that looks like a ComputeBudget instruction
        let mock_compute_budget_data = [
            0, // SetComputeUnitLimit discriminator
            0x10, 0xF6 // 20,000 in little-endian bytes
        ];
        
        let mock_compute_budget_ix = Instruction {
            program_id: COMPUTE_BUDGET_PROGRAM,
            accounts: vec![],
            data: mock_compute_budget_data.clone().to_vec(),
        };
        
        // Create a mock transaction with the compute budget instruction
        let mock_tx = VersionedTransaction {
            signatures: vec![],
            message: VersionedMessage::Legacy(Message::new(
                &[mock_compute_budget_ix],
                Some(&Pubkey::new_unique())
            )),
        };
        
        // Test the calculation logic
        let (frontrun, backrun) = MevInstructionBuilder::create_compute_budget_instructions_from_target(
            &mock_tx,
            Some(1000)
        );
        
        // Verify frontrun has 2 instructions (unit limit and price)
        assert_eq!(frontrun.len(), 2);
        assert_eq!(frontrun[0].program_id, COMPUTE_BUDGET_PROGRAM);
        
        // First instruction should be set_compute_unit_limit with ~35% more units
        let expected_frontrun_units = (20_000 as f32 * 1.35) as u32; // ~27,000
        let extracted_frontrun_units = MevInstructionBuilder::extract_compute_units(&frontrun[0]);
        assert!(extracted_frontrun_units.is_some());
        assert_eq!(extracted_frontrun_units.unwrap(), expected_frontrun_units);
        
        // Second instruction should be set_compute_unit_price with 1000
        assert_eq!(frontrun[1].data[0], 3); // SetComputeUnitPrice discriminator
        
        // Verify backrun has 1 instruction (unit limit only)
        assert_eq!(backrun.len(), 1);
        assert_eq!(backrun[0].program_id, COMPUTE_BUDGET_PROGRAM);
        
        // Backrun instruction should be set_compute_unit_limit with ~35% fewer units
        let expected_backrun_units = (20_000 as f32 * 0.65) as u32; // ~13,000
        let extracted_backrun_units = MevInstructionBuilder::extract_compute_units(&backrun[0]);
        assert!(extracted_backrun_units.is_some());
        assert_eq!(extracted_backrun_units.unwrap(), expected_backrun_units);
        
        // Test fallback to default for transaction with no compute budget
        let mock_tx_no_budget = VersionedTransaction {
            signatures: vec![],
            message: VersionedMessage::Legacy(Message::new_with_compiled_instructions(
                1,
                0,
                0,
                vec![Pubkey::new_unique()], // Some other program
                Hash::default(),
                vec![]
            )),
        };
        
        let (frontrun_default, backrun_default) = 
            MevInstructionBuilder::create_compute_budget_instructions_from_target(&mock_tx_no_budget, None);
        
        // Default should be used: 200_000 baseline
        // Frontrun: ~270,000 (35% more)
        let default_frontrun_units = MevInstructionBuilder::extract_compute_units(&frontrun_default[0]).unwrap();
        assert_eq!(default_frontrun_units, (20_000 as f32 * 1.35) as u32);
        
        // Backrun: ~130,000 (35% less)
        let default_backrun_units = MevInstructionBuilder::extract_compute_units(&backrun_default[0]).unwrap();
        assert_eq!(default_backrun_units, (20_000 as f32 * 0.65) as u32);
    }
}