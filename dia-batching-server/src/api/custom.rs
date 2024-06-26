use std::error::Error;
use std::string::ToString;

use chrono::prelude::*;
use graphql_client::{GraphQLQuery, Response};
use rust_decimal::Decimal;
use serde::Deserialize;

use crate::api::Quotation;
use crate::AssetSpecifier;

pub const CUSTOM_ASSETS: Vec<AssetSpecifier> = vec![AssetSpecifier {
    blockchain: "Amplitude".to_string(),
    symbol: "AMPE".to_string(),
}];

pub struct CustomPriceApi;

impl CustomPriceApi {
    pub async fn get_price(asset: &AssetSpecifier) -> Result<Quotation, Box<dyn Error + Send + Sync>> {
        if AmpePriceView::supports(asset) {
            return AmpePriceView::get_price().await;
        }

        Err("Unsupported asset".into())
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
    async fn get_price() -> Result<Quotation, Box<dyn Error + Send + Sync>> {
        let request_body = AmpePriceView::build_query(ampe_price_view::Variables {});

        let client = reqwest::Client::new();
        let response = client.post(Self::URL).json(&request_body).send().await?;
        let response_body: Response<ampe_price_view::ResponseData> = response.json().await?;

        let response_data = response_body.data.ok_or("No price found for AMPE")?;
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

    #[tokio::test]
    async fn test_ampe_price() {
        let quoted_asset = QuotedAsset {
            asset: Asset {
                symbol: AmpePriceView::SYMBOL.to_string(),
                name: "".to_string(),
                address: "".to_string(),
                decimals: 0,
                blockchain: AmpePriceView::BLOCKCHAIN.to_string(),
            },
            volume: 0.0,
        };
        let price = Dia.get_quotation(&quoted_asset).await.expect("should return a quotation");

        assert_eq!(price.symbol, quoted_asset.asset.symbol);
        assert_eq!(price.blockchain.expect("should return ampe"), quoted_asset.asset.blockchain);
        assert!(price.price < Decimal::new(1, 0));
    }

    #[tokio::test]
    async fn test_fiat_price() {
        let quoted_asset = QuotedAsset {
            asset: Asset {
                symbol: "USD-USD".to_string(),
                name: "".to_string(),
                address: "".to_string(),
                decimals: 0,
                blockchain: "fiat".to_string(),
            },
            volume: 0.0,
        };
        let price = Dia.get_quotation(&quoted_asset).await.expect("should return a quotation");

        assert_eq!(price.symbol, quoted_asset.asset.symbol);
        assert_eq!(price.price, Decimal::new(1, 0));
    }
}
