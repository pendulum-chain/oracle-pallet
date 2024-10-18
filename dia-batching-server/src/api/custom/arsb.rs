use crate::api::binance::BinancePriceApi;
use crate::api::custom::AssetCompatibility;
use crate::api::error::CustomError;
use crate::types::{AssetSpecifier, Quotation};
use async_trait::async_trait;
use chrono::Utc;
use rust_decimal::prelude::Zero;
use rust_decimal::Decimal;

// The blockchain and symbol for the Argentinian blue dollar.
// These are the expected values for the asset specifier.
pub const BLOCKCHAIN: &'static str = "FIAT";
pub const SYMBOL: &'static str = "ARS-USD";

/// Returns the price for the Argentinian blue dollar.
/// The price is fetched from Binance as binance has a very accurate price for the blue dollar.
pub struct ArsBluePriceView {
	binance_price_api: BinancePriceApi,
}

#[async_trait]
impl AssetCompatibility for ArsBluePriceView {
	fn supports(&self, asset: &AssetSpecifier) -> bool {
		asset.blockchain.to_uppercase() == BLOCKCHAIN.to_uppercase() && asset.symbol.to_uppercase() == SYMBOL.to_uppercase()
	}

	async fn get_price(&self, _asset: &AssetSpecifier) -> Result<Quotation, CustomError> {
		self.get_price().await
	}
}

impl ArsBluePriceView {
	pub fn new() -> Self {
		ArsBluePriceView { binance_price_api: BinancePriceApi::new() }
	}

	async fn get_price(&self) -> Result<Quotation, CustomError> {
		// Only the symbol is needed to get the price with the BinancePriceApi client.
		let asset =
			AssetSpecifier { blockchain: "Binance".to_string(), symbol: "USDTARS".to_string() };

		// The direction of the conversion is ARS -> USDT
		let usdt_ars_price = self
			.binance_price_api
			.get_price(&asset)
			.await
			.map_err(|e| CustomError(format!("Failed to get price: {:?}", e)))?
			.price;

		// We need to convert the price from USD -> ARS though so we invert
		let ars_usdt_price =
			Decimal::from(1).checked_div(usdt_ars_price).unwrap_or(Decimal::zero());

		Ok(Quotation {
			symbol: SYMBOL.to_string(),
			name: SYMBOL.to_string(),
			blockchain: Some(BLOCKCHAIN.to_string()),
			price: ars_usdt_price,
			supply: Decimal::zero(),
			time: Utc::now().timestamp().unsigned_abs(),
		})
	}
}

#[cfg(test)]
mod tests {
	use super::{ArsBluePriceView, BLOCKCHAIN, SYMBOL};

	#[tokio::test]
	async fn test_get_arsb_price_from_view() {
		let arsb_quotation =
			ArsBluePriceView::new().get_price().await.expect("should return a quotation");

		assert_eq!(arsb_quotation.symbol, SYMBOL);
		assert_eq!(arsb_quotation.name, SYMBOL);
		assert_eq!(arsb_quotation.blockchain.expect("should return something"), BLOCKCHAIN);
		assert!(arsb_quotation.price > 0.into());
	}
}
