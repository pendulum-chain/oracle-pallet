use crate::api::error::PolygonError;
use crate::api::Quotation;
use crate::args::PolygonConfig;
use crate::AssetSpecifier;
use rust_decimal::Decimal;
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

pub struct PolygonPriceApi {
	client: PolygonClient,
}

impl PolygonPriceApi {
	pub fn new_from_config(config: PolygonConfig) -> Self {
		let api_key = config.pg_api_key.expect("Please provide a Polygon API key");

		Self::new(config.pg_host_url, api_key)
	}

	pub fn new(host_url: String, api_key: String) -> Self {
		let client = PolygonClient::new(host_url, api_key);

		Self { client }
	}

	pub async fn get_prices(
		&self,
		assets: Vec<&AssetSpecifier>,
	) -> Result<Vec<Quotation>, PolygonError> {
		// Map used for the reverse lookup of ticker to asset specifier
		let mut ticker_to_asset_map: HashMap<String, AssetSpecifier> = HashMap::new();

		let from_currency_ticker_names = assets
			.into_iter()
			.filter_map(|asset| match Self::extract_source_currency(asset) {
				Some(currency) => {
					let source_currency_ticker =
						PolygonClient::currency_to_ticker(currency.as_str());
					// Insert the asset into the map
					ticker_to_asset_map.insert(source_currency_ticker.clone(), asset.clone());
					Some(source_currency_ticker)
				},
				None => {
					log::warn!("Unsupported polygon asset: {:?}", asset);
					None
				},
			})
			.collect::<Vec<_>>();

		let quotes = self.client.all_tickers(&from_currency_ticker_names).await?;

		let mut prices = Vec::new();

		// Add extra handling for USD if it was requested as it will not be in the quotes
		if from_currency_ticker_names.contains(&PolygonClient::currency_to_ticker("USD")) {
			let quotation = Quotation {
				symbol: "USD-USD".to_string(),
				name: "USD-USD".to_string(),
				blockchain: Some("FIAT".to_string()),
				price: Decimal::from(1),
				supply: Decimal::from(0),
				time: chrono::Utc::now().timestamp().unsigned_abs(),
			};
			prices.push(quotation);
		}

		for (ticker_name, ticker) in quotes {
			if let Some(asset) = ticker_to_asset_map.get(ticker_name.as_str()) {
				let symbol = asset.symbol.clone();

				let price = if ticker.last_quote.b > 0.into() {
					// If the bid price is available on the last quote, we use it
					ticker.last_quote.b
				} else {
					log::warn!("No bid price available for {symbol} in last quote. Falling back to quote from previous day.");
					// Otherwise we use the close price of the previous day
					ticker.prev_day.c
				};

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
				log::warn!("Could not find asset for ticker: {}", ticker_name);
			}
		}

		Ok(prices)
	}

	pub fn is_supported(asset: &AssetSpecifier) -> bool {
		Self::extract_source_currency(asset).is_some()
	}

	/// Extract the source currency from the asset pair.
	/// We assume that the symbol contained in the `AssetSpecifier` is of the form <from>-<to>.
	fn extract_source_currency(asset: &AssetSpecifier) -> Option<String> {
		let (blockchain, symbol) = (asset.blockchain.as_str(), asset.symbol.as_str());
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
			log::info!("Unsupported target currency: {}", target_currency);
			return None;
		}
		Some(from_currency.to_uppercase())
	}
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct ConversionPrice {
	pub converted: Decimal,
	pub from: String,
	#[serde(rename = "initialAmount")]
	pub initial_amount: i64,
	pub last: serde_json::Value,
	pub request_id: String,
	pub status: String,
	pub symbol: String,
	pub to: String,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
struct Tickers {
	pub tickers: Vec<Ticker>,
	pub status: String,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct Ticker {
	/// The exchange symbol that this item is traded under.
	#[serde(rename = "ticker")]
	pub ticker_name: String,
	/// The last updated timestamp.
	pub updated: u64,
	#[serde(rename = "lastQuote")]
	pub last_quote: LastQuote,
	#[serde(rename = "prevDay")]
	pub prev_day: PrevDayQuote,
}

/// The most recent quote for this ticker.
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct LastQuote {
	/// The ask price
	pub a: Decimal,
	/// The bid price
	pub b: Decimal,
	/// The millisecond accuracy timestamp of the quote.
	pub t: u64,
}

/// The previous day's bar for this ticker.
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct PrevDayQuote {
	/// The close price for the symbol in the given time period.
	pub c: Decimal,
	/// The highest price for the symbol in the given time period.
	pub h: Decimal,
	/// The lowest price for the symbol in the given time period.
	pub l: Decimal,
	/// The open price for the symbol in the given time period.
	pub o: Decimal,
	/// The trading volume of the symbol in the given time period.
	pub v: Decimal,
	/// The volume weighted average price.
	pub vw: Decimal,
}

/// Client to communicate with the Polygon.io API
pub struct PolygonClient {
	host: String,
	api_key: String,
}

impl PolygonClient {
	pub fn new(host: String, api_key: String) -> Self {
		PolygonClient { host, api_key }
	}

	async fn get<R: DeserializeOwned>(&self, endpoint: &str) -> Result<R, PolygonError> {
		let mut headers = reqwest::header::HeaderMap::new();
		headers.insert("accept", reqwest::header::HeaderValue::from_static("application/json"));

		let bearer = "Bearer ".to_string() + self.api_key.as_str();
		let mut api_key_header = reqwest::header::HeaderValue::from_str(bearer.as_str())
			.expect("Could not create header value");
		api_key_header.set_sensitive(true);
		headers.insert("Authorization", api_key_header);

		let client = reqwest::Client::builder()
			.default_headers(headers)
			.build()
			.map_err(|e| PolygonError(e.to_string()))?;

		let url = reqwest::Url::parse(
			format!("{host}/{ep}", host = self.host.as_str(), ep = endpoint).as_str(),
		)
		.expect("Invalid URL");

		let response = client
			.get(url)
			.send()
			.await
			.map_err(|e| PolygonError(format!("Failed to send request: {}", e.to_string())))?;

		if !response.status().is_success() {
			let result = response.text().await;
			return Err(PolygonError(format!(
				"Polygon API error: {}",
				result.unwrap_or("Unknown".to_string()).trim()
			)));
		}

		let result = response.json().await;
		result.map_err(|e| PolygonError(format!("Could not decode Polygon response: {}", e)))
	}

	#[allow(dead_code)]
	/// Get the current price of any fiat currency in USD
	/// from https://polygon.io/docs/forex/get_v1_conversion__from___to
	pub async fn price(&self, from_currency: String) -> Result<ConversionPrice, PolygonError> {
		// Currencies have to be upper-case
		let from_currency = from_currency.to_uppercase();
		// We always query for USD.
		let to_currency = "USD";
		// We always query for full precision
		let precision = "5";
		// We always query for 1 unit
		let amount = "1";

		let req = format!(
			"v1/conversion/{}/{}?amount={}&precision={}",
			from_currency, to_currency, amount, precision
		);
		self.get(&req).await
	}

	/// Get all tickers for the given from_currency_tickers
	/// from https://polygon.io/docs/forex/get_v2_snapshot_locale_global_markets_forex_tickers
	pub async fn all_tickers(
		&self,
		from_currency_tickers: &Vec<String>,
	) -> Result<HashMap<String, Ticker>, PolygonError> {
		let from_currencies = from_currency_tickers.join(",");
		let req =
			format!("v2/snapshot/locale/global/markets/forex/tickers?tickers={}", from_currencies);

		let tickers: Tickers = self.get(&req).await?;
		let quotes = tickers.tickers.iter().map(|t| (t.ticker_name.clone(), t.clone())).collect();
		Ok(quotes)
	}

	/// Convert a currency to a ticker with USD as the target currency.
	fn currency_to_ticker(from_currency: &str) -> String {
		format!("C:{}USD", from_currency)
	}
}


