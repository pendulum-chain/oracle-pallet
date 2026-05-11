use rust_decimal::Decimal;
use serde::Deserialize;
use std::collections::HashMap;

use crate::api::error::FastForexError;
use crate::api::Quotation;
use crate::args::FastForexConfig;
use crate::AssetSpecifier;

#[derive(Clone)]
pub struct FastForexPriceApi {
	client: FastForexClient,
}

impl FastForexPriceApi {
	pub fn new_from_config(config: FastForexConfig) -> Self {
		let api_key = config.ff_api_key.expect("Please provide a FastForex API key");

		Self::new(config.ff_host_url, api_key)
	}

	pub fn new(host_url: String, api_key: String) -> Self {
		let client = FastForexClient::new(host_url, api_key);

		Self { client }
	}

	pub async fn get_prices(
		&self,
		assets: Vec<&AssetSpecifier>,
	) -> Result<Vec<Quotation>, FastForexError> {
		// Map used for the reverse lookup of ticker to asset specifier
		let mut ticker_to_asset_map: HashMap<String, AssetSpecifier> = HashMap::new();

		let from_currency_ticker_names = assets
			.into_iter()
			.filter_map(|asset| match Self::convert_to_pair(asset) {
				Some(pair) => {
					let ticker = pair.clone();
					// Insert the asset into the map
					ticker_to_asset_map.insert(ticker.clone(), asset.clone());
					Some(ticker)
				},
				None => {
					log::warn!("Unsupported FastForex asset: {:?}", asset);
					None
				},
			})
			.collect::<Vec<_>>();

		let pairs = from_currency_ticker_names.clone();

		if pairs.is_empty() {
			return Ok(Vec::new());
		}

		let response = self.client.get_quotes(&pairs).await?;

		let mut prices = Vec::new();

		// Add extra handling for USD if it was requested as it will not be in the quotes
		if from_currency_ticker_names.contains(&"USDUSD".to_string()) {
			let quotation = Quotation {
				symbol: "USD-USD".to_string(),
				name: "USD-USD".to_string(),
				blockchain: Some("FIAT".to_string()),
				price: Decimal::from(1),
				supply: Decimal::ZERO,
				time: chrono::Utc::now().timestamp().unsigned_abs(),
			};
			prices.push(quotation);
		}

		for ticker_name in from_currency_ticker_names {
			if ticker_name == "USDUSD" {
				continue; // already handled
			}
			if let Some(asset) = ticker_to_asset_map.get(ticker_name.as_str()) {
				if let Some(quote) = response.quotes.get(ticker_name.as_str()) {
					let mid_price = (quote.bid + quote.ask) / Decimal::from(2);
					let symbol = asset.symbol.clone();

					let price = mid_price;
					if price == 0.into() {
						log::warn!("Price for {} is 0. Not returning quotation", symbol);
						// We don't want to return a Quotation if the price is 0
						continue;
					}

					// We don't have supply information for fiat currencies
					let supply = Decimal::from(0);
					// We use the current time as the time
					let time = chrono::Utc::now().timestamp().unsigned_abs();

					let quotation = Quotation {
						symbol: symbol.clone(),
						name: symbol,
						blockchain: Some("FIAT".to_string()),
						price,
						supply,
						time,
					};
					prices.push(quotation);
				} else {
					log::warn!("Could not find quote for ticker: {}", ticker_name);
				}
			} else {
				log::warn!("Could not find asset for ticker: {}", ticker_name);
			}
		}

		Ok(prices)
	}

	pub fn is_supported(asset: &AssetSpecifier) -> bool {
		if asset.symbol == "EURC" {
			return true;
		}
		let (blockchain, symbol) = (asset.blockchain.as_str(), asset.symbol.as_str());
		if blockchain.to_uppercase() != "FIAT" {
			return false;
		}

		// We assume to receive a symbol of form <from>-<to> and we want to extract the <from> part
		let parts: Vec<_> = symbol.split('-').collect();
		if parts.len() != 2 {
			return false;
		}

		let _from_currency = parts.get(0).unwrap();
		let target_currency = parts.get(1).unwrap();
		if target_currency.to_uppercase() != "USD" {
			return false;
		}
		Self::convert_to_pair(asset).is_some()
	}

	fn convert_to_pair(asset: &AssetSpecifier) -> Option<String> {
		let symbol = asset.symbol.to_uppercase();
		if symbol == "EURC" {
			return Some("EURUSD".to_string());
		}
		let (blockchain, _) = (asset.blockchain.as_str(), asset.symbol.as_str());
		if blockchain.to_uppercase() != "FIAT" {
			return None;
		}

		// We assume to receive a symbol of form <from>-<to> and we want to extract the <from> part
		let parts: Vec<_> = symbol.split('-').collect();
		if parts.len() != 2 {
			return None;
		}

		let from_currency = parts.get(0)?;
		let target_currency = parts.get(1)?;
		if target_currency.to_uppercase() != "USD" {
			return None;
		}
		Some(format!("{}{}", from_currency, target_currency))
	}
}

#[derive(Deserialize, Debug)]
struct FastForexResponse {
	#[serde(alias = "prices")]
	quotes: HashMap<String, FastForexQuote>,
}

#[derive(Deserialize, Debug)]
struct FastForexQuote {
	bid: Decimal,
	ask: Decimal,
}

#[derive(Clone)]
pub struct FastForexClient {
	host: String,
	api_key: String,
}

impl FastForexClient {
	pub fn new(host: String, api_key: String) -> Self {
		FastForexClient { host, api_key }
	}

	async fn get_quotes(
		&self,
		pairs: &[String],
	) -> Result<FastForexResponse, FastForexError> {
		let client = reqwest::Client::new();
		let pairs_str = pairs.join(",");
		let url = reqwest::Url::parse_with_params(
			&format!("{}/fx/quote", self.host),
			&[("pairs", &pairs_str)],
		)
		.map_err(|e| FastForexError(format!("Failed to build URL: {}", e)))?;

		let response = client
			.get(url)
			.header("X-API-KEY", &self.api_key)
			.send()
			.await
			.map_err(|e| FastForexError(format!("Failed to send request: {}", e)))?;

		if !response.status().is_success() {
			let result = response.text().await.unwrap_or("Unknown".to_string());
			return Err(FastForexError(format!("FastForex API error: {}", result)));
		}

		let quote_response: FastForexResponse = response
			.json()
			.await
			.map_err(|e| FastForexError(format!("Could not decode FastForex response: {}", e)))?;

		Ok(quote_response)
	}
}

#[cfg(test)]
mod tests {
	use std::env;

	use super::*;

	fn read_env_variable(key: &str) -> Option<String> {
		if let None = dotenv::from_filename("../.env").ok() {
			// try looking at current directory
			dotenv::from_filename("./.env").ok();
		}

		env::var(key).ok()
	}

	fn get_fastforex_variables() -> (String, String) {
		let api_key = read_env_variable("FF_API_KEY").expect("Please provide a FastForex API key");
		let host_url =
			read_env_variable("FF_HOST_URL").unwrap_or("https://api.fastforex.io".to_string());
		(api_key, host_url)
	}

	#[tokio::test]
	async fn test_convert_eurc_to_fx_pair() {
		let asset = AssetSpecifier {
			blockchain: "Base".to_string(),
			symbol: "EURC".to_string(),
		};
		let pair = FastForexPriceApi::convert_to_pair(&asset);
		assert_eq!(pair, Some("EURUSD".to_string()));
	}

	#[tokio::test]
	async fn test_is_supported() {
		let eurc = AssetSpecifier {
			blockchain: "Base".to_string(),
			symbol: "EURC".to_string(),
		};
		assert!(FastForexPriceApi::is_supported(&eurc));

		let xyz = AssetSpecifier {
			blockchain: "Base".to_string(),
			symbol: "XYZ".to_string(),
		};
		assert!(!FastForexPriceApi::is_supported(&xyz));
	}

	#[tokio::test]
	async fn test_api_returns_eurusd_price() {
		let (api_key, host_url) = get_fastforex_variables();

		let price_api = FastForexPriceApi::new(host_url, api_key);

		let eur_asset =
			AssetSpecifier { blockchain: "FIAT".to_string(), symbol: "EUR-USD".to_string() };

		let assets = vec![&eur_asset];

		let quotations = price_api.get_prices(assets.clone()).await;
		assert!(quotations.is_ok());
		let quotations = quotations.unwrap();

		assert_eq!(quotations.len(), 1);

		let eur_price = quotations.first().expect("Should return a EUR price");
		assert_eq!(eur_price.symbol, eur_asset.symbol);
		assert_eq!(eur_price.name, eur_asset.symbol);
		assert_eq!(eur_price.blockchain, Some("FIAT".to_string()));
		assert!(eur_price.price > 0.into());
	}

	#[tokio::test]
	async fn test_all_fiat_pairs() {
		let (api_key, host_url) = get_fastforex_variables();
		let price_api = FastForexPriceApi::new(host_url.clone(), api_key.clone());

		let fiat_pairs = vec![
			"USD-USD",
			"EUR-USD",
			"BRL-USD",
			"AUD-USD",
			"NGN-USD",
			"TZS-USD",
			"PEN-USD",
			"ARS-USD",
		];

		let assets: Vec<AssetSpecifier> = fiat_pairs
			.iter()
			.map(|pair| AssetSpecifier {
				blockchain: "FIAT".to_string(),
				symbol: pair.to_string(),
			})
			.collect();

		let asset_refs: Vec<&AssetSpecifier> = assets.iter().collect();
		let result = price_api.get_prices(asset_refs).await;

		println!("Result: {:?}", result);

		match result {
			Ok(quotations) => {
				println!("Successfully retrieved {} quotations:", quotations.len());
				for q in &quotations {
					println!("  - {} (price: {})", q.symbol, q.price);
				}

				let returned_symbols: Vec<&str> =
					quotations.iter().map(|q| q.symbol.as_str()).collect();

				for pair in &fiat_pairs {
					if returned_symbols.contains(pair) {
						println!("✓ {} - SUPPORTED", pair);
					} else {
						println!("✗ {} - NOT SUPPORTED or returned", pair);
					}
				}
			},
			Err(e) => {
				println!("Error: {}", e);
			},
		}
	}

	#[tokio::test]
	async fn test_individual_fiat_pairs() {
		let (api_key, host_url) = get_fastforex_variables();

		let fiat_pairs = vec![
			"USD-USD",
			"EUR-USD",
			"BRL-USD",
			"AUD-USD",
			"NGN-USD",
			"TZS-USD",
			"PEN-USD",
			"ARS-USD",
		];

		for pair in fiat_pairs {
			let price_api = FastForexPriceApi::new(host_url.clone(), api_key.clone());
			let asset = AssetSpecifier {
				blockchain: "FIAT".to_string(),
				symbol: pair.to_string(),
			};

			let is_supported = FastForexPriceApi::is_supported(&asset);
			let ticker = FastForexPriceApi::convert_to_pair(&asset);

			let result = price_api.get_prices(vec![&asset]).await;
			let status = match &result {
				Ok(qs) if !qs.is_empty() => "✓ WORKS",
				Ok(_) => "✗ EMPTY",
				Err(_) => "✗ ERROR",
			};

			println!(
				"{} | symbol: {} | ticker: {:?} | supported: {} | result: {:?}",
				status,
				pair,
				ticker,
				is_supported,
				result.map(|qs| qs.len())
			);
		}
	}
}