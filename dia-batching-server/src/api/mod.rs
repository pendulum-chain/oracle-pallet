use std::error::Error;
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use crate::api::coingecko::CoingeckoPriceApi;
use crate::api::custom::{CustomPriceApi};
use crate::api::polygon::PolygonPriceApi;
use crate::AssetSpecifier;
use futures::future::join_all;
use rust_decimal::Decimal;
use serde::Deserialize;

mod coingecko;
mod custom;
mod polygon;

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

pub struct PriceApiImpl {}

impl PriceApiImpl {
    pub fn new() -> Self {
        Self {}
    }
}

#[async_trait]
impl PriceApi for PriceApiImpl {
    async fn get_quotations(
        &self,
        assets: Vec<&AssetSpecifier>,
    ) -> Result<Vec<Quotation>, Box<dyn Error + Sync + Send>> {
        // let futures = vec![
        //     self.get_fiat_quotations(assets),
        //     self.get_crypto_quotations(assets),
        //     self.get_custom_quotations(assets),
        // ];
        //
        // let results = join_all(futures).await;
        //
        // let quotations: Result<Vec<_>, _> = results.into_iter().collect();
        // let quotations = quotations?.into_iter().flatten().collect();
        //
        let mut quotations = Vec::new();
        quotations.append(&mut self.get_fiat_quotations(assets.clone()).await?);
        quotations.append(&mut self.get_crypto_quotations(assets.clone()).await?);
        quotations.append(&mut self.get_custom_quotations(assets.clone()).await?);
        Ok(quotations)
    }
}

impl PriceApiImpl {
    async fn get_fiat_quotations(
        &self,
        assets: Vec<&AssetSpecifier>,
    ) -> Result<Vec<Quotation>, Box<dyn Error + Sync + Send>> {
        // Filter out fiat assets
        let fiat_assets: Vec<_> = assets
            .into_iter()
            .filter(|asset| asset.blockchain.to_uppercase() == "FIAT")
            .collect();

        let quotations = PolygonPriceApi::get_prices(fiat_assets).await?;
        Ok(quotations)
    }

    async fn get_crypto_quotations(
        &self,
        assets: Vec<&AssetSpecifier>,
    ) -> Result<Vec<Quotation>, Box<dyn Error + Sync + Send>> {
        let crypto_assets = assets
            .into_iter()
            .filter(|asset| asset.blockchain.to_uppercase() == "CRYPTO")
            .collect();

        let quotations = CoingeckoPriceApi::get_prices(crypto_assets).await?;
        Ok(quotations)
    }

    async fn get_custom_quotations(
        &self,
        assets: Vec<&AssetSpecifier>,
    ) -> Result<Vec<Quotation>, Box<dyn Error + Sync + Send>> {
        let custom_assets: Vec<&AssetSpecifier> = assets
            .into_iter()
            .filter(|asset| CustomPriceApi::is_supported(asset))
            .collect();

        let mut quotations = Vec::new();
        for asset in custom_assets {
            let quotation = CustomPriceApi::get_price(asset).await?;
            quotations.push(quotation);
        }
        Ok(quotations)
    }
}
