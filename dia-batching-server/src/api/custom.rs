use std::string::ToString;

use crate::api::error::CustomError;
use crate::api::Quotation;
use crate::AssetSpecifier;
use chrono::prelude::*;
use graphql_client::{GraphQLQuery, Response};
use rust_decimal::Decimal;

pub struct CustomPriceApi;

impl CustomPriceApi {
	pub async fn get_price(asset: &AssetSpecifier) -> Result<Quotation, CustomError> {
		if !Self::is_supported(asset) {
			return Err(CustomError(format!("Unsupported asset: {:?}", asset)));
		}

		// Go through all the custom APIs and check if the asset is supported by any of them
		if AmpePriceView::supports(asset) {
			return AmpePriceView::get_price().await;
		} else {
			Err(CustomError("Unsupported asset".to_string()))
		}
	}

	pub fn is_supported(asset: &AssetSpecifier) -> bool {
		let custom_assets: Vec<AssetSpecifier> = vec![AssetSpecifier {
			blockchain: "Amplitude".to_string(),
			symbol: "AMPE".to_string(),
		}];

		custom_assets.iter().any(|supported_asset| supported_asset == asset)
	}
}

trait AssetCompatibility {
	fn supports(asset: &AssetSpecifier) -> bool;
}

// The paths are relative to the directory where your `Cargo.toml` is located.
// Both json and the GraphQL schema language are supported as sources for the schema
#[derive(GraphQLQuery)]
#[graphql(
	schema_path = "resources/ampe_schema.graphql",
	query_path = "resources/ampe_query.graphql",
	response_derives = "Debug"
)]
pub struct AmpePriceView;

impl AssetCompatibility for AmpePriceView {
	fn supports(asset: &AssetSpecifier) -> bool {
		asset.blockchain == "Amplitude" && asset.symbol == "AMPE"
	}
}

impl AmpePriceView {
	const SYMBOL: &'static str = "AMPE";
	const BLOCKCHAIN: &'static str = "Amplitude";
	const URL: &'static str = "https://squid.subsquid.io/amplitude-squid/graphql";

	/// Response:
	/// ```ignore
	/// Response {
	///     data: Some(
	///         ResponseData {
	///             bundle_by_id: AmpeViewBundleById {
	///                 eth_price: 0.003482,
	///             },
	///         },
	///     ),
	///     errors: None,
	///     extensions: None,
	/// }
	/// ```
	/// Returns the value of `eth_price`, which is the price of AMPE.
	async fn get_price() -> Result<Quotation, CustomError> {
		let request_body = AmpePriceView::build_query(ampe_price_view::Variables {});

		let client = reqwest::Client::new();
		let response = client
			.post(Self::URL)
			.json(&request_body)
			.send()
			.await
			.map_err(|e| CustomError(format!("Failed to send request: {:?}", e)))?;
		let response_body: Response<ampe_price_view::ResponseData> = response
			.json()
			.await
			.map_err(|e| CustomError(format!("Failed to parse response: {:?}", e)))?;

		let response_data =
			response_body.data.ok_or(CustomError("No price found for AMPE".to_string()))?;
		let price = response_data.bundle_by_id.eth_price;

		Ok(Quotation {
			symbol: Self::SYMBOL.to_string(),
			name: Self::SYMBOL.to_string(),
			blockchain: Some(Self::BLOCKCHAIN.to_string()),
			price,
			time: Utc::now(),
		})
	}
}

#[cfg(test)]
mod tests {
	use crate::api::custom::AmpePriceView;
	use crate::api::custom::CustomPriceApi;
	use crate::api::PriceApi;
	use crate::AssetSpecifier;
	use rust_decimal::Decimal;

	#[tokio::test]
	async fn test_get_ampe_price_from_api() {
		let asset =
			AssetSpecifier { blockchain: "Amplitude".to_string(), symbol: "AMPE".to_string() };

		let ampe_quotation =
			CustomPriceApi::get_price(&asset).await.expect("should return a quotation");

		assert_eq!(ampe_quotation.symbol, asset.symbol);
		assert_eq!(ampe_quotation.name, asset.symbol);
		assert_eq!(ampe_quotation.blockchain.expect("should return something"), asset.blockchain);
		assert!(ampe_quotation.price > 0.into());
	}

	#[tokio::test]
	async fn test_get_ampe_price_from_view() {
		let ampe_quotation = AmpePriceView::get_price().await.expect("should return a quotation");

		assert_eq!(ampe_quotation.symbol, AmpePriceView::SYMBOL);
		assert_eq!(ampe_quotation.name, AmpePriceView::SYMBOL);
		assert_eq!(
			ampe_quotation.blockchain.expect("should return something"),
			AmpePriceView::BLOCKCHAIN
		);
		assert!(ampe_quotation.price > 0.into());
	}
}
