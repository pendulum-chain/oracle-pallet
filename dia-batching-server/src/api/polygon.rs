use std::error::Error;
use clap::Parser;
use rust_decimal::Decimal;
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use crate::api::error::{CoingeckoError, PolygonError};
use crate::api::Quotation;
use crate::AssetSpecifier;

#[derive(Parser, Debug, Clone)]
pub struct PolygonConfig {
    /// The API key for Polygon.io
    #[clap(long, env = "PG_API_KEY")]
    pub pg_api_key: Option<String>,

    /// The host URL for the Polygon.io API.
    #[clap(long, env = "PG_HOST_URL", default_value = "https://api.polygon.io/v1")]
    pub pg_host_url: String,
}

pub struct PolygonPriceApi {
    client: PolygonClient,
}

impl PolygonPriceApi {
    pub async fn get_prices(assets: Vec<&AssetSpecifier>) -> Result<Vec<Quotation>, Box<dyn Error + Send + Sync>> {
        Err("Unsupported asset".into())
    }
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct PolygonPrice {
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

/// Polygon network client
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
        let mut api_key_header = reqwest::header::HeaderValue::from_str(bearer.as_str()).expect("Could not create header value");
        api_key_header.set_sensitive(true);
        headers.insert("Authorization", api_key_header);

        let client = reqwest::Client::builder().default_headers(headers).build().map_err(|e| PolygonError(e.to_string()))?;

        let url = reqwest::Url::parse(format!("{host}/{ep}", host = self.host.as_str(), ep = endpoint).as_str()).expect("Invalid URL");

        let response = client.get(url)
            .send()
            .await;

        match response {
            Ok(response) => {
                if !response.status().is_success() {
                    let result = response.text().await;
                    Err(PolygonError(result.unwrap()))
                } else {
                    let result = response.json().await;
                    result.map_err(|e| PolygonError("Could not decode Polygon response: ".to_owned() + &e.to_string()))
                }
            }
            Err(e) => {
                Err(PolygonError(e.to_string()))
            }
        }
    }

    /// Get the current price of any fiat currency in USD
    pub async fn price(
        &self,
        from_currency: &str,
    ) -> Result<PolygonPrice, PolygonError> {
        // Currencies have to be upper-case
        let from_currency = from_currency.to_uppercase();
        // We always query for USD.
        let to_currency = "USD";
        // We always query for full precision
        let precision = "5";
        // We always query for 1 unit
        let amount = "1";

        let req = format!("conversion/{}/{}?amount={}&precision={}", from_currency, to_currency, amount, precision);
        self.get(&req).await
    }
}

#[cfg(test)]
mod tests {
    use std::env;
    use crate::api::coingecko::CoingeckoClient;
    use crate::api::polygon::PolygonClient;

    fn read_env_variable(key: &str) -> Option<String> {
        if let None = dotenv::from_filename("../.env").ok() {
            // try looking at current directory
            dotenv::from_filename("./.env").ok();
        }

        env::var(key).ok()
    }

    fn get_polygon_variables() -> (String, String) {
        let api_key = read_env_variable("PG_API_KEY").expect("Please provide a Polygon API key");
        let host_url = read_env_variable("PG_HOST_URL").unwrap_or("https://api.polygon.io/v1".to_string());
        (api_key, host_url)
    }

    fn create_client() -> PolygonClient {
        let (api_key, host_url) = get_polygon_variables();
        PolygonClient::new(host_url, api_key)
    }
    #[tokio::test]
    async fn test_fetching_price() {
        let client = create_client();

        let result = client.price(&"brl").await;

        assert!(result.is_ok());
        let brl_price = result.unwrap();
        assert_eq!(brl_price.from, "BRL");
        assert_eq!(brl_price.to, "USD");
        assert!(brl_price.converted > 0.into());
    }
}