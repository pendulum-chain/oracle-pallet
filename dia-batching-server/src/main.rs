use crate::handlers::currencies_post;
use crate::storage::CoinInfoStorage;
use std::collections::HashSet;
use std::error::Error;

use crate::api::PriceApiImpl;
use crate::args::DiaApiArgs;
use crate::types::AssetSpecifier;
use actix_web::{web, App, HttpServer};
use log::error;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use structopt::StructOpt;

mod api;
mod args;
mod handlers;
mod price_updater;
mod storage;
mod types;

#[actix_web::main]
async fn main() -> Result<(), Box<dyn Error + Send + Sync>> {
	pretty_env_logger::init();

	let args: DiaApiArgs = DiaApiArgs::from_args();
	let storage = Arc::new(CoinInfoStorage::default());
	let data = web::Data::from(storage.clone());

	let supported_currencies = args.supported_currencies.0;
	let supported_currencies: HashSet<AssetSpecifier> = supported_currencies
		.iter()
		.filter_map(|asset| {
			let (blockchain, symbol) =
				asset.trim().split_once(":").or_else(|| {
					error!("Invalid asset '{}' – every asset needs to have the form <blockchain>:<symbol>", asset);
					None
				})?;

			Some(AssetSpecifier { blockchain: blockchain.into(), symbol: symbol.into() })
		})
		.collect();

	if supported_currencies.is_empty() {
		error!("No supported currencies provided. Exiting.");
		return Ok(());
	}

	let price_api = PriceApiImpl::new();

	price_updater::run_update_prices_loop(
		storage,
		supported_currencies,
		std::time::Duration::from_secs(args.update_interval_seconds),
		price_api,
	)
	.await?;

	println!("Running dia-batching-server... (Press CTRL+C to quit)");
	HttpServer::new(move || App::new().app_data(data.clone()).service(currencies_post))
		.on_connect(|_, _| println!("Serving Request"))
		.bind("0.0.0.0:8070")?
		.run()
		.await?;

	Ok(())
}
