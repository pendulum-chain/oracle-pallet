use std::collections::HashMap;
use std::error::Error;
use std::fmt;

use clap::Parser;
use serde::{Deserialize, Serialize};
use serde::de::DeserializeOwned;
use crate::api::error::CoingeckoError;
use crate::api::Quotation;
use crate::AssetSpecifier;
use rust_decimal::Decimal;
use rust_decimal::prelude::FromPrimitive;

#[derive(Parser, Debug, Clone)]
pub struct CoingeckoConfig {
    /// The API key for CoinGecko.
    #[clap(long, env = "CG_API_KEY")]
    pub cg_api_key: Option<String>,

    /// The host URL for CoinGecko.
    /// Defaults to the CoinGecko Pro API.
    #[clap(long, env = "CG_HOST_URL", default_value = "https://pro-api.coingecko.com/api/v3")]
    pub cg_host_url: String,
}

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

        Self {
            client,
        }
    }

    pub async fn get_prices(&self, assets: Vec<&AssetSpecifier>) -> Result<Vec<Quotation>, CoingeckoError> {
        let coingecko_ids = assets.clone().into_iter().filter_map(|asset| {
            let id = Self::convert_to_coingecko_id(asset);
            match id {
                Some(id) => Some(id),
                None => {
                    log::warn!("Could not find CoinGecko ID for asset {:?}", asset);
                    None
                }
            }
        }).collect::<Vec<_>>();

        let prices = self.client.price(&coingecko_ids, false, false, false, true).await.map_err(
            |e| CoingeckoError(e.to_string())
        )?;

        let quotations = prices.into_iter().filter_map(|(id, price)| {
            let asset = assets.iter().find(|asset| {
                Self::convert_to_coingecko_id(asset).as_deref() == Some(id.as_str())
            })?;
            let price_usd = price.usd.unwrap_or_default();
            match Decimal::from_f64(price_usd) {
                Some(price_decimal) => Some(Quotation {
                    symbol: asset.symbol.clone(),
                    name: asset.symbol.clone(),
                    blockchain: Some(asset.blockchain.clone()),
                    price: price_decimal,
                    time: chrono::Utc::now(),
                }),
                None => None
            }
        }).collect();

        Ok(quotations)
    }

    pub fn is_supported(asset: &AssetSpecifier) -> bool {
        Self::convert_to_coingecko_id(asset).is_some()
    }

    /// Maps the blockchain and symbol pair to the CoinGecko ID.
    /// For now, this conversion is using a hard-coded list.
    /// We need to change our on-chain data to use CoinGecko IDs in the future.
    fn convert_to_coingecko_id(asset: &AssetSpecifier) -> Option<String> {
        match (asset.blockchain.as_str(), asset.symbol.as_str()) {
            ("Pendulum", "PEN") => Some("pendulum".to_string()),
            ("Polkadot", "DOT") => Some("polkadot".to_string()),
            ("Kusama", "kusama") => Some("kusama".to_string()),
            ("Astar", "ASTR") => Some("astar".to_string()),
            ("Bifrost", "BNC") => Some("bifrost-native-coin".to_string()),
            ("Bifrost", "vDOT") => Some("voucher-dot".to_string()),
            ("HydraDX", "HDX") => Some("hydration".to_string()),
            ("Moonbeam", "GLMR") => Some("moonbeam".to_string()),
            ("Polkadex", "PDEX") => Some("polkadex".to_string()),
            ("Stellar", "XLM") => Some("stellar".to_string()),
            _ => None
        }
    }
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct SimplePing {
    pub gecko_says: String,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct Price {
    pub usd: Option<f64>,
    pub usd_market_cap: Option<f64>,
    pub usd_24h_vol: Option<f64>,
    pub usd_24h_change: Option<f64>,
    pub last_updated_at: Option<u64>,
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
        let mut api_key = reqwest::header::HeaderValue::from_str(self.api_key.as_str()).expect("Could not create header value");
        api_key.set_sensitive(true);
        if self.host.contains("pro-api") {
            headers.insert("x-cg-pro-api-key", api_key);
        } else {
            headers.insert("x-cg-demo-api-key", api_key);
        }

        let client = reqwest::Client::builder().default_headers(headers).build().map_err(|e| CoingeckoError(e.to_string()))?;

        let url = reqwest::Url::parse(format!("{host}/{ep}", host = self.host.as_str(), ep = endpoint).as_str()).expect("Invalid URL");

        let response = client.get(url)
            .send()
            .await;

        match response {
            Ok(response) => {
                if !response.status().is_success() {
                    // return Err(reqwest::Error::from_str("Request failed"));
                    let result = response.text().await;
                    Err(CoingeckoError(result.unwrap()))
                } else {
                    let result = response.json().await;
                    result.map_err(|e| CoingeckoError(e.to_string()))
                }
            }
            Err(e) => {
                Err(CoingeckoError(e.to_string()))
            }
        }
    }

    /// Check API server status
    pub async fn ping(&self) -> Result<SimplePing, CoingeckoError> {
        self.get("/ping").await
    }

    /// Get the current price of any cryptocurrencies vs USD with full precision
    pub async fn price<Id: AsRef<str>>(
        &self,
        ids: &[Id],
        include_market_cap: bool,
        include_24hr_vol: bool,
        include_24hr_change: bool,
        include_last_updated_at: bool,
    ) -> Result<HashMap<String, Price>, CoingeckoError> {
        let ids = ids.iter().map(AsRef::as_ref).collect::<Vec<_>>();
        // We always query for USD
        let vs_currencies = vec!["usd"];
        let vs_currencies = vs_currencies.iter().map(AsRef::as_ref).collect::<Vec<_>>();
        // We always query for full precision
        let precision = "full";
        let req = format!("/simple/price?ids={}&vs_currencies={}&precision={}&include_market_cap={}&include_24hr_vol={}&include_24hr_change={}&include_last_updated_at={}", ids.join("%2C"), vs_currencies.join("%2C"), precision, include_market_cap, include_24hr_vol, include_24hr_change, include_last_updated_at);
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
        let host_url = read_env_variable("CG_HOST_URL").unwrap_or("https://pro-api.coingecko.com/api/v3".to_string());
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

        let prices = client.price(&ids, true, true, true, true).await.expect("Should return a price");
        assert_eq!(prices.len(), ids.len());

        let stellar_price = prices.get("stellar").expect("Should return a price");
        assert!(stellar_price.usd.is_some());
        assert!(stellar_price.usd.unwrap() > 0.0);
        assert!(stellar_price.usd_market_cap.is_some());
        assert!(stellar_price.usd_24h_vol.is_some());
        assert!(stellar_price.usd_24h_change.is_some());
        assert!(stellar_price.last_updated_at.is_some());
    }

    #[tokio::test]
    async fn test_fetching_multiple_prices() {
        let client = create_client();

        let ids = vec!["stellar", "voucher-dot"];

        let prices = client.price(&ids, true, true, true, true).await.expect("Should return a price");
        assert_eq!(prices.len(), ids.len());

        let stellar_price = prices.get("stellar").expect("Should return a price");
        assert!(stellar_price.usd.is_some());
        assert!(stellar_price.usd.unwrap() > 0.0);
        assert!(stellar_price.usd_market_cap.is_some());
        assert!(stellar_price.usd_24h_vol.is_some());
        assert!(stellar_price.usd_24h_change.is_some());
        assert!(stellar_price.last_updated_at.is_some());

        let vdot_price = prices.get("voucher-dot").expect("Should return a price");
        assert!(vdot_price.usd.is_some());
        assert!(vdot_price.usd.unwrap() > 0.0);
        assert!(vdot_price.usd_market_cap.is_some());
        assert!(vdot_price.usd_24h_vol.is_some());
        assert!(vdot_price.usd_24h_change.is_some());
        assert!(vdot_price.last_updated_at.is_some());
    }

    #[tokio::test]
    async fn test_api_returns_prices() {
        let (api_key, host_url) = get_coingecko_variables();

        let price_api = CoingeckoPriceApi::new(host_url, api_key);

        let stellar_asset = AssetSpecifier {
            blockchain: "Stellar".to_string(),
            symbol: "XLM".to_string(),
        };
        let voucher_dot_asset = AssetSpecifier {
            blockchain: "Bifrost".to_string(),
            symbol: "vDOT".to_string(),
        };

        let assets = vec![&stellar_asset, &voucher_dot_asset];

        let quotations = price_api.get_prices(assets).await;
        assert!(quotations.is_ok());
        let quotations = quotations.unwrap();
        assert_eq!(quotations.len(), 2);

        let stellar_quotation = quotations.iter().find(|q| q.symbol == stellar_asset.symbol).expect("Should return a Stellar quotation");
        assert_eq!(stellar_quotation.symbol, stellar_asset.symbol);
        assert_eq!(stellar_quotation.name, stellar_asset.symbol);
        assert_eq!(stellar_quotation.blockchain, Some(stellar_asset.blockchain));
        assert!(stellar_quotation.price > Decimal::from_f64(0.0).unwrap());

        let vdot_quotation = quotations.iter().find(|q| q.symbol == voucher_dot_asset.symbol).expect("Should return a vDOT quotation");
        assert_eq!(vdot_quotation.symbol, voucher_dot_asset.symbol);
        assert_eq!(vdot_quotation.name, voucher_dot_asset.symbol);
        assert_eq!(vdot_quotation.blockchain, Some(voucher_dot_asset.blockchain));
        assert!(vdot_quotation.price > Decimal::from_f64(0.0).unwrap());
    }
}