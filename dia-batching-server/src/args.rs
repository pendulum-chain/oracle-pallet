use clap::Parser;

fn parse_currency_vec(src: &str) -> SupportedCurrencies {
	let mut vec = Vec::new();
	for s in src.split(',') {
		vec.push(s.to_string());
	}
	SupportedCurrencies(vec)
}

// We need the extra struct to be able to parse the currencies to a Vec
#[derive(Clone, Debug)]
pub struct SupportedCurrencies(pub Vec<String>);

#[derive(Parser, Debug, Clone)]
#[clap(name = "dia-batching-server")]
pub struct DiaApiArgs {
	/// Iteration duration after one batch of requests
	#[clap(short, long, default_value = "10")]
	pub update_interval_seconds: u64,

	/// Currencies to support
	/// Each currency needs to have the format <blockchain>:<symbol>
	/// Fiat currencies need to have the format FIAT:<from>-<to>
	#[clap(short, long,
        parse(from_str = parse_currency_vec),
        default_value = "FIAT:USD-USD,FIAT:EUR-USD,FIAT:BRL-USD,FIAT:AUD-USD,FIAT:NGN-USD,FIAT:TZS-USD,Pendulum:PEN,Amplitude:AMPE,Polkadot:DOT,Kusama:KSM,Astar:ASTR,Bifrost:BNC,Bifrost:vDOT,HydraDX:HDX,Moonbeam:GLMR,Polkadex:PDEX,Stellar:XLM"
    )]
	pub supported_currencies: SupportedCurrencies,

	/// The port to run the server on
	#[clap(short, long, default_value = "8070")]
	pub port: u16,

	#[clap(flatten)]
	pub coingecko_config: CoingeckoConfig,
	#[clap(flatten)]
	pub polygon_config: PolygonConfig,
}

#[derive(Parser, Debug, Clone)]
pub struct CoingeckoConfig {
	/// The API key for CoinGecko.
	#[clap(long, env = "CG_API_KEY")]
	pub cg_api_key: Option<String>,

	/// The host URL for CoinGecko.
	/// Defaults to the CoinGecko Pro API.
	#[clap(long, env = "CG_HOST_URL", default_value = "https://pro-api.coingecko.com/api/v3")]
	pub cg_host_url: String,
}

#[derive(Parser, Debug, Clone)]
pub struct PolygonConfig {
	/// The API key for Polygon.io
	#[clap(long, env = "PG_API_KEY")]
	pub pg_api_key: Option<String>,

	/// The host URL for the Polygon.io API.
	#[clap(long, env = "PG_HOST_URL", default_value = "https://api.polygon.io/v1")]
	pub pg_host_url: String,
}
