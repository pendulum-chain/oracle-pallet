use rust_decimal::Decimal;
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use crate::api::error::CoingeckoError;
use crate::api::Quotation;
use crate::args::CoingeckoConfig;
use crate::AssetSpecifier;

pub struct CoingeckoPriceApi {
	client: CoingeckoClient,
}

impl CoingeckoPriceApi {
	pub fn new_from_config(config: CoingeckoConfig) -> Self {
		let api_key = config.cg_api_key.expect("Please provide a CoinGecko API key");

		Self::new(config.cg_host_url, api_key)
	}

	pub fn new(host_url: String, api_key: String) -> Self {
		let client = CoingeckoClient::new(host_url, api_key);

		Self { client }
	}

	pub async fn get_prices(
		&self,
		assets: Vec<&AssetSpecifier>,
	) -> Result<Vec<Quotation>, CoingeckoError> {
		// Map used for the reverse lookup of the CoinGecko ID to the asset
		let mut id_to_asset_map: HashMap<String, AssetSpecifier> = HashMap::new();

		let coingecko_ids = assets
			.clone()
			.into_iter()
			.filter_map(|asset| {
				Self::convert_to_coingecko_id(asset)
					.and_then(|id| {
						id_to_asset_map.insert(id.clone(), asset.clone());
						Some(id)
					})
					.or_else(|| {
						log::warn!("Could not find CoinGecko ID for asset {:?}", asset);
						None
					})
			})
			.collect::<Vec<_>>();

		let id_to_price_map =
			self.client.price(&coingecko_ids, false, true, false, true).await.map_err(|e| {
				CoingeckoError(format!("Couldn't query CoinGecko prices {}", e.to_string()))
			})?;

		let quotations = id_to_price_map
			.into_iter()
			.filter_map(|(id, price)| {
				let asset = id_to_asset_map.get(&id)?;

				let supply = price.usd_24h_vol.unwrap_or_default();

				Some(Quotation {
					symbol: asset.symbol.clone(),
					name: asset.symbol.clone(),
					blockchain: Some(asset.blockchain.clone()),
					price: price.usd,
					supply,
					time: price.last_updated_at,
				})
			})
			.collect();

		Ok(quotations)
	}

	pub fn is_supported(asset: &AssetSpecifier) -> bool {
		Self::convert_to_coingecko_id(asset).is_some()
	}

	/// Maps the blockchain and symbol pair to the CoinGecko ID.
	/// For now, this conversion is using a hard-coded list.
	/// We need to change our on-chain data to use CoinGecko IDs in the future.
	fn convert_to_coingecko_id(asset: &AssetSpecifier) -> Option<String> {
		// Capitalize the blockchain and symbol
		let blockchain = asset.blockchain.to_uppercase();
		let symbol = asset.symbol.to_uppercase();
		match (blockchain.as_str(), symbol.as_str()) {
			("PENDULUM", "PEN") => Some("pendulum-chain".to_string()),
			("POLKADOT", "DOT") => Some("polkadot".to_string()),
			("KUSAMA", "KSM") => Some("kusama".to_string()),
			("ASTAR", "ASTR") => Some("astar".to_string()),
			("BIFROST", "BNC") => Some("bifrost-native-coin".to_string()),
			("BIFROST", "VDOT") => Some("voucher-dot".to_string()),
			("HYDRADX", "HDX") => Some("hydradx".to_string()),
			("MOONBEAM", "GLMR") => Some("moonbeam".to_string()),
			("POLKADEX", "PDEX") => Some("polkadex".to_string()),
			("STELLAR", "XLM") => Some("stellar".to_string()),
			_ => None,
		}
	}
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct SimplePing {
	pub gecko_says: String,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct CoingeckoPrice {
	pub usd: Decimal,
	pub usd_market_cap: Option<Decimal>,
	pub usd_24h_vol: Option<Decimal>,
	pub usd_24h_change: Option<Decimal>,
	pub last_updated_at: u64,
}

/// CoinGecko network client
pub struct CoingeckoClient {
	host: String,
	api_key: String,
}

impl CoingeckoClient {
	pub fn new(host: String, api_key: String) -> Self {
		CoingeckoClient { host, api_key }
	}

	async fn get<R: DeserializeOwned>(&self, endpoint: &str) -> Result<R, CoingeckoError> {
		let mut headers = reqwest::header::HeaderMap::new();
		headers.insert("accept", reqwest::header::HeaderValue::from_static("application/json"));

		// We supply a different header for the demo API
		let mut api_key = reqwest::header::HeaderValue::from_str(self.api_key.as_str())
			.map_err(|e| CoingeckoError(format!("Could not set API key header value: {}", e)))?;
		api_key.set_sensitive(true);
		if self.host.contains("pro-api") {
			headers.insert("x-cg-pro-api-key", api_key);
		} else {
			headers.insert("x-cg-demo-api-key", api_key);
		}

		let client = reqwest::Client::builder()
			.default_headers(headers)
			.build()
			.map_err(|e| CoingeckoError(e.to_string()))?;

		let url = reqwest::Url::parse(
			format!("{host}/{ep}", host = self.host.as_str(), ep = endpoint).as_str(),
		)
		.expect("Invalid URL");

		let response =
			client.get(url).send().await.map_err(|e| {
				CoingeckoError(format!("Failed to send request: {}", e.to_string()))
			})?;

		if !response.status().is_success() {
			let result = response.text().await;
			return Err(CoingeckoError(format!(
				"CoinGecko API error: {}",
				result.unwrap_or("Unknown".to_string()).trim()
			)));
		}

		let result = response.json().await;
		result.map_err(|e| CoingeckoError(format!("Could not decode CoinGecko response: {}", e)))
	}

	/// Check API server status
	#[allow(dead_code)]
	pub async fn ping(&self) -> Result<SimplePing, CoingeckoError> {
		self.get("/api/v3/ping").await
	}

	/// Get the current price of any cryptocurrencies vs USD with full precision
	pub async fn price<Id: AsRef<str>>(
		&self,
		ids: &[Id],
		include_market_cap: bool,
		include_24hr_vol: bool,
		include_24hr_change: bool,
		include_last_updated_at: bool,
	) -> Result<HashMap<String, CoingeckoPrice>, CoingeckoError> {
		let ids = ids.iter().map(AsRef::as_ref).collect::<Vec<_>>();
		// We always query for USD
		let vs_currencies = vec!["usd"];
		// We always query for full precision
		let precision = "full";
		let req = format!("/api/v3/simple/price?ids={}&vs_currencies={}&precision={}&include_market_cap={}&include_24hr_vol={}&include_24hr_change={}&include_last_updated_at={}", ids.join("%2C"), vs_currencies.join("%2C"), precision, include_market_cap, include_24hr_vol, include_24hr_change, include_last_updated_at);
		self.get(&req).await
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

	fn get_coingecko_variables() -> (String, String) {
		let api_key = read_env_variable("CG_API_KEY").expect("Please provide a CoinGecko API key");
		let host_url =
			read_env_variable("CG_HOST_URL").unwrap_or("https://pro-api.coingecko.com".to_string());
		(api_key, host_url)
	}

	fn create_client() -> CoingeckoClient {
		let (api_key, host_url) = get_coingecko_variables();
		CoingeckoClient::new(host_url, api_key)
	}

	#[tokio::test]
	async fn test_ping() {
		let client = create_client();

		let ping = client.ping().await.expect("Should return a ping");
		assert_eq!(ping.gecko_says, "(V3) To the Moon!");
	}

	#[tokio::test]
	async fn test_fetching_single_price() {
		let client = create_client();

		let ids = vec!["stellar"];

		let prices =
			client.price(&ids, true, true, true, true).await.expect("Should return a price");
		assert_eq!(prices.len(), ids.len());

		let stellar_price = prices.get("stellar").expect("Should return a price");
		assert!(stellar_price.usd > 0.into());
		assert!(stellar_price.usd_market_cap.is_some());
		assert!(stellar_price.usd_24h_vol.is_some());
		assert!(stellar_price.usd_24h_change.is_some());
		assert!(stellar_price.last_updated_at > 0);
	}

	#[tokio::test]
	async fn test_fetching_multiple_prices() {
		let client = create_client();

		let ids = vec!["stellar", "voucher-dot"];

		let prices =
			client.price(&ids, true, true, true, true).await.expect("Should return a price");
		assert_eq!(prices.len(), ids.len());

		let stellar_price = prices.get("stellar").expect("Should return a price");
		assert!(stellar_price.usd > 0.into());
		assert!(stellar_price.usd_market_cap.is_some());
		assert!(stellar_price.usd_24h_vol.is_some());
		assert!(stellar_price.usd_24h_change.is_some());
		assert!(stellar_price.last_updated_at > 0);

		let vdot_price = prices.get("voucher-dot").expect("Should return a price");
		assert!(vdot_price.usd > 0.into());
		assert!(vdot_price.usd_market_cap.is_some());
		assert!(vdot_price.usd_24h_vol.is_some());
		assert!(vdot_price.usd_24h_change.is_some());
		assert!(vdot_price.last_updated_at > 0);
	}

	#[tokio::test]
	async fn test_api_returns_prices() {
		let (api_key, host_url) = get_coingecko_variables();

		let price_api = CoingeckoPriceApi::new(host_url, api_key);

		let pen_asset =
			AssetSpecifier { blockchain: "Pendulum".to_string(), symbol: "PEN".to_string() };
		let polkadot_asset =
			AssetSpecifier { blockchain: "Polkadot".to_string(), symbol: "DOT".to_string() };
		let kusama_asset =
			AssetSpecifier { blockchain: "Kusama".to_string(), symbol: "KSM".to_string() };
		let astar_asset =
			AssetSpecifier { blockchain: "Astar".to_string(), symbol: "ASTR".to_string() };
		let bifrost_asset =
			AssetSpecifier { blockchain: "Bifrost".to_string(), symbol: "BNC".to_string() };
		let voucher_dot_asset =
			AssetSpecifier { blockchain: "Bifrost".to_string(), symbol: "vDOT".to_string() };
		let hydradx_asset =
			AssetSpecifier { blockchain: "HydraDX".to_string(), symbol: "HDX".to_string() };
		let moonbeam_asset =
			AssetSpecifier { blockchain: "Moonbeam".to_string(), symbol: "GLMR".to_string() };
		let polkadex_asset =
			AssetSpecifier { blockchain: "Polkadex".to_string(), symbol: "PDEX".to_string() };
		let stellar_asset =
			AssetSpecifier { blockchain: "Stellar".to_string(), symbol: "XLM".to_string() };

		let assets = vec![
			&pen_asset,
			&polkadot_asset,
			&kusama_asset,
			&astar_asset,
			&bifrost_asset,
			&voucher_dot_asset,
			&hydradx_asset,
			&moonbeam_asset,
			&polkadex_asset,
			&stellar_asset,
		];

		let quotations = price_api.get_prices(assets.clone()).await;
		assert!(quotations.is_ok());
		let quotations = quotations.unwrap();

		// Check if all assets have a quotation and if not, print the missing ones
		for asset in assets {
			let quotation = quotations
				.iter()
				.find(|q| q.symbol == asset.symbol)
				.expect(format!("Could not find a quotation for asset specifier {:?}", asset).as_str());
			assert_eq!(quotation.symbol, asset.symbol);
			assert_eq!(quotation.name, asset.symbol);
			assert_eq!(quotation.blockchain, Some(asset.blockchain.clone()));
			assert!(quotation.price > 0.into());
		}
	}
}
