use std::error::Error;
use crate::api::Quotation;
use crate::AssetSpecifier;

pub struct CoingeckoPriceApi;

impl CoingeckoPriceApi {
    pub async fn get_prices(assets: Vec<&AssetSpecifier>) -> Result<Vec<Quotation>, Box<dyn Error + Send + Sync>> {
        Err("Unsupported asset".into())
    }
}
