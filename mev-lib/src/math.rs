// use solana_sdk::transaction::VersionedTransaction;

// use crate::result::MevResult;

// pub fn calculate_swap_input_output(target_tx: &VersionedTransaction) -> MevResult<(u64, u64)> {
//     todo!()
// }
pub struct PoolInfo {
    x: u128,
    y: u128,
    k: u128,
}
impl PoolInfo {
    pub fn calculate_min_out_amount(&self, in_amount: u128, slippage_bp: u128) -> u128 {
        let fee = in_amount.checked_mul(RAYDIUM_FEE_BP).unwrap() / FEE_DENOMINATOR;
        let net_in = in_amount.checked_sub(fee).unwrap();
        let x_after = self.x.checked_add(net_in).unwrap();
        let new_y = self.k.checked_div(x_after).unwrap();
        let gross_out = self.y.checked_sub(new_y).unwrap();
        let slippage_factor = FEE_DENOMINATOR.checked_sub(slippage_bp).unwrap();
        return gross_out.checked_mul(slippage_factor).unwrap() / FEE_DENOMINATOR;
    }

    pub fn swap(&mut self, in_amount: u128) -> u128 {
        let fee = in_amount.checked_mul(RAYDIUM_FEE_BP).unwrap() / FEE_DENOMINATOR;
        let net_in = in_amount - fee;
        let old_y = self.y;
        let x_after_fee = self.x.checked_add(net_in).unwrap();
        let new_y = self.k.checked_div(x_after_fee).unwrap();
        self.x = self.x.checked_add(in_amount).unwrap();
        self.y = new_y;
        return old_y - new_y;
    }
    pub fn print(&self) {
        println!("x: {}, y: {}, k: {}", self.x, self.y, self.k);
    }
}
const RAYDIUM_FEE_BP: u128 = 25;
const FEE_DENOMINATOR: u128 = 10000;
const FEE_DENOMINATOR: u128 = 10_000;

/// exactly UniswapV2Library.getAmountOut (with 0.25% fee)
fn get_amount_out(amount_in: u128, reserve_in: u128, reserve_out: u128) -> u128 {
    let fee_numer = FEE_DENOMINATOR - RAYDIUM_FEE_BP;
    let amount_in_with_fee = amount_in.checked_mul(fee_numer).unwrap(); // amount_in * 9_975
    let numerator = amount_in_with_fee.checked_mul(reserve_out).unwrap(); // × reserve_out
    let denominator = reserve_in
        .checked_mul(FEE_DENOMINATOR)
        .unwrap() // reserve_in * 10_000
        .checked_add(amount_in_with_fee)
        .unwrap(); // + amount_in_with_fee
    numerator / denominator // floor()
}

/// Find the **maximum** Δ in [0..reserve_in] such that
/// after you swap Δ, a user swap of `user_in` still gives ≥ `user_min_out`.
pub fn calculate_tx_input_raydium(
    in_amount: u128,
    min_out_amount: u128,
    x_to_y: bool,
    pool_info: PoolInfo,
    user_in: u128,      // how much the user will swap
    user_min_out: u128, // the minimum they must receive
    x_to_y: bool,       // direction: X→Y if true, else Y→X
    pool: &PoolInfo,
) -> u128 {
    if x_to_y {
        (pool_info.k / (pool_info.y - min_out_amount)) - pool_info.x
    let (reserve_in, reserve_out) = if x_to_y {
        (pool.x, pool.y)
    } else {
        (pool_info.k / (pool_info.x - min_out_amount)) - pool_info.y
        (pool.y, pool.x)
    };

    // If even with no sandwich, user gets ≥ min, start from Δ=0.
    let base_out = get_amount_out(user_in, reserve_in, reserve_out);
    if base_out < user_min_out {
        // impossible to satisfy user_min_out
        return 0;
    }

    // binary‐search for the largest Δ with user_out(Δ) >= user_min_out
    let mut low = 0u128;
    let mut high = reserve_in;

    while low < high {
        // use upper mid to avoid infinite loop
        let mid = (low + high + 1) / 2;

        // simulate your front‐run swap of Δ=mid
        let fr_out = get_amount_out(mid, reserve_in, reserve_out);
        let in_after = reserve_in.checked_add(mid).unwrap();
        let out_after = reserve_out.checked_sub(fr_out).unwrap();

        // simulate the user’s swap against the shifted pool
        let user_out = get_amount_out(user_in, in_after, out_after);

        if user_out >= user_min_out {
            // Δ=mid still lets the user hit their min ⇒ try a bigger Δ
            low = mid;
        } else {
            // too large ⇒ user_out < min ⇒ shrink Δ
            high = mid - 1;
        }
    }

    return low;
}
// pub fn calculate_tx_input_pump(in_amount: u64, out_amount: u64) -> u64 {}
// fn main() {
//     let slippage_bp = 500; // 5%
//     let mut pool = PoolInfo {
//         x: 10_000,
//         y: 10_000,
//         k: 10_000 * 10_000,
//     };

//     // 1) compute min_out (with slippage)
//     let min_out = pool.calculate_min_out_amount(10_000, slippage_bp);

//     // 2) find the sandwich amount
//     let sandwich = calculate_tx_input_raydium(10_000, min_out, true, &pool);

//     // 3) apply your sandwich swap
//     let _fr_out = pool.swap(sandwich);

//     // 4) now apply the user’s swap
//     let user_out = pool.swap(10_000);

//     println!(
//         "user_out={}  min_out={}  sandwich_in={}",
//         user_out, min_out, sandwich
//     );
//     // you should see user_out ≥ min_out
// }
