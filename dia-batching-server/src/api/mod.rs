use crate::api::coingecko::CoingeckoPriceApi;
use crate::api::custom::CustomPriceApi;
use crate::api::error::{CoingeckoError, CustomError, PolygonError};
use crate::api::polygon::PolygonPriceApi;
use crate::args::{CoingeckoConfig, PolygonConfig};
use crate::types::Quotation;
use crate::AssetSpecifier;
use async_trait::async_trait;
use clap::Parser;

mod binance;
mod coingecko;
mod custom;
mod error;
mod polygon;

#[async_trait]
pub trait PriceApi {
	/// A method to get quotations for a list of assets.
	/// The method will return a list of quotations for the assets that are supported by the API.
	/// If an asset is not supported, the method will log an error and continue.
	/// The method will return an empty list if no quotations are available.
	async fn get_quotations(&self, assets: Vec<&AssetSpecifier>) -> Vec<Quotation>;
}

pub struct PriceApiImpl {
	coingecko_price_api: CoingeckoPriceApi,
	polygon_price_api: PolygonPriceApi,
	custom_price_api: CustomPriceApi,
}

impl PriceApiImpl {
	pub fn new() -> Self {
		Self {
			coingecko_price_api: CoingeckoPriceApi::new_from_config(CoingeckoConfig::parse()),
			polygon_price_api: PolygonPriceApi::new_from_config(PolygonConfig::parse()),
			custom_price_api: CustomPriceApi::new(),
		}
	}
}

#[async_trait]
impl PriceApi for PriceApiImpl {
	async fn get_quotations(&self, assets: Vec<&AssetSpecifier>) -> Vec<Quotation> {
		let mut quotations = Vec::new();

		// Split all assets into custom vs other assets. This is important because it could happen that
		// a custom asset is also supported by the Polygon or Coingecko API. In this case, we want to
		// use the custom asset and not the other API.
		let (custom_assets, assets): (Vec<&AssetSpecifier>, Vec<&AssetSpecifier>) =
			assets.into_iter().partition(|asset| self.custom_price_api.is_supported(asset));

		let (custom_quotes, custom_quote_errors) =
			self.get_custom_quotations(custom_assets.clone()).await;

		quotations.extend(custom_quotes);
		for error in custom_quote_errors {
			log::error!("Error getting custom quotation: {}", error);
		}

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

		let crypto_assets = assets
			.into_iter()
			.filter(|asset| CoingeckoPriceApi::is_supported(asset))
			.collect::<Vec<_>>();

		let crypto_quotes = self.get_crypto_quotations(crypto_assets).await;
		match crypto_quotes {
			Ok(crypto_quotes) => quotations.extend(crypto_quotes),
			Err(e) => log::error!("Error getting crypto quotations: {}", e),
		}

		quotations
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
	) -> (Vec<Quotation>, Vec<CustomError>) {
		let mut quotations = Vec::new();
		let mut errors = Vec::new();

		for asset in assets {
			let quotation_result = self.custom_price_api.get_price(asset).await;
			match quotation_result {
				Ok(quotation) => quotations.push(quotation),
				Err(e) => errors.push(e),
			};
		}

		(quotations, errors)
	}
}
