use std::string::ToString;
use async_trait::async_trait;
use crate::api::error::CustomError;
use crate::api::Quotation;
use crate::AssetSpecifier;
use chrono::prelude::*;
use graphql_client::{GraphQLQuery, Response};
use rust_decimal::Decimal;

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
		let compatible_apis: Vec<Box<dyn AssetCompatibility>> = vec![Box::new(AmpePriceView)];

		for api in compatible_apis {
			if api.supports(asset) {
				return Some(api);
			}
		}

		None
	}
}

#[async_trait]
trait AssetCompatibility: Send {
	fn supports(&self, asset: &AssetSpecifier) -> bool;

	async fn get_price(&self, asset: &AssetSpecifier) -> Result<Quotation, CustomError>;
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

#[async_trait]
impl AssetCompatibility for AmpePriceView {
	fn supports(&self, asset: &AssetSpecifier) -> bool {
		asset.blockchain == "Amplitude" && asset.symbol == "AMPE"
	}

	async fn get_price(&self, _asset: &AssetSpecifier) -> Result<Quotation, CustomError> {
		AmpePriceView::get_price().await
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
			supply: Decimal::from(0),
			time: Utc::now().timestamp().unsigned_abs(),
		})
	}
}

#[cfg(test)]
mod tests {
	use crate::api::custom::AmpePriceView;
	use crate::api::custom::CustomPriceApi;
	use crate::AssetSpecifier;

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
