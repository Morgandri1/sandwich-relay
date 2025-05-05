use std::sync::Arc;

use solana_sdk::{signature::Keypair, transaction::VersionedTransaction};

use crate::{
    comp::SwapProviders, math::calculate_tx_input_raydium, raydium::subscribe::PoolKeys, result::{MevError, MevResult}, SwapArgs
};

pub fn buy_router(provider: SwapProviders, target_tx: VersionedTransaction, signer: Arc<Keypair>, priority_fee_lamports: u64) -> MevResult<VersionedTransaction> {
    todo!()
    // let (input, output) = match  {
    //     Ok((input, output)) => (input, output),
    //     Err(_) => return Err(MevError::FailedToBuildTx)
    // };
    // let args = SwapArgs {
    //     signer,
    //     input,
    //     priority_fee_lamports,
    //     target_tx_blockhash: *target_tx.message.recent_blockhash(),
    //     expected_output: output,
    //     pool_keys: todo!(),
    // };
    // match provider {
    //     SwapProviders::Raydium => crate::raydium::swap::buy(args),
    //     _ => Err(MevError::ValueError)
    // }
}

pub fn sell_router(provider: SwapProviders, target_tx: VersionedTransaction) -> MevResult<VersionedTransaction> {
    
    match provider {
        SwapProviders::Raydium => crate::raydium::swap::sell(todo!()),
        _ => Err(MevError::ValueError)
    }
}