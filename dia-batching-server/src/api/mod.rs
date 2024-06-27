use std::error::Error;
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use clap::Parser;
use crate::api::coingecko::{CoingeckoConfig, CoingeckoPriceApi};
use crate::api::custom::{CustomPriceApi};
use crate::api::polygon::{PolygonConfig, PolygonPriceApi};
use crate::AssetSpecifier;
use futures::future::join_all;
use rust_decimal::Decimal;
use serde::Deserialize;
use crate::api::error::{ApiError, PolygonError};

mod coingecko;
mod custom;
mod polygon;
mod error;

#[derive(Deserialize, Debug, Clone)]
pub struct Quotation {
    #[serde(rename(deserialize = "Symbol"))]
    pub symbol: String,
    #[serde(rename(deserialize = "Name"))]
    pub name: String,
    #[serde(rename(deserialize = "Blockchain"))]
    pub blockchain: Option<String>,
    #[serde(rename(deserialize = "Price"))]
    pub price: Decimal,
    #[serde(rename(deserialize = "Time"))]
    pub time: DateTime<Utc>,
}

#[async_trait]
pub trait PriceApi {
    async fn get_quotations(
        &self,
        assets: Vec<&AssetSpecifier>,
    ) -> Result<Vec<Quotation>, Box<dyn Error + Sync + Send>>;
}

pub struct PriceApiImpl {
    coingecko_price_api: CoingeckoPriceApi,
    polygon_price_api: PolygonPriceApi,
}

impl PriceApiImpl {
    pub fn new() -> Self {
        Self {
            coingecko_price_api: CoingeckoPriceApi::new_from_config(CoingeckoConfig::parse()),
            polygon_price_api: PolygonPriceApi::new_from_config(PolygonConfig::parse()),
        }
    }
}

#[async_trait]
impl PriceApi for PriceApiImpl {
    async fn get_quotations(
        &self,
        assets: Vec<&AssetSpecifier>,
    ) -> Result<Vec<Quotation>, Box<dyn Error + Sync + Send>> {
        let mut quotations = Vec::new();

        // First, get fiat quotations
        let fiat_assets: Vec<_> = assets.clone()
            .into_iter()
            .filter(|asset| PolygonPriceApi::is_supported(asset))
            .collect();

        let fiat_quotes = self.get_fiat_quotations(fiat_assets.clone()).await;
        if let Ok(fiat_quotes) = fiat_quotes {
            quotations.extend(fiat_quotes);
        }

        // Then, get quotations for custom assets
        let custom_assets: Vec<&AssetSpecifier> = assets.clone()
            .into_iter()
            .filter(|asset| CustomPriceApi::is_supported(asset))
            .collect();

        let custom_quotes = self.get_custom_quotations(custom_assets.clone()).await;
        if let Ok(custom_quotes) = custom_quotes {
            quotations.extend(custom_quotes);
        }

        // Finally, get supported crypto quotations
        let crypto_assets = assets.into_iter().filter(|asset| {
            CoingeckoPriceApi::is_supported(asset)
        }).collect::<Vec<_>>();

        let crypto_quotes = self.get_crypto_quotations(crypto_assets).await;
        if let Ok(crypto_quotes) = crypto_quotes {
            quotations.extend(crypto_quotes);
        }

        Ok(quotations)
    }
}

impl PriceApiImpl {
    async fn get_fiat_quotations(
        &self,
        assets: Vec<&AssetSpecifier>,
    ) -> Result<Vec<Quotation>, PolygonError> {
        let quotations = self.polygon_price_api.get_prices(assets).await?;
        Ok(quotations)
    }

    async fn get_crypto_quotations(
        &self,
        assets: Vec<&AssetSpecifier>,
    ) -> Result<Vec<Quotation>, ApiError> {
        let quotations = self.coingecko_price_api.get_prices(assets).await.map_err(
            |e| {
                ApiError::CoingeckoError(e)
            },
        )?;
        Ok(quotations)
    }

    async fn get_custom_quotations(
        &self,
        assets: Vec<&AssetSpecifier>,
    ) -> Result<Vec<Quotation>, Box<dyn Error + Sync + Send>> {
        let mut quotations = Vec::new();
        for asset in assets {
            let quotation = CustomPriceApi::get_price(asset).await?;
            quotations.push(quotation);
        }
        Ok(quotations)
    }
}
