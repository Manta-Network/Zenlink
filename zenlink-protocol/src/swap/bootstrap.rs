use super::*;

impl<T: Config> Pallet<T> {
	pub(crate) fn do_bootstrap_create(
		pair: (T::AssetId, T::AssetId),
		target_supply_0: AssetBalance,
		target_supply_1: AssetBalance,
		capacity_supply_0: AssetBalance,
		capacity_supply_1: AssetBalance,
		end: T::BlockNumber,
		rewards: Vec<T::AssetId>,
		limits: Vec<(T::AssetId, AssetBalance)>,
	) -> DispatchResult {
		PairStatuses::<T>::try_mutate(pair, |status| match status {
			Trading(_) => Err(Error::<T>::PairAlreadyExists),
			Bootstrap(params) => {
				if Self::bootstrap_disable(params) {
					*status = Bootstrap(BootstrapParameter {
						target_supply: (target_supply_0, target_supply_1),
						capacity_supply: (capacity_supply_0, capacity_supply_1),
						accumulated_supply: params.accumulated_supply,
						end_block_number: end,
						pair_account: Self::account_id(),
					});

					// must no reward before update.
					let exist_rewards = BootstrapRewards::<T>::get(pair);
					for (_, exist_reward) in exist_rewards {
						if exist_reward != Zero::zero() {
							return Err(Error::<T>::ExistRewardsInBootstrap)
						}
					}

					BootstrapRewards::<T>::insert(
						pair,
						rewards
							.into_iter()
							.map(|asset_id| (asset_id, Zero::zero()))
							.collect::<BTreeMap<T::AssetId, AssetBalance>>(),
					);

					BootstrapLimits::<T>::insert(
						pair,
						limits.into_iter().collect::<BTreeMap<T::AssetId, AssetBalance>>(),
					);

					Ok(())
				} else {
					Err(Error::<T>::PairAlreadyExists)
				}
			},
			Disable => {
				*status = Bootstrap(BootstrapParameter {
					target_supply: (target_supply_0, target_supply_1),
					capacity_supply: (capacity_supply_0, capacity_supply_1),
					accumulated_supply: (Zero::zero(), Zero::zero()),
					end_block_number: end,
					pair_account: Self::account_id(),
				});

				BootstrapRewards::<T>::insert(
					pair,
					rewards
						.into_iter()
						.map(|asset_id| (asset_id, Zero::zero()))
						.collect::<BTreeMap<T::AssetId, AssetBalance>>(),
				);

				BootstrapLimits::<T>::insert(
					pair,
					limits.into_iter().collect::<BTreeMap<T::AssetId, AssetBalance>>(),
				);

				Ok(())
			},
		})?;
		Ok(())
	}

	pub(crate) fn do_bootstrap_update(
		pair: (T::AssetId, T::AssetId),
		target_supply_0: AssetBalance,
		target_supply_1: AssetBalance,
		capacity_supply_0: AssetBalance,
		capacity_supply_1: AssetBalance,
		end: T::BlockNumber,
		rewards: Vec<T::AssetId>,
		limits: Vec<(T::AssetId, AssetBalance)>,
	) -> DispatchResult {
		PairStatuses::<T>::try_mutate(pair, |status| match status {
			Trading(_) => Err(Error::<T>::PairAlreadyExists),
			Bootstrap(params) => {
				*status = Bootstrap(BootstrapParameter {
					target_supply: (target_supply_0, target_supply_1),
					capacity_supply: (capacity_supply_0, capacity_supply_1),
					accumulated_supply: params.accumulated_supply,
					end_block_number: end,
					pair_account: Self::account_id(),
				});

				// must no reward before update.
				let exist_rewards = BootstrapRewards::<T>::get(pair);
				for (_, exist_reward) in exist_rewards {
					if exist_reward != Zero::zero() {
						return Err(Error::<T>::ExistRewardsInBootstrap)
					}
				}

				BootstrapRewards::<T>::insert(
					pair,
					rewards
						.into_iter()
						.map(|asset_id| (asset_id, Zero::zero()))
						.collect::<BTreeMap<T::AssetId, AssetBalance>>(),
				);

				BootstrapLimits::<T>::insert(
					pair,
					limits.into_iter().collect::<BTreeMap<T::AssetId, AssetBalance>>(),
				);

				Ok(())
			},
			Disable => Err(Error::<T>::NotInBootstrap),
		})?;
		Ok(())
	}

	pub(crate) fn do_bootstrap_contribute(
		who: T::AccountId,
		asset_0: T::AssetId,
		asset_1: T::AssetId,
		amount_0_contribute: AssetBalance,
		amount_1_contribute: AssetBalance,
	) -> DispatchResult {
		let pair = Self::sort_asset_id(asset_0, asset_1);
		let mut bootstrap_parameter = match Self::pair_status(pair) {
			PairStatus::Bootstrap(bootstrap_parameter) => {
				ensure!(
					frame_system::Pallet::<T>::block_number() <
						bootstrap_parameter.end_block_number,
					Error::<T>::NotInBootstrap
				);
				bootstrap_parameter
			},
			_ => return Err(Error::<T>::NotInBootstrap.into()),
		};
		let (mut amount_0_contribute, mut amount_1_contribute) = if pair.0 == asset_0 {
			(amount_0_contribute, amount_1_contribute)
		} else {
			(amount_1_contribute, amount_0_contribute)
		};

		if amount_0_contribute
			.checked_add(bootstrap_parameter.accumulated_supply.0)
			.ok_or(Error::<T>::Overflow)? >
			bootstrap_parameter.capacity_supply.0
		{
			amount_0_contribute = bootstrap_parameter
				.capacity_supply
				.0
				.checked_sub(bootstrap_parameter.accumulated_supply.0)
				.ok_or(Error::<T>::Overflow)?;
		}

		if amount_1_contribute
			.checked_add(bootstrap_parameter.accumulated_supply.1)
			.ok_or(Error::<T>::Overflow)? >
			bootstrap_parameter.capacity_supply.1
		{
			amount_1_contribute = bootstrap_parameter
				.capacity_supply
				.1
				.checked_sub(bootstrap_parameter.accumulated_supply.1)
				.ok_or(Error::<T>::Overflow)?;
		}

		ensure!(
			amount_0_contribute >= One::one() || amount_1_contribute >= One::one(),
			Error::<T>::InvalidContributionAmount
		);

		BootstrapPersonalSupply::<T>::try_mutate((pair, &who), |contribution| {
			contribution.0 =
				contribution.0.checked_add(amount_0_contribute).ok_or(Error::<T>::Overflow)?;
			contribution.1 =
				contribution.1.checked_add(amount_1_contribute).ok_or(Error::<T>::Overflow)?;

			let pair_account = Self::account_id();

			T::MultiAssetsHandler::transfer(pair.0, &who, &pair_account, amount_0_contribute)?;
			T::MultiAssetsHandler::transfer(pair.1, &who, &pair_account, amount_1_contribute)?;

			let accumulated_supply_0 = bootstrap_parameter
				.accumulated_supply
				.0
				.checked_add(amount_0_contribute)
				.ok_or(Error::<T>::Overflow)?;

			let accumulated_supply_1 = bootstrap_parameter
				.accumulated_supply
				.1
				.checked_add(amount_1_contribute)
				.ok_or(Error::<T>::Overflow)?;
			bootstrap_parameter.accumulated_supply = (accumulated_supply_0, accumulated_supply_1);
			PairStatuses::<T>::insert(pair, Bootstrap(bootstrap_parameter));

			Self::deposit_event(Event::BootstrapContribute(
				who.clone(),
				pair.0,
				amount_0_contribute,
				pair.1,
				amount_1_contribute,
			));
			Ok(())
		})
	}

	pub(crate) fn do_end_bootstrap(asset_0: T::AssetId, asset_1: T::AssetId) -> DispatchResult {
		let pair = Self::sort_asset_id(asset_0, asset_1);
		match Self::pair_status(pair) {
			Bootstrap(bootstrap_parameter) => {
				ensure!(
					frame_system::Pallet::<T>::block_number() >=
						bootstrap_parameter.end_block_number &&
						bootstrap_parameter.accumulated_supply.0 >=
							bootstrap_parameter.target_supply.0 &&
						bootstrap_parameter.accumulated_supply.1 >=
							bootstrap_parameter.target_supply.1,
					Error::<T>::UnqualifiedBootstrap
				);

				let total_lp_supply = calculate_liquidity(
					bootstrap_parameter.accumulated_supply.0,
					bootstrap_parameter.accumulated_supply.1,
					Zero::zero(),
					Zero::zero(),
					Zero::zero(),
				);

				ensure!(total_lp_supply > Zero::zero(), Error::<T>::Overflow);

				let pair_account = Self::pair_account_id(pair.0, pair.1);
				let lp_asset_id = Self::lp_pairs(pair).ok_or(Error::<T>::PairNotExists)?;

				T::MultiAssetsHandler::transfer(
					pair.0,
					&bootstrap_parameter.pair_account,
					&pair_account,
					bootstrap_parameter.accumulated_supply.0,
				)?;

				T::MultiAssetsHandler::transfer(
					pair.1,
					&bootstrap_parameter.pair_account,
					&pair_account,
					bootstrap_parameter.accumulated_supply.1,
				)?;

				T::MultiAssetsHandler::deposit(lp_asset_id, &pair_account, total_lp_supply)
					.map(|_| total_lp_supply)?;

				PairStatuses::<T>::insert(
					pair,
					Trading(PairMetadata { pair_account, total_supply: total_lp_supply }),
				);

				BootstrapEndStatus::<T>::insert(pair, Bootstrap(bootstrap_parameter.clone()));

				Self::deposit_event(Event::BootstrapEnd(
					pair.0,
					pair.1,
					bootstrap_parameter.accumulated_supply.0,
					bootstrap_parameter.accumulated_supply.1,
					total_lp_supply,
				));

				Ok(())
			},
			_ => Err(Error::<T>::NotInBootstrap.into()),
		}
	}

	pub(crate) fn do_bootstrap_claim(
		who: T::AccountId,
		recipient: T::AccountId,
		asset_0: T::AssetId,
		asset_1: T::AssetId,
	) -> DispatchResult {
		let pair = Self::sort_asset_id(asset_0, asset_1);
		match Self::pair_status(pair) {
			Trading(_) =>
				BootstrapPersonalSupply::<T>::try_mutate_exists((pair, &who), |contribution| {
					if let Some((amount_0_contribute, amount_1_contribute)) = contribution.take() {
						if let Bootstrap(bootstrap_parameter) = Self::bootstrap_end_status(pair) {
							ensure!(
								!Self::bootstrap_disable(&bootstrap_parameter),
								Error::<T>::DisableBootstrap
							);
							let exact_amount_0 = U256::from(amount_0_contribute)
								.checked_mul(U256::from(bootstrap_parameter.accumulated_supply.1))
								.and_then(|n| {
									n.checked_add(
										U256::from(amount_1_contribute)
											.checked_mul(U256::from(
												bootstrap_parameter.accumulated_supply.0,
											))
											.ok_or(Error::<T>::Overflow)
											.ok()?,
									)
								})
								.and_then(|r| {
									r.checked_div(
										U256::from(bootstrap_parameter.accumulated_supply.1)
											.checked_mul(U256::from(2u128))
											.ok_or(Error::<T>::Overflow)
											.ok()?,
									)
								})
								.ok_or(Error::<T>::Overflow)?;

							let exact_amount_1 = U256::from(amount_1_contribute)
								.checked_mul(U256::from(bootstrap_parameter.accumulated_supply.0))
								.and_then(|n| {
									n.checked_add(
										U256::from(amount_0_contribute)
											.checked_mul(U256::from(
												bootstrap_parameter.accumulated_supply.1,
											))
											.ok_or(Error::<T>::Overflow)
											.ok()?,
									)
								})
								.and_then(|r| {
									r.checked_div(
										U256::from(bootstrap_parameter.accumulated_supply.0)
											.checked_mul(U256::from(2u128))
											.ok_or(Error::<T>::Overflow)
											.ok()?,
									)
								})
								.ok_or(Error::<T>::Overflow)?;

							let claim_liquidity = exact_amount_0
								.checked_mul(exact_amount_1)
								.map(|n| n.integer_sqrt())
								.and_then(|r| TryInto::<AssetBalance>::try_into(r).ok())
								.ok_or(Error::<T>::Overflow)?;

							let pair_account = Self::pair_account_id(pair.0, pair.1);
							let lp_asset_id =
								Self::lp_pairs(pair).ok_or(Error::<T>::PairNotExists)?;

							T::MultiAssetsHandler::transfer(
								lp_asset_id,
								&pair_account,
								&recipient,
								claim_liquidity,
							)?;

							let bootstrap_total_liquidity =
								U256::from(bootstrap_parameter.accumulated_supply.0)
									.checked_mul(U256::from(
										bootstrap_parameter.accumulated_supply.1,
									))
									.map(|n| n.integer_sqrt())
									.and_then(|r| TryInto::<AssetBalance>::try_into(r).ok())
									.ok_or(Error::<T>::Overflow)?;

							Self::bootstrap_distribute_reward(
								&who,
								&bootstrap_parameter.pair_account,
								pair.0,
								pair.1,
								claim_liquidity,
								bootstrap_total_liquidity,
							)?;

							Self::deposit_event(Event::BootstrapClaim(
								pair_account,
								who.clone(),
								recipient,
								pair.0,
								pair.1,
								amount_0_contribute,
								amount_1_contribute,
								claim_liquidity,
							));

							Ok(())
						} else {
							Err(Error::<T>::NotInBootstrap.into())
						}
					} else {
						Err(Error::<T>::ZeroContribute.into())
					}
				}),
			_ => Err(Error::<T>::NotInBootstrap.into()),
		}
	}

	pub(crate) fn do_bootstrap_refund(
		who: T::AccountId,
		asset_0: T::AssetId,
		asset_1: T::AssetId,
	) -> DispatchResult {
		let pair = Self::sort_asset_id(asset_0, asset_1);

		match Self::pair_status(pair) {
			Bootstrap(params) => {
				ensure!(Self::bootstrap_disable(&params), Error::<T>::DenyRefund);
			},
			_ =>
				if let Bootstrap(bootstrap_parameter) = Self::bootstrap_end_status(pair) {
					ensure!(Self::bootstrap_disable(&bootstrap_parameter), Error::<T>::DenyRefund);
				} else {
					return Err(Error::<T>::DenyRefund.into())
				},
		};

		BootstrapPersonalSupply::<T>::try_mutate_exists(
			(pair, &who),
			|contribution| -> DispatchResult {
				if let Some((amount_0_contribute, amount_1_contribute)) = contribution.take() {
					let pair_account = Self::account_id();
					T::MultiAssetsHandler::transfer(
						pair.0,
						&pair_account,
						&who,
						amount_0_contribute,
					)?;
					T::MultiAssetsHandler::transfer(
						pair.1,
						&pair_account,
						&who,
						amount_1_contribute,
					)?;

					PairStatuses::<T>::try_mutate(pair, |status| -> DispatchResult {
						if let Bootstrap(parameter) = status {
							parameter.accumulated_supply.0 = parameter
								.accumulated_supply
								.0
								.checked_sub(amount_0_contribute)
								.ok_or(Error::<T>::Overflow)?;

							parameter.accumulated_supply.1 = parameter
								.accumulated_supply
								.1
								.checked_sub(amount_1_contribute)
								.ok_or(Error::<T>::Overflow)?;
						}
						Ok(())
					})?;

					*contribution = None;

					Self::deposit_event(Event::BootstrapRefund(
						pair_account,
						who.clone(),
						pair.0,
						pair.1,
						amount_0_contribute,
						amount_1_contribute,
					));

					Ok(())
				} else {
					Err(Error::<T>::ZeroContribute.into())
				}
			},
		)?;

		Ok(())
	}

	// After end block, bootstrap has not enough asset. Is will become disable.
	pub(crate) fn bootstrap_disable(
		params: &BootstrapParameter<AssetBalance, T::BlockNumber, T::AccountId>,
	) -> bool {
		let now = frame_system::Pallet::<T>::block_number();
		if now > params.end_block_number &&
			(params.accumulated_supply.0 < params.target_supply.0 ||
				params.accumulated_supply.1 < params.target_supply.1)
		{
			return true
		}
		false
	}

	pub(crate) fn bootstrap_check_limits(
		asset_0: T::AssetId,
		asset_1: T::AssetId,
		account: &T::AccountId,
	) -> bool {
		let pair = Self::sort_asset_id(asset_0, asset_1);
		let limits = Self::get_bootstrap_limits(pair);

		for (asset_id, limit) in limits.into_iter() {
			if T::MultiAssetsHandler::balance_of(asset_id, account) < limit {
				return false
			}
		}

		true
	}

	pub(crate) fn bootstrap_distribute_reward(
		owner: &T::AccountId,
		reward_holder: &T::AccountId,
		asset_0: T::AssetId,
		asset_1: T::AssetId,
		share_lp: AssetBalance,
		total_lp: AssetBalance,
	) -> DispatchResult {
		let pair = Self::sort_asset_id(asset_0, asset_1);
		let rewards = Self::get_bootstrap_rewards(pair);

		let mut distribute_rewards = Vec::<(T::AssetId, AssetBalance)>::new();
		for (asset_id, reward_amount) in rewards.into_iter() {
			let owner_reward = U256::from(share_lp)
				.checked_mul(U256::from(reward_amount))
				.and_then(|r| r.checked_div(U256::from(total_lp)))
				.and_then(|n| TryInto::<AssetBalance>::try_into(n).ok())
				.ok_or(Error::<T>::Overflow)?;

			T::MultiAssetsHandler::transfer(asset_id, reward_holder, owner, owner_reward)?;

			distribute_rewards.push((asset_id, owner_reward));
		}

		if !distribute_rewards.is_empty() {
			Self::deposit_event(Event::DistributeReward(
				pair.0,
				pair.1,
				reward_holder.clone(),
				distribute_rewards,
			));
		}

		Ok(())
	}
}
