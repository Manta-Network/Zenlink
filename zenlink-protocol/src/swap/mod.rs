// Copyright 2021-2022 Zenlink.
// Licensed under Apache 2.0.

//! # SWAP Module
//!
//! ## Overview
//!
//! Built-in decentralized exchange modules in Substrate network, the swap
//! mechanism refers to the design of Uniswap V2.

use super::*;
use crate::swap::util::*;
use primitives::AssetId;

#[cfg(test)]
mod mock;
#[cfg(test)]
mod tests;

mod bootstrap;
pub mod util;

impl<T: Config> Pallet<T> {
	#[allow(clippy::too_many_arguments)]
	pub(crate) fn inner_add_liquidity(
		who: &T::AccountId,
		asset_0: T::AssetId,
		asset_1: T::AssetId,
		amount_0_desired: AssetBalance,
		amount_1_desired: AssetBalance,
		amount_0_min: AssetBalance,
		amount_1_min: AssetBalance,
	) -> DispatchResult {
		let pair = Self::sort_asset_id(asset_0, asset_1);
		PairStatuses::<T>::try_mutate(pair, |pair_status| {
			if let Trading(status) = pair_status {
				let lp_account = &status.pair_account;
				let reserve_0 = T::MultiAssetsHandler::balance_of(asset_0, lp_account);
				let reserve_1 = T::MultiAssetsHandler::balance_of(asset_1, lp_account);

				let (amount_0, amount_1) = Self::calculate_added_amount(
					amount_0_desired,
					amount_1_desired,
					amount_0_min,
					amount_1_min,
					reserve_0,
					reserve_1,
				)?;

				let balance_asset_0 = T::MultiAssetsHandler::balance_of(asset_0, who);
				let balance_asset_1 = T::MultiAssetsHandler::balance_of(asset_1, who);
				ensure!(
					balance_asset_0 >= amount_0 && balance_asset_1 >= amount_1,
					Error::<T>::InsufficientAssetBalance
				);

				let lp_asset_id = Self::lp_pairs(pair).ok_or(Error::<T>::PairNotExists)?;

				let mint_fee = Self::mint_protocol_fee(
					reserve_0,
					reserve_1,
					asset_0,
					asset_1,
					status.total_supply,
				)?;
				if let Some(fee_to) = Self::fee_meta().0 {
					if mint_fee > 0 && Self::fee_meta().1 > 0 {
						T::MultiAssetsHandler::deposit(lp_asset_id, &fee_to, mint_fee)?;
						status.total_supply = status
							.total_supply
							.checked_add(mint_fee)
							.ok_or(Error::<T>::Overflow)?;
					}
				}

				let mint_liquidity = calculate_liquidity(
					amount_0,
					amount_1,
					reserve_0,
					reserve_1,
					status.total_supply,
				);
				ensure!(mint_liquidity > Zero::zero(), Error::<T>::ZeroLiquidity);

				status.total_supply =
					status.total_supply.checked_add(mint_liquidity).ok_or(Error::<T>::Overflow)?;

				T::MultiAssetsHandler::deposit(lp_asset_id, who, mint_liquidity)?;

				T::MultiAssetsHandler::transfer(asset_0, who, lp_account, amount_0)?;
				T::MultiAssetsHandler::transfer(asset_1, who, lp_account, amount_1)?;

				if let Some(_fee_to) = Self::fee_meta().0 {
					if Self::fee_meta().1 > 0 {
						// update reserve_0 and reserve_1
						let reserve_0 = T::MultiAssetsHandler::balance_of(asset_0, lp_account);
						let reserve_1 = T::MultiAssetsHandler::balance_of(asset_1, lp_account);

						let last_k_value = U256::from(reserve_0)
							.checked_mul(U256::from(reserve_1))
							.ok_or(Error::<T>::Overflow)?;
						Self::mutate_k_last(asset_0, asset_1, last_k_value);
					}
				}

				Self::deposit_event(Event::LiquidityAdded(
					who.clone(),
					asset_0,
					asset_1,
					amount_0,
					amount_1,
					mint_liquidity,
				));

				Ok(())
			} else {
				Err(Error::<T>::InvalidStatus.into())
			}
		})
	}

	#[allow(clippy::too_many_arguments)]
	pub(crate) fn inner_remove_liquidity(
		who: &T::AccountId,
		asset_0: T::AssetId,
		asset_1: T::AssetId,
		remove_liquidity: AssetBalance,
		amount_0_min: AssetBalance,
		amount_1_min: AssetBalance,
		recipient: &T::AccountId,
	) -> DispatchResult {
		let lp_pair = Self::sort_asset_id(asset_0, asset_1);
		PairStatuses::<T>::try_mutate(lp_pair, |pair_status| {
			if let Trading(status) = pair_status {
				let lp_account = &status.pair_account;
				let reserve_0 = T::MultiAssetsHandler::balance_of(asset_0, lp_account);
				let reserve_1 = T::MultiAssetsHandler::balance_of(asset_1, lp_account);

				let (amount_0, amount_1) = calculate_share_amounts(
					remove_liquidity,
					status.total_supply,
					reserve_0,
					reserve_1,
				);

				ensure!(
					amount_0 >= amount_0_min && amount_1 >= amount_1_min,
					Error::<T>::InsufficientTargetAmount
				);

				let lp_asset_id = Self::lp_pairs(lp_pair).ok_or(Error::<T>::PairNotExists)?;

				let mint_fee = Self::mint_protocol_fee(
					reserve_0,
					reserve_1,
					asset_0,
					asset_1,
					status.total_supply,
				)?;
				if let Some(fee_to) = Self::fee_meta().0 {
					if mint_fee > 0 && Self::fee_meta().1 > 0 {
						T::MultiAssetsHandler::deposit(lp_asset_id, &fee_to, mint_fee)?;
						status.total_supply = status
							.total_supply
							.checked_add(mint_fee)
							.ok_or(Error::<T>::Overflow)?;
					}
				}

				status.total_supply = status
					.total_supply
					.checked_sub(remove_liquidity)
					.ok_or(Error::<T>::InsufficientLiquidity)?;

				T::MultiAssetsHandler::withdraw(lp_asset_id, who, remove_liquidity)?;

				T::MultiAssetsHandler::transfer(asset_0, lp_account, recipient, amount_0)?;
				T::MultiAssetsHandler::transfer(asset_1, lp_account, recipient, amount_1)?;

				if let Some(_fee_to) = Self::fee_meta().0 {
					if Self::fee_meta().1 > 0 {
						// update reserve_0 and reserve_1
						let reserve_0 = T::MultiAssetsHandler::balance_of(asset_0, lp_account);
						let reserve_1 = T::MultiAssetsHandler::balance_of(asset_1, lp_account);

						let last_k_value = U256::from(reserve_0)
							.checked_mul(U256::from(reserve_1))
							.ok_or(Error::<T>::Overflow)?;
						Self::mutate_k_last(asset_0, asset_1, last_k_value);
					}
				}

				Self::deposit_event(Event::LiquidityRemoved(
					who.clone(),
					recipient.clone(),
					asset_0,
					asset_1,
					amount_0,
					amount_1,
					remove_liquidity,
				));

				Ok(())
			} else {
				Err(Error::<T>::InvalidStatus.into())
			}
		})
	}

	#[allow(clippy::too_many_arguments)]
	pub(crate) fn inner_swap_exact_assets_for_assets(
		who: &T::AccountId,
		amount_in: AssetBalance,
		amount_out_min: AssetBalance,
		path: &[T::AssetId],
		recipient: &T::AccountId,
	) -> DispatchResult {
		let mut new_amount_in = amount_in;
		if path[0].is_native(T::SelfParaId::get()) {
			// charge 0.5% going to pallet account for later distribution
			let fee = amount_in.checked_div(200).unwrap_or_default();
			new_amount_in = amount_in.saturating_sub(fee);
			let native_swap_fees_account = T::PotId::get().into_account_truncating();
			T::MultiAssetsHandler::transfer(path[0], who, &native_swap_fees_account, fee)?;
		}

		let amounts = Self::get_amount_out_by_path(new_amount_in, path)?;
		ensure!(amounts[amounts.len() - 1] >= amount_out_min, Error::<T>::InsufficientTargetAmount);

		let pair_account = Self::pair_account_id(path[0], path[1]);

		T::MultiAssetsHandler::transfer(path[0], who, &pair_account, new_amount_in)?;
		Self::swap(&amounts, path, recipient)?;

		Self::deposit_event(Event::AssetSwap(
			who.clone(),
			recipient.clone(),
			Vec::from(path),
			amounts,
		));

		Ok(())
	}

	#[allow(clippy::too_many_arguments)]
	pub(crate) fn inner_swap_assets_for_exact_assets(
		who: &T::AccountId,
		amount_out: AssetBalance,
		amount_in_max: AssetBalance,
		path: &[T::AssetId],
		recipient: &T::AccountId,
	) -> DispatchResult {
		let amounts = Self::get_amount_in_by_path(amount_out, path)?;

		ensure!(amounts[0] <= amount_in_max, Error::<T>::ExcessiveSoldAmount);

		let pair_account = Self::pair_account_id(path[0], path[1]);

		T::MultiAssetsHandler::transfer(path[0], who, &pair_account, amounts[0])?;
		Self::swap(&amounts, path, recipient)?;

		Self::deposit_event(Event::AssetSwap(
			who.clone(),
			recipient.clone(),
			Vec::from(path),
			amounts,
		));

		Ok(())
	}

	fn swap(
		amounts: &[AssetBalance],
		path: &[T::AssetId],
		recipient: &T::AccountId,
	) -> DispatchResult {
		for i in 0..(amounts.len() - 1) {
			let input = path[i];
			let output = path[i + 1];
			let mut amount0_out: AssetBalance = AssetBalance::default();
			let mut amount1_out = amounts[i + 1];

			let (asset_0, asset_1) = Self::sort_asset_id(input, output);
			if input != asset_0 {
				amount0_out = amounts[i + 1];
				amount1_out = AssetBalance::default();
			}

			let pair_account = Self::pair_account_id(asset_0, asset_1);

			if i < (amounts.len() - 2) {
				let mid_account = Self::pair_account_id(output, path[i + 2]);
				Self::pair_swap(
					asset_0,
					asset_1,
					&pair_account,
					amount0_out,
					amount1_out,
					&mid_account,
				)?;
			} else {
				Self::pair_swap(
					asset_0,
					asset_1,
					&pair_account,
					amount0_out,
					amount1_out,
					recipient,
				)?;
			};
		}
		Ok(())
	}

	fn pair_swap(
		asset_0: T::AssetId,
		asset_1: T::AssetId,
		pair_account: &T::AccountId,
		amount_0: AssetBalance,
		amount_1: AssetBalance,
		recipient: &T::AccountId,
	) -> DispatchResult {
		let pair = Self::sort_asset_id(asset_0, asset_1);
		match Self::pair_status(pair) {
			Trading(_) => Ok(()),
			_ => Err(Error::<T>::InvalidStatus),
		}?;

		let reserve_0 = T::MultiAssetsHandler::balance_of(asset_0, pair_account);
		let reserve_1 = T::MultiAssetsHandler::balance_of(asset_1, pair_account);

		ensure!(
			amount_0 <= reserve_0 && amount_1 <= reserve_1,
			Error::<T>::InsufficientPairReserve
		);

		if amount_0 > Zero::zero() {
			T::MultiAssetsHandler::transfer(asset_0, pair_account, recipient, amount_0)?;
		}

		if amount_1 > Zero::zero() {
			T::MultiAssetsHandler::transfer(asset_1, pair_account, recipient, amount_1)?;
		}

		Ok(())
	}
}

impl<T: Config> ExportZenlink<T::AccountId, T::AssetId> for Pallet<T> {
	fn get_amount_in_by_path(
		amount_out: AssetBalance,
		path: &[T::AssetId],
	) -> Result<Vec<AssetBalance>, DispatchError> {
		Self::get_amount_in_by_path(amount_out, path)
	}

	fn get_amount_out_by_path(
		amount_in: AssetBalance,
		path: &[T::AssetId],
	) -> Result<Vec<AssetBalance>, DispatchError> {
		Self::get_amount_out_by_path(amount_in, path)
	}

	fn inner_swap_assets_for_exact_assets(
		who: &T::AccountId,
		amount_out: AssetBalance,
		amount_in_max: AssetBalance,
		path: &[T::AssetId],
		recipient: &T::AccountId,
	) -> DispatchResult {
		Self::inner_swap_assets_for_exact_assets(who, amount_out, amount_in_max, path, recipient)
	}

	fn inner_swap_exact_assets_for_assets(
		who: &T::AccountId,
		amount_in: AssetBalance,
		amount_out_min: AssetBalance,
		path: &[T::AssetId],
		recipient: &T::AccountId,
	) -> DispatchResult {
		Self::inner_swap_exact_assets_for_assets(who, amount_in, amount_out_min, path, recipient)
	}

	fn inner_add_liquidity(
		who: &T::AccountId,
		asset_0: T::AssetId,
		asset_1: T::AssetId,
		amount_0_desired: AssetBalance,
		amount_1_desired: AssetBalance,
		amount_0_min: AssetBalance,
		amount_1_min: AssetBalance,
	) -> DispatchResult {
		Self::inner_add_liquidity(
			who,
			asset_0,
			asset_1,
			amount_0_desired,
			amount_1_desired,
			amount_0_min,
			amount_1_min,
		)
	}

	fn inner_remove_liquidity(
		who: &T::AccountId,
		asset_0: T::AssetId,
		asset_1: T::AssetId,
		remove_liquidity: AssetBalance,
		amount_0_min: AssetBalance,
		amount_1_min: AssetBalance,
		recipient: &T::AccountId,
	) -> DispatchResult {
		Self::inner_remove_liquidity(
			who,
			asset_0,
			asset_1,
			remove_liquidity,
			amount_0_min,
			amount_1_min,
			recipient,
		)
	}
}

impl<AccountId, AssetId> ExportZenlink<AccountId, AssetId> for () {
	fn get_amount_in_by_path(
		_amount_out: AssetBalance,
		_path: &[AssetId],
	) -> Result<Vec<AssetBalance>, DispatchError> {
		unimplemented!()
	}

	fn get_amount_out_by_path(
		_amount_in: AssetBalance,
		_path: &[AssetId],
	) -> Result<Vec<AssetBalance>, DispatchError> {
		unimplemented!()
	}

	fn inner_swap_assets_for_exact_assets(
		_who: &AccountId,
		_amount_out: AssetBalance,
		_amount_in_max: AssetBalance,
		_path: &[AssetId],
		_recipient: &AccountId,
	) -> DispatchResult {
		unimplemented!()
	}

	fn inner_swap_exact_assets_for_assets(
		_who: &AccountId,
		_amount_in: AssetBalance,
		_amount_out_min: AssetBalance,
		_path: &[AssetId],
		_recipient: &AccountId,
	) -> DispatchResult {
		unimplemented!()
	}

	fn inner_add_liquidity(
		_who: &AccountId,
		_asset_0: AssetId,
		_asset_1: AssetId,
		_amount_0_desired: AssetBalance,
		_amount_1_desired: AssetBalance,
		_amount_0_min: AssetBalance,
		_amount_1_min: AssetBalance,
	) -> DispatchResult {
		unimplemented!()
	}

	fn inner_remove_liquidity(
		_who: &AccountId,
		_asset_0: AssetId,
		_asset_1: AssetId,
		_remove_liquidity: AssetBalance,
		_amount_0_min: AssetBalance,
		_amount_1_min: AssetBalance,
		_recipient: &AccountId,
	) -> DispatchResult {
		unimplemented!()
	}
}
