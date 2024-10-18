use crate::api::custom::AssetCompatibility;
use crate::api::error::CustomError;
use crate::types::{AssetSpecifier, Quotation};
use async_trait::async_trait;
use chrono::Utc;
use graphql_client::{GraphQLQuery, Response};
use rust_decimal::prelude::Zero;
use rust_decimal::Decimal;

// The blockchain and symbol for the Amplitude native token
// These are the expected values for the asset specifier.
const BLOCKCHAIN: &'static str = "Amplitude";
const SYMBOL: &'static str = "AMPE";

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
		asset.blockchain.to_uppercase() == BLOCKCHAIN.to_uppercase() && asset.symbol.to_uppercase() == SYMBOL.to_uppercase()
	}

	async fn get_price(&self, _asset: &AssetSpecifier) -> Result<Quotation, CustomError> {
		AmpePriceView::get_price().await
	}
}

impl AmpePriceView {
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
			symbol: SYMBOL.to_string(),
			name: SYMBOL.to_string(),
			blockchain: Some(BLOCKCHAIN.to_string()),
			price,
			supply: Decimal::zero(),
			time: Utc::now().timestamp().unsigned_abs(),
		})
	}
}

#[cfg(test)]
mod tests {
	use super::{AmpePriceView, BLOCKCHAIN, SYMBOL};
	use crate::api::custom::CustomPriceApi;
	use crate::AssetSpecifier;

	#[tokio::test]
	async fn test_get_ampe_price_from_api() {
		let asset =
			AssetSpecifier { blockchain: "Amplitude".to_string(), symbol: "AMPE".to_string() };

		let ampe_quotation = CustomPriceApi::new()
			.get_price(&asset)
			.await
			.expect("should return a quotation");

		assert_eq!(ampe_quotation.symbol, asset.symbol);
		assert_eq!(ampe_quotation.name, asset.symbol);
		assert_eq!(ampe_quotation.blockchain.expect("should return something"), asset.blockchain);
		assert!(ampe_quotation.price > 0.into());
	}

	#[tokio::test]
	async fn test_get_ampe_price_from_view() {
		let ampe_quotation = AmpePriceView::get_price().await.expect("should return a quotation");

		assert_eq!(ampe_quotation.symbol, SYMBOL);
		assert_eq!(ampe_quotation.name, SYMBOL);
		assert_eq!(ampe_quotation.blockchain.expect("should return something"), BLOCKCHAIN);
		assert!(ampe_quotation.price > 0.into());
	}
}
