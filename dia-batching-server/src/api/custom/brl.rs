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
pub const SYMBOL: &'static str = "BRL-USD";

// The basis point reduction for the price. This is used to adjust the price to a more favorable rate for the user.
const BPS_REDUCTION: u32 = 5;

/// Returns the price for the Argentinian blue dollar.
/// The price is fetched from Binance as binance has a very accurate price for the blue dollar.
pub struct BrlBluePriceView {
	binance_price_api: BinancePriceApi,
}

#[async_trait]
impl AssetCompatibility for BrlBluePriceView {
	fn supports(&self, asset: &AssetSpecifier) -> bool {
		asset.blockchain.to_uppercase() == BLOCKCHAIN.to_uppercase() && asset.symbol.to_uppercase() == SYMBOL.to_uppercase()
	}

	async fn get_price(&self, _asset: &AssetSpecifier) -> Result<Quotation, CustomError> {
		self.get_price().await
	}
}

impl BrlBluePriceView {
	pub fn new() -> Self {
		BrlBluePriceView { binance_price_api: BinancePriceApi::new() }
	}

	async fn get_price(&self) -> Result<Quotation, CustomError> {
		// Only the symbol is needed to get the price with the BinancePriceApi client.
		let asset =
			AssetSpecifier { blockchain: "Binance".to_string(), symbol: "USDTBRL".to_string() };

		// The direction of the conversion is BRL -> USDT
		let usdt_brl_price = self
			.binance_price_api
			.get_price(&asset)
			.await
			.map_err(|e| CustomError(format!("Failed to get price: {:?}", e)))?
			.price;


		// Apply the basis point reduction to the price (to the USDT->BRL price, resulting in a favorable buy price for the user)
		// let usdt_brl_price = usdt_brl_price
		// 	.checked_sub(usdt_brl_price * Decimal::from(BPS_REDUCTION) / Decimal::from(10_000))
		// 	.unwrap_or(Decimal::zero());

		// We need to convert the price from USD -> BRL though so we invert
		let brl_usdt_price =
			Decimal::from(1).checked_div(usdt_brl_price).unwrap_or(Decimal::zero());

		// Apply the basis point reduction to the price (to the BRL->USD price, resulting in a favorable sell price for the user)
		let brl_usdt_price = brl_usdt_price
			.checked_sub(brl_usdt_price * Decimal::from(BPS_REDUCTION) / Decimal::from(10_000))
			.unwrap_or(Decimal::zero());

		Ok(Quotation {
			symbol: SYMBOL.to_string(),
			name: SYMBOL.to_string(),
			blockchain: Some(BLOCKCHAIN.to_string()),
			price: brl_usdt_price,
			supply: Decimal::zero(),
			time: Utc::now().timestamp().unsigned_abs(),
		})
	}
}

#[cfg(test)]
mod tests {
	use crate::api::custom::CustomPriceApi;
	use crate::types::AssetSpecifier;
	use super::{BrlBluePriceView, BLOCKCHAIN, SYMBOL};

	#[tokio::test]
	async fn test_get_brl_price_from_api() {
		let asset =
			AssetSpecifier { blockchain: "FIAT".to_string(), symbol: "BRL-USD".to_string() };

		let brl_quotation = CustomPriceApi::new()
			.get_price(&asset)
			.await
			.expect("should return a quotation");

		assert_eq!(brl_quotation.symbol, asset.symbol);
		assert_eq!(brl_quotation.name, asset.symbol);
		assert_eq!(brl_quotation.blockchain.expect("should return something"), asset.blockchain);
		assert!(brl_quotation.price > 0.into());
	}

	#[tokio::test]
	async fn test_get_brlb_price_from_view() {
		let brlb_quotation =
			BrlBluePriceView::new().get_price().await.expect("should return a quotation");

		assert_eq!(brlb_quotation.symbol, SYMBOL);
		assert_eq!(brlb_quotation.name, SYMBOL);
		assert_eq!(brlb_quotation.blockchain.expect("should return something"), BLOCKCHAIN);
		assert!(brlb_quotation.price > 0.into());
	}
}
