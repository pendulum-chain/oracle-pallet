use std::error::Error;
use crate::api::Quotation;
use crate::AssetSpecifier;
use serde::de::DeserializeOwned;
use clap::Parser;
use std::collections::HashMap;

#[derive(Parser, Debug, Clone)]
struct ServiceConfig {
    /// The API key for CoinGecko.
    #[clap(long, env = "CG_API_KEY")]
    pub cg_api_key: String,

    /// Logging output format.
    #[clap(long, env = "CG_HOST_URL", default_value = "https://pro-api.coingecko.com/api/v3")]
    pub cg_host_url: String,
}

pub struct CoingeckoPriceApi {
    client: CoinGeckoClient,
}

impl CoingeckoPriceApi {
    pub fn new() -> Self {
        let config = ServiceConfig::parse();
        let client = CoinGeckoClient::new(config.cg_host_url, config.cg_api_key);

        Self {
            client,
        }
    }

    pub async fn get_prices(assets: Vec<&AssetSpecifier>) -> Result<Vec<Quotation>, Box<dyn Error + Send + Sync>> {
        Err("Unsupported asset".into())
    }
}

use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct SimplePing {
    pub gecko_says: String,
}

// ---------------------------------------------
//  /simple/price and /simple/token_price/{id}
// ---------------------------------------------
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct Price {
    pub usd: Option<f64>,
    pub usd_market_cap: Option<f64>,
    pub usd_24h_vol: Option<f64>,
    pub usd_24h_change: Option<f64>,
    pub last_updated_at: Option<u64>,
}

/// CoinGecko client
pub struct CoinGeckoClient {
    host: String,
    api_key: String,
}

#[derive(Debug)]
struct CoinGeckoClientError(String);

impl CoinGeckoClient {
    pub fn new(host: String, api_key: String) -> Self {
        CoinGeckoClient { host, api_key }
    }

    async fn get<R: DeserializeOwned>(&self, endpoint: &str) -> Result<R, CoinGeckoClientError> {
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

        let client = reqwest::Client::builder().default_headers(headers).build().map_err(|e| CoinGeckoClientError(e.to_string()))?;

        let url = reqwest::Url::parse(format!("{host}/{ep}", host = self.host.as_str(), ep = endpoint).as_str()).expect("Invalid URL");

        let response = client.get(url)
            .send()
            .await;

        match response {
            Ok(response) => {
                if !response.status().is_success() {
                    // return Err(reqwest::Error::from_str("Request failed"));
                    let result = response.text().await;
                    Err(CoinGeckoClientError(result.unwrap()))
                } else {
                    let result = response.json().await;
                    result.map_err(|e| CoinGeckoClientError(e.to_string()))
                }
            }
            Err(e) => {
                Err(CoinGeckoClientError(e.to_string()))
            }
        }
    }

    /// Check API server status
    pub async fn ping(&self) -> Result<SimplePing, CoinGeckoClientError> {
        self.get("/ping").await
    }

    /// Get the current price of any cryptocurrencies in any other supported currencies that you need
    ///
    /// # Examples
    ///
    /// ```rust
    /// #[tokio::main]
    /// async fn main() {
    ///     use coingecko::CoinGeckoClient;
    ///     let client = CoinGeckoClient::default();
    ///
    ///     client.price(&["bitcoin", "ethereum"], &["usd"], true, true, true, true).await;
    /// }
    /// ```
    pub async fn price<Id: AsRef<str>>(
        &self,
        ids: &[Id],
        include_market_cap: bool,
        include_24hr_vol: bool,
        include_24hr_change: bool,
        include_last_updated_at: bool,
    ) -> Result<HashMap<String, Price>, CoinGeckoClientError> {
        let ids = ids.iter().map(AsRef::as_ref).collect::<Vec<_>>();
        // We always query for USD
        let vs_currencies = vec!["usd"];
        let vs_currencies = vs_currencies.iter().map(AsRef::as_ref).collect::<Vec<_>>();
        // We always query for full precision
        let precision = "full";
        let req = format!("/simple/price?ids={}&vs_currencies={}&precision={}&include_market_cap={}&include_24hr_vol={}&include_24hr_change={}&include_last_updated_at={}", ids.join("%2C"), vs_currencies.join("%2C"), precision, include_market_cap, include_24hr_vol, include_24hr_change, include_last_updated_at);
        // TODO maybe use json::Value instead of Price for arbitrary values
        self.get(&req).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_ping() {
        let client = CoinGeckoClient::new("https://api.coingecko.com/api/v3".to_string(), "CG-KX1Xs7FcZKAiEN22SXTWRjZx".to_string());
        let ping = client.ping().await.expect("Should return a ping");
        assert_eq!(ping.gecko_says, "(V3) To the Moon!");
    }

    #[tokio::test]
    async fn test_price() {
        let client = CoinGeckoClient::new("https://api.coingecko.com/api/v3".to_string(), "CG-KX1Xs7FcZKAiEN22SXTWRjZx".to_string());
        let price = client.price(&["stellar"], true, true, true, true).await.expect("Should return a price");
        assert_eq!(price.len(), 1);
    }
}

// 0.088943079510600376
// 0.08894307951060038