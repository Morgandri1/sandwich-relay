use solana_sdk::transaction::VersionedTransaction;

use crate::result::MevResult;

pub fn calculate_swap_input_output(target_tx: &VersionedTransaction) -> MevResult<(u64, u64)> {
    todo!()
}
struct PoolInfo {
    x: u128,
    y: u128,
    k: u128,
}
const RAYDIUM_FEE_BP: u128 = 25;
const FEE_DENOMINATOR: u128 = 10000;
pub fn calculate_tx_input_raydium(
    in_amount: u128,
    min_out_amount: u128,
    x_to_y: bool,
    pool_info: PoolInfo,
) -> u128 {
    if x_to_y {
        (pool_info.k / (pool_info.y - min_out_amount)) - pool_info.x
    } else {
        (pool_info.k / (pool_info.x - min_out_amount)) - pool_info.y
    }
}
// pub fn calculate_tx_input_pump(in_amount: u64, out_amount: u64) -> u64 {}
