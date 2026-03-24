use crate::api::PriceApi;
use crate::storage::CoinInfoStorage;
use crate::types::{CoinInfo, Quotation};
use crate::AssetSpecifier;
use alloy::{
    primitives::{Uint, Address, U256},
    providers::ProviderBuilder,
	network::EthereumWallet,
    signers::local::PrivateKeySigner,
    sol,
};
use reqwest::Url;
use log::{error, info, warn};
use rust_decimal::prelude::ToPrimitive;
use rust_decimal::Decimal;
use std::collections::HashSet;
use std::convert::TryFrom;
use std::fmt::{Display, Formatter};
use std::str::FromStr;
use std::sync::Arc;
use std::{error::Error};

type U48 = Uint<48, 1>;
type U56 = Uint<56, 1>;

sol! {
    #[sol(rpc)]
    contract DarkOracle {
        function updatePriceFeeds(uint48[5] _prices, uint56 _timestamp) external returns (bool success_);
    }
}

pub async fn run_update_prices_loop<T>(
	storage: Arc<CoinInfoStorage>,
	supported_currencies: HashSet<AssetSpecifier>,
	update_interval: std::time::Duration,
	api: T,
) -> Result<(), Box<dyn Error + Send + Sync + 'static>>
where
	T: PriceApi + Send + Sync + 'static,
{
	let coins = Arc::clone(&storage);
	let coins = Arc::clone(&coins);

	update_prices(coins, &supported_currencies, &api).await;

	Ok(())
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

async fn update_prices<T>(
	coins: Arc<CoinInfoStorage>,
	supported_currencies: &HashSet<AssetSpecifier>,
	api: &T,
) where
	T: PriceApi + Send + Sync + 'static,
{
	let mut currencies = vec![];

	let supported_currencies = supported_currencies.iter().collect::<Vec<_>>();

	api.get_quotations(supported_currencies)
		.await
		.into_iter()
		.for_each(|quotation| match convert_to_coin_info(quotation) {
			Ok(coin_info) => currencies.push(coin_info),
			Err(e) => error!("Error converting to CoinInfo: {:#?}", e),
		});

	coins.replace_currencies_by_symbols(currencies.clone());
	info!("Currencies Updated");

	// Update contract prices
	if let Err(e) = update_contract_prices(&currencies).await {
		error!("Failed to update contract prices: {:?}", e);
	}
}

async fn update_contract_prices(currencies: &Vec<CoinInfo>) -> Result<(), Box<dyn Error + Send + Sync + 'static>> {
	warn!("Starting contract price update...");
	let private_key_str = std::env::var("PRIVATE_KEY").map_err(|_| "PRIVATE_KEY not set")?;
	let contract_address = std::env::var("CONTRACT_ADDRESS").map_err(|_| "CONTRACT_ADDRESS not set")?;
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

	let symbol_to_price: std::collections::HashMap<&str, u128> = currencies.iter().map(|c| (c.symbol.as_str(), c.price)).collect();

	let mut prices: [u64; 5] = [0; 5];

	// ETH index 0
	if let Some(eth_price) = symbol_to_price.get("ETH") {
		prices[0] = u64::try_from(*eth_price)?;
	}

	// BTC index 1
	if let Some(btc_price) = symbol_to_price.get("BTC") {
		prices[1] = u64::try_from(*btc_price)?;
	}

	// USDC index 2
	if let Some(usdc_price) = symbol_to_price.get("USDC") {
		prices[2] = u64::try_from(*usdc_price)?;
	}

	// BRL index 3
	if let Some(brl_price) = symbol_to_price.get("BRL") {
		prices[3] = u64::try_from(*brl_price)?;
	}

	// EURC index 4
	if let Some(eurc_price) = symbol_to_price.get("EURC") {
		prices[4] = u64::try_from(*eurc_price)?;
	}

	

	let timestamp = u64::try_from(std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH)?.as_millis())?;

	// log prices
	info!("Updating contract prices: {:?}", prices);
	info!("Timestamp: {:?}", timestamp);
	
	// Set explicit gas limit to maximum (30M) for isolation testing
	let call = oracle.updatePriceFeeds(prices, timestamp).gas(10_000_000);
	warn!("Sending transaction with gas limit: 10,000,000");
	let tx = call.send().await?;
	warn!("Transaction sent, waiting for receipt...");
	let receipt = tx.get_receipt().await?;


	if receipt.status() {
		info!("Contract prices updated successfully - Transaction succeeded");
	} else {
		warn!("Contract prices update failed - Transaction reverted");
		return Err("Transaction reverted".into());
	}

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

		update_prices(coins, &all_currencies, &mock_api).await;

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

		update_prices(coins, &all_currencies, &mock_api).await;

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

		update_prices(coins, &all_currencies, &mock_api).await;

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
		update_prices(coins, &all_currencies, &mock_api).await;

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
		update_prices(coins, &all_currencies, &mock_api).await;

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
		update_prices(coins, &all_currencies, &mock_api).await;

		let c = storage.get_currencies_by_blockchains_and_symbols(vec![]);

		assert_eq!(0, c.len());
	}

	#[tokio::test]
	async fn test_update_prices_get_integers() {
		let mock_api = MockDia::new();
		let storage = Arc::new(CoinInfoStorage::default());
		let coins = Arc::clone(&storage);
		let all_currencies = HashSet::default();

		update_prices(coins, &all_currencies, &mock_api).await;

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

		update_prices(coins, &all_currencies, &mock_api).await;

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
