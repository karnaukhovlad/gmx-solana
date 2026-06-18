use std::{
    collections::HashSet,
    sync::{Arc, Mutex},
};

use gmsol_programs::anchor_lang;
use gmsol_sdk::{
    client::{
        ops::{
            AddressLookupTableOps, ExchangeOps, GlvOps, MarketOps, TokenConfigOps,
            VirtualInventoryOps,
        },
        pull_oracle::{PullOraclePriceConsumer, WithPullOracle},
        pyth::pull_oracle::PythPullOracleWithHermes,
    },
    constants::MARKET_USD_UNIT,
};
use gmsol_solana_utils::{
    bundle_builder::SendBundleOptions, make_bundle_builder::MakeBundleBuilder,
    transaction_builder::default_before_sign,
};
use gmsol_store::CoreError;
use gmsol_utils::{
    glv::GlvMarketFlag, market::MarketConfigKey, oracle::PriceProviderKind,
    token_config::UpdateTokenConfigParams,
};
use solana_client::{
    client_error::ClientErrorKind,
    rpc_config::RpcSendTransactionConfig,
    rpc_request::{RpcError, RpcResponseErrorData},
};
use solana_sdk::{
    address_lookup_table::AddressLookupTableAccount, message::VersionedMessage,
    native_token::LAMPORTS_PER_SOL, packet::PACKET_DATA_SIZE, pubkey::Pubkey,
    transaction::TransactionError,
};
use tracing::Instrument;

use crate::anchor_test::setup::{current_deployment, Deployment};

const ACCOUNT_LIMIT_MARKETS: usize = 12;
const ACCOUNT_LIMIT_PATH_MARKETS: usize = 8;
const ACTIVE_GLV_MARKETS: usize = 1;
const HEAVY_DEPOSIT_ACCOUNTS: usize = 65;
const HEAVY_WITHDRAWAL_ACCOUNTS: usize = 68;

#[derive(Clone, Copy)]
struct TransactionScenario {
    name: &'static str,
    swap_path_markets: usize,
    account_shape: &'static str,
}

impl TransactionScenario {
    const fn new(
        name: &'static str,
        swap_path_markets: usize,
        account_shape: &'static str,
    ) -> Self {
        Self {
            name,
            swap_path_markets,
            account_shape,
        }
    }
}

const BASELINE_DEPOSIT: TransactionScenario = TransactionScenario::new(
    "baseline deposit + close",
    0,
    "Only the active GLV market is priced; there is no swap path or route inventory.",
);
const BASELINE_WITHDRAWAL: TransactionScenario = TransactionScenario::new(
    "baseline withdrawal + close",
    0,
    "Only the active GLV market is priced; there is no output swap path or route inventory.",
);
const HEAVY_DEPOSIT: TransactionScenario = TransactionScenario::new(
    "route-heavy deposit + close",
    ACCOUNT_LIMIT_PATH_MARKETS,
    "Eight swap-path market references, distinct swap/position virtual inventories, and merged close accounts produce a 65-account transaction.",
);
const HEAVY_WITHDRAWAL: TransactionScenario = TransactionScenario::new(
    "route-heavy withdrawal + close",
    4,
    "Four output-route markets, virtual inventories, the external route market, receiver/output accounts, and merged close accounts produce a 68-account transaction.",
);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum TransactionOutcome {
    Executed,
    RejectedTooManyAccountLocks,
}

#[derive(Clone, Debug)]
struct TransactionAttempt {
    metrics: TransactionMetrics,
    outcome: TransactionOutcome,
}

#[derive(Clone, Debug)]
struct TransactionMetrics {
    total_account_references: usize,
    unique_accounts: usize,
    unique_oracle_feeds: usize,
    serialized_size: usize,
    close_merged: bool,
}

impl TransactionMetrics {
    fn from_message(
        message: &VersionedMessage,
        unique_oracle_feeds: usize,
        close_merged: bool,
    ) -> Self {
        let total_account_references = message
            .instructions()
            .iter()
            .map(|ix| ix.accounts.len() + 1)
            .sum();
        let unique_accounts = match message {
            VersionedMessage::Legacy(message) => message.account_keys.len(),
            VersionedMessage::V0(message) => {
                message.account_keys.len()
                    + message
                        .address_table_lookups
                        .iter()
                        .map(|lookup| lookup.writable_indexes.len() + lookup.readonly_indexes.len())
                        .sum::<usize>()
            }
        };
        let num_signatures = usize::from(message.header().num_required_signatures);
        let serialized_size = bincode::serialized_size(message).expect("message serializes")
            as usize
            + 1
            + 64 * num_signatures;
        Self {
            total_account_references,
            unique_accounts,
            unique_oracle_feeds,
            serialized_size,
            close_merged,
        }
    }
}

struct AccountLimitFixture {
    glv_token: Pubkey,
    market_tokens: Vec<Pubkey>,
    alt: AddressLookupTableAccount,
}

fn preflight_failure(
    error: &gmsol_sdk::Error,
) -> Option<&solana_client::rpc_response::RpcSimulateTransactionResult> {
    let gmsol_sdk::Error::SolanaUtils(gmsol_solana_utils::Error::Client(error)) = error else {
        return None;
    };
    let ClientErrorKind::RpcError(RpcError::RpcResponseError { data, .. }) = error.kind() else {
        return None;
    };
    let RpcResponseErrorData::SendTransactionPreflightFailure(result) = data else {
        return None;
    };
    Some(result)
}

async fn execute_with_metrics<'a, T>(
    deployment: &'a Deployment,
    execute: &mut T,
    scenario: TransactionScenario,
    close_merged: bool,
) -> gmsol_sdk::Result<TransactionAttempt>
where
    T: PullOraclePriceConsumer + MakeBundleBuilder<'a, gmsol_solana_utils::signer::SignerRef>,
{
    for attempt in 0..3 {
        match execute_with_metrics_once(deployment, execute, scenario, close_merged).await {
            Ok(metrics) => return Ok(metrics),
            Err(error)
                if attempt < 2
                    && error.anchor_error_code()
                        == Some(CoreError::OracleTimestampsAreSmallerThanRequired.into()) =>
            {
                tracing::warn!(
                    case = scenario.name,
                    attempt = attempt + 1,
                    "oracle update is stale; retrying transaction"
                );
                tokio::time::sleep(std::time::Duration::from_secs(2)).await;
            }
            Err(error) => return Err(error),
        }
    }
    unreachable!("the final attempt always returns")
}

async fn execute_with_metrics_once<'a, T>(
    deployment: &'a Deployment,
    execute: &mut T,
    scenario: TransactionScenario,
    close_merged: bool,
) -> gmsol_sdk::Result<TransactionAttempt>
where
    T: PullOraclePriceConsumer + MakeBundleBuilder<'a, gmsol_solana_utils::signer::SignerRef>,
{
    tracing::info!(
        case = scenario.name,
        configured_glv_markets = ACCOUNT_LIMIT_MARKETS,
        active_glv_markets = ACTIVE_GLV_MARKETS,
        inactive_glv_markets = ACCOUNT_LIMIT_MARKETS - ACTIVE_GLV_MARKETS,
        swap_path_markets = scenario.swap_path_markets,
        close_merged,
        account_shape = scenario.account_shape,
        "TRANSACTION SCENARIO"
    );
    eprintln!(
        "GLV_SCENARIO case={:?} configured_markets={} active_markets={} inactive_markets={} swap_path_markets={} close_merged={} account_shape={:?}",
        scenario.name,
        ACCOUNT_LIMIT_MARKETS,
        ACTIVE_GLV_MARKETS,
        ACCOUNT_LIMIT_MARKETS - ACTIVE_GLV_MARKETS,
        scenario.swap_path_markets,
        close_merged,
        scenario.account_shape,
    );

    tokio::time::sleep(std::time::Duration::from_secs(2)).await;
    let oracle = PythPullOracleWithHermes::from_parts(
        &deployment.client,
        &deployment.hermes,
        &deployment.pyth,
    );
    let (mut execute, feed_ids) = WithPullOracle::from_consumer(oracle, execute, None).await?;
    let unique_oracle_feeds = feed_ids.feeds.iter().copied().collect::<HashSet<_>>().len();
    let metrics = Arc::new(Mutex::new(Vec::new()));
    let captured = metrics.clone();

    let result = execute
        .build()
        .await?
        .build()?
        .send_all_with_opts(
            SendBundleOptions {
                config: RpcSendTransactionConfig {
                    skip_preflight: false,
                    ..Default::default()
                },
                ..Default::default()
            },
            move |message| {
                captured
                    .lock()
                    .expect("metrics lock")
                    .push(TransactionMetrics::from_message(
                        message,
                        unique_oracle_feeds,
                        close_merged,
                    ));
                default_before_sign(message)
            },
        )
        .await;

    let metrics = metrics
        .lock()
        .expect("metrics lock")
        .iter()
        .max_by_key(|metric| metric.unique_accounts)
        .cloned()
        .expect("at least one transaction must be built");

    tracing::info!(
        case = scenario.name,
        configured_glv_markets = ACCOUNT_LIMIT_MARKETS,
        active_glv_markets = ACTIVE_GLV_MARKETS,
        swap_path_markets = scenario.swap_path_markets,
        total_account_references = metrics.total_account_references,
        unique_accounts = metrics.unique_accounts,
        unique_oracle_feeds = metrics.unique_oracle_feeds,
        serialized_size = metrics.serialized_size,
        packet_limit = PACKET_DATA_SIZE,
        packet_headroom = PACKET_DATA_SIZE as i64 - metrics.serialized_size as i64,
        close_merged = metrics.close_merged,
        "GLV ACCOUNT REPORT"
    );
    eprintln!(
        "GLV_ACCOUNT_REPORT case={:?} configured_markets={} active_markets={} swap_path_markets={} total_account_references={} unique_accounts={} unique_oracle_feeds={} serialized_size={} packet_limit={} close_merged={close_merged}",
        scenario.name,
        ACCOUNT_LIMIT_MARKETS,
        ACTIVE_GLV_MARKETS,
        scenario.swap_path_markets,
        metrics.total_account_references,
        metrics.unique_accounts,
        metrics.unique_oracle_feeds,
        metrics.serialized_size,
        PACKET_DATA_SIZE,
    );

    match result {
        Ok(_) => {
            tracing::info!(
                case = scenario.name,
                actual_outcome = "EXECUTED",
                unique_accounts = metrics.unique_accounts,
                "RESULT: transaction executed"
            );
            eprintln!(
                "GLV_RESULT case={:?} actual=EXECUTED unique_accounts={}",
                scenario.name, metrics.unique_accounts,
            );
            Ok(TransactionAttempt {
                metrics,
                outcome: TransactionOutcome::Executed,
            })
        }
        Err((_, error)) => {
            let error = error.into();
            if preflight_failure(&error)
                .is_some_and(|result| result.err == Some(TransactionError::TooManyAccountLocks))
            {
                tracing::info!(
                    case = scenario.name,
                    actual_outcome = "REJECTED_TOO_MANY_ACCOUNT_LOCKS",
                    unique_accounts = metrics.unique_accounts,
                    "RESULT: transaction rejected before GMX execution"
                );
                eprintln!(
                    "GLV_RESULT case={:?} actual=REJECTED_TOO_MANY_ACCOUNT_LOCKS unique_accounts={}",
                    scenario.name, metrics.unique_accounts,
                );
                assert!(
                    metrics.serialized_size <= PACKET_DATA_SIZE,
                    "account-lock reproduction must not be a packet-size failure: {metrics:?}"
                );
                return Ok(TransactionAttempt {
                    metrics,
                    outcome: TransactionOutcome::RejectedTooManyAccountLocks,
                });
            }
            Err(error)
        }
    }
}

async fn create_account_limit_fixture(
    deployment: &Deployment,
) -> eyre::Result<AccountLimitFixture> {
    tracing::info!(
        markets = ACCOUNT_LIMIT_MARKETS,
        shared_index_feed = true,
        path_markets = ACCOUNT_LIMIT_PATH_MARKETS,
        "SETUP: creating isolated 12-market GLV fixture"
    );
    let keeper = deployment.user_client(Deployment::DEFAULT_KEEPER)?;
    let store = &deployment.store;
    let token_map = deployment.token_map();
    let rpc = keeper.store_program().rpc();
    let funding_signature = rpc
        .request_airdrop(&keeper.payer(), 50 * LAMPORTS_PER_SOL)
        .await?;
    rpc.confirm_transaction(&funding_signature).await?;
    let long_token = deployment.token("fBTC").expect("fBTC must exist");
    let short_token = deployment.token("USDG").expect("USDG must exist");
    let shared_feed = long_token.config.feed_id;
    let mut market_tokens = Vec::with_capacity(ACCOUNT_LIMIT_MARKETS);
    let mut index_tokens = Vec::with_capacity(ACCOUNT_LIMIT_MARKETS);

    for index in 0..ACCOUNT_LIMIT_MARKETS {
        let index_token = Pubkey::new_unique();
        let config = UpdateTokenConfigParams::default()
            .update_price_feed(&PriceProviderKind::Pyth, shared_feed, None)?
            .with_expected_provider(PriceProviderKind::Pyth)
            .with_precision(long_token.config.precision);
        keeper
            .insert_synthetic_token_config(
                store,
                &token_map,
                &format!("account-limit-index-{index}"),
                &index_token,
                long_token.config.decimals,
                config,
                true,
                true,
            )
            .send_without_preflight()
            .await?;

        let (create_market, market_token) = keeper
            .create_market(
                store,
                &format!("ACCOUNT-LIMIT-{index}/USD[fBTC-USDG]"),
                &index_token,
                &long_token.address,
                &short_token.address,
                true,
                Some(&token_map),
            )
            .await?;
        create_market.send_without_preflight().await?;
        market_tokens.push(market_token);
        index_tokens.push(index_token);
    }

    let current_market = market_tokens[0];
    for (key, value) in [
        (
            MarketConfigKey::MaxPoolValueForDepositForLongToken,
            10_000_000 * MARKET_USD_UNIT,
        ),
        (
            MarketConfigKey::MaxPoolValueForDepositForShortToken,
            10_000_000 * MARKET_USD_UNIT,
        ),
    ] {
        keeper
            .update_market_config_by_key(store, &current_market, key, &value)?
            .send_without_preflight()
            .await?;
    }

    let (initialize_glv, glv_token) =
        keeper.initialize_glv(store, 4_096, market_tokens[..1].iter().copied())?;
    initialize_glv.send_without_preflight().await?;
    for market_token in market_tokens.iter().skip(1) {
        keeper
            .insert_glv_market(store, &glv_token, market_token, None)
            .send_without_preflight()
            .await?;
    }

    for market_token in &market_tokens {
        keeper
            .toggle_glv_market_flag(
                store,
                &glv_token,
                market_token,
                GlvMarketFlag::IsDepositAllowed,
                true,
            )
            .send_without_preflight()
            .await?;
    }

    let mut virtual_inventories = Vec::with_capacity(2 * (ACCOUNT_LIMIT_PATH_MARKETS + 1));
    for (index, (market_token, index_token)) in market_tokens
        .iter()
        .zip(index_tokens.iter())
        .take(ACCOUNT_LIMIT_PATH_MARKETS + 1)
        .enumerate()
    {
        let market = keeper.find_market_address(store, market_token);
        let (create_swap_inventory, swap_inventory) = keeper
            .create_virtual_inventory_for_swaps(
                store,
                10_000 + index as u32,
                long_token.config.decimals,
                short_token.config.decimals,
            )?
            .swap_output(());
        create_swap_inventory.send_without_preflight().await?;
        keeper
            .join_virtual_inventory_for_swaps(store, &market, &swap_inventory, Some(&token_map))
            .await?
            .send_without_preflight()
            .await?;
        virtual_inventories.push(swap_inventory);

        let (create_position_inventory, position_inventory) = keeper
            .create_virtual_inventory_for_positions(store, index_token)?
            .swap_output(());
        create_position_inventory.send_without_preflight().await?;
        keeper
            .join_virtual_inventory_for_positions(store, &market, &position_inventory)?
            .send_without_preflight()
            .await?;
        virtual_inventories.push(position_inventory);
    }

    let (create_alt, alt_address) = keeper.create_alt().await?;
    create_alt.send_without_preflight().await?;
    let glv = keeper.find_glv_address(&glv_token);
    let mut alt_addresses = vec![glv, glv_token];
    for market_token in &market_tokens {
        alt_addresses.push(*market_token);
        alt_addresses.push(keeper.find_market_address(store, market_token));
        alt_addresses.push(keeper.find_market_vault_address(store, market_token));
    }
    alt_addresses.extend(virtual_inventories);
    keeper
        .extend_alt(&alt_address, alt_addresses, Some(20))?
        .build()?
        .send_all(false)
        .await
        .map_err(|(_, error)| error)?;
    tokio::time::sleep(std::time::Duration::from_secs(1)).await;
    let alt = keeper.alt(&alt_address).await?.expect("ALT must exist");

    tracing::info!(
        %glv_token,
        markets = market_tokens.len(),
        alt = %alt.key,
        alt_addresses = alt.addresses.len(),
        "SETUP COMPLETE"
    );

    Ok(AccountLimitFixture {
        glv_token,
        market_tokens,
        alt,
    })
}

#[tokio::test]
async fn initialize_glv() -> eyre::Result<()> {
    let deployment = current_deployment().await?;
    let _guard = deployment.use_accounts().await?;
    let span = tracing::info_span!("initialize_glv");
    let _enter = span.enter();

    let store = &deployment.store;
    let keeper = deployment.user_client(Deployment::DEFAULT_KEEPER)?;

    let market_token_1 = deployment
        .market_token("fBTC", "WSOL", "USDG")
        .expect("must exist");
    let market_token_2 = deployment
        .market_token("SOL", "WSOL", "USDG")
        .expect("must exist");

    let index = 255;
    let (rpc, glv_token) = keeper.initialize_glv(store, 255, [*market_token_1, *market_token_2])?;
    let signature = rpc.send_without_preflight().await?;
    tracing::info!(%signature, %index, %glv_token, "initialized a new GLV token");

    Ok(())
}

#[tokio::test]
async fn glv_deposit() -> eyre::Result<()> {
    let deployment = current_deployment().await?;
    let _guard = deployment.use_accounts().await?;
    let span = tracing::info_span!("glv_deposit");
    let _enter = span.enter();

    let user = deployment.user_client(Deployment::DEFAULT_USER)?;
    let keeper = deployment.user_client(Deployment::DEFAULT_KEEPER)?;

    let store = &deployment.store;
    let oracle = &deployment.oracle();
    let glv_token = &deployment.glv_token;
    let market_token = deployment.market_token("SOL", "fBTC", "USDG").unwrap();
    let market_token_2 = deployment.market_token("fBTC", "fBTC", "USDG").unwrap();

    let long_token_amount = 1_000;

    deployment
        .mint_or_transfer_to_user("fBTC", Deployment::DEFAULT_USER, 3 * long_token_amount + 14)
        .await?;

    // Create and then cancel.
    let (rpc, deposit) = user
        .create_glv_deposit(store, glv_token, market_token)
        .long_token_deposit(long_token_amount, None, None)
        .build_with_address()
        .await?;
    let signature = rpc.send_without_preflight().await?;
    tracing::info!(%signature, %deposit, "created a glv deposit");

    let signature = user
        .close_glv_deposit(&deposit)
        .build()
        .await?
        .send_without_preflight()
        .await?;
    tracing::info!(%signature, %deposit, "cancelled a glv deposit");

    // Create and then execute.
    let (rpc, deposit) = user
        .create_glv_deposit(store, glv_token, market_token)
        .long_token_deposit(long_token_amount, None, None)
        .build_with_address()
        .await?;
    let signature = rpc.send_without_preflight().await?;
    tracing::info!(%signature, %deposit, "created a glv deposit again");

    let mut execute = keeper.execute_glv_deposit(oracle, &deposit, false);
    deployment
        .execute_with_pyth(
            execute
                .add_alt(deployment.common_alt().clone())
                .add_alt(deployment.market_alt().clone()),
            None,
            false,
            true,
        )
        .instrument(tracing::info_span!("executing glv deposit", glv_deposit=%deposit))
        .await?;

    // Deposit with another market token.
    let (rpc, deposit) = user
        .create_glv_deposit(store, glv_token, market_token_2)
        .long_token_deposit(long_token_amount, None, None)
        .build_with_address()
        .await?;
    let signature = rpc.send_without_preflight().await?;
    tracing::info!(%signature, %deposit, "created a glv deposit");

    let mut execute = keeper.execute_glv_deposit(oracle, &deposit, false);
    deployment
        .execute_with_pyth(
            execute
                .add_alt(deployment.common_alt().clone())
                .add_alt(deployment.market_alt().clone()),
            None,
            false,
            true,
        )
        .instrument(tracing::info_span!("executing glv deposit", glv_deposit=%deposit))
        .await?;

    // Deposit again.
    let (rpc, deposit) = user
        .create_glv_deposit(store, glv_token, market_token)
        .long_token_deposit(long_token_amount, None, None)
        .build_with_address()
        .await?;
    let signature = rpc.send_without_preflight().await?;
    tracing::info!(%signature, %deposit, "created a glv deposit");

    // Update max value.
    let signature = keeper
        .update_glv_market_config(store, glv_token, market_token, None, Some(1))
        .send_without_preflight()
        .await?;
    tracing::info!(%signature, %market_token, "updated market config in the GLV");

    let mut execute = keeper.execute_glv_deposit(oracle, &deposit, false);
    let err = deployment
        .execute_with_pyth(
            execute
                .add_alt(deployment.common_alt().clone())
                .add_alt(deployment.market_alt().clone()),
            None,
            false,
            false,
        )
        .instrument(tracing::info_span!("executing glv deposit", glv_deposit=%deposit))
        .await
        .expect_err("should throw error for exceeding max value");
    assert_eq!(
        err.anchor_error_code(),
        Some(CoreError::ExceedMaxGlvMarketTokenBalanceValue.into())
    );

    // Restore the max value.
    let signature = keeper
        .update_glv_market_config(store, glv_token, market_token, None, Some(0))
        .send_without_preflight()
        .await?;
    tracing::info!(%signature, %market_token, "restored market config in the GLV");

    Ok(())
}

#[tokio::test]
async fn glv_withdrawal() -> eyre::Result<()> {
    let deployment = current_deployment().await?;
    let _guard = deployment.use_accounts().await?;
    let span = tracing::info_span!("glv_withdrawal");
    let _enter = span.enter();

    let user = deployment.user_client(Deployment::DEFAULT_USER)?;
    let keeper = deployment.user_client(Deployment::DEFAULT_KEEPER)?;

    let store = &deployment.store;
    let oracle = &deployment.oracle();
    let glv_token = &deployment.glv_token;
    let market_token = deployment.market_token("fBTC", "fBTC", "USDG").unwrap();

    let short_token_amount = 1_000 * 100_000_000;

    deployment
        .mint_or_transfer_to_user(
            "USDG",
            Deployment::DEFAULT_USER,
            3 * short_token_amount + 17,
        )
        .await?;

    // GLV deposit.
    let (rpc, deposit) = user
        .create_glv_deposit(store, glv_token, market_token)
        .short_token_deposit(short_token_amount, None, None)
        .build_with_address()
        .await?;
    let signature = rpc.send_without_preflight().await?;
    tracing::info!(%signature, %deposit, "created a glv deposit");

    let mut execute = keeper.execute_glv_deposit(oracle, &deposit, false);
    deployment
        .execute_with_pyth(
            execute
                .add_alt(deployment.common_alt().clone())
                .add_alt(deployment.market_alt().clone()),
            None,
            false,
            true,
        )
        .instrument(tracing::info_span!("executing glv deposit", glv_deposit=%deposit))
        .await?;

    let glv_amount = 500 * 1_000_000_000;

    // Create and cancel a GLV withdrawal.
    let (rpc, withdrawal) = user
        .create_glv_withdrawal(store, glv_token, market_token, glv_amount)
        .build_with_address()
        .await?;
    let signature = rpc.send_without_preflight().await?;
    tracing::info!(%signature, %withdrawal, "created a glv withdrawal");

    let signature = user
        .close_glv_withdrawal(&withdrawal)
        .build()
        .await?
        .send_without_preflight()
        .await?;
    tracing::info!(%signature, %withdrawal, "cancelled the glv withdrawal");

    // Create and execute a GLV withdrawal.
    let (rpc, withdrawal) = user
        .create_glv_withdrawal(store, glv_token, market_token, glv_amount)
        .build_with_address()
        .await?;
    let signature = rpc.send_without_preflight().await?;
    tracing::info!(%signature, %withdrawal, "created a glv withdrawal again");

    let mut execute = keeper.execute_glv_withdrawal(oracle, &withdrawal, false);
    deployment
        .execute_with_pyth(
            execute
                .add_alt(deployment.common_alt().clone())
                .add_alt(deployment.market_alt().clone()),
            None,
            false,
            true,
        )
        .instrument(tracing::info_span!("executing glv withdrawal", glv_withdrawal=%withdrawal))
        .await?;

    Ok(())
}

#[tokio::test]
async fn glv_shift() -> eyre::Result<()> {
    let deployment = current_deployment().await?;
    let _guard = deployment.use_accounts().await?;
    let span = tracing::info_span!("glv_shift");
    let _enter = span.enter();

    let user = deployment.user_client(Deployment::DEFAULT_USER)?;
    let keeper = deployment.user_client(Deployment::DEFAULT_KEEPER)?;

    let store = &deployment.store;
    let oracle = &deployment.oracle();
    let glv_token = &deployment.glv_token;
    let to_market_token = deployment.market_token("fBTC", "fBTC", "USDG").unwrap();
    let market_token = deployment.market_token("SOL", "fBTC", "USDG").unwrap();

    let long_token_amount = 1_000;
    let short_token_amount = 1_000 * 100_000_000;

    deployment
        .mint_or_transfer_to_user("fBTC", Deployment::DEFAULT_USER, 3 * long_token_amount + 37)
        .await?;
    deployment
        .mint_or_transfer_to_user(
            "USDG",
            Deployment::DEFAULT_USER,
            3 * short_token_amount + 37,
        )
        .await?;

    // GLV deposit.
    let (rpc, deposit) = user
        .create_glv_deposit(store, glv_token, market_token)
        .long_token_deposit(long_token_amount, None, None)
        .short_token_deposit(short_token_amount, None, None)
        .build_with_address()
        .await?;
    let signature = rpc.send_without_preflight().await?;
    tracing::info!(%signature, %deposit, "created a glv deposit");

    let mut execute = keeper.execute_glv_deposit(oracle, &deposit, false);
    deployment
        .execute_with_pyth(
            execute
                .add_alt(deployment.common_alt().clone())
                .add_alt(deployment.market_alt().clone()),
            None,
            false,
            true,
        )
        .instrument(tracing::info_span!("executing glv deposit", glv_deposit=%deposit))
        .await?;

    let shift_amount = 500 * 1_000_000_000;

    // Create and cancel a GLV shift.
    let (rpc, shift) = keeper
        .create_glv_shift(
            store,
            glv_token,
            market_token,
            to_market_token,
            shift_amount,
        )
        .build_with_address()?;
    let signature = rpc.send_without_preflight().await?;
    tracing::info!(%signature, %shift, "created a glv shift");

    let signature = keeper
        .close_glv_shift(&shift)
        .build()
        .await?
        .send_without_preflight()
        .await?;
    tracing::info!(%signature, %shift, "cancelled the glv shift");

    // Create and execute a GLV shift.
    let (rpc, shift) = keeper
        .create_glv_shift(
            store,
            glv_token,
            market_token,
            to_market_token,
            shift_amount,
        )
        .build_with_address()?;
    let signature = rpc.send_without_preflight().await?;
    tracing::info!(%signature, %shift, "created a glv shift again");

    let mut execute = keeper.execute_glv_shift(oracle, &shift, false);
    deployment
        .execute_with_pyth(
            execute
                .add_alt(deployment.common_alt().clone())
                .add_alt(deployment.market_alt().clone()),
            None,
            true,
            true,
        )
        .instrument(tracing::info_span!("executing glv shift", glv_shift=%shift))
        .await?;

    let (rpc, _shift) = keeper
        .create_glv_shift(
            store,
            glv_token,
            market_token,
            to_market_token,
            shift_amount,
        )
        .build_with_address()?;
    let err = rpc.send().await.expect_err("should throw an error");
    assert_eq!(
        gmsol_sdk::Error::from(err).anchor_error_code(),
        Some(CoreError::GlvShiftIntervalNotYetPassed.into())
    );

    Ok(())
}

#[tokio::test]
async fn get_glv_token_value() -> eyre::Result<()> {
    let deployment = current_deployment().await?;
    let _guard = deployment.use_accounts().await?;
    let span = tracing::info_span!("get_glv_token_value");
    let _enter = span.enter();

    let user = deployment.user_client(Deployment::DEFAULT_USER)?;
    let keeper = deployment.user_client(Deployment::DEFAULT_KEEPER)?;

    let store = &deployment.store;
    let oracle = &deployment.oracle();
    let glv_token = &deployment.glv_token;
    let market_token = deployment.market_token("fBTC", "fBTC", "USDG").unwrap();

    let short_token_amount = 1_000 * 100_000_000;

    deployment
        .mint_or_transfer_to_user(
            "USDG",
            Deployment::DEFAULT_USER,
            3 * short_token_amount + 19,
        )
        .await?;

    // GLV deposit.
    let (rpc, deposit) = user
        .create_glv_deposit(store, glv_token, market_token)
        .short_token_deposit(short_token_amount, None, None)
        .build_with_address()
        .await?;
    let signature = rpc.send_without_preflight().await?;
    tracing::info!(%signature, %deposit, "created a glv deposit");

    // GLV deposit.
    let mut execute = keeper.execute_glv_deposit(oracle, &deposit, false);
    deployment
        .execute_with_pyth(
            execute
                .add_alt(deployment.common_alt().clone())
                .add_alt(deployment.market_alt().clone()),
            None,
            false,
            true,
        )
        .instrument(tracing::info_span!("executing glv deposit", glv_deposit=%deposit))
        .await?;

    let glv_amount = 500 * 1_000_000_000;

    let mut builder = keeper.get_glv_token_value(store, oracle, glv_token, glv_amount);
    deployment
        .execute_with_pyth(&mut builder, None, true, true)
        .instrument(tracing::info_span!("get GLV token value", %glv_token, %glv_amount))
        .await?;

    let mut builder = user.get_glv_token_value(store, oracle, glv_token, glv_amount);
    let err = deployment
        .execute_with_pyth(&mut builder, None, false, false)
        .await
        .expect_err(
            "should throw error when the authority of the oracle buffer account is not signed",
        );
    assert_eq!(
        err.anchor_error_code(),
        Some(anchor_lang::error::ErrorCode::ConstraintHasOne.into())
    );

    Ok(())
}

#[tokio::test]
async fn glv_account_limit_follows_validator_configuration() -> eyre::Result<()> {
    let deployment = current_deployment().await?;
    let _guard = deployment.use_accounts().await?;
    let span = tracing::info_span!("glv_account_limit_follows_validator_configuration");
    let _enter = span.enter();

    let fixture = create_account_limit_fixture(deployment).await?;
    assert_eq!(fixture.market_tokens.len(), ACCOUNT_LIMIT_MARKETS);

    let user = deployment.user_client(Deployment::DEFAULT_USER)?;
    let keeper = deployment.user_client(Deployment::DEFAULT_KEEPER)?;
    let store = &deployment.store;
    let oracle = &deployment.oracle();
    let current_market = fixture.market_tokens[0];
    let path = fixture.market_tokens[1..=ACCOUNT_LIMIT_PATH_MARKETS].to_vec();
    let long_token_amount = 10_000_000;
    let short_token_amount = 10_000_000;

    tracing::info!("========== PHASE 1: seed liquidity for the selected GLV market ==========");
    deployment
        .mint_or_transfer_to_user("fBTC", Deployment::DEFAULT_USER, 2 * long_token_amount)
        .await?;
    deployment
        .mint_or_transfer_to_user("USDG", Deployment::DEFAULT_USER, 2 * short_token_amount)
        .await?;

    let (create_market_deposit, market_deposit) = user
        .create_deposit(store, &current_market)
        .long_token(long_token_amount, None, None)
        .short_token(short_token_amount, None, None)
        .build_with_address()
        .await?;
    create_market_deposit.send_without_preflight().await?;
    for attempt in 0..3 {
        let mut execute_market_deposit =
            keeper.execute_deposit(store, oracle, &market_deposit, false);
        match deployment
            .execute_with_pyth(&mut execute_market_deposit, None, false, true)
            .await
        {
            Ok(()) => break,
            Err(error)
                if attempt < 2
                    && error.anchor_error_code()
                        == Some(CoreError::OracleTimestampsAreSmallerThanRequired.into()) =>
            {
                tokio::time::sleep(std::time::Duration::from_secs(2)).await;
            }
            Err(error) => return Err(error.into()),
        }
    }

    tracing::info!(
        "========== PHASE 2: baseline — 12 markets, no swap path, close merged =========="
    );
    let market_token_amount = 1_000;
    let (create_baseline_deposit, baseline_deposit) = user
        .create_glv_deposit(store, &fixture.glv_token, &current_market)
        .market_token_deposit(market_token_amount, None)
        .build_with_address()
        .await?;
    create_baseline_deposit.send_without_preflight().await?;

    let mut baseline_execute = keeper.execute_glv_deposit(oracle, &baseline_deposit, false);
    baseline_execute
        .add_alt(deployment.common_alt().clone())
        .add_alt(deployment.market_alt().clone())
        .add_alt(fixture.alt.clone());
    let baseline_deposit_metrics =
        execute_with_metrics(deployment, &mut baseline_execute, BASELINE_DEPOSIT, true).await?;
    assert_eq!(
        baseline_deposit_metrics.outcome,
        TransactionOutcome::Executed
    );
    assert!(baseline_deposit_metrics.metrics.serialized_size <= PACKET_DATA_SIZE);

    let baseline_withdrawal_amount = 100;
    let (create_baseline_withdrawal, baseline_withdrawal) = user
        .create_glv_withdrawal(
            store,
            &fixture.glv_token,
            &current_market,
            baseline_withdrawal_amount,
        )
        .build_with_address()
        .await?;
    create_baseline_withdrawal.send_without_preflight().await?;
    let mut baseline_withdrawal_execute =
        keeper.execute_glv_withdrawal(oracle, &baseline_withdrawal, false);
    baseline_withdrawal_execute
        .add_alt(deployment.common_alt().clone())
        .add_alt(deployment.market_alt().clone())
        .add_alt(fixture.alt.clone());
    let baseline_withdrawal_metrics = execute_with_metrics(
        deployment,
        &mut baseline_withdrawal_execute,
        BASELINE_WITHDRAWAL,
        true,
    )
    .await?;
    assert_eq!(
        baseline_withdrawal_metrics.outcome,
        TransactionOutcome::Executed
    );
    assert!(baseline_withdrawal_metrics.metrics.serialized_size <= PACKET_DATA_SIZE);

    tracing::info!(
        "========== PHASE 3: route-heavy deposit — inactive configured markets omitted =========="
    );
    let (create_heavy_deposit, heavy_deposit) = user
        .create_glv_deposit(store, &fixture.glv_token, &current_market)
        .market_token_deposit(market_token_amount, None)
        .long_token_swap_path(path.clone())
        .build_with_address()
        .await?;
    create_heavy_deposit.send_without_preflight().await?;

    let mut heavy_deposit_with_close = keeper.execute_glv_deposit(oracle, &heavy_deposit, false);
    heavy_deposit_with_close
        .add_alt(deployment.common_alt().clone())
        .add_alt(deployment.market_alt().clone())
        .add_alt(fixture.alt.clone());
    let heavy_deposit = execute_with_metrics(
        deployment,
        &mut heavy_deposit_with_close,
        HEAVY_DEPOSIT,
        true,
    )
    .await?;
    assert_eq!(path.len(), HEAVY_DEPOSIT.swap_path_markets);
    assert_eq!(
        heavy_deposit.metrics.unique_accounts,
        HEAVY_DEPOSIT_ACCOUNTS
    );
    assert!(heavy_deposit.metrics.serialized_size <= PACKET_DATA_SIZE);

    tracing::info!(
        "========== PHASE 4: route-heavy withdrawal — inactive configured markets omitted =========="
    );
    let wsol = deployment.token("WSOL").expect("WSOL must exist").address;
    let usdg_to_wsol_market = *deployment
        .market_token("SOL", "WSOL", "USDG")
        .expect("USDG/WSOL market must exist");
    let mut withdrawal_path = path[..3].to_vec();
    withdrawal_path.push(usdg_to_wsol_market);
    let withdrawal_receiver = deployment.user(Deployment::USER_1)?;
    let (create_heavy_withdrawal, heavy_withdrawal) = user
        .create_glv_withdrawal(
            store,
            &fixture.glv_token,
            &current_market,
            baseline_withdrawal_amount,
        )
        .final_long_token(Some(&wsol), 0, withdrawal_path)
        .receiver(Some(withdrawal_receiver))
        .build_with_address()
        .await?;
    create_heavy_withdrawal.send_without_preflight().await?;

    let mut heavy_withdrawal_with_close =
        keeper.execute_glv_withdrawal(oracle, &heavy_withdrawal, false);
    heavy_withdrawal_with_close
        .add_alt(deployment.common_alt().clone())
        .add_alt(deployment.market_alt().clone())
        .add_alt(fixture.alt.clone());
    let heavy_withdrawal = execute_with_metrics(
        deployment,
        &mut heavy_withdrawal_with_close,
        HEAVY_WITHDRAWAL,
        true,
    )
    .await?;
    assert_eq!(
        heavy_withdrawal.metrics.unique_accounts,
        HEAVY_WITHDRAWAL_ACCOUNTS
    );
    assert!(heavy_withdrawal.metrics.serialized_size <= PACKET_DATA_SIZE);

    tracing::info!(
        baseline_deposit_accounts = baseline_deposit_metrics.metrics.unique_accounts,
        baseline_withdrawal_accounts = baseline_withdrawal_metrics.metrics.unique_accounts,
        route_heavy_deposit_accounts = heavy_deposit.metrics.unique_accounts,
        route_heavy_deposit_outcome = ?heavy_deposit.outcome,
        route_heavy_withdrawal_accounts = heavy_withdrawal.metrics.unique_accounts,
        route_heavy_withdrawal_outcome = ?heavy_withdrawal.outcome,
        "========== PROOF COMPLETE: identical transaction cases were submitted to the validator =========="
    );

    Ok(())
}
