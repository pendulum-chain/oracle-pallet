use std::collections::HashSet;
use crate::handlers::currencies_post;
use crate::storage::CoinInfoStorage;
use std::error::Error;

use crate::args::DiaApiArgs;
use actix_web::{web, App, HttpServer};
use log::error;
use std::sync::Arc;
use structopt::StructOpt;
use crate::api::PriceApiImpl;

mod api;
mod args;
mod handlers;
mod price_updater;
mod storage;

/// This struct is used to identify a specific asset.
#[derive(PartialEq, Eq, Hash)]
pub struct AssetSpecifier {
    blockchain: String,
    symbol: String,
}

#[actix_web::main]
async fn main() -> Result<(), Box<dyn Error + Send + Sync>> {
    pretty_env_logger::init();

    let args: DiaApiArgs = DiaApiArgs::from_args();
    let storage = Arc::new(CoinInfoStorage::default());
    let data = web::Data::from(storage.clone());

    let supported_currencies = args.supported_currencies.0;
    let supported_currencies: HashSet<AssetSpecifier> =
        supported_currencies.iter().filter_map(|asset| {
            let (blockchain, symbol) = asset.trim().split_once(":").or_else(|| {
                error!("Invalid asset '{}' – every asset needs to have the form <blockchain>:<symbol>", asset);
                None
            })?;

            Some(AssetSpecifier { blockchain: blockchain.into(), symbol: symbol.into() })
        }).collect();

    if supported_currencies.is_empty() {
        error!("No supported currencies provided. Exiting.");
        return Ok(());
    }

    let price_api = PriceApiImpl::new();

    price_updater::run_update_prices_loop(
        storage,
        supported_currencies,
        std::time::Duration::from_secs(args.update_interval_seconds),
        price_api
    ).await?;

    println!("Running dia-batching-server... (Press CTRL+C to quit)");
    HttpServer::new(move || App::new().app_data(data.clone()).service(currencies_post))
        .on_connect(|_, _| println!("Serving Request"))
        .bind("0.0.0.0:8070")?
        .run()
        .await?;

    Ok(())
}
