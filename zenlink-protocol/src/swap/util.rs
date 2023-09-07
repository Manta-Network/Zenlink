use super::*;

impl<T: Config> Pallet<T> {
	pub(crate) fn account_id() -> T::AccountId {
		T::PalletId::get().into_account_truncating()
	}
	pub fn pair_account_id(asset_0: T::AssetId, asset_1: T::AssetId) -> T::AccountId {
		let (asset_0, asset_1) = Self::sort_asset_id(asset_0, asset_1);
		let pair_hash: T::Hash = T::Hashing::hash_of(&(asset_0, asset_1));

		T::PalletId::get().into_sub_account_truncating(pair_hash.as_ref())
	}

	/// Sorted the assets pair
	pub fn sort_asset_id(asset_0: T::AssetId, asset_1: T::AssetId) -> (T::AssetId, T::AssetId) {
		if asset_0 < asset_1 {
			(asset_0, asset_1)
		} else {
			(asset_1, asset_0)
		}
	}

	pub fn lp_asset_id(asset_0: &T::AssetId, asset_1: &T::AssetId) -> Option<T::AssetId> {
		let (asset_0, asset_1) = Self::sort_asset_id(*asset_0, *asset_1);
		T::LpGenerate::generate_lp_asset_id(asset_0, asset_1)
	}

	pub(crate) fn mutate_lp_pairs(asset_0: T::AssetId, asset_1: T::AssetId) -> DispatchResult {
		Ok(LiquidityPairs::<T>::insert(
			Self::sort_asset_id(asset_0, asset_1),
			Some(Self::lp_asset_id(&asset_0, &asset_1).ok_or(Error::<T>::AssetNotExists)?),
		))
	}

	pub(crate) fn mutate_k_last(asset_0: T::AssetId, asset_1: T::AssetId, last: U256) {
		KLast::<T>::mutate(Self::sort_asset_id(asset_0, asset_1), |k| *k = last)
	}

	/// Refer: https://github.com/Uniswap/uniswap-v2-core/blob/master/contracts/UniswapV2Pair.sol#L88
	/// Take as a [0, 100%] cut of the exchange fees earned by liquidity providers
	pub(crate) fn mint_protocol_fee(
		reserve_0: AssetBalance,
		reserve_1: AssetBalance,
		asset_0: T::AssetId,
		asset_1: T::AssetId,
		total_liquidity: AssetBalance,
	) -> Result<AssetBalance, DispatchError> {
		let new_k_last = Self::k_last(Self::sort_asset_id(asset_0, asset_1));
		let mut mint_fee: AssetBalance = 0;

		if let Some(_fee_to) = Self::fee_meta().0 {
			if !new_k_last.is_zero() && Self::fee_meta().1 > 0 {
				let root_k = U256::from(reserve_0)
					.checked_mul(U256::from(reserve_1))
					.map(|n| n.integer_sqrt())
					.ok_or(Error::<T>::Overflow)?;

				let root_k_last = new_k_last.integer_sqrt();
				if root_k > root_k_last {
					let fee_point = Self::fee_meta().1;
					let fix_fee_point = (30 - fee_point) / fee_point;
					let numerator = U256::from(total_liquidity)
						.checked_mul(root_k.checked_sub(root_k_last).ok_or(Error::<T>::Overflow)?)
						.ok_or(Error::<T>::Overflow)?;

					let denominator = root_k
						.checked_mul(U256::from(fix_fee_point))
						.and_then(|n| n.checked_add(root_k_last))
						.ok_or(Error::<T>::Overflow)?;

					let liquidity = numerator
						.checked_div(denominator)
						.and_then(|n| TryInto::<AssetBalance>::try_into(n).ok())
						.unwrap_or_else(Zero::zero);

					if liquidity > 0 {
						mint_fee = liquidity
					}
				}
			}
		} else if !new_k_last.is_zero() {
			Self::mutate_k_last(asset_0, asset_1, U256::zero())
		}

		Ok(mint_fee)
	}

	pub(crate) fn calculate_added_amount(
		amount_0_desired: AssetBalance,
		amount_1_desired: AssetBalance,
		amount_0_min: AssetBalance,
		amount_1_min: AssetBalance,
		reserve_0: AssetBalance,
		reserve_1: AssetBalance,
	) -> Result<(AssetBalance, AssetBalance), DispatchError> {
		if reserve_0 == Zero::zero() || reserve_1 == Zero::zero() {
			return Ok((amount_0_desired, amount_1_desired))
		}
		let amount_1_optimal = calculate_share_amount(amount_0_desired, reserve_0, reserve_1);
		if amount_1_optimal <= amount_1_desired {
			ensure!(amount_1_optimal >= amount_1_min, Error::<T>::IncorrectAssetAmountRange);
			return Ok((amount_0_desired, amount_1_optimal))
		}
		let amount_0_optimal = calculate_share_amount(amount_1_desired, reserve_1, reserve_0);
		ensure!(
			amount_0_optimal >= amount_0_min && amount_0_optimal <= amount_0_desired,
			Error::<T>::IncorrectAssetAmountRange
		);
		Ok((amount_0_optimal, amount_1_desired))
	}

	pub(crate) fn get_amount_in(
		output_amount: AssetBalance,
		input_reserve: AssetBalance,
		output_reserve: AssetBalance,
	) -> Result<AssetBalance, DispatchError> {
		ensure!(
			!input_reserve.is_zero() && !output_reserve.is_zero() && !output_amount.is_zero(),
			Error::<T>::Overflow
		);

		// 0.3% exchange fee rate
		let numerator = U256::from(input_reserve)
			.checked_mul(U256::from(output_amount))
			.and_then(|n| n.checked_mul(U256::from(1000u128)))
			.ok_or(Error::<T>::Overflow)?;

		let denominator = (U256::from(output_reserve).checked_sub(U256::from(output_amount)))
			.and_then(|n| n.checked_mul(U256::from(997u128)))
			.ok_or(Error::<T>::Overflow)?;

		let amount_in = numerator
			.checked_div(denominator)
			.and_then(|r| r.checked_add(U256::one()))
			.and_then(|n| TryInto::<AssetBalance>::try_into(n).ok())
			.ok_or(Error::<T>::Overflow)?;

		Ok(amount_in)
	}

	pub(crate) fn get_amount_out(
		input_amount: AssetBalance,
		input_reserve: AssetBalance,
		output_reserve: AssetBalance,
	) -> Result<AssetBalance, DispatchError> {
		ensure!(
			!input_reserve.is_zero() && !output_reserve.is_zero() && !input_amount.is_zero(),
			Error::<T>::Overflow
		);

		// 0.3% exchange fee rate
		let input_amount_with_fee = U256::from(input_amount)
			.checked_mul(U256::from(997u128))
			.ok_or(Error::<T>::Overflow)?;

		let numerator = input_amount_with_fee
			.checked_mul(U256::from(output_reserve))
			.ok_or(Error::<T>::Overflow)?;

		let denominator = U256::from(input_reserve)
			.checked_mul(U256::from(1000u128))
			.and_then(|n| n.checked_add(input_amount_with_fee))
			.ok_or(Error::<T>::Overflow)?;

		let amount_out = numerator
			.checked_div(denominator)
			.and_then(|n| TryInto::<AssetBalance>::try_into(n).ok())
			.ok_or(Error::<T>::Overflow)?;
		Ok(amount_out)
	}

	pub fn get_amount_in_by_path(
		amount_out: AssetBalance,
		path: &[T::AssetId],
	) -> Result<Vec<AssetBalance>, DispatchError> {
		let len = path.len();
		ensure!(len > 1, Error::<T>::3);

		let mut i = len - 1;
		let mut out_vec = vec![amount_out];

		while i > 0 {
			let pair_account = Self::pair_account_id(path[i], path[i - 1]);
			let reserve_0 = T::MultiAssetsHandler::balance_of(path[i], &pair_account);
			let reserve_1 = T::MultiAssetsHandler::balance_of(path[i - 1], &pair_account);

			ensure!(reserve_1 > Zero::zero() && reserve_0 > Zero::zero(), Error::<T>::InvalidPath1);

			let amount = Self::get_amount_in(out_vec[len - 1 - i], reserve_1, reserve_0)?;
			ensure!(amount > One::one(), Error::<T>::InvalidPath2);

			// check K
			let invariant_before_swap: U256 = U256::from(reserve_0)
				.checked_mul(U256::from(reserve_1))
				.ok_or(Error::<T>::Overflow)?;

			let reserve_1_after_swap = reserve_1.checked_add(amount).ok_or(Error::<T>::Overflow)?;
			let reserve_0_after_swap =
				reserve_0.checked_sub(out_vec[len - 1 - i]).ok_or(Error::<T>::Overflow)?;

			let invariant_after_swap: U256 = U256::from(reserve_1_after_swap)
				.checked_mul(U256::from(reserve_0_after_swap))
				.ok_or(Error::<T>::Overflow)?;

			ensure!(
				invariant_after_swap >= invariant_before_swap,
				Error::<T>::InvariantCheckFailed,
			);
			out_vec.push(amount);
			i -= 1;
		}

		out_vec.reverse();
		Ok(out_vec)
	}

	pub fn get_amount_out_by_path(
		amount_in: AssetBalance,
		path: &[T::AssetId],
	) -> Result<Vec<AssetBalance>, DispatchError> {
		ensure!(path.len() > 1, Error::<T>::InvalidPath4);

		let len = path.len() - 1;
		let mut out_vec = vec![amount_in];

		for i in 0..len {
			let pair_account = Self::pair_account_id(path[i], path[i + 1]);
			let reserve_0 = T::MultiAssetsHandler::balance_of(path[i], &pair_account);
			let reserve_1 = T::MultiAssetsHandler::balance_of(path[i + 1], &pair_account);

			ensure!(reserve_1 > Zero::zero() && reserve_0 > Zero::zero(), Error::<T>::InvalidPath5);

			let amount = Self::get_amount_out(out_vec[i], reserve_0, reserve_1)?;
			ensure!(amount > Zero::zero(), Error::<T>::InvalidPath6);

			// check K
			let invariant_before_swap: U256 = U256::from(reserve_0)
				.checked_mul(U256::from(reserve_1))
				.ok_or(Error::<T>::Overflow)?;

			let reserve_0_after_swap =
				reserve_0.checked_add(out_vec[i]).ok_or(Error::<T>::Overflow)?;
			let reserve_1_after_swap = reserve_1.checked_sub(amount).ok_or(Error::<T>::Overflow)?;

			let invariant_after_swap: U256 = U256::from(reserve_1_after_swap)
				.checked_mul(U256::from(reserve_0_after_swap))
				.ok_or(Error::<T>::Overflow)?;

			ensure!(
				invariant_after_swap >= invariant_before_swap,
				Error::<T>::InvariantCheckFailed,
			);

			out_vec.push(amount);
		}

		Ok(out_vec)
	}
}

pub(crate) fn calculate_share_amount(
	amount: AssetBalance,
	supply: AssetBalance,
	reserve: AssetBalance,
) -> AssetBalance {
	U256::from(amount)
		.checked_mul(U256::from(reserve))
		.and_then(|n| n.checked_div(U256::from(supply)))
		.and_then(|n| TryInto::<AssetBalance>::try_into(n).ok())
		.unwrap_or_else(Zero::zero)
}

pub(crate) fn calculate_share_amounts(
	amount: AssetBalance,
	supply: AssetBalance,
	reserve_0: AssetBalance,
	reserve_1: AssetBalance,
) -> (AssetBalance, AssetBalance) {
	let amount0 = calculate_share_amount(amount, supply, reserve_0);
	let amount1 = calculate_share_amount(amount, supply, reserve_1);
	(amount0, amount1)
}

pub fn calculate_liquidity(
	amount_0: AssetBalance,
	amount_1: AssetBalance,
	reserve_0: AssetBalance,
	reserve_1: AssetBalance,
	total_liquidity: AssetBalance,
) -> AssetBalance {
	if total_liquidity == Zero::zero() {
		U256::from(amount_0)
			.checked_mul(U256::from(amount_1))
			.map(|n| n.integer_sqrt())
			.and_then(|n| TryInto::<AssetBalance>::try_into(n).ok())
			.unwrap_or_else(Zero::zero)
	} else {
		core::cmp::min(
			calculate_share_amount(amount_0, reserve_0, total_liquidity),
			calculate_share_amount(amount_1, reserve_1, total_liquidity),
		)
	}
}
