use std::error::Error;
use std::string::ToString;

use chrono::prelude::*;
use graphql_client::{GraphQLQuery, Response};
use rust_decimal::Decimal;
use serde::Deserialize;
use crate::api::error::CustomError;
use crate::api::Quotation;
use crate::AssetSpecifier;

pub struct CustomPriceApi;

impl CustomPriceApi {
    pub async fn get_price(asset: &AssetSpecifier) -> Result<Quotation, CustomError> {
        if AmpePriceView::supports(asset) {
            return AmpePriceView::get_price().await;
        }

        Err(CustomError(format!("Unsupported asset: {:?}", asset)))
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
        asset.blockchain == "CRYPTO" && asset.symbol == "AMPE"
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
        let response = client.post(Self::URL).json(&request_body).send().await.map_err(
            |e| CustomError(format!("Failed to send request: {:?}", e))
        )?;
        let response_body: Response<ampe_price_view::ResponseData> = response.json().await.map_err(
            |e| CustomError(format!("Failed to parse response: {:?}", e))
        )?;

        let response_data = response_body.data.ok_or(CustomError("No price found for AMPE".to_string()))?;
        let price = response_data.bundle_by_id.eth_price;

        Ok(Quotation {
            symbol: Self::SYMBOL.to_string(),
            name: Self::BLOCKCHAIN.to_string(),
            blockchain: Some(Self::BLOCKCHAIN.to_string()),
            price,
            time: Utc::now(),
        })
    }
}

#[cfg(test)]
mod tests {
    use rust_decimal::Decimal;
    use crate::api::PriceApiImpl;
    use crate::api::PriceApi;
    use crate::AssetSpecifier;

    #[tokio::test]
    async fn test_ampe_price() {
        let asset = AssetSpecifier {
            blockchain: "Amplitude".to_string(),
            symbol: "AMPE".to_string(),
        };
        let assets = vec![&asset];

        let price_api = PriceApiImpl::new();

        let prices = price_api.get_quotations(assets).await.expect("should return a quotation");

        assert!(!prices.is_empty());
        let ampe_quote = prices.first().expect("should return a price").clone();

        assert_eq!(ampe_quote.symbol, "AMPE");
        assert_eq!(ampe_quote.blockchain.expect("should return something"), "Amplitude");
        assert!(ampe_quote.price > Decimal::new(0, 0));
    }
}
