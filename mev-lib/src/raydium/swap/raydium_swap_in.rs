use crate::raydium::subscribe::PoolKeys;
use crate::raydium::swap::instructions::{swap_base_in, SOLC_MINT};
use crate::result::{MevError, MevResult};
use solana_sdk::transaction::VersionedTransaction;
use solana_sdk::{signature::Keypair, signer::Signer};
use solana_sdk::hash::Hash;
use std::sync::Arc;

pub fn raydium_in(
    wallet: &Arc<Keypair>,
    pool_keys: PoolKeys,
    amount_in: u64,
    amount_out: u64,
    priority_fee: u64,
    latest_blockhash: Hash
) -> MevResult<VersionedTransaction> {
    let user_source_owner = wallet.pubkey();

    let token_address = if pool_keys.base_mint == SOLC_MINT {
        pool_keys.clone().quote_mint
    } else {
        pool_keys.clone().base_mint
    };

    let swap_instructions = swap_base_in(
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
        &user_source_owner,
        &token_address,
        amount_in,
        amount_out,
        priority_fee,
    ).map_err(|_| crate::result::MevError::ValueError)?;

    let message = match solana_program::message::v0::Message::try_compile(
        &user_source_owner,
        &swap_instructions,
        &[],
        latest_blockhash,
    ) {
        Ok(x) => x,
        Err(e) => {
            println!("Error: {:?}", e);
            return Err(MevError::FailedToBuildTx);
        }
    };

    let transaction = match VersionedTransaction::try_new(
        solana_program::message::VersionedMessage::V0(message),
        &[&wallet],
    ) {
        Ok(x) => x,
        Err(e) => {
            println!("Error: {:?}", e);
            return Err(MevError::FailedToBuildTx);
        }
    };
    
    Ok(transaction)
}