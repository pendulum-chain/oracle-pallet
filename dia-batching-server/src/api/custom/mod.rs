use crate::api::error::CustomError;
use crate::types::Quotation;
use crate::AssetSpecifier;
use async_trait::async_trait;
use std::string::ToString;

mod ampe;
mod arsb;

use ampe::AmpePriceView;
use arsb::ArsBluePriceView;

#[async_trait]
pub trait AssetCompatibility: Send {
	fn supports(&self, asset: &AssetSpecifier) -> bool;

	async fn get_price(&self, asset: &AssetSpecifier) -> Result<Quotation, CustomError>;
}

pub struct CustomPriceApi;

impl CustomPriceApi {
	pub async fn get_price(asset: &AssetSpecifier) -> Result<Quotation, CustomError> {
		let api =
			Self::get_supported_api(asset).ok_or(CustomError("Unsupported asset".to_string()))?;
		api.get_price(asset).await
	}

	pub fn is_supported(asset: &AssetSpecifier) -> bool {
		Self::get_supported_api(asset).is_some()
	}

	/// Iterates over all supported APIs and returns the first one that supports the given asset.
	fn get_supported_api(asset: &AssetSpecifier) -> Option<Box<dyn AssetCompatibility>> {
		let compatible_apis: Vec<Box<dyn AssetCompatibility>> =
			vec![Box::new(AmpePriceView), Box::new(ArsBluePriceView)];

		for api in compatible_apis {
			if api.supports(asset) {
				return Some(api);
			}
		}

		None
	}
}
