use solana_sdk::transaction::VersionedTransaction;
use solana_sdk::{signature::Keypair, signer::Signer};
use std::sync::Arc;

use crate::raydium::subscribe::PoolKeys;
use crate::raydium::swap::instructions::{swap_base_out, SOLC_MINT};
use crate::result::{MevError, MevResult};

pub fn raydium_out(
    wallet: &Arc<Keypair>,
    pool_keys: PoolKeys,
    amount_in: u64,
    amount_out: u64,
    priority_fee: u64,
    latest_blockhash: solana_sdk::hash::Hash
) -> MevResult<VersionedTransaction> {
    let user_source_owner = wallet.pubkey();

    let token_address = if pool_keys.base_mint == SOLC_MINT {
        pool_keys.clone().quote_mint
    } else {
        pool_keys.clone().base_mint
    };

    let swap_instructions = swap_base_out(
        &pool_keys.program_id,
        &pool_keys.id,
        &pool_keys.authority,
        &pool_keys.open_orders,
        &pool_keys.target_orders,
        &pool_keys.base_vault,
        &pool_keys.quote_vault,
        &pool_keys.market_program_id,
        &pool_keys.market_id,
        &pool_keys.market_bids,
        &pool_keys.market_asks,
        &pool_keys.market_event_queue,
        &pool_keys.market_base_vault,
        &pool_keys.market_quote_vault,
        &pool_keys.market_authority,
        &user_source_owner,
        &user_source_owner,
        &token_address,
        amount_in,
        amount_out,
        priority_fee,
    ).map_err(|_| MevError::FailedToBuildTx)?;

    let message = solana_program::message::v0::Message::try_compile(
        &user_source_owner,
        &swap_instructions,
        &[],
        latest_blockhash,
    ).map_err(|_| MevError::FailedToBuildTx)?;

    let transaction = VersionedTransaction::try_new(
        solana_program::message::VersionedMessage::V0(message),
        &[&wallet],
    ).map_err(|_| MevError::FailedToBuildTx)?;

    Ok(transaction)
}
