use crate::handlers::currencies_post;
use crate::storage::CoinInfoStorage;
use std::collections::HashSet;
use std::error::Error;

use crate::api::PriceApiImpl;
use crate::args::DiaApiArgs;
use crate::types::AssetSpecifier;
use actix_cors::Cors;
use actix_web::{web, App, HttpServer};
use clap::Parser;
use log::error;
use std::sync::Arc;

mod api;
mod args;
mod handlers;
mod price_updater;
mod storage;
mod types;

#[actix_web::main]
async fn main() -> Result<(), Box<dyn Error + Send + Sync>> {
	pretty_env_logger::init();

	let args: DiaApiArgs = DiaApiArgs::parse();
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

	let port = args.port;
	println!("Running dia-batching-server on port {port}... (Press CTRL+C to quit)");
	HttpServer::new(move || {
		let cors = Cors::default().allowed_origin("https://portal.pendulumchain.org")
			.allowed_methods(vec!["POST"])
			.allowed_headers(vec!["Content-Type"])
			.max_age(3600);
		App::new().app_data(data.clone()).wrap(cors).service(currencies_post)
	})
	.on_connect(|_, _| println!("Serving Request"))
	.bind(format!("0.0.0.0:{port}"))?
	.run()
	.await?;

	Ok(())
}
