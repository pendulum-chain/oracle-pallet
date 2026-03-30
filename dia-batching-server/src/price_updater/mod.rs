pub mod chain;
pub mod pyth;
pub use chain::PriceData;
pub use pyth::PythPriceUpdater;

use crate::api::PriceApi;
use std::sync::Arc;
use crate::storage::CoinInfoStorage;
use crate::types::{CoinInfo, Quotation};
use crate::AssetSpecifier;
use log::{error, info, warn, debug};
use rust_decimal::prelude::ToPrimitive;
use rust_decimal::Decimal;
use std::collections::HashSet;
use std::fmt::{Display, Formatter};
use std::{error::Error};

const BIPS_DIVISOR: u64 = 10000;

// ── Public entry point ────────────────────────────────────────────────────────

pub async fn run_update_prices_loop<T>(
	storage: Arc<CoinInfoStorage>,
	supported_currencies: HashSet<AssetSpecifier>,
	update_interval: std::time::Duration,
	pyth_update_interval: std::time::Duration,
	divergence_threshold_bp: u64,
	api: T,
) -> Result<(), Box<dyn Error + Send + Sync + 'static>>
where
	T: PriceApi + Send + Sync + 'static,
{
	let mut pyth_updater = PythPriceUpdater::new(pyth_update_interval);

	// Initialize nonce manager
	let nonce_manager = chain::initialize_nonce_manager().await?;
	info!("Initialized nonce manager");

	loop {
		let start = tokio::time::Instant::now();
		let coins = Arc::clone(&storage);
		update_prices(coins, &supported_currencies, &api, &mut pyth_updater, &nonce_manager, divergence_threshold_bp).await;
		let elapsed = start.elapsed();
		let target_duration = update_interval;
		if elapsed < target_duration {
			let sleep_duration = target_duration - elapsed;
			tokio::time::sleep(sleep_duration).await;
		}
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

pub(crate) async fn update_prices<T>(
	coins: Arc<CoinInfoStorage>,
	supported_currencies: &HashSet<AssetSpecifier>,
	api: &T,
	pyth_updater: &mut PythPriceUpdater,
	nonce_manager: &Arc<chain::NonceManager>,
	divergence_threshold_bp: u64,
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

	let dark_oracle_fut = chain::update_dark_oracle_contract_prices(&currencies, nonce_manager.clone());
	let pyth_fut = pyth_updater.run_update_pyth_prices(nonce_manager.clone());

	let (dark_oracle_result, pyth_result) = tokio::join!(dark_oracle_fut, pyth_fut);

	match &dark_oracle_result {
		Ok(prices) => info!("DarkOracle updated with prices: USDC={}, EURC={}", prices.usdc, prices.eurc),
		Err(e) => error!("Failed to update DarkOracle contract prices: {:?}", e),
	}
	match &pyth_result {
		Ok(prices) => info!("Pyth prices: USDC={}, EURC={}", prices.usdc, prices.eurc),
		Err(e) => error!("Failed to fetch/update Pyth prices: {:?}", e),
	}

	// Price divergence validation. Mirrors `_validatePrice` in SafePriceProvider.sol (DarkOracle contract)
	if let (Ok(dark_prices), Ok(pyth_prices)) = (&dark_oracle_result, &pyth_result) {

		// Validate EURC
		let fallback_price = pyth_prices.eurc;
		let price = dark_prices.eurc;
		let absolute_divergence = if fallback_price > price { fallback_price - price } else { price - fallback_price };
		let bp_divergence = (absolute_divergence * BIPS_DIVISOR as f64) / fallback_price;
		debug!("EURC price divergence: {:.2} bp (DarkOracle: {}, Pyth: {})", bp_divergence, price, fallback_price);
		if bp_divergence > divergence_threshold_bp as f64 {
			error!("EURC price divergence too high: {:.2} bp > {} bp (prices: DarkOracle: {}, Pyth: {})", bp_divergence, divergence_threshold_bp, price, fallback_price);
		}
	}
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
#[path = "../price_updater_tests.rs"]
mod tests;
