use std::sync::Arc;

use solana_sdk::{signature::Keypair, transaction::VersionedTransaction};

use crate::{
    comp::SwapProviders, math::calculate_swap_input_output, raydium::subscribe::PoolKeys, result::{MevError, MevResult}, tx::get_pool_keys_from_target, SwapArgs
};

pub fn buy_router(provider: SwapProviders, target_tx: VersionedTransaction, signer: Arc<Keypair>, priority_fee_lamports: u64) -> MevResult<VersionedTransaction> {
    let (input, output) = match calculate_swap_input_output(&target_tx) {
        Ok((input, output)) => (input, output),
        Err(_) => return Err(MevError::FailedToBuildTx)
    };
    let args = SwapArgs {
        signer,
        input,
        priority_fee_lamports,
        target_tx_blockhash: *target_tx.message.recent_blockhash(),
        expected_output: output,
        pool_keys: get_pool_keys_from_target(&target_tx)?,
    };
    match provider {
        SwapProviders::Raydium => crate::raydium::swap::buy(args),
        _ => Err(MevError::ValueError)
    }
}

pub fn sell_router(provider: SwapProviders, target_tx: VersionedTransaction) -> MevResult<VersionedTransaction> {
    
    match provider {
        SwapProviders::Raydium => crate::raydium::swap::sell(todo!()),
        _ => Err(MevError::ValueError)
    }
}