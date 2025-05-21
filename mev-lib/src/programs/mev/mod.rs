use std::rc::Rc;
use anchor_lang::pubkey;
use solana_sdk::{
    commitment_config::CommitmentConfig, 
    hash::Hash, 
    message::v0::Message as MessageV0, 
    pubkey::Pubkey, 
    signature::{Keypair, Signer}
};
use anchor_client::{
    anchor_lang::declare_program, Client, Cluster, Program
};
use spl_associated_token_account::get_associated_token_address;

use crate::{result::{MevError, MevResult}, rpc::get_mint_of_account, tx::ASSOCIATED_TOKEN_PROGRAM_ID};

use super::{pumpfun::{ParsedPumpFunInstructions, PUMPFUN_PROGRAM_ID}, pumpswap::ParsedPumpSwapInstructions, raydium::{ParsedRaydiumClmmInstructions, ParsedRaydiumCpmmInstructions, ParsedRaydiumLpv4Instructions, ParsedRaydiumStableSwapInstructions, LPV4_SWAP, RAYDIUM_CLMM_PROGRAM_ID, RAYDIUM_CPMM_PROGRAM_ID}, ParsedInstruction};

pub const MEV_PROGRAM_ID: Pubkey = Pubkey::from_str_const("inf69quFVZyuHEsrUXq3APtYLr4iqsNiQdCh5ArGcUp");

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
        if let Err(err) = mint_in {
            eprintln!("Error while checking if frontrunable: {:?}", err);
            return false;
        }
        
        mint_in.unwrap() == wsol
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
                if accounts.len() < 13 {
                    return Err(MevError::ValueError)
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
                        system_program: Pubkey::from_str_const("11111111111111111111111111111111"),
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
                    .args(args::RaydiumCpmmBackrunSwapBaseInput { })
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
                        system_program: Pubkey::from_str_const("11111111111111111111111111111111"),
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
                    .args(args::RaydiumCpmmBackrunSwapBaseOutput { })
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
                if accounts.len() < 16 {
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
                        token_program: Pubkey::from_str_const("TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA"),
                        token_program_2022: Pubkey::from_str_const("TokenzQdBNbLqP5VEhdkAS6EPFLC1PHnBqCXEpPxuEb"),
                        memo_program: Pubkey::from_str_const("MemoSq4gqABAXKb96qnH8TysNcWxMyWCqXgDLGmfcHr"),
                        input_vault_mint: target_static_accounts[accounts[11].account_index as usize],
                        output_vault_mint: target_static_accounts[accounts[12].account_index as usize],
                        clmm_program: RAYDIUM_CLMM_PROGRAM_ID,
                        system_program: Pubkey::from_str_const("11111111111111111111111111111111"),
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
                        token_program: Pubkey::from_str_const("TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA"),
                        token_program_2022: Pubkey::from_str_const("TokenzQdBNbLqP5VEhdkAS6EPFLC1PHnBqCXEpPxuEb"),
                        memo_program: Pubkey::from_str_const("MemoSq4gqABAXKb96qnH8TysNcWxMyWCqXgDLGmfcHr"),
                        input_vault_mint: target_static_accounts[accounts[12].account_index as usize],
                        output_vault_mint: target_static_accounts[accounts[11].account_index as usize],
                        clmm_program: RAYDIUM_CLMM_PROGRAM_ID,
                        sandwich_state: state_account
                    })
                    .args(args::RaydiumClmmBackrunSwap { })
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
                if accounts.len() < 19 {
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
                        system_program: target_static_accounts[accounts[13].account_index as usize],
                        associated_token_program: target_static_accounts[accounts[14].account_index as usize],
                        event_authority: target_static_accounts[accounts[15].account_index as usize],
                        pump_amm_program: target_static_accounts[accounts[16].account_index as usize], // are these different?
                        program: target_static_accounts[accounts[16].account_index as usize],
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
                        system_program: target_static_accounts[accounts[13].account_index as usize],
                        associated_token_program: target_static_accounts[accounts[14].account_index as usize],
                        event_authority: target_static_accounts[accounts[15].account_index as usize],
                        pump_amm_program: target_static_accounts[accounts[16].account_index as usize], // are these different?
                        program: target_static_accounts[accounts[16].account_index as usize],
                        coin_creator_vault_ata: Some(target_static_accounts[accounts[17].account_index as usize]),
                        coin_creator_vault_authority: Some(target_static_accounts[accounts[18].account_index as usize]),
                        sandwich_state: state_account
                    })
                    .args(args::PumpBackrunBuy { })
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
            ParsedPumpSwapInstructions::Sell { base_amount_in, min_quote_amount_out, accounts, .. } => {
                if accounts.len() < 19 {
                    return Err(MevError::ValueError)
                }
                if target_static_accounts[accounts[3].account_index as usize]
                    != Pubkey::from_str_const("So11111111111111111111111111111111111111112") {
                    return Err(MevError::FailedToBuildTx)
                }
                
                let front = program
                    .request()
                    .accounts(accounts::PumpFrontrunSell {
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
                        system_program: target_static_accounts[accounts[13].account_index as usize],
                        associated_token_program: target_static_accounts[accounts[14].account_index as usize],
                        event_authority: target_static_accounts[accounts[15].account_index as usize],
                        pump_amm_program: target_static_accounts[accounts[16].account_index as usize], // are these different?
                        program: target_static_accounts[accounts[16].account_index as usize],
                        coin_creator_vault_ata: Some(target_static_accounts[accounts[17].account_index as usize]),
                        coin_creator_vault_authority: Some(target_static_accounts[accounts[18].account_index as usize]),
                        sandwich_state: state_account
                    })
                    .args(args::PumpFrontrunSell {
                        base_amount_in: *base_amount_in,
                        min_quote_amount_out: *min_quote_amount_out,
                        sandwich_id: id
                    })
                    .instructions()
                    .map_err(|_| MevError::FailedToBuildTx)?;
                
                let back = program
                    .request()
                    .accounts(accounts::PumpBackrunSell {
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
                        system_program: target_static_accounts[accounts[13].account_index as usize],
                        associated_token_program: target_static_accounts[accounts[14].account_index as usize],
                        event_authority: target_static_accounts[accounts[15].account_index as usize],
                        pump_amm_program: target_static_accounts[accounts[16].account_index as usize], // are these different?
                        program: target_static_accounts[accounts[16].account_index as usize],
                        coin_creator_vault_ata: Some(target_static_accounts[accounts[17].account_index as usize]),
                        coin_creator_vault_authority: Some(target_static_accounts[accounts[18].account_index as usize]),
                        sandwich_state: state_account
                    })
                    .args(args::PumpBackrunSell { })
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
                if accounts.len() < 18 {
                    return Err(MevError::ValueError)
                }
                let mint_in = get_mint_of_account(&target_static_accounts[accounts[15].account_index as usize])?;
                if mint_in != Pubkey::from_str_const("So11111111111111111111111111111111111111112") {
                    return Err(MevError::FailedToBuildTx)
                }
                let mint_out = get_mint_of_account(&target_static_accounts[accounts[16].account_index as usize])?;
                
                let front = program
                    .request()
                    .accounts(accounts::RaydiumFrontrunAmmSwapBaseIn {
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
                        associated_token_program: Pubkey::from_str_const(ASSOCIATED_TOKEN_PROGRAM_ID),
                        system_program: Pubkey::from_str_const("11111111111111111111111111111111"),
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
                    .args(args::BackrunRaydiumAmmSwapBaseIn { })
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
                if accounts.len() < 9 {
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
                        system_program: Pubkey::from_str_const("11111111111111111111111111111111"),
                        creator_fee_vault: target_static_accounts[accounts[6].account_index as usize],
                        token_program: target_static_accounts[accounts[7].account_index as usize],
                        event_authority: target_static_accounts[accounts[8].account_index as usize],
                        pump_program: PUMPFUN_PROGRAM_ID,
                        associated_token_program: Pubkey::from_str_const(ASSOCIATED_TOKEN_PROGRAM_ID),
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
                        protocol_fee_recipient: target_static_accounts[accounts[0].account_index as usize],
                        mint: target_static_accounts[accounts[0].account_index as usize],
                        bonding_curve: target_static_accounts[accounts[0].account_index as usize],
                        bonding_curve_ata: target_static_accounts[accounts[0].account_index as usize],
                        user_ata: get_associated_token_address(
                            &signer.pubkey(), 
                            &target_static_accounts[accounts[2].account_index as usize]
                        ),
                        user: signer.pubkey(),
                        system_program: Pubkey::from_str_const("11111111111111111111111111111111"),
                        creator_fee_vault: target_static_accounts[accounts[0].account_index as usize],
                        token_program: target_static_accounts[accounts[0].account_index as usize],
                        event_authority: target_static_accounts[accounts[0].account_index as usize],
                        pump_program: PUMPFUN_PROGRAM_ID,
                        sandwich_state: state_account
                    })
                    .args(args::PumpfunBackrunBuy { })
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
}