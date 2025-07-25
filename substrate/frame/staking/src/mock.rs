// This file is part of Substrate.

// Copyright (C) Parity Technologies (UK) Ltd.
// SPDX-License-Identifier: Apache-2.0

// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
// 	http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

//! Test utilities

use crate::{self as pallet_staking, *};
use frame_election_provider_support::{
	bounds::{ElectionBounds, ElectionBoundsBuilder},
	onchain, BoundedSupports, SequentialPhragmen, Support, VoteWeight,
};
use frame_support::{
	assert_ok, derive_impl, ord_parameter_types, parameter_types,
	traits::{
		ConstU64, EitherOfDiverse, FindAuthor, Get, Imbalance, OnUnbalanced, OneSessionHandler,
		RewardsReporter,
	},
	weights::constants::RocksDbWeight,
};
use frame_system::{EnsureRoot, EnsureSignedBy};
use sp_core::ConstBool;
use sp_io;
use sp_runtime::{curve::PiecewiseLinear, testing::UintAuthorityId, traits::Zero, BuildStorage};
use sp_staking::{
	offence::{OffenceDetails, OnOffenceHandler},
	OnStakingUpdate, StakingAccount,
};

pub const INIT_TIMESTAMP: u64 = 30_000;
pub const BLOCK_TIME: u64 = 1000;
pub(crate) const SINGLE_PAGE: u32 = 0;

/// The AccountId alias in this test module.
pub(crate) type AccountId = u64;
pub(crate) type BlockNumber = u64;
pub(crate) type Balance = u128;

/// Another session handler struct to test on_disabled.
pub struct OtherSessionHandler;
impl OneSessionHandler<AccountId> for OtherSessionHandler {
	type Key = UintAuthorityId;

	fn on_genesis_session<'a, I: 'a>(_: I)
	where
		I: Iterator<Item = (&'a AccountId, Self::Key)>,
		AccountId: 'a,
	{
	}

	fn on_new_session<'a, I: 'a>(_: bool, _: I, _: I)
	where
		I: Iterator<Item = (&'a AccountId, Self::Key)>,
		AccountId: 'a,
	{
	}

	fn on_disabled(_validator_index: u32) {}
}

impl sp_runtime::BoundToRuntimeAppPublic for OtherSessionHandler {
	type Public = UintAuthorityId;
}

pub fn is_disabled(controller: AccountId) -> bool {
	let stash = Ledger::<Test>::get(&controller).unwrap().stash;
	let validator_index = match Session::validators().iter().position(|v| *v == stash) {
		Some(index) => index as u32,
		None => return false,
	};

	Session::disabled_validators().contains(&validator_index)
}

type Block = frame_system::mocking::MockBlock<Test>;

frame_support::construct_runtime!(
	pub enum Test
	{
		System: frame_system,
		Authorship: pallet_authorship,
		Timestamp: pallet_timestamp,
		Balances: pallet_balances,
		Staking: pallet_staking,
		Session: pallet_session,
		Historical: pallet_session::historical,
		VoterBagsList: pallet_bags_list::<Instance1>,
	}
);

/// Author of block is always 11
pub struct Author11;
impl FindAuthor<AccountId> for Author11 {
	fn find_author<'a, I>(_digests: I) -> Option<AccountId>
	where
		I: 'a + IntoIterator<Item = (frame_support::ConsensusEngineId, &'a [u8])>,
	{
		Some(11)
	}
}

parameter_types! {
	pub static SessionsPerEra: SessionIndex = 3;
	pub static ExistentialDeposit: Balance = 1;
	pub static SlashDeferDuration: EraIndex = 0;
	pub static Period: BlockNumber = 5;
	pub static Offset: BlockNumber = 0;
	pub static MaxControllersInDeprecationBatch: u32 = 5900;
}

#[derive_impl(frame_system::config_preludes::TestDefaultConfig)]
impl frame_system::Config for Test {
	type DbWeight = RocksDbWeight;
	type Block = Block;
	type AccountData = pallet_balances::AccountData<Balance>;
}
#[derive_impl(pallet_balances::config_preludes::TestDefaultConfig)]
impl pallet_balances::Config for Test {
	type MaxLocks = frame_support::traits::ConstU32<1024>;
	type Balance = Balance;
	type ExistentialDeposit = ExistentialDeposit;
	type AccountStore = System;
}

sp_runtime::impl_opaque_keys! {
	pub struct SessionKeys {
		pub other: OtherSessionHandler,
	}
}
impl pallet_session::Config for Test {
	type SessionManager = pallet_session::historical::NoteHistoricalRoot<Test, Staking>;
	type Keys = SessionKeys;
	type ShouldEndSession = pallet_session::PeriodicSessions<Period, Offset>;
	type SessionHandler = (OtherSessionHandler,);
	type RuntimeEvent = RuntimeEvent;
	type ValidatorId = AccountId;
	type ValidatorIdOf = sp_runtime::traits::ConvertInto;
	type NextSessionRotation = pallet_session::PeriodicSessions<Period, Offset>;
	type DisablingStrategy =
		pallet_session::disabling::UpToLimitWithReEnablingDisablingStrategy<DISABLING_LIMIT_FACTOR>;
	type WeightInfo = ();
	type Currency = Balances;
	type KeyDeposit = ();
}

impl pallet_session::historical::Config for Test {
	type RuntimeEvent = RuntimeEvent;
	type FullIdentification = ();
	type FullIdentificationOf = crate::UnitIdentificationOf<Self>;
}
impl pallet_authorship::Config for Test {
	type FindAuthor = Author11;
	type EventHandler = ();
}

impl pallet_timestamp::Config for Test {
	type Moment = u64;
	type OnTimestampSet = ();
	type MinimumPeriod = ConstU64<5>;
	type WeightInfo = ();
}

pallet_staking_reward_curve::build! {
	const I_NPOS: PiecewiseLinear<'static> = curve!(
		min_inflation: 0_025_000,
		max_inflation: 0_100_000,
		ideal_stake: 0_500_000,
		falloff: 0_050_000,
		max_piece_count: 40,
		test_precision: 0_005_000,
	);
}
parameter_types! {
	pub const BondingDuration: EraIndex = 3;
	pub const RewardCurve: &'static PiecewiseLinear<'static> = &I_NPOS;
}

parameter_types! {
	pub static RewardRemainderUnbalanced: u128 = 0;
}

pub struct RewardRemainderMock;

impl OnUnbalanced<NegativeImbalanceOf<Test>> for RewardRemainderMock {
	fn on_nonzero_unbalanced(amount: NegativeImbalanceOf<Test>) {
		RewardRemainderUnbalanced::mutate(|v| {
			*v += amount.peek();
		});
		drop(amount);
	}
}

const THRESHOLDS: [sp_npos_elections::VoteWeight; 9] =
	[10, 20, 30, 40, 50, 60, 1_000, 2_000, 10_000];

parameter_types! {
	pub static BagThresholds: &'static [sp_npos_elections::VoteWeight] = &THRESHOLDS;
	pub static HistoryDepth: u32 = 80;
	pub static MaxExposurePageSize: u32 = 64;
	pub static MaxUnlockingChunks: u32 = 32;
	pub static RewardOnUnbalanceWasCalled: bool = false;
	pub static MaxValidatorSet: u32 = 100;
	pub static ElectionsBounds: ElectionBounds = ElectionBoundsBuilder::default().build();
	pub static AbsoluteMaxNominations: u32 = 16;
}

type VoterBagsListInstance = pallet_bags_list::Instance1;
impl pallet_bags_list::Config<VoterBagsListInstance> for Test {
	type RuntimeEvent = RuntimeEvent;
	type WeightInfo = ();
	// Staking is the source of truth for voter bags list, since they are not kept up to date.
	type ScoreProvider = Staking;
	type BagThresholds = BagThresholds;
	type Score = VoteWeight;
}

parameter_types! {
	pub static MaxBackersPerWinner: u32 = 256;
	pub static MaxWinnersPerPage: u32 = MaxValidatorSet::get();
}
pub struct OnChainSeqPhragmen;
impl onchain::Config for OnChainSeqPhragmen {
	type System = Test;
	type Solver = SequentialPhragmen<AccountId, Perbill>;
	type DataProvider = Staking;
	type WeightInfo = ();
	type MaxBackersPerWinner = MaxBackersPerWinner;
	type MaxWinnersPerPage = MaxWinnersPerPage;
	type Bounds = ElectionsBounds;
	type Sort = ConstBool<true>;
}

pub struct MockReward {}
impl OnUnbalanced<PositiveImbalanceOf<Test>> for MockReward {
	fn on_unbalanced(_: PositiveImbalanceOf<Test>) {
		RewardOnUnbalanceWasCalled::set(true);
	}
}

parameter_types! {
	pub static LedgerSlashPerEra:
		(BalanceOf<Test>, BTreeMap<EraIndex, BalanceOf<Test>>) =
		(Zero::zero(), BTreeMap::new());
	pub static SlashObserver: BTreeMap<AccountId, BalanceOf<Test>> = BTreeMap::new();
	pub static RestrictedAccounts: Vec<AccountId> = Vec::new();
}

pub struct EventListenerMock;
impl OnStakingUpdate<AccountId, Balance> for EventListenerMock {
	fn on_slash(
		pool_account: &AccountId,
		slashed_bonded: Balance,
		slashed_chunks: &BTreeMap<EraIndex, Balance>,
		total_slashed: Balance,
	) {
		LedgerSlashPerEra::set((slashed_bonded, slashed_chunks.clone()));
		SlashObserver::mutate(|map| {
			map.insert(*pool_account, map.get(pool_account).unwrap_or(&0) + total_slashed)
		});
	}
}

pub struct MockedRestrictList;
impl Contains<AccountId> for MockedRestrictList {
	fn contains(who: &AccountId) -> bool {
		RestrictedAccounts::get().contains(who)
	}
}

// Disabling threshold for `UpToLimitDisablingStrategy` and
// `UpToLimitWithReEnablingDisablingStrategy``
pub(crate) const DISABLING_LIMIT_FACTOR: usize = 3;

#[derive_impl(crate::config_preludes::TestDefaultConfig)]
impl crate::pallet::pallet::Config for Test {
	type OldCurrency = Balances;
	type Currency = Balances;
	type UnixTime = Timestamp;
	type RewardRemainder = RewardRemainderMock;
	type Reward = MockReward;
	type SessionsPerEra = SessionsPerEra;
	type SlashDeferDuration = SlashDeferDuration;
	type AdminOrigin = EnsureOneOrRoot;
	type SessionInterface = Self;
	type EraPayout = ConvertCurve<RewardCurve>;
	type NextNewSession = Session;
	type MaxExposurePageSize = MaxExposurePageSize;
	type MaxValidatorSet = MaxValidatorSet;
	type ElectionProvider = onchain::OnChainExecution<OnChainSeqPhragmen>;
	type GenesisElectionProvider = Self::ElectionProvider;
	// NOTE: consider a macro and use `UseNominatorsAndValidatorsMap<Self>` as well.
	type VoterList = VoterBagsList;
	type TargetList = UseValidatorsMap<Self>;
	type NominationsQuota = WeightedNominationsQuota<16>;
	type MaxUnlockingChunks = MaxUnlockingChunks;
	type HistoryDepth = HistoryDepth;
	type MaxControllersInDeprecationBatch = MaxControllersInDeprecationBatch;
	type EventListeners = EventListenerMock;
	type Filter = MockedRestrictList;
}

pub struct WeightedNominationsQuota<const MAX: u32>;
impl<Balance, const MAX: u32> NominationsQuota<Balance> for WeightedNominationsQuota<MAX>
where
	u128: From<Balance>,
{
	type MaxNominations = AbsoluteMaxNominations;

	fn curve(balance: Balance) -> u32 {
		match balance.into() {
			// random curve for testing.
			0..=110 => MAX,
			111 => 0,
			222 => 2,
			333 => MAX + 10,
			_ => MAX,
		}
	}
}

pub(crate) type StakingCall = crate::Call<Test>;
pub(crate) type TestCall = <Test as frame_system::Config>::RuntimeCall;

parameter_types! {
	// if true, skips the try-state for the test running.
	pub static SkipTryStateCheck: bool = false;
}

pub struct ExtBuilder {
	nominate: bool,
	validator_count: u32,
	minimum_validator_count: u32,
	invulnerables: Vec<AccountId>,
	has_stakers: bool,
	initialize_first_session: bool,
	pub min_nominator_bond: Balance,
	min_validator_bond: Balance,
	balance_factor: Balance,
	status: BTreeMap<AccountId, StakerStatus<AccountId>>,
	stakes: BTreeMap<AccountId, Balance>,
	stakers: Vec<(AccountId, AccountId, Balance, StakerStatus<AccountId>)>,
}

impl Default for ExtBuilder {
	fn default() -> Self {
		Self {
			nominate: true,
			validator_count: 2,
			minimum_validator_count: 0,
			balance_factor: 1,
			invulnerables: vec![],
			has_stakers: true,
			initialize_first_session: true,
			min_nominator_bond: ExistentialDeposit::get(),
			min_validator_bond: ExistentialDeposit::get(),
			status: Default::default(),
			stakes: Default::default(),
			stakers: Default::default(),
		}
	}
}

impl ExtBuilder {
	pub fn existential_deposit(self, existential_deposit: Balance) -> Self {
		EXISTENTIAL_DEPOSIT.with(|v| *v.borrow_mut() = existential_deposit);
		self
	}
	pub fn nominate(mut self, nominate: bool) -> Self {
		self.nominate = nominate;
		self
	}
	pub fn validator_count(mut self, count: u32) -> Self {
		self.validator_count = count;
		self
	}
	pub fn minimum_validator_count(mut self, count: u32) -> Self {
		self.minimum_validator_count = count;
		self
	}
	pub fn slash_defer_duration(self, eras: EraIndex) -> Self {
		SLASH_DEFER_DURATION.with(|v| *v.borrow_mut() = eras);
		self
	}
	pub fn invulnerables(mut self, invulnerables: Vec<AccountId>) -> Self {
		self.invulnerables = invulnerables;
		self
	}
	pub fn session_per_era(self, length: SessionIndex) -> Self {
		SESSIONS_PER_ERA.with(|v| *v.borrow_mut() = length);
		self
	}
	pub fn period(self, length: BlockNumber) -> Self {
		PERIOD.with(|v| *v.borrow_mut() = length);
		self
	}
	pub fn has_stakers(mut self, has: bool) -> Self {
		self.has_stakers = has;
		self
	}
	pub fn initialize_first_session(mut self, init: bool) -> Self {
		self.initialize_first_session = init;
		self
	}
	pub fn offset(self, offset: BlockNumber) -> Self {
		OFFSET.with(|v| *v.borrow_mut() = offset);
		self
	}
	pub fn min_nominator_bond(mut self, amount: Balance) -> Self {
		self.min_nominator_bond = amount;
		self
	}
	pub fn min_validator_bond(mut self, amount: Balance) -> Self {
		self.min_validator_bond = amount;
		self
	}
	pub fn set_status(mut self, who: AccountId, status: StakerStatus<AccountId>) -> Self {
		self.status.insert(who, status);
		self
	}
	pub fn set_stake(mut self, who: AccountId, stake: Balance) -> Self {
		self.stakes.insert(who, stake);
		self
	}
	pub fn add_staker(
		mut self,
		stash: AccountId,
		ctrl: AccountId,
		stake: Balance,
		status: StakerStatus<AccountId>,
	) -> Self {
		self.stakers.push((stash, ctrl, stake, status));
		self
	}
	pub fn balance_factor(mut self, factor: Balance) -> Self {
		self.balance_factor = factor;
		self
	}
	pub fn try_state(self, enable: bool) -> Self {
		SkipTryStateCheck::set(!enable);
		self
	}
	fn build(self) -> sp_io::TestExternalities {
		sp_tracing::try_init_simple();
		let mut storage = frame_system::GenesisConfig::<Test>::default().build_storage().unwrap();
		let ed = ExistentialDeposit::get();

		let _ = pallet_balances::GenesisConfig::<Test> {
			balances: vec![
				(1, 10 * self.balance_factor),
				(2, 20 * self.balance_factor),
				(3, 300 * self.balance_factor),
				(4, 400 * self.balance_factor),
				// controllers (still used in some tests. Soon to be deprecated).
				(10, self.balance_factor),
				(20, self.balance_factor),
				(30, self.balance_factor),
				(40, self.balance_factor),
				(50, self.balance_factor),
				// stashes
				// Note: Previously this pallet used locks and stakers could stake all their
				// balance including ED. Now with holds, stakers are required to maintain
				// (non-staked) ED in their accounts. Therefore, we drop an additional existential
				// deposit to genesis stakers.
				(11, self.balance_factor * 1000 + ed),
				(21, self.balance_factor * 2000 + ed),
				(31, self.balance_factor * 2000 + ed),
				(41, self.balance_factor * 2000 + ed),
				(51, self.balance_factor * 2000 + ed),
				(201, self.balance_factor * 2000 + ed),
				(202, self.balance_factor * 2000 + ed),
				// optional nominator
				(100, self.balance_factor * 2000 + ed),
				(101, self.balance_factor * 2000 + ed),
				// aux accounts
				(60, self.balance_factor),
				(61, self.balance_factor * 2000 + ed),
				(70, self.balance_factor),
				(71, self.balance_factor * 2000),
				(80, self.balance_factor),
				(81, self.balance_factor * 2000),
				// This allows us to have a total_payout different from 0.
				(999, 1_000_000_000_000),
			],
			..Default::default()
		}
		.assimilate_storage(&mut storage);

		let mut stakers = vec![];
		if self.has_stakers {
			stakers = vec![
				// (stash, ctrl, stake, status)
				// these two will be elected in the default test where we elect 2.
				(11, 11, self.balance_factor * 1000, StakerStatus::<AccountId>::Validator),
				(21, 21, self.balance_factor * 1000, StakerStatus::<AccountId>::Validator),
				// a loser validator
				(31, 31, self.balance_factor * 500, StakerStatus::<AccountId>::Validator),
				// an idle validator
				(41, 41, self.balance_factor * 1000, StakerStatus::<AccountId>::Idle),
				(51, 51, self.balance_factor * 1000, StakerStatus::<AccountId>::Idle),
				(201, 201, self.balance_factor * 1000, StakerStatus::<AccountId>::Idle),
				(202, 202, self.balance_factor * 1000, StakerStatus::<AccountId>::Idle),
			]; // optionally add a nominator
			if self.nominate {
				stakers.push((
					101,
					101,
					self.balance_factor * 500,
					StakerStatus::<AccountId>::Nominator(vec![11, 21]),
				))
			}
			// replace any of the status if needed.
			self.status.into_iter().for_each(|(stash, status)| {
				let (_, _, _, ref mut prev_status) = stakers
					.iter_mut()
					.find(|s| s.0 == stash)
					.expect("set_status staker should exist; qed");
				*prev_status = status;
			});
			// replaced any of the stakes if needed.
			self.stakes.into_iter().for_each(|(stash, stake)| {
				let (_, _, ref mut prev_stake, _) = stakers
					.iter_mut()
					.find(|s| s.0 == stash)
					.expect("set_stake staker should exits; qed.");
				*prev_stake = stake;
			});
			// extend stakers if needed.
			stakers.extend(self.stakers)
		}

		let _ = pallet_staking::GenesisConfig::<Test> {
			stakers: stakers.clone(),
			validator_count: self.validator_count,
			minimum_validator_count: self.minimum_validator_count,
			invulnerables: self.invulnerables,
			slash_reward_fraction: Perbill::from_percent(10),
			min_nominator_bond: self.min_nominator_bond,
			min_validator_bond: self.min_validator_bond,
			..Default::default()
		}
		.assimilate_storage(&mut storage);

		let _ = pallet_session::GenesisConfig::<Test> {
			keys: if self.has_stakers {
				// set the keys for the first session.
				stakers
					.into_iter()
					.map(|(id, ..)| (id, id, SessionKeys { other: id.into() }))
					.collect()
			} else {
				// set some dummy validators in genesis.
				(0..self.validator_count as u64)
					.map(|id| (id, id, SessionKeys { other: id.into() }))
					.collect()
			},
			..Default::default()
		}
		.assimilate_storage(&mut storage);

		let mut ext = sp_io::TestExternalities::from(storage);

		if self.initialize_first_session {
			ext.execute_with(|| {
				run_to_block(1);

				// Force reset the timestamp to the initial timestamp for easy testing.
				Timestamp::set_timestamp(INIT_TIMESTAMP);
			});
		}

		ext
	}
	pub fn build_and_execute(self, test: impl FnOnce() -> ()) {
		sp_tracing::try_init_simple();
		let mut ext = self.build();
		ext.execute_with(test);
		ext.execute_with(|| {
			if !SkipTryStateCheck::get() {
				Staking::do_try_state(System::block_number()).unwrap();
			}
		});
	}
}

pub(crate) fn active_era() -> EraIndex {
	pallet_staking::ActiveEra::<Test>::get().unwrap().index
}

pub(crate) fn current_era() -> EraIndex {
	pallet_staking::CurrentEra::<Test>::get().unwrap()
}

pub(crate) fn bond(who: AccountId, val: Balance) {
	let _ = asset::set_stakeable_balance::<Test>(&who, val);
	assert_ok!(Staking::bond(RuntimeOrigin::signed(who), val, RewardDestination::Stash));
}

pub(crate) fn bond_validator(who: AccountId, val: Balance) {
	bond(who, val);
	assert_ok!(Staking::validate(RuntimeOrigin::signed(who), ValidatorPrefs::default()));
	assert_ok!(Session::set_keys(
		RuntimeOrigin::signed(who),
		SessionKeys { other: who.into() },
		vec![]
	));
}

pub(crate) fn bond_nominator(who: AccountId, val: Balance, target: Vec<AccountId>) {
	bond(who, val);
	assert_ok!(Staking::nominate(RuntimeOrigin::signed(who), target));
}

pub(crate) fn bond_virtual_nominator(
	who: AccountId,
	payee: AccountId,
	val: Balance,
	target: Vec<AccountId>,
) {
	// Bond who virtually.
	assert_ok!(<Staking as sp_staking::StakingUnchecked>::virtual_bond(&who, val, &payee));
	assert_ok!(Staking::nominate(RuntimeOrigin::signed(who), target));
}

/// Progress to the given block, triggering session and era changes as we progress.
///
/// This will finalize the previous block, initialize up to the given block, essentially simulating
/// a block import/propose process where we first initialize the block, then execute some stuff (not
/// in the function), and then finalize the block.
pub(crate) fn run_to_block(n: BlockNumber) {
	System::run_to_block_with::<AllPalletsWithSystem>(
		n,
		frame_system::RunToBlockHooks::default().after_initialize(|bn| {
			Timestamp::set_timestamp(bn * BLOCK_TIME + INIT_TIMESTAMP);
		}),
	);
}

/// Progresses from the current block number (whatever that may be) to the `P * session_index + 1`.
pub(crate) fn start_session(end_session_idx: SessionIndex) {
	let period = Period::get();
	let end: u64 = if Offset::get().is_zero() {
		(end_session_idx as u64) * period
	} else {
		Offset::get() + (end_session_idx.saturating_sub(1) as u64) * period
	};

	run_to_block(end);

	let curr_session_idx = Session::current_index();

	// session must have progressed properly.
	assert_eq!(
		curr_session_idx, end_session_idx,
		"current session index = {curr_session_idx}, expected = {end_session_idx}",
	);
}

/// Go one session forward.
pub(crate) fn advance_session() {
	let current_index = Session::current_index();
	start_session(current_index + 1);
}

/// Progress until the given era.
pub(crate) fn start_active_era(era_index: EraIndex) {
	start_session((era_index * <SessionsPerEra as Get<u32>>::get()).into());
	assert_eq!(active_era(), era_index);
	// One way or another, current_era must have changed before the active era, so they must match
	// at this point.
	assert_eq!(current_era(), active_era());
}

pub(crate) fn current_total_payout_for_duration(duration: u64) -> Balance {
	let (payout, _rest) = <Test as Config>::EraPayout::era_payout(
		pallet_staking::ErasTotalStake::<Test>::get(active_era()),
		pallet_balances::TotalIssuance::<Test>::get(),
		duration,
	);
	assert!(payout > 0);
	payout
}

pub(crate) fn maximum_payout_for_duration(duration: u64) -> Balance {
	let (payout, rest) = <Test as Config>::EraPayout::era_payout(
		pallet_staking::ErasTotalStake::<Test>::get(active_era()),
		pallet_balances::TotalIssuance::<Test>::get(),
		duration,
	);
	payout + rest
}

/// Time it takes to finish a session.
///
/// Note, if you see `time_per_session() - BLOCK_TIME`, it is fine. This is because we set the
/// timestamp after on_initialize, so the timestamp is always one block old.
pub(crate) fn time_per_session() -> u64 {
	Period::get() * BLOCK_TIME
}

/// Time it takes to finish an era.
///
/// Note, if you see `time_per_era() - BLOCK_TIME`, it is fine. This is because we set the
/// timestamp after on_initialize, so the timestamp is always one block old.
pub(crate) fn time_per_era() -> u64 {
	time_per_session() * SessionsPerEra::get() as u64
}

/// Time that will be calculated for the reward per era.
pub(crate) fn reward_time_per_era() -> u64 {
	time_per_era() - BLOCK_TIME
}

pub(crate) fn reward_all_elected() {
	let rewards = <Test as Config>::SessionInterface::validators().into_iter().map(|v| (v, 1));

	<Pallet<Test>>::reward_by_ids(rewards)
}

pub(crate) fn validator_controllers() -> Vec<AccountId> {
	Session::validators()
		.into_iter()
		.map(|s| Staking::bonded(&s).expect("no controller for validator"))
		.collect()
}

pub(crate) fn on_offence_in_era(
	offenders: &[OffenceDetails<
		AccountId,
		pallet_session::historical::IdentificationTuple<Test>,
	>],
	slash_fraction: &[Perbill],
	era: EraIndex,
) {
	let bonded_eras = crate::BondedEras::<Test>::get();
	for &(bonded_era, start_session) in bonded_eras.iter() {
		if bonded_era == era {
			let _ = <Staking as OnOffenceHandler<_, _, _>>::on_offence(
				offenders,
				slash_fraction,
				start_session,
			);
			return
		} else if bonded_era > era {
			break
		}
	}

	if pallet_staking::ActiveEra::<Test>::get().unwrap().index == era {
		let _ = <Staking as OnOffenceHandler<_, _, _>>::on_offence(
			offenders,
			slash_fraction,
			pallet_staking::ErasStartSessionIndex::<Test>::get(era).unwrap(),
		);
	} else {
		panic!("cannot slash in era {}", era);
	}
}

pub(crate) fn on_offence_now(
	offenders: &[OffenceDetails<
		AccountId,
		pallet_session::historical::IdentificationTuple<Test>,
	>],
	slash_fraction: &[Perbill],
) {
	let now = pallet_staking::ActiveEra::<Test>::get().unwrap().index;
	on_offence_in_era(offenders, slash_fraction, now)
}

pub(crate) fn offence_from(
	offender: AccountId,
	reporter: Option<Vec<AccountId>>,
) -> OffenceDetails<AccountId, pallet_session::historical::IdentificationTuple<Test>> {
	OffenceDetails { offender: (offender, ()), reporters: reporter.unwrap_or_default() }
}

pub(crate) fn add_slash(who: &AccountId) {
	on_offence_now(&[offence_from(*who, None)], &[Perbill::from_percent(10)]);
}

/// Make all validator and nominator request their payment
pub(crate) fn make_all_reward_payment(era: EraIndex) {
	let validators_with_reward = ErasRewardPoints::<Test>::get(era)
		.individual
		.keys()
		.cloned()
		.collect::<Vec<_>>();

	// reward validators
	for validator_controller in validators_with_reward.iter().filter_map(Staking::bonded) {
		let ledger = <Ledger<Test>>::get(&validator_controller).unwrap();
		for page in 0..EraInfo::<Test>::get_page_count(era, &ledger.stash) {
			assert_ok!(Staking::payout_stakers_by_page(
				RuntimeOrigin::signed(1337),
				ledger.stash,
				era,
				page
			));
		}
	}
}

pub(crate) fn bond_controller_stash(controller: AccountId, stash: AccountId) -> Result<(), String> {
	<Bonded<Test>>::get(&stash).map_or(Ok(()), |_| Err("stash already bonded"))?;
	<Ledger<Test>>::get(&controller).map_or(Ok(()), |_| Err("controller already bonded"))?;

	<Bonded<Test>>::insert(stash, controller);
	<Ledger<Test>>::insert(controller, StakingLedger::<Test>::default_from(stash));

	Ok(())
}

// simulates `set_controller` without corrupted ledger checks for testing purposes.
pub(crate) fn set_controller_no_checks(stash: &AccountId) {
	let controller = Bonded::<Test>::get(stash).expect("testing stash should be bonded");
	let ledger = Ledger::<Test>::get(&controller).expect("testing ledger should exist");

	Ledger::<Test>::remove(&controller);
	Ledger::<Test>::insert(stash, ledger);
	Bonded::<Test>::insert(stash, stash);
}

// simulates `bond_extra` without corrupted ledger checks for testing purposes.
pub(crate) fn bond_extra_no_checks(stash: &AccountId, amount: Balance) {
	let controller = Bonded::<Test>::get(stash).expect("bond must exist to bond_extra");
	let mut ledger = Ledger::<Test>::get(&controller).expect("ledger must exist to bond_extra");

	let new_total = ledger.total + amount;
	let _ = asset::update_stake::<Test>(stash, new_total);
	ledger.total = new_total;
	ledger.active = new_total;
	Ledger::<Test>::insert(controller, ledger);
}

pub(crate) fn setup_double_bonded_ledgers() {
	let init_ledgers = Ledger::<Test>::iter().count();

	let _ = asset::set_stakeable_balance::<Test>(&333, 2000);
	let _ = asset::set_stakeable_balance::<Test>(&444, 2000);
	let _ = asset::set_stakeable_balance::<Test>(&555, 2000);
	let _ = asset::set_stakeable_balance::<Test>(&777, 2000);

	assert_ok!(Staking::bond(RuntimeOrigin::signed(333), 10, RewardDestination::Staked));
	assert_ok!(Staking::bond(RuntimeOrigin::signed(444), 20, RewardDestination::Staked));
	assert_ok!(Staking::bond(RuntimeOrigin::signed(555), 20, RewardDestination::Staked));
	// not relevant to the test case, but ensures try-runtime checks pass.
	[333, 444, 555]
		.iter()
		.for_each(|s| Payee::<Test>::insert(s, RewardDestination::Staked));

	// we want to test the case where a controller can also be a stash of another ledger.
	// for that, we change the controller/stash bonding so that:
	// * 444 becomes controller of 333.
	// * 555 becomes controller of 444.
	// * 777 becomes controller of 555.
	let ledger_333 = Ledger::<Test>::get(333).unwrap();
	let ledger_444 = Ledger::<Test>::get(444).unwrap();
	let ledger_555 = Ledger::<Test>::get(555).unwrap();

	// 777 becomes controller of 555.
	Bonded::<Test>::mutate(555, |controller| *controller = Some(777));
	Ledger::<Test>::insert(777, ledger_555);

	// 555 becomes controller of 444.
	Bonded::<Test>::mutate(444, |controller| *controller = Some(555));
	Ledger::<Test>::insert(555, ledger_444);

	// 444 becomes controller of 333.
	Bonded::<Test>::mutate(333, |controller| *controller = Some(444));
	Ledger::<Test>::insert(444, ledger_333);

	// 333 is not controller anymore.
	Ledger::<Test>::remove(333);

	// checks. now we have:
	// * +3 ledgers
	assert_eq!(Ledger::<Test>::iter().count(), 3 + init_ledgers);

	// * stash 333 has controller 444.
	assert_eq!(Bonded::<Test>::get(333), Some(444));
	assert_eq!(StakingLedger::<Test>::paired_account(StakingAccount::Stash(333)), Some(444));
	assert_eq!(Ledger::<Test>::get(444).unwrap().stash, 333);

	// * stash 444 has controller 555.
	assert_eq!(Bonded::<Test>::get(444), Some(555));
	assert_eq!(StakingLedger::<Test>::paired_account(StakingAccount::Stash(444)), Some(555));
	assert_eq!(Ledger::<Test>::get(555).unwrap().stash, 444);

	// * stash 555 has controller 777.
	assert_eq!(Bonded::<Test>::get(555), Some(777));
	assert_eq!(StakingLedger::<Test>::paired_account(StakingAccount::Stash(555)), Some(777));
	assert_eq!(Ledger::<Test>::get(777).unwrap().stash, 555);
}

#[macro_export]
macro_rules! assert_session_era {
	($session:expr, $era:expr) => {
		assert_eq!(
			Session::current_index(),
			$session,
			"wrong session {} != {}",
			Session::current_index(),
			$session,
		);
		assert_eq!(
			CurrentEra::<T>::get().unwrap(),
			$era,
			"wrong current era {} != {}",
			CurrentEra::<T>::get().unwrap(),
			$era,
		);
	};
}

pub(crate) fn staking_events() -> Vec<crate::Event<Test>> {
	System::events()
		.into_iter()
		.map(|r| r.event)
		.filter_map(|e| if let RuntimeEvent::Staking(inner) = e { Some(inner) } else { None })
		.collect()
}

pub(crate) fn session_events() -> Vec<pallet_session::Event<Test>> {
	System::events()
		.into_iter()
		.map(|r| r.event)
		.filter_map(|e| if let RuntimeEvent::Session(inner) = e { Some(inner) } else { None })
		.collect()
}

parameter_types! {
	static StakingEventsIndex: usize = 0;
}
ord_parameter_types! {
	pub const One: u64 = 1;
}

type EnsureOneOrRoot = EitherOfDiverse<EnsureRoot<AccountId>, EnsureSignedBy<One, AccountId>>;

pub(crate) fn staking_events_since_last_call() -> Vec<crate::Event<Test>> {
	let all: Vec<_> = System::events()
		.into_iter()
		.filter_map(|r| if let RuntimeEvent::Staking(inner) = r.event { Some(inner) } else { None })
		.collect();
	let seen = StakingEventsIndex::get();
	StakingEventsIndex::set(all.len());
	all.into_iter().skip(seen).collect()
}

pub(crate) fn balances(who: &AccountId) -> (Balance, Balance) {
	(asset::stakeable_balance::<Test>(who), Balances::reserved_balance(who))
}

pub(crate) fn restrict(who: &AccountId) {
	if !RestrictedAccounts::get().contains(who) {
		RestrictedAccounts::mutate(|l| l.push(*who));
	}
}

pub(crate) fn remove_from_restrict_list(who: &AccountId) {
	RestrictedAccounts::mutate(|l| l.retain(|x| x != who));
}

pub(crate) fn to_bounded_supports(
	supports: Vec<(AccountId, Support<AccountId>)>,
) -> BoundedSupports<
	AccountId,
	<<Test as Config>::ElectionProvider as ElectionProvider>::MaxWinnersPerPage,
	<<Test as Config>::ElectionProvider as ElectionProvider>::MaxBackersPerWinner,
> {
	supports.try_into().unwrap()
}
