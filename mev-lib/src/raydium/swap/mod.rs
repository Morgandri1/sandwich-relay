pub mod instructions;
pub mod raydium_swap_in;
pub mod raydium_swap_out;

use solana_sdk::transaction::VersionedTransaction;
use crate::result::MevResult;
use raydium_swap_in::raydium_in;
use raydium_swap_out::raydium_out;

use crate::SwapArgs;

pub fn buy(args: SwapArgs) -> MevResult<VersionedTransaction> {
    return raydium_in(
        &args.signer, 
        args.pool_keys, 
        args.input, 
        args.expected_output, 
        args.priority_fee_lamports, 
        args.target_tx_blockhash
    )
}

pub fn sell(args: SwapArgs) -> MevResult<VersionedTransaction> {
    return raydium_out(
        &args.signer, 
        args.pool_keys, 
        args.input, 
        args.expected_output, 
        args.priority_fee_lamports, 
        args.target_tx_blockhash
    )
}