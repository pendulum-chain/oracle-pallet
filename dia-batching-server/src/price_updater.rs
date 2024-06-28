use crate::api::PriceApi;
use crate::storage::{CoinInfoStorage};
use crate::types::{CoinInfo, Quotation};
use crate::AssetSpecifier;
use log::{error, info};
use rust_decimal::prelude::ToPrimitive;
use rust_decimal::Decimal;
use std::collections::HashSet;
use std::fmt::{Display, Formatter};
use std::{error::Error, sync::Arc};

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
	let _ = tokio::spawn(async move {
		loop {
			let time_elapsed = std::time::Instant::now();

			let coins = Arc::clone(&coins);

			update_prices(coins, &supported_currencies, &api).await;

			tokio::time::delay_for(update_interval.saturating_sub(time_elapsed.elapsed())).await;
		}
	});

	Ok(())
}

fn convert_to_coin_info(value: Quotation) -> Result<CoinInfo, Box<dyn Error + Sync + Send>> {
	let Quotation { name, symbol, blockchain, price, time, .. } = value;

	let price = convert_decimal_to_u128(&price)?;
	let supply = 0;

	let coin_info = CoinInfo {
		name: name.into(),
		symbol: symbol.into(),
		blockchain: blockchain.unwrap_or("FIAT".to_string()).into(),
		price,
		last_update_timestamp: time.timestamp().unsigned_abs(),
		supply,
	};

	info!("Coin Price: {:#?}", price);
	info!("Coin Info : {:#?}", coin_info);

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
		.unwrap_or_default()
		.into_iter()
		.for_each(|quotation| match convert_to_coin_info(quotation) {
			Ok(coin_info) => currencies.push(coin_info),
			Err(e) => error!("Error converting to CoinInfo: {:#?}", e),
		});

	coins.replace_currencies_by_symbols(currencies);
	info!("Currencies Updated");
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
	let fract = (input.fract() * Decimal::from(1_000_000_000_000_u128))
		.to_u128()
		.ok_or(ConvertingError::DecimalTooLarge)?;
	let trunc = (input.trunc() * Decimal::from(1_000_000_000_000_u128))
		.to_u128()
		.ok_or(ConvertingError::DecimalTooLarge)?;

	Ok(trunc.saturating_add(fract))
}

#[cfg(test)]
mod tests {
	use std::{collections::HashMap, error::Error, sync::Arc};

	use super::*;
	use crate::api::ApiError;
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
					time: Utc::now(),
					blockchain: Some("Bitcoin".into()),
				},
			);
			quotation.insert(
				AssetSpecifier { blockchain: "Ethereum".into(), symbol: "ETH".into() },
				Quotation {
					name: "ETH".into(),
					price: dec!(1.000000000000),
					symbol: "ETH".into(),
					time: Utc::now(),
					blockchain: Some("Ethereum".into()),
				},
			);
			quotation.insert(
				AssetSpecifier { blockchain: "Ethereum".into(), symbol: "USDT".into() },
				Quotation {
					name: "USDT".into(),
					price: dec!(1.000000000001),
					symbol: "USDT".into(),
					time: Utc::now(),
					blockchain: Some("Ethereum".into()),
				},
			);
			quotation.insert(
				AssetSpecifier { blockchain: "Ethereum".into(), symbol: "USDC".into() },
				Quotation {
					name: "USDC".into(),
					price: dec!(123456789.123456789012345),
					symbol: "USDC".into(),
					time: Utc::now(),
					blockchain: Some("Ethereum".into()),
				},
			);
			quotation.insert(
				AssetSpecifier { blockchain: "FIAT".into(), symbol: "MXN-USD".into() },
				Quotation {
					name: "MXNUSD=X".into(),
					price: dec!(0.053712327),
					symbol: "MXN-USD".into(),
					time: Utc::now(),
					blockchain: None,
				},
			);
			quotation.insert(
				AssetSpecifier { blockchain: "FIAT".into(), symbol: "USD-USD".into() },
				Quotation {
					symbol: "USD-USD".to_string(),
					name: "USD-X".to_string(),
					blockchain: None,
					price: Decimal::new(1, 0),
					time: Utc::now(),
				},
			);
			Self { quotation }
		}
	}

	#[async_trait]
	impl PriceApi for MockDia {
		async fn get_quotations(
			&self,
			assets: Vec<&AssetSpecifier>,
		) -> Result<Vec<Quotation>, ApiError> {
			let mut quotations = Vec::new();
			for asset in assets {
				if let Some(q) = self.quotation.get(asset) {
					quotations.push(q.clone());
				}
			}
			Ok(quotations)
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

		assert_eq!(c[1].price, 1000000000000);

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

		assert_eq!(c[1].price, 53712327000);

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

		assert_eq!(c[0].price, 1000000000000);

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

		assert_eq!(c[0].price, 1000000000000);

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

		assert_eq!(c[0].price, 1000000000000);
		assert_eq!(c[0].supply, 0);

		assert_eq!(c[1].price, 123456789123456789012);
		assert_eq!(c[1].supply, 0);

		assert_eq!(c[2].price, 1000000000001);
		assert_eq!(c[2].supply, 0);

		assert_eq!(c[0].name, "BTC");
		assert_eq!(c[1].name, "USDC");
		assert_eq!(c[2].name, "USDT");
	}
}
