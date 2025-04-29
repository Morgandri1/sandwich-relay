use crate::env::EngineSettings;
use crate::raydium::subscribe::PoolKeysSniper;
use crate::raydium::swap::instructions::{swap_base_in, SOLC_MINT};
use crate::rpc::HTTP_CLIENT;
use crate::{
    env::load_settings,
    raydium::{subscribe::auto_sniper_stream, swap::instructions::USDC_MINT},
};
use eyre::Result;
use log::{error, info};
use solana_client::rpc_config::RpcSendTransactionConfig;
use solana_client::rpc_request::TokenAccountsFilter;
use solana_program::pubkey::Pubkey;
use solana_sdk::commitment_config::CommitmentLevel;
use solana_sdk::native_token::lamports_to_sol;
use solana_sdk::transaction::VersionedTransaction;
use solana_sdk::{signature::Keypair, signer::Signer};
use std::str::FromStr;
use std::time::Duration;
use std::{error::Error, sync::Arc};
use tokio::time;

use super::instructions::token_price_data;

use std::{env, path::PathBuf};

use clap::{Parser, Subcommand};
use env_logger::TimestampPrecision;
use futures_util::StreamExt;
use jito_protos::{
    convert::versioned_tx_from_packet,
    searcher::{
        mempool_subscription, searcher_service_client::SearcherServiceClient,
        ConnectedLeadersRegionedRequest, GetTipAccountsRequest, MempoolSubscription,
        NextScheduledLeaderRequest, PendingTxNotification, ProgramSubscriptionV0,
        SubscribeBundleResultsRequest, WriteLockedAccountSubscriptionV0,
    },
};
use jito_searcher_client::{
    get_searcher_client, send_bundle_no_wait, send_bundle_with_confirmation,
    token_authenticator::ClientInterceptor,
};

use solana_client::nonblocking::rpc_client::RpcClient;
use solana_sdk::hash::Hash;
use solana_sdk::{
    commitment_config::CommitmentConfig, signature::read_keypair_file,
    system_instruction::transfer, transaction::Transaction,
};
use spl_memo::build_memo;
use tokio::time::{sleep, timeout};
use tonic::{codegen::InterceptedService, transport::Channel, Streaming};
#[derive(Parser, Debug)]
#[clap(author, version, about, long_about = None)]
struct Args {
    /// URL of the block engine.
    /// See: https://jito-labs.gitbook.io/mev/searcher-resources/block-engine#connection-details
    #[arg(long, env)]
    block_engine_url: String,

    /// Path to keypair file used to authenticate with the Jito Block Engine
    /// See: https://jito-labs.gitbook.io/mev/searcher-resources/getting-started#block-engine-api-key
    #[arg(long, env)]
    keypair_path: PathBuf,

    /// Comma-separated list of regions to request cross-region data from.
    /// If no region specified, then default to the currently connected block engine's region.
    /// Details: https://jito-labs.gitbook.io/mev/searcher-services/recommendations#cross-region
    /// Available regions: https://jito-labs.gitbook.io/mev/searcher-resources/block-engine#connection-details
    #[arg(long, env, value_delimiter = ',')]
    regions: Vec<String>,
}

pub async fn raydium_in(
    wallet: &Arc<Keypair>,
    pool_keys: PoolKeysSniper,
    amount_in: u64,
    amount_out: u64,
    priority_fee: u64,
) -> eyre::Result<()> {
    let user_source_owner = wallet.pubkey();
    let rpc_client = {
        let http_client = HTTP_CLIENT.lock().unwrap();
        http_client.get("http_client").unwrap().clone()
    };

    let rpc_client = Arc::new(rpc_client);

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
    )
    .await?;

    let config = CommitmentLevel::Confirmed;
    let (latest_blockhash, _) = rpc_client
        .get_latest_blockhash_with_commitment(solana_sdk::commitment_config::CommitmentConfig {
            commitment: config,
        })
        .await?;

    let message = match solana_program::message::v0::Message::try_compile(
        &user_source_owner,
        &swap_instructions,
        &[],
        latest_blockhash,
    ) {
        Ok(x) => x,
        Err(e) => {
            println!("Error: {:?}", e);
            return Ok(());
        }
    };

    let transaction = match VersionedTransaction::try_new(
        solana_program::message::VersionedMessage::V0(message),
        &[&wallet],
    ) {
        Ok(x) => x,
        Err(e) => {
            println!("Error: {:?}", e);
            return Ok(());
        }
    };

    let config = RpcSendTransactionConfig {
        skip_preflight: true,
        ..Default::default()
    };

    let result = match rpc_client
        .send_transaction_with_config(&transaction, config)
        .await
    {
        Ok(x) => x,
        Err(e) => {
            error!("Error: {:?}", e);
            return Ok(());
        }
    };

    info!("Transaction Signature: {:?}", result.to_string());

    // let rpc_client_1 = rpc_client.clone();
    // tokio::spawn(async move {
    //     let _ = match rpc_client_1
    //         .confirm_transaction_with_spinner(
    //             &result,
    //             &rpc_client_1.get_latest_blockhash().await.unwrap(),
    //             solana_sdk::commitment_config::CommitmentConfig::processed(),
    //         )
    //         .await
    //     {
    //         Ok(x) => x,
    //         Err(e) => {
    //             error!("Error: {:?}", e);
    //         }
    //     };
    // });

    let pool_keys_clone = pool_keys.clone();
    let wallet_clone = wallet.clone();
    let _ = price_logger(pool_keys_clone, &wallet_clone).await;
    Ok(())
}
pub async fn raydium_bundle_in(
    wallet: &Arc<Keypair>,
    pool_keys: PoolKeysSniper,
    amount_in: u64,
    amount_out: u64,
    priority_fee: u64,
    blockhash: Hash,
) -> eyre::Result<VersionedTransaction> {
    let user_source_owner = wallet.pubkey();

    let token_address = if pool_keys.base_mint == SOLC_MINT {
        pool_keys.clone().quote_mint
    } else {
        pool_keys.clone().base_mint
    };
    println!("{:?}", token_address);
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
    )
    .await?;

    let config = CommitmentLevel::Confirmed;

    let message = match solana_program::message::v0::Message::try_compile(
        &user_source_owner,
        &swap_instructions,
        &[],
        blockhash,
    ) {
        Ok(x) => x,
        Err(e) => {
            println!("Error: {:?}", e);
            return Err(eyre::Report::from(e));
        }
    };

    let transaction = match VersionedTransaction::try_new(
        solana_program::message::VersionedMessage::V0(message),
        &[&wallet],
    ) {
        Ok(x) => x,
        Err(e) => {
            println!("Error: {:?}", e);
            return Err(eyre::Report::from(e));
        }
    };
    println!("{:?}", blockhash);

    // let rpc_client_1 = rpc_client.clone();
    // tokio::spawn(async move {
    //     let _ = match rpc_client_1
    //         .confirm_transaction_with_spinner(
    //             &result,
    //             &rpc_client_1.get_latest_blockhash().await.unwrap(),
    //             solana_sdk::commitment_config::CommitmentConfig::processed(),
    //         )
    //         .await
    //     {
    //         Ok(x) => x,
    //         Err(e) => {
    //             error!("Error: {:?}", e);
    //         }
    //     };
    // });

    Ok(transaction)
}

pub async fn bundle_transactions(
    wallet: &Arc<Keypair>,
    pool_keys: PoolKeysSniper,
    amount_in: u64,
    amount_out: u64,
    priority_fee: u64,
) -> Result<(), Box<dyn Error>> {

    let args = match load_settings().await {
        Ok(args) => args,
        Err(e) => {
            error!("Error: {:?}", e);
            return Err(e.into());
        }
    };
    let keypair_string = args.payer_keypair[0].to_string();
    // let json_bytes = string_to_json_bytes(&keypair_string)?;
    let mut settings_path = env::entcurr_exe()?;
    settings_path.pop(); // Remove the executable name, leaving just the directory path
    settings_path.push("key.json");
    // Check if the file exists
    // Attempt to read the keypair file
    let payer_keypair = Arc::new(read_keypair_file(settings_path)?);

    let mut client = get_searcher_client(args.block_engine_url.as_str(), &payer_keypair)
        .await
        .expect("connects to searcher client");
    let rpc_url = args.rpc_url.clone();
    let rpc_client = RpcClient::new_with_commitment(rpc_url, CommitmentConfig::confirmed());
    let balance = rpc_client
        .get_balance(&payer_keypair.pubkey())
        .await
        .expect("reads balance");

    info!(
        "payer public key: {:?} lamports: {balance:?}",
        payer_keypair.pubkey(),
    );
    let message = "test";
    let mut tries = 0;

    while tries < 50 {
        let mut bundle_results_subscription = client
            .subscribe_bundle_results(SubscribeBundleResultsRequest {})
            .await
            .expect("subscribe to bundle results")
            .into_inner();

        // wait for jito-solana leader slot
        let mut is_leader_slot = false;
        while !is_leader_slot {
            let next_leader = client
                .get_next_scheduled_leader(NextScheduledLeaderRequest {
                    regions: Vec::new(),
                })
                .await
                .expect("gets next scheduled leader")
                .into_inner();
            let num_slots = next_leader.next_leader_slot - next_leader.current_slot;
            is_leader_slot = num_slots <= 2;
            info!(
                "next jito leader slot in {num_slots} slots in {}",
                next_leader.next_leader_region
            );
            sleep(Duration::from_millis(100)).await;
        }

        // build + sign the transactions
        let tip_account_pubkey = Pubkey::from_str(&args.tipAccount).unwrap();

        fn getTransactions(
            payer_keypair: &Keypair,
            blockhash: Hash,
            tip_account_pubkey: Pubkey,
            buyTransaction: VersionedTransaction,
            args: EngineSettings,
        ) -> Vec<VersionedTransaction> {
            let tx0 = buyTransaction;
            let tx1 = VersionedTransaction::from(Transaction::new_signed_with_payer(
                &[transfer(
                    &payer_keypair.pubkey(),
                    &tip_account_pubkey,
                    args.lamports,
                )],
                Some(&payer_keypair.pubkey()),
                &[&payer_keypair],
                blockhash,
            ));

            vec![tx0, tx1]
        }
        let txs: Vec<_> = (0..args.num_txs).map(|i| {}).collect();
        let payer_keypair_ref = Arc::as_ref(&payer_keypair);
        let blockhash = rpc_client
            .get_latest_blockhash()
            .await
            .expect("get blockhash");
       
        let payer_keypair_ref = Arc::as_ref(&payer_keypair);
 
        let buy_transaction = raydium_bundle_in(
            wallet,
            pool_keys.clone(),
            amount_in,
            amount_out,
            priority_fee,
            blockhash,
        )
        .await?;
        let transactions = getTransactions(
            payer_keypair_ref,
            blockhash,
            tip_account_pubkey,
            buy_transaction,
            args.clone(),
        );

        // match send_bundle_with_confirmation(
        //     &transactions,
        //     &rpc_client,
        //     &mut client,
        //     &mut bundle_results_subscription,
        // )
        // .await
        // {
        //     Ok(_) => {
        //         // Handle success, the bundle was sent and confirmed
        //     }
        //     Err(e) => {
        //         // Handle the error without stopping the entire CLI
        //         eprintln!("Failed to send bundle: {}", e);
        //         // You can decide to retry, log the error, or take other actions
        //     }
        // }

        match send_bundle_no_wait(
            &transactions,

            &mut client,
        ).await{
            Ok(_) => {
                // Handle success, the bundle was sent and confirmed
            },
            Err(e) => {
                // Handle the error without stopping the entire CLI
                eprintln!("Failed to send bundle: {}", e);
                // You can decide to retry, log the error, or take other actions
            }
        }

        // Sleep for 100 ms before repeating the process
        tries += 1;
    }

    Ok(())
}
async fn price_logger(
    //mut stop_rx: mpsc::Receiver<()>,
    pool_keys: PoolKeysSniper,
    wallet: &Arc<Keypair>,
) -> eyre::Result<()> {
    let rpc_client = {
        let http_client = HTTP_CLIENT.lock().unwrap();
        http_client.get("http_client").unwrap().clone()
    };
    loop {
        // if let Ok(_) = stop_rx.try_recv() {
        //     break;
        // }

        let mut token_balance = 0;
        let rpc_client_clone = rpc_client.clone();
        let pool_keys_clone = pool_keys.clone();
        let wallet_clone = Arc::clone(wallet);
        let token_accounts = rpc_client_clone
            .get_token_accounts_by_owner(
                &wallet_clone.pubkey(),
                TokenAccountsFilter::Mint(pool_keys_clone.base_mint),
            )
            .await?;

        for rpc_keyed_account in &token_accounts {
            let pubkey = &rpc_keyed_account.pubkey;
            //convert to pubkey
            let pubkey = Pubkey::from_str(&pubkey)?;

            let balance = rpc_client_clone.get_token_account_balance(&pubkey).await?;
            info!("balance: {:?}", balance.ui_amount_string);
            let lamports = match balance.amount.parse::<u64>() {
                Ok(lamports) => lamports,
                Err(e) => {
                    eprintln!("Failed to parse balance: {}", e);
                    break;
                }
            };

            token_balance = lamports;

            if lamports != 0 {
                break;
            }

            std::thread::sleep(Duration::from_secs(1));
        }

        let price = token_price_data(
            rpc_client_clone,
            pool_keys_clone,
            wallet_clone,
            token_balance,
        )
        .await?;

        info!("Worth: {:?} Sol", lamports_to_sol(price as u64));
        // Sleep for a while
        time::sleep(Duration::from_secs(1)).await;
    }
}
