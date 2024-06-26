use std::error::Error;
use crate::api::Quotation;
use crate::AssetSpecifier;
use serde::de::DeserializeOwned;
use clap::Parser;
use std::collections::HashMap;
use std::convert::TryInto;

#[derive(Parser, Debug, Clone)]
struct ServiceConfig {
    /// The API key for CoinGecko.
    #[clap(long, env = "CG_API_KEY")]
    pub cg_api_key: String,

    /// Logging output format.
    #[clap(long, env = "CG_HOST_URL", default_value = "https://api.coingecko.com/api/v3")]
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
pub struct Price {}

/// CoinGecko client
pub struct CoinGeckoClient {
    host: String,
    api_key: String,
}


impl CoinGeckoClient {
    pub fn new(host: String, api_key: String) -> Self {
        CoinGeckoClient { host, api_key }
    }

    async fn get<R: DeserializeOwned>(&self, endpoint: &str) -> Result<R, reqwest::Error> {
        let mut headers = reqwest::header::HeaderMap::new();

        // We supply both API keys because one of them will work
        let mut api_key = reqwest::header::HeaderValue::from_str(self.api_key.as_str()).expect("Could not create header value");
        api_key.set_sensitive(true);
        headers.insert("x-cg-demo-api-key", api_key.clone());
        headers.insert("x-cg-pro-api-key", api_key);

        let client = reqwest::Client::builder().default_headers(headers).build()?;

        let url = reqwest::Url::parse(format!("{host}/{ep}", host = self.host.as_str(), ep = endpoint).as_str()).expect("Invalid URL");

        client.get(url)
            .send()
            .await?
            .json()
            .await
    }

    /// Check API server status
    pub async fn ping(&self) -> Result<SimplePing, reqwest::Error> {
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
    pub async fn price<Id: AsRef<str>, Curr: AsRef<str>>(
        &self,
        ids: &[Id],
        vs_currencies: &[Curr],
        include_market_cap: bool,
        include_24hr_vol: bool,
        include_24hr_change: bool,
        include_last_updated_at: bool,
    ) -> Result<HashMap<String, Price>, reqwest::Error> {
        let ids = ids.iter().map(AsRef::as_ref).collect::<Vec<_>>();
        let vs_currencies = vs_currencies.iter().map(AsRef::as_ref).collect::<Vec<_>>();
        let req = format!("/simple/price?ids={}&vs_currencies={}&include_market_cap={}&include_24hr_vol={}&include_24hr_change={}&include_last_updated_at={}", ids.join("%2C"), vs_currencies.join("%2C"), include_market_cap, include_24hr_vol, include_24hr_change, include_last_updated_at);
        self.get(&req).await
    }
}