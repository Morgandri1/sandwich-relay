pub mod result;
pub mod tx;
mod comp;
mod packets;
#[cfg(test)]
mod test;
mod swap;
mod raydium;
mod math;
mod tx_types;
mod programs;

use crate::raydium::subscribe::PoolKeys;
use solana_sdk::{
    hash::Hash,
    signature::Keypair,
};
use std::sync::Arc;

pub struct SwapArgs {
    pub signer: Arc<Keypair>,
    pub pool_keys: PoolKeys,
    pub input: u64,
    pub expected_output: u64,
    pub priority_fee_lamports: u64,
    pub target_tx_blockhash: Hash
}

pub use packets::*;