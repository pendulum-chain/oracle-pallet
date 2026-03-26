use crate::api::PriceApi;
use crate::storage::CoinInfoStorage;
use crate::types::{CoinInfo, Quotation};
use crate::AssetSpecifier;
use alloy::{
	primitives::{Address, Bytes, Uint, U256},
	providers::ProviderBuilder,
	network::EthereumWallet,
	signers::local::PrivateKeySigner,
	sol,
};
use reqwest::Url;
use log::{error, info, warn};
use rust_decimal::prelude::ToPrimitive;
use rust_decimal::Decimal;
use serde::Deserialize;
use std::collections::HashSet;
use std::convert::TryFrom;
use std::fmt::{Display, Formatter};
use std::str::FromStr;
use std::sync::Arc;
use std::{error::Error};

type U48 = Uint<48, 1>;
type U56 = Uint<56, 1>;

// ── Pyth Hermes API types ─────────────────────────────────────────────────────

const USDC_PRICE_FEED_ID: &str =
	"eaa020c61cc479712813461ce153894a96a6c00b21ed0cfc2798d1f9a9e9c94a";
const EURC_PRICE_FEED_ID: &str =
	"76fa85158bf14ede77087fe3ae472f66213f6ea2f5b411cb2de472794990fa5c";

#[derive(Debug, Deserialize)]
pub struct HermesPrice {
	pub price: String,
	pub conf: String,
	pub expo: i32,
	pub publish_time: u64,
}

#[derive(Debug, Deserialize)]
pub struct HermesParsedEntry {
	pub id: String,
	pub price: HermesPrice,
	pub ema_price: HermesPrice,
}

#[derive(Debug, Deserialize)]
struct HermesBinary {
	data: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct HermesResponse {
	binary: HermesBinary,
	parsed: Vec<HermesParsedEntry>,
}

// ── Solidity contracts ────────────────────────────────────────────────────────

sol! {
	#[sol(rpc)]
	contract DarkOracle {
		function updatePriceFeeds(uint48[5] _prices, uint56 _timestamp) external returns (bool success_);
	}
}

sol! {
	#[sol(rpc)]
	contract PythAdapter {
		function getUpdateFee(bytes[] _updateData) external view returns (uint256 updateFee_);
		function updatePriceFeeds(bytes[] _priceUpdateData) external payable returns (bool success_);
	}
}


struct PythPriceUpdater {
	update_interval: std::time::Duration,
	last_update: Option<std::time::Instant>,
}

impl PythPriceUpdater {
	fn new(update_interval: std::time::Duration) -> Self {
		Self { update_interval, last_update: None }
	}

	async fn run_update_pyth_prices(
		&mut self,
	) -> Result<Vec<HermesParsedEntry>, Box<dyn Error + Send + Sync + 'static>> {
		let should_update_contract = match self.last_update {
			None => true,
			Some(t) => t.elapsed() >= self.update_interval,
		};

		let api_url = format!(
			"https://hermes.pyth.network/v2/updates/price/latest?ids%5B%5D={}&ids%5B%5D={}",
			USDC_PRICE_FEED_ID, EURC_PRICE_FEED_ID
		);

		info!("Fetching Pyth prices from Hermes API...");
		let response = reqwest::get(&api_url).await?;
		if !response.status().is_success() {
			return Err(format!("Hermes API request failed: {}", response.status()).into());
		}

		let data: HermesResponse = response.json().await?;
		info!("Pyth prices fetched: {} entries", data.parsed.len());
		for entry in &data.parsed {
			info!(
				"  id={} price={} expo={} publish_time={}",
				entry.id, entry.price.price, entry.price.expo, entry.price.publish_time
			);
		}

		if should_update_contract {
			let update_data: Vec<String> =
				data.binary.data.iter().map(|hex| format!("0x{}", hex)).collect();
			if let Err(e) = update_pyth_contract_prices(&update_data).await {
				error!("Failed to update Pyth contract prices: {:?}", e);
			} else {
				info!("Pyth prices updated on-chain ✓");
				self.last_update = Some(std::time::Instant::now());
			}
		}

		Ok(data.parsed)
	}
}

// ── Public entry point ────────────────────────────────────────────────────────

pub async fn run_update_prices_loop<T>(
	storage: Arc<CoinInfoStorage>,
	supported_currencies: HashSet<AssetSpecifier>,
	update_interval: std::time::Duration,
	pyth_update_interval: std::time::Duration,
	api: T,
) -> Result<(), Box<dyn Error + Send + Sync + 'static>>
where
	T: PriceApi + Send + Sync + 'static,
{
	let mut pyth_updater = PythPriceUpdater::new(pyth_update_interval);

	loop {
		let coins = Arc::clone(&storage);
		update_prices(coins, &supported_currencies, &api, &mut pyth_updater).await;

		tokio::time::sleep(update_interval).await;
	}
}

fn convert_to_coin_info(value: Quotation) -> Result<CoinInfo, Box<dyn Error + Sync + Send>> {
	let Quotation { name, symbol, blockchain, price, time, supply } = value;

	let price = convert_decimal_to_u128(&price)?;
	let supply = convert_decimal_to_u128(&supply)?;

	let coin_info = CoinInfo {
		name: name.into(),
		symbol: symbol.into(),
		blockchain: blockchain.unwrap_or("FIAT".to_string()).into(),
		price,
		last_update_timestamp: time,
		supply,
	};

	Ok(coin_info)
}

async fn update_pyth_contract_prices(
	update_data: &[String],
) -> Result<(), Box<dyn Error + Send + Sync + 'static>> {
	let private_key_str = std::env::var("PRIVATE_KEY").map_err(|_| "PRIVATE_KEY not set")?;
	let pyth_adapter_address = std::env::var("PYTH_ADAPTER_ADDRESS")
		.map_err(|_| "PYTH_ADAPTER_ADDRESS not set")?;
	let rpc_url = std::env::var("RPC_URL").map_err(|_| "RPC_URL not set")?;

	warn!("Connecting to Ethereum provider at {} for Pyth update", rpc_url);
	warn!("Using PythAdapter contract address: {}", pyth_adapter_address);

	let signer = PrivateKeySigner::from_str(&private_key_str)?;
	let wallet = EthereumWallet::from(signer);

	let provider = ProviderBuilder::new()
		.with_recommended_fillers()
		.wallet(wallet)
		.on_http(Url::parse(&rpc_url).expect("Invalid RPC_URL"));

	let addr = pyth_adapter_address.parse::<Address>()?;
	let pyth_adapter = PythAdapter::new(addr, &provider);

	let bytes_data: Vec<Bytes> = update_data
		.iter()
		.map(|hex_str| {
			let stripped = hex_str.strip_prefix("0x").unwrap_or(hex_str);
			Bytes::from(hex::decode(stripped).unwrap_or_default())
		})
		.collect();

	info!("Prepared price update data for Pyth contract: {:?}", bytes_data);


	// Get the required fee
	let update_fee: U256 =
		pyth_adapter.getUpdateFee(bytes_data.clone()).call().await?.updateFee_;
	info!("Pyth update fee: {} wei", update_fee);

	// Send the update transaction
	let call = pyth_adapter
		.updatePriceFeeds(bytes_data)
		.value(update_fee)
		.gas(10_000_000);
	let tx = call.send().await?;

	warn!("Pyth updatePriceFeeds tx sent");
	info!("Pyth updatePriceFeeds tx hash: {:?}", tx.tx_hash());
	// let receipt = tx.get_receipt().await?;

	// if receipt.status() {
	// 	info!("Pyth contract prices updated successfully ✓");
	// } else {
	// 	warn!("Pyth contract prices update failed – transaction reverted");
	// 	return Err("Pyth updatePriceFeeds transaction reverted".into());
	// }

	Ok(())
}

async fn update_prices<T>(
	coins: Arc<CoinInfoStorage>,
	supported_currencies: &HashSet<AssetSpecifier>,
	api: &T,
	pyth_updater: &mut PythPriceUpdater,
) where
	T: PriceApi + Send + Sync + 'static,
{
	let mut currencies = vec![];

	let supported_currencies_vec = supported_currencies.iter().collect::<Vec<_>>();

	api.get_quotations(supported_currencies_vec)
		.await
		.into_iter()
		.for_each(|quotation| match convert_to_coin_info(quotation) {
			Ok(coin_info) => currencies.push(coin_info),
			Err(e) => error!("Error converting to CoinInfo: {:#?}", e),
		});

	coins.replace_currencies_by_symbols(currencies.clone());
	info!("Currencies Updated");


	let dark_oracle_fut = update_dark_oracle_contract_prices(&currencies);
	let pyth_fut = pyth_updater.run_update_pyth_prices();

	let (dark_oracle_result, pyth_result) = tokio::join!(dark_oracle_fut, pyth_fut);

	if let Err(e) = dark_oracle_result {
		error!("Failed to update DarkOracle contract prices: {:?}", e);
	}
	if let Err(e) = pyth_result {
		error!("Failed to fetch/update Pyth prices: {:?}", e);
	}
}

async fn update_dark_oracle_contract_prices(
	currencies: &Vec<CoinInfo>,
) -> Result<(), Box<dyn Error + Send + Sync + 'static>> {
	warn!("Starting contract price update...");
	let private_key_str = std::env::var("PRIVATE_KEY").map_err(|_| "PRIVATE_KEY not set")?;
	let contract_address =
		std::env::var("CONTRACT_ADDRESS").map_err(|_| "CONTRACT_ADDRESS not set")?;
	let rpc_url = std::env::var("RPC_URL").map_err(|_| "RPC_URL not set")?;

	warn!("Connecting to Ethereum provider at {}", rpc_url);
	warn!("Using contract address: {}", contract_address);

	let signer = PrivateKeySigner::from_str(&private_key_str)?;
	let wallet_address = signer.address();
	warn!("Using wallet address (public key): {}", wallet_address);

	let wallet = EthereumWallet::from(signer);

	let provider = ProviderBuilder::new()
		.with_recommended_fillers()
		.wallet(wallet)
		.on_http(Url::parse(&rpc_url).expect("Invalid RPC_URL"));

	let addr = contract_address.parse::<Address>()?;
	let oracle = DarkOracle::new(addr, provider);

	let symbol_to_price: std::collections::HashMap<&str, u128> =
		currencies.iter().map(|c| (c.symbol.as_str(), c.price)).collect();

	let mut prices: [u64; 5] = [0; 5];

	// ETH index 0
	if let Some(eth_price) = symbol_to_price.get("ETH") {
		prices[0] = u64::try_from(*eth_price / 10_000_000_000)?;
	}

	// BTC index 1
	if let Some(btc_price) = symbol_to_price.get("BTC") {
		prices[1] = u64::try_from(*btc_price / 10_000_000_000)?;
	}

	// USDC index 2
	if let Some(usdc_price) = symbol_to_price.get("USDC") {
		prices[2] = u64::try_from(*usdc_price / 10_000_000_000)?;
	}

	// BRL index 3
	if let Some(brl_price) = symbol_to_price.get("BRL") {
		prices[3] = u64::try_from(*brl_price / 10_000_000_000)?;
	}

	// EURC index 4
	if let Some(eurc_price) = symbol_to_price.get("EURC") {
		prices[4] = u64::try_from(*eurc_price / 10_000_000_000)?;
	}

	let timestamp = u64::try_from(
		std::time::SystemTime::now()
			.duration_since(std::time::UNIX_EPOCH)?
			.as_millis(),
	)?;

	info!("Updating contract prices: {:?}", prices);
	info!("Timestamp: {:?}", timestamp);

	let call = oracle.updatePriceFeeds(prices, timestamp).gas(10_000_000);
	warn!("Sending transaction with gas limit: 10,000,000");
	let tx = call.send().await?;
	warn!("Transaction sent");
	info!("DarkOracle updatePriceFeeds tx hash: {:?}", tx.tx_hash());

	Ok(())
}

#[derive(Debug)]
pub enum ConvertingError {
	DecimalTooLarge,
}

impl Display for ConvertingError {
	fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
		match self {
			ConvertingError::DecimalTooLarge => write!(f, "Decimal given is too large"),
		}
	}
}

impl Error for ConvertingError {}

fn convert_decimal_to_u128(input: &Decimal) -> Result<u128, ConvertingError> {
	let fract = (input.fract() * Decimal::from(1_000_000_000_000_000_000_u128))
		.to_u128()
		.ok_or(ConvertingError::DecimalTooLarge)?;
	let trunc = (input.trunc() * Decimal::from(1_000_000_000_000_000_000_u128))
		.to_u128()
		.ok_or(ConvertingError::DecimalTooLarge)?;

	Ok(trunc.saturating_add(fract))
}

#[cfg(test)]
mod tests {
	use std::{collections::HashMap, sync::Arc};

	use super::*;
	use async_trait::async_trait;
	use chrono::Utc;
	use rust_decimal_macros::dec;

	struct MockDia {
		quotation: HashMap<AssetSpecifier, Quotation>,
	}

	impl MockDia {
		pub fn new() -> Self {
			let mut quotation = HashMap::new();
			quotation.insert(
				AssetSpecifier { blockchain: "Bitcoin".into(), symbol: "BTC".into() },
				Quotation {
					name: "BTC".into(),
					price: dec!(1.000000000000),
					symbol: "BTC".into(),
					time: Utc::now().timestamp().unsigned_abs(),
					blockchain: Some("Bitcoin".into()),
					supply: Decimal::from(1),
				},
			);
			quotation.insert(
				AssetSpecifier { blockchain: "Ethereum".into(), symbol: "ETH".into() },
				Quotation {
					name: "ETH".into(),
					price: dec!(1.000000000000),
					symbol: "ETH".into(),
					time: Utc::now().timestamp().unsigned_abs(),
					blockchain: Some("Ethereum".into()),
					supply: Decimal::from(1),
				},
			);
			quotation.insert(
				AssetSpecifier { blockchain: "Ethereum".into(), symbol: "USDT".into() },
				Quotation {
					name: "USDT".into(),
					price: dec!(1.000000000001),
					symbol: "USDT".into(),
					time: Utc::now().timestamp().unsigned_abs(),
					blockchain: Some("Ethereum".into()),
					supply: Decimal::from(1),
				},
			);
			quotation.insert(
				AssetSpecifier { blockchain: "Ethereum".into(), symbol: "USDC".into() },
				Quotation {
					name: "USDC".into(),
					price: dec!(123456789.123456789012345),
					symbol: "USDC".into(),
					time: Utc::now().timestamp().unsigned_abs(),
					blockchain: Some("Ethereum".into()),
					supply: Decimal::from(1),
				},
			);
			quotation.insert(
				AssetSpecifier { blockchain: "FIAT".into(), symbol: "MXN-USD".into() },
				Quotation {
					name: "MXNUSD=X".into(),
					price: dec!(0.053712327),
					symbol: "MXN-USD".into(),
					time: Utc::now().timestamp().unsigned_abs(),
					blockchain: None,
					supply: Decimal::from(1),
				},
			);
			quotation.insert(
				AssetSpecifier { blockchain: "FIAT".into(), symbol: "USD-USD".into() },
				Quotation {
					symbol: "USD-USD".to_string(),
					name: "USD-X".to_string(),
					blockchain: None,
					price: Decimal::new(1, 0),
					time: Utc::now().timestamp().unsigned_abs(),
					supply: Decimal::from(1),
				},
			);
			Self { quotation }
		}
	}

	#[async_trait]
	impl PriceApi for MockDia {
		async fn get_quotations(&self, assets: Vec<&AssetSpecifier>) -> Vec<Quotation> {
			let mut quotations = Vec::new();
			for asset in assets {
				if let Some(q) = self.quotation.get(asset) {
					quotations.push(q.clone());
				}
			}
			quotations
		}
	}

	#[tokio::test]
	async fn test_update_prices() {
		let mock_api = MockDia::new();
		let storage = Arc::new(CoinInfoStorage::default());
		let coins = Arc::clone(&storage);
		let mut all_currencies = HashSet::default();
		let supported_currencies = vec![
			AssetSpecifier { blockchain: "Bitcoin".into(), symbol: "BTC".into() },
			AssetSpecifier { blockchain: "Ethereum".into(), symbol: "ETH".into() },
			AssetSpecifier { blockchain: "Ethereum".into(), symbol: "USDT".into() },
			AssetSpecifier { blockchain: "Ethereum".into(), symbol: "USDC".into() },
		];
		for currency in supported_currencies.clone() {
			all_currencies.insert(currency);
		}

		let mut pyth_updater = PythPriceUpdater::new(std::time::Duration::from_secs(300));
		update_prices(coins, &all_currencies, &mock_api, &mut pyth_updater).await;

		let c = storage.get_currencies_by_blockchains_and_symbols(supported_currencies);

		assert_eq!(4, c.len());

		assert_eq!(c[1].price, 1000000000000000000);

		assert_eq!(c[1].name, "ETH");
	}

	#[tokio::test]
	async fn test_update_prices_with_fiat_and_crypto_asset_works() {
		let mock_api = MockDia::new();
		let storage = Arc::new(CoinInfoStorage::default());
		let coins = Arc::clone(&storage);

		let mut all_currencies = HashSet::new();
		all_currencies
			.insert(AssetSpecifier { blockchain: "Bitcoin".into(), symbol: "BTC".into() });
		all_currencies
			.insert(AssetSpecifier { blockchain: "FIAT".into(), symbol: "MXN-USD".into() });

		let mut pyth_updater = PythPriceUpdater::new(std::time::Duration::from_secs(300));
		update_prices(coins, &all_currencies, &mock_api, &mut pyth_updater).await;

		let c = storage.get_currencies_by_blockchains_and_symbols(vec![
			AssetSpecifier { blockchain: "Bitcoin".into(), symbol: "BTC".into() },
			AssetSpecifier { blockchain: "FIAT".into(), symbol: "MXN-USD".into() },
		]);

		assert_eq!(2, c.len());

		assert_eq!(c[1].price, 53712327000000000);

		assert_eq!(c[1].name, "MXNUSD=X");
	}

	#[tokio::test]
	async fn test_update_prices_with_fiat_usd_works() {
		let mock_api = MockDia::new();
		let storage = Arc::new(CoinInfoStorage::default());
		let coins = Arc::clone(&storage);

		let mut all_currencies = HashSet::new();
		all_currencies
			.insert(AssetSpecifier { blockchain: "FIAT".into(), symbol: "USD-USD".into() });

		let mut pyth_updater = PythPriceUpdater::new(std::time::Duration::from_secs(300));
		update_prices(coins, &all_currencies, &mock_api, &mut pyth_updater).await;

		let c = storage.get_currencies_by_blockchains_and_symbols(vec![AssetSpecifier {
			blockchain: "FIAT".into(),
			symbol: "USD-USD".into(),
		}]);

		assert_eq!(1, c.len());

		assert_eq!(c[0].price, 1000000000000000000);

		assert_eq!(c[0].name, "USD-X");
	}

	#[tokio::test]
	async fn test_update_prices_non_existent() {
		let mock_api = MockDia::new();
		let storage = Arc::new(CoinInfoStorage::default());
		let coins = Arc::clone(&storage);
		let all_currencies = HashSet::default();
		let mut pyth_updater = PythPriceUpdater::new(std::time::Duration::from_secs(300));
		update_prices(coins, &all_currencies, &mock_api, &mut pyth_updater).await;

		let c = storage.get_currencies_by_blockchains_and_symbols(vec![
			AssetSpecifier { blockchain: "Bitcoin".into(), symbol: "BTCCash".into() },
			AssetSpecifier { blockchain: "Ethereum".into(), symbol: "ETHCase".into() },
		]);

		assert_eq!(0, c.len());
	}

	#[tokio::test]
	async fn test_update_prices_one_available() {
		let mock_api = MockDia::new();
		let storage = Arc::new(CoinInfoStorage::default());
		let coins = Arc::clone(&storage);
		let mut all_currencies = HashSet::default();
		let supported_currencies = vec![
			AssetSpecifier { blockchain: "Bitcoin".into(), symbol: "BTC".into() },
			AssetSpecifier { blockchain: "Ethereum".into(), symbol: "ETHCase".into() },
		];
		for currency in supported_currencies.clone() {
			all_currencies.insert(currency);
		}
		let mut pyth_updater = PythPriceUpdater::new(std::time::Duration::from_secs(300));
		update_prices(coins, &all_currencies, &mock_api, &mut pyth_updater).await;

		let c = storage.get_currencies_by_blockchains_and_symbols(supported_currencies);

		assert_eq!(1, c.len());

		assert_eq!(c[0].price, 1000000000000000000);

		assert_eq!(c[0].name, "BTC");
	}

	#[tokio::test]
	async fn test_update_prices_get_nothing() {
		let mock_api = MockDia::new();
		let storage = Arc::new(CoinInfoStorage::default());
		let coins = Arc::clone(&storage);
		let all_currencies = HashSet::default();
		let mut pyth_updater = PythPriceUpdater::new(std::time::Duration::from_secs(300));
		update_prices(coins, &all_currencies, &mock_api, &mut pyth_updater).await;

		let c = storage.get_currencies_by_blockchains_and_symbols(vec![]);

		assert_eq!(0, c.len());
	}

	#[tokio::test]
	async fn test_update_prices_get_integers() {
		let mock_api = MockDia::new();
		let storage = Arc::new(CoinInfoStorage::default());
		let coins = Arc::clone(&storage);
		let all_currencies = HashSet::default();

		let mut pyth_updater = PythPriceUpdater::new(std::time::Duration::from_secs(300));
		update_prices(coins, &all_currencies, &mock_api, &mut pyth_updater).await;

		let c = storage.get_currencies_by_blockchains_and_symbols(vec![AssetSpecifier {
			blockchain: "Bitcoin".into(),
			symbol: "123".into(),
		}]);

		assert_eq!(0, c.len());
	}

	#[tokio::test]
	async fn test_convert_result() {
		let mock_api = MockDia::new();
		let storage = Arc::new(CoinInfoStorage::default());
		let coins = Arc::clone(&storage);
		let mut all_currencies = HashSet::default();
		let supported_currencies = vec![
			AssetSpecifier { blockchain: "Bitcoin".into(), symbol: "BTC".into() },
			AssetSpecifier { blockchain: "Ethereum".into(), symbol: "USDC".into() },
			AssetSpecifier { blockchain: "Ethereum".into(), symbol: "USDT".into() },
		];
		for currency in supported_currencies.clone() {
			all_currencies.insert(currency);
		}

		let mut pyth_updater = PythPriceUpdater::new(std::time::Duration::from_secs(300));
		update_prices(coins, &all_currencies, &mock_api, &mut pyth_updater).await;

		let c = storage.get_currencies_by_blockchains_and_symbols(supported_currencies);

		assert_eq!(c[0].price, 1000000000000000000);
		assert_eq!(c[0].supply, 1000000000000000000);

		assert_eq!(c[1].price, 123456789123456789012345000000);
		assert_eq!(c[1].supply, 1000000000000000000);

		assert_eq!(c[2].price, 1000000000001000000);
		assert_eq!(c[2].supply, 1000000000000000000);

		assert_eq!(c[0].name, "BTC");
		assert_eq!(c[1].name, "USDC");
		assert_eq!(c[2].name, "USDT");
	}
}
