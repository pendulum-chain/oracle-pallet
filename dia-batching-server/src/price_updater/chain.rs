use alloy::{
	primitives::{Address, Bytes, Uint, U256},
	providers::{ProviderBuilder, Provider},
	network::EthereumWallet,
	signers::local::PrivateKeySigner,
	sol,
};
use reqwest::Url;
use log::{error, info, warn};
use std::collections::HashMap;
use std::convert::TryFrom;
use std::sync::{Arc, Mutex};
use std::{error::Error};
use std::str::FromStr;
use crate::types::CoinInfo;

type U48 = Uint<48, 1>;
type U56 = Uint<56, 1>;

pub struct NonceManager {
	nonce: Mutex<u64>,
}

impl NonceManager {
	pub fn new(initial_nonce: u64) -> Self {
		Self {
			nonce: Mutex::new(initial_nonce),
		}
	}

	pub fn next_nonce(&self) -> u64 {
		let mut nonce = self.nonce.lock().unwrap();
		let current = *nonce;
		*nonce += 1;
		current
	}
}

pub async fn initialize_nonce_manager() -> Result<Arc<NonceManager>, Box<dyn Error + Send + Sync + 'static>> {
	let private_key_str = std::env::var("PRIVATE_KEY").map_err(|_| "PRIVATE_KEY not set")?;
	let rpc_url = std::env::var("RPC_URL").map_err(|_| "RPC_URL not set")?;
	let signer = PrivateKeySigner::from_str(&private_key_str)?;
	let wallet_address = signer.address();
	let temp_provider = ProviderBuilder::new()
		.on_http(Url::parse(&rpc_url).expect("Invalid RPC_URL"));
	let initial_nonce = temp_provider.get_transaction_count(wallet_address).await?;
	Ok(Arc::new(NonceManager::new(initial_nonce)))
}

// ── Solidity contracts ─────────────────────────────────────────────────────────

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

#[derive(Debug)]
pub struct PriceData {
	pub usdc: f64,
	pub eurc: f64,
}

pub async fn update_dark_oracle_contract_prices(
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

	let symbol_to_price: HashMap<&str, u128> =
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

pub async fn update_pyth_contract_prices(
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
	let update_fee = pyth_adapter.getUpdateFee(bytes_data.clone()).call().await?.updateFee_;
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

	Ok(())
}