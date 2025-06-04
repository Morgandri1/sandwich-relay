use solana_client::rpc_client::RpcClient;
use solana_sdk::pubkey::Pubkey;
use spl_token::state::{Account, GenericTokenAccount};

use crate::result::{MevError, MevResult};

fn client() -> MevResult<RpcClient> {
    let c = RpcClient::new("http://localhost:8899/");
    if c.get_health().is_ok() {
        Ok(c)
    } else {
        Err(MevError::UnknownError)
    }
}

pub fn get_mint_of_account(account: &Pubkey) -> MevResult<Pubkey> {
    let c = client()?;
    let acct = c.get_account(account).map_err(|_| MevError::UnknownError)?;
    match Account::unpack_account_mint(&acct.data) {
        Some(p) => Ok(*p),
        None => Err(MevError::ValueError)
    }
}

