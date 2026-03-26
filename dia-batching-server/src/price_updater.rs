use crate::api::PriceApi;
use crate::storage::CoinInfoStorage;
use crate::types::{CoinInfo, Quotation};
use crate::AssetSpecifier;
use alloy::{
	primitives::{Address, Bytes, Uint, U256},
	providers::{ProviderBuilder, Provider},
	network::EthereumWallet,
	signers::local::PrivateKeySigner,
	sol,
};
use reqwest::Url;
use log::{error, info, warn};
use rust_decimal::prelude::ToPrimitive;
use rust_decimal::Decimal;
use serde::Deserialize;
use std::collections::HashSet;
use std::convert::TryFrom;
use std::fmt::{Display, Formatter};
use std::str::FromStr;
use std::sync::{Arc, Mutex};
use std::{error::Error};

type U48 = Uint<48, 1>;
type U56 = Uint<56, 1>;

const BIPS_DIVISOR: u64 = 10000;

struct NonceManager {
    nonce: Mutex<u64>,
}

impl NonceManager {
    fn new(initial_nonce: u64) -> Self {
        Self {
            nonce: Mutex::new(initial_nonce),
        }
    }

    fn next_nonce(&self) -> u64 {
        let mut nonce = self.nonce.lock().unwrap();
        let current = *nonce;
        *nonce += 1;
        current
    }
}

// ── Pyth Hermes API types ─────────────────────────────────────────────────────

const USDC_PRICE_FEED_ID: &str =
	"eaa020c61cc479712813461ce153894a96a6c00b21ed0cfc2798d1f9a9e9c94a";
const EURC_PRICE_FEED_ID: &str =
	"76fa85158bf14ede77087fe3ae472f66213f6ea2f5b411cb2de472794990fa5c";

#[derive(Debug, Deserialize)]
pub struct HermesPrice {
	pub price: String,
	pub conf: String,
	pub expo: i32,
	pub publish_time: u64,
}

#[derive(Debug, Deserialize)]
pub struct HermesParsedEntry {
	pub id: String,
	pub price: HermesPrice,
	pub ema_price: HermesPrice,
}

#[derive(Debug, Deserialize)]
struct HermesBinary {
	data: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct HermesResponse {
	binary: HermesBinary,
	parsed: Vec<HermesParsedEntry>,
}

#[derive(Debug)]
pub struct PriceData {
	pub usdc: f64,
	pub eurc: f64,
}

// ── Solidity contracts ────────────────────────────────────────────────────────

sol! {
	#[sol(rpc)]
	contract DarkOracle {
		function updatePriceFeeds(uint48[5] _prices, uint56 _timestamp) external returns (bool success_);
	}
}

sol! {
	#[sol(rpc)]
	contract PythAdapter {
		function getUpdateFee(bytes[] _updateData) external view returns (uint256 updateFee_);
		function updatePriceFeeds(bytes[] _priceUpdateData) external payable returns (bool success_);
	}
}


struct PythPriceUpdater {
	update_interval: std::time::Duration,
	last_update: Option<std::time::Instant>,
}

impl PythPriceUpdater {
	fn new(update_interval: std::time::Duration) -> Self {
		Self { update_interval, last_update: None }
	}

	async fn run_update_pyth_prices(
		&mut self,
		nonce_manager: Arc<NonceManager>,
	) -> Result<PriceData, Box<dyn Error + Send + Sync + 'static>> {
		let should_update_contract = match self.last_update {
			None => true,
			Some(t) => t.elapsed() >= self.update_interval,
		};

		let api_url = format!(
			"https://hermes.pyth.network/v2/updates/price/latest?ids%5B%5D={}&ids%5B%5D={}",
			USDC_PRICE_FEED_ID, EURC_PRICE_FEED_ID
		);

		debug!("Fetching Pyth prices from Hermes API...");
		let response = reqwest::get(&api_url).await?;
		if !response.status().is_success() {
			return Err(format!("Hermes API request failed: {}", response.status()).into());
		}

		let data: HermesResponse = response.json().await?;


		let mut usdc_price = None;
		let mut eurc_price = None;
		for entry in &data.parsed {
			let price_val = entry.price.price.parse::<f64>().map_err(|e| format!("Failed to parse price: {}", e))?;
			let actual_price = price_val * 10f64.powi(entry.price.expo);
			if entry.id == USDC_PRICE_FEED_ID {
				usdc_price = Some(actual_price);
			} else if entry.id == EURC_PRICE_FEED_ID {
				eurc_price = Some(actual_price);
			}
		}
		let usdc = usdc_price.ok_or("USDC price not found")?;
		let eurc = eurc_price.ok_or("EURC price not found")?;

		if should_update_contract {
			let update_data: Vec<String> =
				data.binary.data.iter().map(|hex| format!("0x{}", hex)).collect();
			if let Err(e) = update_pyth_contract_prices(&update_data, nonce_manager.clone()).await {
				error!("Failed to update Pyth contract prices: {:?}", e);
			} else {
				info!("Pyth prices updated on-chain ✓");
				self.last_update = Some(std::time::Instant::now());
			}
		}

		Ok(PriceData { usdc, eurc })
	}
}

// ── Public entry point ────────────────────────────────────────────────────────

pub async fn run_update_prices_loop<T>(
	storage: Arc<CoinInfoStorage>,
	supported_currencies: HashSet<AssetSpecifier>,
	update_interval: std::time::Duration,
	pyth_update_interval: std::time::Duration,
	divergence_threshold_bp: u64,
	api: T,
) -> Result<(), Box<dyn Error + Send + Sync + 'static>>
where
	T: PriceApi + Send + Sync + 'static,
{
	let mut pyth_updater = PythPriceUpdater::new(pyth_update_interval);

	// Initialize nonce manager
	let private_key_str = std::env::var("PRIVATE_KEY").map_err(|_| "PRIVATE_KEY not set")?;
	let rpc_url = std::env::var("RPC_URL").map_err(|_| "RPC_URL not set")?;
	let signer = PrivateKeySigner::from_str(&private_key_str)?;
	let wallet_address = signer.address();
	let temp_provider = ProviderBuilder::new()
		.on_http(Url::parse(&rpc_url).expect("Invalid RPC_URL"));
	let initial_nonce = temp_provider.get_transaction_count(wallet_address).await?;
	let nonce_manager = Arc::new(NonceManager::new(initial_nonce));
	info!("Initialized nonce manager with nonce: {}", initial_nonce);

	loop {
		let start = tokio::time::Instant::now();
		let coins = Arc::clone(&storage);
		update_prices(coins, &supported_currencies, &api, &mut pyth_updater, &nonce_manager, divergence_threshold_bp).await;
		let elapsed = start.elapsed();
		let target_duration = std::time::Duration::from_secs(2);
		if elapsed < target_duration {
			let sleep_duration = target_duration - elapsed;
			tokio::time::sleep(sleep_duration).await;
		}
	}
}

fn convert_to_coin_info(value: Quotation) -> Result<CoinInfo, Box<dyn Error + Sync + Send>> {
	let Quotation { name, symbol, blockchain, price, time, supply } = value;

	let price = convert_decimal_to_u128(&price)?;
	let supply = convert_decimal_to_u128(&supply)?;

	let coin_info = CoinInfo {
		name: name.into(),
		symbol: symbol.into(),
		blockchain: blockchain.unwrap_or("FIAT".to_string()).into(),
		price,
		last_update_timestamp: time,
		supply,
	};

	Ok(coin_info)
}

async fn update_pyth_contract_prices(
	update_data: &[String],
	nonce_manager: Arc<NonceManager>,
) -> Result<(), Box<dyn Error + Send + Sync + 'static>> {
	let private_key_str = std::env::var("PRIVATE_KEY").map_err(|_| "PRIVATE_KEY not set")?;
	let pyth_adapter_address = std::env::var("PYTH_ADAPTER_ADDRESS")
		.map_err(|_| "PYTH_ADAPTER_ADDRESS not set")?;
	let rpc_url = std::env::var("RPC_URL").map_err(|_| "RPC_URL not set")?;

	warn!("Connecting to Ethereum provider at {} for Pyth update", rpc_url);
	warn!("Using PythAdapter contract address: {}", pyth_adapter_address);

	let signer = PrivateKeySigner::from_str(&private_key_str)?;
	let wallet = EthereumWallet::from(signer);

	let provider = ProviderBuilder::new()
		.with_recommended_fillers()
		.wallet(wallet)
		.on_http(Url::parse(&rpc_url).expect("Invalid RPC_URL"));

	let addr = pyth_adapter_address.parse::<Address>()?;
	let pyth_adapter = PythAdapter::new(addr, &provider);

	let bytes_data: Vec<Bytes> = update_data
		.iter()
		.map(|hex_str| {
			let stripped = hex_str.strip_prefix("0x").unwrap_or(hex_str);
			Bytes::from(hex::decode(stripped).unwrap_or_default())
		})
		.collect();

	info!("Prepared price update data for Pyth contract: {:?}", bytes_data);


	// Get the required fee
	let update_fee: U256 =
		pyth_adapter.getUpdateFee(bytes_data.clone()).call().await?.updateFee_;
	info!("Pyth update fee: {} wei", update_fee);

	// Estimate and set higher priority fee
	let fees = provider.estimate_eip1559_fees(None).await?;
	let priority_fee = fees.max_priority_fee_per_gas * (3u128 / 2u128);
	info!("Pyth priority fee: {} wei", priority_fee);

	// Send the update transaction
	let nonce = nonce_manager.next_nonce();
	let call = pyth_adapter
		.updatePriceFeeds(bytes_data)
		.value(update_fee)
		.gas(10_000_000)
		.max_priority_fee_per_gas(priority_fee)
		.nonce(nonce);
	let tx = call.send().await?;

	warn!("Pyth updatePriceFeeds tx sent");
	info!("Pyth updatePriceFeeds tx hash: {:?}", tx.tx_hash());
	// let receipt = tx.get_receipt().await?;

	// if receipt.status() {
	// 	info!("Pyth contract prices updated successfully ✓");
	// } else {
	// 	warn!("Pyth contract prices update failed – transaction reverted");
	// 	return Err("Pyth updatePriceFeeds transaction reverted".into());
	// }

	Ok(())
}

async fn update_prices<T>(
	coins: Arc<CoinInfoStorage>,
	supported_currencies: &HashSet<AssetSpecifier>,
	api: &T,
	pyth_updater: &mut PythPriceUpdater,
	nonce_manager: &Arc<NonceManager>,
	divergence_threshold_bp: u64,
) where
	T: PriceApi + Send + Sync + 'static,
{
	let mut currencies = vec![];

	let supported_currencies_vec = supported_currencies.iter().collect::<Vec<_>>();

	api.get_quotations(supported_currencies_vec)
		.await
		.into_iter()
		.for_each(|quotation| match convert_to_coin_info(quotation) {
			Ok(coin_info) => currencies.push(coin_info),
			Err(e) => error!("Error converting to CoinInfo: {:#?}", e),
		});

	coins.replace_currencies_by_symbols(currencies.clone());
	info!("Currencies Updated");


	let dark_oracle_fut = update_dark_oracle_contract_prices(&currencies, nonce_manager.clone());
	let pyth_fut = pyth_updater.run_update_pyth_prices(nonce_manager.clone());

	let (dark_oracle_result, pyth_result) = tokio::join!(dark_oracle_fut, pyth_fut);

	match &dark_oracle_result {
		Ok(prices) => info!("DarkOracle updated with prices: USDC={}, EURC={}", prices.usdc, prices.eurc),
		Err(e) => error!("Failed to update DarkOracle contract prices: {:?}", e),
	}
	match &pyth_result {
		Ok(prices) => info!("Pyth prices: USDC={}, EURC={}", prices.usdc, prices.eurc),
		Err(e) => error!("Failed to fetch/update Pyth prices: {:?}", e),
	}

	// Price divergence validation. Mirrors `_validatePrice` in SafePriceProvider.sol (DarkOracle contract)
	if let (Ok(dark_prices), Ok(pyth_prices)) = (&dark_oracle_result, &pyth_result) {

		// Validate EURC
		let fallback_price = pyth_prices.eurc;
		let price = dark_prices.eurc;
		let absolute_divergence = if fallback_price > price { fallback_price - price } else { price - fallback_price };
		let bp_divergence = (absolute_divergence * BIPS_DIVISOR as f64) / fallback_price;
		debug!("EURC price divergence: {:.2} bp (DarkOracle: {}, Pyth: {})", bp_divergence, price, fallback_price);
		if bp_divergence > divergence_threshold_bp as f64 {
			error!("EURC price divergence too high: {:.2} bp > {} bp", bp_divergence, divergence_threshold_bp);
		}
	}
}

async fn update_dark_oracle_contract_prices(
	currencies: &Vec<CoinInfo>,
	nonce_manager: Arc<NonceManager>,
) -> Result<PriceData, Box<dyn Error + Send + Sync + 'static>> {
	warn!("Starting contract price update...");
	let private_key_str = std::env::var("PRIVATE_KEY").map_err(|_| "PRIVATE_KEY not set")?;
	let contract_address =
		std::env::var("CONTRACT_ADDRESS").map_err(|_| "CONTRACT_ADDRESS not set")?;
	let rpc_url = std::env::var("RPC_URL").map_err(|_| "RPC_URL not set")?;

	warn!("Connecting to Ethereum provider at {}", rpc_url);
	warn!("Using contract address: {}", contract_address);

	let signer = PrivateKeySigner::from_str(&private_key_str)?;
	let wallet_address = signer.address();
	warn!("Using wallet address (public key): {}", wallet_address);

	let wallet = EthereumWallet::from(signer);

	let provider = ProviderBuilder::new()
		.with_recommended_fillers()
		.wallet(wallet)
		.on_http(Url::parse(&rpc_url).expect("Invalid RPC_URL"));

	let addr = contract_address.parse::<Address>()?;
	let oracle = DarkOracle::new(addr, provider.clone());

	let symbol_to_price: std::collections::HashMap<&str, u128> =
		currencies.iter().map(|c| (c.symbol.as_str(), c.price)).collect();

	let mut prices: [u64; 5] = [0; 5];

	// ETH index 0
	if let Some(eth_price) = symbol_to_price.get("ETH") {
		prices[0] = u64::try_from(*eth_price / 10_000_000_000)?;
	}

	// BTC index 1
	if let Some(btc_price) = symbol_to_price.get("BTC") {
		prices[1] = u64::try_from(*btc_price / 10_000_000_000)?;
	}

	// USDC index 2
	if let Some(usdc_price) = symbol_to_price.get("USDC") {
		prices[2] = u64::try_from(*usdc_price / 10_000_000_000)?;
	}

	// BRL index 3
	if let Some(brl_price) = symbol_to_price.get("BRL") {
		prices[3] = u64::try_from(*brl_price / 10_000_000_000)?;
	}

	// EURC index 4
	if let Some(eurc_price) = symbol_to_price.get("EURC") {
		prices[4] = u64::try_from(*eurc_price / 10_000_000_000)?;
	}

	let timestamp = u64::try_from(
		std::time::SystemTime::now()
			.duration_since(std::time::UNIX_EPOCH)?
			.as_millis(),
	)?;

	info!("Updating contract prices: {:?}", prices);
	info!("Timestamp: {:?}", timestamp);

	// Estimate and set higher priority fee
	let fees = provider.estimate_eip1559_fees(None).await?;
	let priority_fee = fees.max_priority_fee_per_gas * (3u128 / 2u128);
	info!("DarkOracle priority fee: {} wei", priority_fee);

	let nonce = nonce_manager.next_nonce();
	let call = oracle.updatePriceFeeds(prices, timestamp).gas(10_000_000).max_priority_fee_per_gas(priority_fee).nonce(nonce);
	warn!("Sending transaction with gas limit: 10,000,000");
	let tx = call.send().await?;
	warn!("Transaction sent");
	info!("DarkOracle updatePriceFeeds tx hash: {:?}", tx.tx_hash());

	let usdc_raw = symbol_to_price.get("USDC").ok_or("USDC price not found")?;
	let eurc_raw = symbol_to_price.get("EURC").ok_or("EURC price not found")?;
	let usdc_units = *usdc_raw as f64 / 10f64.powi(18);
	let eurc_units = *eurc_raw as f64 / 10f64.powi(18);

	Ok(PriceData { usdc: usdc_units, eurc: eurc_units })
}

#[derive(Debug)]
pub enum ConvertingError {
	DecimalTooLarge,
}

impl Display for ConvertingError {
	fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
		match self {
			ConvertingError::DecimalTooLarge => write!(f, "Decimal given is too large"),
		}
	}
}

impl Error for ConvertingError {}

fn convert_decimal_to_u128(input: &Decimal) -> Result<u128, ConvertingError> {
	let fract = (input.fract() * Decimal::from(1_000_000_000_000_000_000_u128))
		.to_u128()
		.ok_or(ConvertingError::DecimalTooLarge)?;
	let trunc = (input.trunc() * Decimal::from(1_000_000_000_000_000_000_u128))
		.to_u128()
		.ok_or(ConvertingError::DecimalTooLarge)?;

	Ok(trunc.saturating_add(fract))
}


