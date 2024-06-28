use crate::api::coingecko::{CoingeckoConfig, CoingeckoPriceApi};
use crate::api::custom::CustomPriceApi;
pub use crate::api::error::ApiError;
use crate::api::error::{CoingeckoError, CustomError, PolygonError};
use crate::api::polygon::{PolygonConfig, PolygonPriceApi};
use crate::types::Quotation;
use crate::AssetSpecifier;
use async_trait::async_trait;
use clap::Parser;

mod coingecko;
mod custom;
mod error;
mod polygon;

#[async_trait]
pub trait PriceApi {
	async fn get_quotations(
		&self,
		assets: Vec<&AssetSpecifier>,
	) -> Result<Vec<Quotation>, ApiError>;
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
	) -> Result<Vec<Quotation>, ApiError> {
		let mut quotations = Vec::new();

		// First, get fiat quotations
		let fiat_assets: Vec<_> = assets
			.clone()
			.into_iter()
			.filter(|asset| PolygonPriceApi::is_supported(asset))
			.collect();

		let fiat_quotes = self.get_fiat_quotations(fiat_assets.clone()).await;
		match fiat_quotes {
			Ok(fiat_quotes) => quotations.extend(fiat_quotes),
			Err(e) => log::error!("Error getting fiat quotations: {}", e),
		}

		// Then, get quotations for custom assets
		let custom_assets: Vec<&AssetSpecifier> = assets
			.clone()
			.into_iter()
			.filter(|asset| CustomPriceApi::is_supported(asset))
			.collect();

		let custom_quotes = self.get_custom_quotations(custom_assets.clone()).await;
		match custom_quotes {
			Ok(custom_quotes) => quotations.extend(custom_quotes),
			Err(e) => log::error!("Error getting custom quotations: {}", e),
		}

		// Finally, get supported crypto quotations
		let crypto_assets = assets
			.into_iter()
			.filter(|asset| CoingeckoPriceApi::is_supported(asset))
			.collect::<Vec<_>>();

		let crypto_quotes = self.get_crypto_quotations(crypto_assets).await;
		match crypto_quotes {
			Ok(crypto_quotes) => quotations.extend(crypto_quotes),
			Err(e) => log::error!("Error getting crypto quotations: {}", e),
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
	) -> Result<Vec<Quotation>, CoingeckoError> {
		let quotations = self.coingecko_price_api.get_prices(assets).await?;
		Ok(quotations)
	}

	async fn get_custom_quotations(
		&self,
		assets: Vec<&AssetSpecifier>,
	) -> Result<Vec<Quotation>, CustomError> {
		let mut quotations = Vec::new();
		for asset in assets {
			let quotation = CustomPriceApi::get_price(asset).await?;
			quotations.push(quotation);
		}
		Ok(quotations)
	}
}
