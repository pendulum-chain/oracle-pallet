use std::error::Error;
use crate::api::Quotation;
use crate::AssetSpecifier;

pub struct CoingeckoPriceApi;

impl CoingeckoPriceApi {
    pub async fn get_price(asset: &AssetSpecifier) -> Result<Quotation, Box<dyn Error + Send + Sync>> {
        Err("Unsupported asset".into())
    }
}
