use crate::api::error::BinanceError;
use crate::types::{AssetSpecifier, Quotation};
use chrono::Utc;
use rust_decimal::prelude::Zero;
use rust_decimal::Decimal;
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};

pub struct BinancePriceApi {
	client: BinanceClient,
}

impl BinancePriceApi {
	pub fn new() -> Self {
		let client: BinanceClient = BinanceClient::default();

		Self { client }
	}

	pub async fn get_price(&self, asset: &AssetSpecifier) -> Result<Quotation, BinanceError> {
		let binance_asset_identifier = Self::convert_to_binance_id(&asset);

		match self.client.price(binance_asset_identifier).await {
			Ok(price) => Ok(Quotation {
				symbol: asset.symbol.clone(),
				name: asset.symbol.clone(),
				blockchain: Some(asset.blockchain.clone().into()),
				price: price.price,
				supply: Decimal::zero(),
				time: Utc::now().timestamp().unsigned_abs(),
			}),
			Err(error) => {
				log::warn!("Error getting price for {:?} from Binance: {:?}", asset, error);
				Err(error)
			},
		}
	}

	// We assume here that the `symbol` field of the `AssetSpecifier` is a valid asset for Binance.
	// The `blockchain` field is not used and ignored.
	fn convert_to_binance_id(asset: &AssetSpecifier) -> String {
		asset.symbol.to_uppercase()
	}
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct BinancePrice {
	pub symbol: String,
	pub price: Decimal,
}

pub struct BinanceClient {
	host: String,
	inner: reqwest::Client,
}

impl BinanceClient {
	pub fn default() -> Self {
		Self::new("https://api.binance.com".to_string())
	}

	pub fn new(host: String) -> Self {
		let inner = reqwest::Client::new();

		Self { host, inner }
	}

	async fn get<R: DeserializeOwned>(&self, endpoint: &str) -> Result<R, BinanceError> {
		let url = reqwest::Url::parse(
			format!("{host}/{ep}", host = self.host.as_str(), ep = endpoint).as_str(),
		)
		.expect("Invalid URL");

		let response = self
			.inner
			.get(url)
			.send()
			.await
			.map_err(|e| BinanceError(format!("Failed to send request: {}", e.to_string())))?;

		if !response.status().is_success() {
			let result = response.text().await;
			return Err(BinanceError(format!(
				"Binance API error: {}",
				result.unwrap_or("Unknown".to_string()).trim()
			)));
		}

		let result = response.json().await;
		result.map_err(|e| BinanceError(format!("Could not decode Binance response: {}", e)))
	}

	pub async fn price(&self, symbol: String) -> Result<BinancePrice, BinanceError> {
		let endpoint = format!("api/v3/ticker/price?symbol={}", symbol);
		let response: BinancePrice = self.get(&endpoint).await?;
		Ok(response)
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	#[tokio::test]
	async fn test_fetching_single_price() {
		let client = BinanceClient::default();

		let id = "USDTARS";

		let price = client.price(id.to_string()).await.expect("Should return a price");

		assert_eq!(price.symbol, id);
		assert!(price.price > 0.into());
	}
}
