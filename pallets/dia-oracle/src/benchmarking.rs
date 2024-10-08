//! Benchmarking setup for dia-oracle
use super::*;

#[allow(unused)]
use crate::Pallet as DiaOracle;
use frame_benchmarking::{account, benchmarks, impl_benchmark_test_suite, whitelisted_caller};
use frame_system::RawOrigin;
use sp_std::{vec, vec::Vec};

benchmarks! {
	add_currency {
		let caller: T::AccountId = whitelisted_caller();
		DiaOracle::<T>::authorize_account(RawOrigin::Root.into(), caller.clone())?;
	}: _(RawOrigin::Signed(caller), vec![1,2,3],vec![1,2,3])

	remove_currency {
		let caller: T::AccountId = whitelisted_caller();
		DiaOracle::<T>::authorize_account(RawOrigin::Root.into(), caller.clone())?;
	} : _(RawOrigin::Signed(caller), vec![1,2,3], vec![1,2,3])

	authorize_account {
		let account: T::AccountId = whitelisted_caller();
	} : _(RawOrigin::Root, account)

	authorize_account_signed {
		let caller: T::AccountId = whitelisted_caller();
		let account: T::AccountId = account("test",2,2);
		DiaOracle::<T>::authorize_account(RawOrigin::Root.into(), caller.clone())?;
	} : authorize_account(RawOrigin::Signed(caller), account)

	deauthorize_account {
		let account: T::AccountId = whitelisted_caller();
	} : _(RawOrigin::Root, account)

	deauthorize_account_signed {
		let caller: T::AccountId = whitelisted_caller();
		let account: T::AccountId = account("test",2,2);
		DiaOracle::<T>::authorize_account(RawOrigin::Root.into(), caller.clone())?;
	} : authorize_account(RawOrigin::Signed(caller), account)

	set_updated_coin_infos {
		let example_info: CoinInfo = CoinInfo {
			symbol: vec![2, 2, 2],
			name: vec![2, 2, 2],
			blockchain: vec![2, 2, 2],
			supply: 9,
			last_update_timestamp: 9,
			price: 9,
		};
		let coin_infos = (0..=5000).map(|_|{
			((vec![2, 2, 2], vec![2, 2, 2]), example_info.clone())
		}).collect::<Vec<_>>();

		let caller: T::AccountId = whitelisted_caller();
		DiaOracle::<T>::authorize_account(RawOrigin::Root.into(), caller.clone())?;

	}: _(RawOrigin::Signed(caller), coin_infos)

	set_batching_api {
		let api = "http://localhost:8070/currencies2".as_bytes().to_vec();
		let caller: T::AccountId = whitelisted_caller();
		DiaOracle::<T>::authorize_account(RawOrigin::Root.into(), caller.clone())?;
	}: _(RawOrigin::Signed(caller), api)


}

impl_benchmark_test_suite!(DiaOracle, crate::mock::new_test_ext(), crate::mock::Test,);
