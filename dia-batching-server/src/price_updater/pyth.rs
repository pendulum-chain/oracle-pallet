use super::chain::{self, NonceManager};
use log::{error, info, warn, debug};
use reqwest::Url;
use serde::Deserialize;
use std::error::Error;
use std::sync::Arc;

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





// ── Pyth price updater ────────────────────────────────────────────────────────

pub struct PythPriceUpdater {
	update_interval: std::time::Duration,
	last_update: Option<std::time::Instant>,
}

impl PythPriceUpdater {
	pub fn new(update_interval: std::time::Duration) -> Self {
		Self { update_interval, last_update: None }
	}

	pub async fn run_update_pyth_prices(
		&mut self,
		nonce_manager: Arc<NonceManager>,
	) -> Result<chain::PriceData, Box<dyn Error + Send + Sync + 'static>> {
		let should_update_contract = match self.last_update {
			None => true,
			Some(t) => t.elapsed() >= self.update_interval,
		};

		let api_url = format!(
			"https://hermes.pyth.network/v2/updates/price/latest?ids%5B%5D={}&ids%5B%5D={}",
			USDC_PRICE_FEED_ID, EURC_PRICE_FEED_ID
		);

		debug!("Fetching Pyth prices from Hermes API...");
		let response = reqwest::get(&api_url).await?;
		if !response.status().is_success() {
			return Err(format!("Hermes API request failed: {}", response.status()).into());
		}

		let data: HermesResponse = response.json().await?;

		let mut usdc_price = None;
		let mut eurc_price = None;
		for entry in &data.parsed {
			let price_val = entry
				.price
				.price
				.parse::<f64>()
				.map_err(|e| format!("Failed to parse price: {}", e))?;
			let actual_price = price_val * 10f64.powi(entry.price.expo);
			if entry.id == USDC_PRICE_FEED_ID {
				usdc_price = Some(actual_price);
			} else if entry.id == EURC_PRICE_FEED_ID {
				eurc_price = Some(actual_price);
			}
		}
		let usdc = usdc_price.ok_or("USDC price not found")?;
		let eurc = eurc_price.ok_or("EURC price not found")?;

		if should_update_contract {
			let update_data: Vec<String> =
				data.binary.data.iter().map(|hex| format!("0x{}", hex)).collect();
			if let Err(e) =
				chain::update_pyth_contract_prices(&update_data, nonce_manager.clone()).await
			{
				error!("Failed to update Pyth contract prices: {:?}", e);
			} else {
				info!("Pyth prices updated on-chain ✓");
				self.last_update = Some(std::time::Instant::now());
			}
		}

		Ok(chain::PriceData { usdc, eurc })
	}
}


