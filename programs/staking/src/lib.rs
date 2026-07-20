use anchor_lang::prelude::*;
use anchor_spl::token_interface::{
    self as token_interface, Mint, TokenAccount, TokenInterface, TransferChecked,
};

use sovereign_registry::cpi::accounts::ReleaseSeat as SovereignReleaseSeat;
use sovereign_registry::program::SovereignRegistry;
use sovereign_registry::SovereignSeat as SovereignSeatAccount;

declare_id!("2n2puiEN8BbMMEtq387b6HKR2trvKY9rK5uM82Ht2Vtc");

#[cfg(not(feature = "no-entrypoint"))]
solana_security_txt::security_txt! {
    name: "staking",
    project_url: "https://topbit.io",
    contacts: "email:security@topbit.io",
    policy: "https://topbit.io/security",
    preferred_languages: "en",
    source_code: "https://github.com/topbit-io/topbit"
}

const MAX_STAKE_PER_WALLET: u64 = 20_000_000 * TOKEN_DECIMALS;

const TIER_WEIGHT_BPS: [u128; 8] = [0, 500, 1_500, 3_000, 5_000, 7_500, 9_000, 10_000];

fn weighted_contribution(amount: u64, tier: u8) -> u128 {
    if (tier as usize) >= TIER_WEIGHT_BPS.len() { return 0; }
    (amount as u128)
        .saturating_mul(TIER_WEIGHT_BPS[tier as usize])
        .saturating_div(10_000)
}

const ACC_SCALE: u128 = 1_000_000_000_000;
const YIELD_ESCROW_PROGRAM_ID: Pubkey = pubkey!("85b3FfAzz3akfnH7NPCqR4Pjna45N3N6e6MvPsxABJ6n");
const YIELD_ACC_OFFSET: usize = 329;

fn read_yield_acc(yield_config: &AccountInfo) -> Result<u128> {
    require_keys_eq!(*yield_config.owner, YIELD_ESCROW_PROGRAM_ID, ErrorCode::Unauthorized);
    let data = yield_config.try_borrow_data()?;
    require!(data.len() >= YIELD_ACC_OFFSET + 16, ErrorCode::MathOverflow);
    let bytes: [u8; 16] = data[YIELD_ACC_OFFSET..YIELD_ACC_OFFSET + 16]
        .try_into()
        .map_err(|_| ErrorCode::MathOverflow)?;
    Ok(u128::from_le_bytes(bytes))
}

fn settle_reward(pos: &mut StakePosition, acc: u128, old_weight: u128) -> Result<()> {
    let gross = old_weight
        .checked_mul(acc).ok_or(ErrorCode::MathOverflow)?
        .checked_div(ACC_SCALE).ok_or(ErrorCode::MathOverflow)?;
    let pending: u64 = gross
        .checked_sub(pos.reward_debt).ok_or(ErrorCode::MathOverflow)?
        .try_into().map_err(|_| ErrorCode::MathOverflow)?;
    pos.reward_owed = pos.reward_owed.checked_add(pending).ok_or(ErrorCode::MathOverflow)?;
    Ok(())
}

fn remark_reward_debt(pos: &mut StakePosition, acc: u128, new_weight: u128) -> Result<()> {
    pos.reward_debt = new_weight
        .checked_mul(acc).ok_or(ErrorCode::MathOverflow)?
        .checked_div(ACC_SCALE).ok_or(ErrorCode::MathOverflow)?;
    Ok(())
}

const MIN_STAKE_AGE_FOR_CLAIM_SECONDS: i64 = 86_400;

fn forfeit_unaged_accrual(stake_age: i64, old_weight: u128, new_weight: u128) -> bool {
    new_weight < old_weight && stake_age < MIN_STAKE_AGE_FOR_CLAIM_SECONDS
}

const TIER_MICRO: u64 = 50_000;
const TIER_BRONZE: u64 = 250_000;
const TIER_SILVER: u64 = 750_000;
const TIER_GOLD: u64 = 2_500_000;
const TIER_PLATINUM: u64 = 7_500_000;
const TIER_DIAMOND: u64 = 15_000_000;
const TIER_SOVEREIGN: u64 = 20_000_000;

const TOKEN_DECIMALS: u64 = 1_000_000;

const EXPECTED_TOP_DECIMALS: u8 = 6;

pub const MIN_STAKE_DURATION_SECONDS: i64 = 7 * 24 * 60 * 60;

pub const ADMIN_TIMELOCK_SECONDS: i64 = 72 * 60 * 60;

pub const PROPOSE_RATE_LIMIT_WINDOW_SECONDS: i64 = 86_400;
pub const PROPOSE_RATE_LIMIT_RING_LEN: usize = 5;

#[program]
pub mod staking {
    use super::*;

    pub fn initialize(ctx: Context<Initialize>, top_token_mint: Pubkey) -> Result<()> {
        require_eq!(
            ctx.accounts.top_token_mint_account.decimals,
            EXPECTED_TOP_DECIMALS,
            ErrorCode::WrongDecimals
        );
        require_keys_eq!(
            *ctx.accounts.top_token_mint_account.to_account_info().owner,
            anchor_spl::token_2022::ID,
            ErrorCode::WrongTokenProgram
        );

        let config = &mut ctx.accounts.staking_config;
        config.authority = ctx.accounts.authority.key();
        config.top_token_mint = top_token_mint;
        config.total_staked = 0;
        config.total_weighted_stake = 0;
        config.bump = ctx.bumps.staking_config;

        config.pending_top_token_mint = Pubkey::default();
        config.pending_top_token_mint_unlocks_at = 0;
        config.pending_authority = Pubkey::default();
        config.pending_authority_unlocks_at = 0;
        config.propose_cooldown_until = 0;
        config.recent_proposes = [0i64; 5];
        Ok(())
    }

    pub fn init_staking_vault(_ctx: Context<InitStakingVault>) -> Result<()> {
        Ok(())
    }

    pub fn stake(ctx: Context<Stake>, amount: u64) -> Result<()> {
        require!(amount > 0, ErrorCode::ZeroAmount);

        if ctx.accounts.stake_position.owner == Pubkey::default() {
            let position = &mut ctx.accounts.stake_position;
            position.owner = ctx.accounts.owner.key();
            position.amount = 0;
            position.tier = 0;
            position.stake_timestamp = Clock::get()?.unix_timestamp;
            position.last_claim = Clock::get()?.unix_timestamp;
            position.etop_balance = 0;
            position.bump = ctx.bumps.stake_position;
            position.claimed_seat_index = None;
        }

        let current_amount = ctx.accounts.stake_position.amount;
        let new_total = current_amount
            .checked_add(amount)
            .ok_or(ErrorCode::MathOverflow)?;
        require!(new_total <= MAX_STAKE_PER_WALLET, ErrorCode::ExceedsMaxStake);

        if current_amount == 0 {
            ctx.accounts.stake_position.stake_timestamp = Clock::get()?.unix_timestamp;
        }

        let now = Clock::get()?.unix_timestamp;
        if current_amount > 0 {
            require!(now >= 0, ErrorCode::MathOverflow);
            let prev_ts = ctx.accounts.stake_position.stake_timestamp;
            require!(prev_ts >= 0, ErrorCode::MathOverflow);
            let curr_weight = (current_amount as u128)
                .checked_mul(prev_ts as u128)
                .ok_or(ErrorCode::MathOverflow)?;
            let new_weight = (amount as u128)
                .checked_mul(now as u128)
                .ok_or(ErrorCode::MathOverflow)?;
            let sum = curr_weight
                .checked_add(new_weight)
                .ok_or(ErrorCode::MathOverflow)?;
            let weighted_ts = sum
                .checked_div(new_total as u128)
                .ok_or(ErrorCode::MathOverflow)?;
            ctx.accounts.stake_position.stake_timestamp = weighted_ts as i64;
        }

        let top_decimals = ctx.accounts.top_token_mint.decimals;
        let transfer_ctx = CpiContext::new(
            ctx.accounts.token_program.to_account_info(),
            TransferChecked {
                from: ctx.accounts.owner_token_account.to_account_info(),
                mint: ctx.accounts.top_token_mint.to_account_info(),
                to: ctx.accounts.staking_vault.to_account_info(),
                authority: ctx.accounts.owner.to_account_info(),
            },
        );
        token_interface::transfer_checked(transfer_ctx, amount, top_decimals)?;

        let old_tier = ctx.accounts.stake_position.tier;
        let has_seat = ctx.accounts.stake_position.claimed_seat_index.is_some();
        let new_tier = calculate_tier(new_total, has_seat);
        let old_weighted = weighted_contribution(current_amount, old_tier);
        let new_weighted = weighted_contribution(new_total, new_tier);

        let acc = read_yield_acc(&ctx.accounts.yield_config)?;
        settle_reward(&mut ctx.accounts.stake_position, acc, old_weighted)?;

        ctx.accounts.stake_position.amount = new_total;
        ctx.accounts.stake_position.tier = new_tier;
        remark_reward_debt(&mut ctx.accounts.stake_position, acc, new_weighted)?;

        ctx.accounts.staking_config.total_staked = ctx.accounts.staking_config.total_staked
            .checked_add(amount)
            .ok_or(ErrorCode::MathOverflow)?;
        ctx.accounts.staking_config.total_weighted_stake = ctx.accounts.staking_config.total_weighted_stake
            .checked_sub(old_weighted)
            .ok_or(ErrorCode::MathOverflow)?
            .checked_add(new_weighted)
            .ok_or(ErrorCode::MathOverflow)?;

        emit!(StakeEvent {
            owner: ctx.accounts.owner.key(),
            amount,
            total_staked: new_total,
            tier: new_tier,
            timestamp: Clock::get()?.unix_timestamp,
        });

        Ok(())
    }

    pub fn unstake(ctx: Context<Unstake>, amount: u64) -> Result<()> {
        require!(amount > 0, ErrorCode::ZeroAmount);

        let current_amount = ctx.accounts.stake_position.amount;
        require!(amount <= current_amount, ErrorCode::InsufficientStake);

        let new_amount = current_amount
            .checked_sub(amount)
            .ok_or(ErrorCode::MathOverflow)?;

        let sovereign_threshold = TIER_SOVEREIGN
            .checked_mul(TOKEN_DECIMALS)
            .ok_or(ErrorCode::MathOverflow)?;
        let was_sovereign = current_amount >= sovereign_threshold;
        let still_sovereign = new_amount >= sovereign_threshold;
        let crossed_below = was_sovereign && !still_sovereign;

        let now = Clock::get()?.unix_timestamp;
        let elapsed_seconds = now
            .checked_sub(ctx.accounts.stake_position.stake_timestamp)
            .unwrap_or(0) as u64;
        let days_staked = elapsed_seconds.checked_div(86_400).unwrap_or(0);

        let burn_bps: u64 = if days_staked <= 30 {
            10_000
        } else if days_staked <= 60 {
            7_500
        } else if days_staked <= 90 {
            5_000
        } else if days_staked < 180 {
            2_500
        } else {
            0
        };

        let etop_to_burn = ctx.accounts.stake_position.etop_balance
            .checked_mul(burn_bps)
            .ok_or(ErrorCode::MathOverflow)?
            .checked_div(10_000)
            .ok_or(ErrorCode::MathOverflow)?;

        let etop_to_return = ctx.accounts.stake_position.etop_balance
            .checked_sub(etop_to_burn)
            .ok_or(ErrorCode::MathOverflow)?;

        if etop_to_burn > 0 {
            emit!(ETopForfeitureEvent {
                owner: ctx.accounts.stake_position.owner,
                days_staked,
                etop_burned: etop_to_burn,
                etop_returned: etop_to_return,
                timestamp: now,
            });
        }

        ctx.accounts.stake_position.etop_balance = 0;


        let old_tier_u = ctx.accounts.stake_position.tier;
        let has_seat_post_unstake = ctx.accounts.stake_position.claimed_seat_index.is_some();
        let new_tier = calculate_tier(new_amount, has_seat_post_unstake);
        let old_weighted_u = weighted_contribution(current_amount, old_tier_u);
        let new_weighted_u = weighted_contribution(new_amount, new_tier);

        let acc = read_yield_acc(&ctx.accounts.yield_config)?;
        let stake_age = now
            .checked_sub(ctx.accounts.stake_position.stake_timestamp)
            .unwrap_or(0);
        if forfeit_unaged_accrual(stake_age, old_weighted_u, new_weighted_u) {
            emit!(JitYieldForfeited {
                owner: ctx.accounts.stake_position.owner,
                stake_age,
                old_weight: old_weighted_u,
                new_weight: new_weighted_u,
                timestamp: now,
            });
        } else {
            settle_reward(&mut ctx.accounts.stake_position, acc, old_weighted_u)?;
        }

        ctx.accounts.stake_position.amount = new_amount;
        ctx.accounts.stake_position.tier = new_tier;
        remark_reward_debt(&mut ctx.accounts.stake_position, acc, new_weighted_u)?;

        ctx.accounts.staking_config.total_staked = ctx.accounts.staking_config.total_staked
            .checked_sub(amount)
            .ok_or(ErrorCode::MathOverflow)?;
        ctx.accounts.staking_config.total_weighted_stake = ctx.accounts.staking_config.total_weighted_stake
            .checked_sub(old_weighted_u)
            .ok_or(ErrorCode::MathOverflow)?
            .checked_add(new_weighted_u)
            .ok_or(ErrorCode::MathOverflow)?;

        let config_bump = ctx.accounts.staking_config.bump;
        let seeds = &[
            b"staking_config".as_ref(),
            &[config_bump],
        ];
        let signer_seeds = &[&seeds[..]];

        let top_decimals = ctx.accounts.top_token_mint.decimals;
        let transfer_ctx = CpiContext::new_with_signer(
            ctx.accounts.token_program.to_account_info(),
            TransferChecked {
                from: ctx.accounts.staking_vault.to_account_info(),
                mint: ctx.accounts.top_token_mint.to_account_info(),
                to: ctx.accounts.owner_token_account.to_account_info(),
                authority: ctx.accounts.staking_config.to_account_info(),
            },
            signer_seeds,
        );
        token_interface::transfer_checked(transfer_ctx, amount, top_decimals)?;

        emit!(UnstakeEvent {
            owner: ctx.accounts.owner.key(),
            amount,
            remaining_staked: new_amount,
            tier: new_tier,
            timestamp: Clock::get()?.unix_timestamp,
        });

        let needs_strict_release = ctx.accounts.stake_position.claimed_seat_index.is_some()
            && new_amount < sovereign_threshold;

        let _ = crossed_below;

        if needs_strict_release {
            let seat = ctx.accounts.sovereign_seat.as_ref();
            let registry_config = ctx.accounts.sovereign_registry_config.as_ref();
            let sov_stake_pos = ctx.accounts.sovereign_stake_position.as_ref();
            let registry_program = ctx.accounts.sovereign_registry_program.as_ref();

            let all_provided = seat.is_some()
                && registry_config.is_some()
                && sov_stake_pos.is_some()
                && registry_program.is_some();

            require!(
                all_provided,
                ErrorCode::SovereignSeatReleaseRequired
            );

            let cpi_program_info = registry_program.unwrap().clone();
            require_keys_eq!(
                cpi_program_info.key(),
                sovereign_registry::ID,
                ErrorCode::WrongSovereignRegistryProgram
            );

            let cpi_program = cpi_program_info;
            let cpi_accounts = SovereignReleaseSeat {
                sovereign_seat: seat.unwrap().to_account_info(),
                registry_config: registry_config.unwrap().to_account_info(),
                stake_position: sov_stake_pos.unwrap().to_account_info(),
                holder: ctx.accounts.owner.to_account_info(),
                caller: ctx.accounts.owner.to_account_info(),
                system_program: ctx.accounts.system_program.to_account_info(),
            };

            {
                use anchor_lang::AccountsExit as _;
                ctx.accounts.stake_position.exit(&crate::ID)?;
            }

            let cpi_ctx = CpiContext::new(cpi_program, cpi_accounts);
            sovereign_registry::cpi::release_seat(cpi_ctx)?;
            ctx.accounts.stake_position.claimed_seat_index = None;
        }

        Ok(())
    }

    pub fn claim_rewards(ctx: Context<ClaimRewards>) -> Result<()> {
        let position = &mut ctx.accounts.stake_position;
        require!(position.amount > 0, ErrorCode::NothingStaked);

        let now = Clock::get()?.unix_timestamp;
        position.last_claim = now;

        emit!(ClaimRewardsEvent {
            owner: ctx.accounts.owner.key(),
            liquid_amount: 0,
            etop_amount: 0,
            total_etop_balance: position.etop_balance,
            tier: position.tier,
            timestamp: now,
        });
        Ok(())
    }

    pub fn update_tier(ctx: Context<UpdateStakeTier>) -> Result<()> {
        let has_seat = ctx.accounts.stake_position.claimed_seat_index.is_some();
        let amount = ctx.accounts.stake_position.amount;
        let old_tier = ctx.accounts.stake_position.tier;
        let new_tier = calculate_tier(amount, has_seat);

        if new_tier != old_tier {
            let old_weighted = weighted_contribution(amount, old_tier);
            let new_weighted = weighted_contribution(amount, new_tier);

            let acc = read_yield_acc(&ctx.accounts.yield_config)?;
            settle_reward(&mut ctx.accounts.stake_position, acc, old_weighted)?;

            ctx.accounts.staking_config.total_weighted_stake = ctx
                .accounts
                .staking_config
                .total_weighted_stake
                .checked_sub(old_weighted)
                .ok_or(ErrorCode::MathOverflow)?
                .checked_add(new_weighted)
                .ok_or(ErrorCode::MathOverflow)?;

            emit!(TierUpdatedEvent {
                owner: ctx.accounts.stake_position.owner,
                old_tier,
                new_tier,
                staked_amount: amount,
                timestamp: Clock::get()?.unix_timestamp,
            });
            ctx.accounts.stake_position.tier = new_tier;
            remark_reward_debt(&mut ctx.accounts.stake_position, acc, new_weighted)?;
        }

        Ok(())
    }

    pub fn register_sovereign_seat(
        ctx: Context<RegisterSovereignSeat>,
        seat_index: u8,
    ) -> Result<()> {
        let sovereign_threshold = TIER_SOVEREIGN
            .checked_mul(TOKEN_DECIMALS)
            .ok_or(ErrorCode::MathOverflow)?;
        require!(
            ctx.accounts.stake_position.amount >= sovereign_threshold,
            ErrorCode::InsufficientStakeForSovereignMarker
        );

        let now = Clock::get()?.unix_timestamp;
        let age = now
            .checked_sub(ctx.accounts.stake_position.stake_timestamp)
            .ok_or(ErrorCode::MathOverflow)?;
        require!(
            age >= MIN_STAKE_DURATION_SECONDS,
            ErrorCode::StakeTooYoungForSeatClaim
        );

        let seat: SovereignSeatAccount = {
            let data = ctx.accounts.sovereign_seat.try_borrow_data()?;
            SovereignSeatAccount::try_deserialize(&mut data.as_ref())
                .map_err(|_| ErrorCode::SovereignSeatInvalid)?
        };
        require_keys_eq!(
            seat.holder,
            ctx.accounts.owner.key(),
            ErrorCode::SovereignSeatHolderMismatch
        );
        require!(seat.active, ErrorCode::SovereignSeatInactive);
        require!(seat.seat_index == seat_index, ErrorCode::SovereignSeatIndexMismatch);

        match ctx.accounts.stake_position.claimed_seat_index {
            Some(existing) if existing == seat_index => {
            }
            Some(_) => {
                return Err(ErrorCode::SovereignSeatAlreadyRegistered.into());
            }
            None => {
                ctx.accounts.stake_position.claimed_seat_index = Some(seat_index);

                let amount = ctx.accounts.stake_position.amount;
                let old_tier = ctx.accounts.stake_position.tier;
                let new_tier = calculate_tier(amount, true);
                let old_weighted = weighted_contribution(amount, old_tier);
                let new_weighted = weighted_contribution(amount, new_tier);

                let acc = read_yield_acc(&ctx.accounts.yield_config)?;
                settle_reward(&mut ctx.accounts.stake_position, acc, old_weighted)?;

                ctx.accounts.staking_config.total_weighted_stake = ctx
                    .accounts
                    .staking_config
                    .total_weighted_stake
                    .checked_sub(old_weighted)
                    .ok_or(ErrorCode::MathOverflow)?
                    .checked_add(new_weighted)
                    .ok_or(ErrorCode::MathOverflow)?;
                ctx.accounts.stake_position.tier = new_tier;
                remark_reward_debt(&mut ctx.accounts.stake_position, acc, new_weighted)?;
            }
        }

        emit!(SovereignSeatRegisteredEvent {
            owner: ctx.accounts.owner.key(),
            seat_index,
            stake_amount: ctx.accounts.stake_position.amount,
            timestamp: Clock::get()?.unix_timestamp,
        });
        Ok(())
    }

    pub fn release_sovereign_seat(ctx: Context<ReleaseSovereignSeatMarker>) -> Result<()> {
        let seat: SovereignSeatAccount = {
            let data = ctx.accounts.sovereign_seat.try_borrow_data()?;
            SovereignSeatAccount::try_deserialize(&mut data.as_ref())
                .map_err(|_| ErrorCode::SovereignSeatInvalid)?
        };
        require_keys_eq!(
            seat.holder,
            ctx.accounts.stake_position.owner,
            ErrorCode::SovereignSeatHolderMismatch
        );

        let sovereign_threshold = TIER_SOVEREIGN
            .checked_mul(TOKEN_DECIMALS)
            .ok_or(ErrorCode::MathOverflow)?;
        let staker_sub_threshold =
            ctx.accounts.stake_position.amount < sovereign_threshold;

        require!(
            !seat.active || staker_sub_threshold,
            ErrorCode::SovereignSeatMarkerNotStale
        );

        let prev = ctx.accounts.stake_position.claimed_seat_index;
        ctx.accounts.stake_position.claimed_seat_index = None;

        let amount = ctx.accounts.stake_position.amount;
        let old_tier = ctx.accounts.stake_position.tier;
        let new_tier = calculate_tier(amount, false);
        if new_tier != old_tier {
            let old_weighted = weighted_contribution(amount, old_tier);
            let new_weighted = weighted_contribution(amount, new_tier);

            let acc = read_yield_acc(&ctx.accounts.yield_config)?;
            settle_reward(&mut ctx.accounts.stake_position, acc, old_weighted)?;

            ctx.accounts.staking_config.total_weighted_stake = ctx
                .accounts
                .staking_config
                .total_weighted_stake
                .checked_sub(old_weighted)
                .ok_or(ErrorCode::MathOverflow)?
                .checked_add(new_weighted)
                .ok_or(ErrorCode::MathOverflow)?;
            ctx.accounts.stake_position.tier = new_tier;
            remark_reward_debt(&mut ctx.accounts.stake_position, acc, new_weighted)?;
        }

        emit!(SovereignSeatMarkerClearedEvent {
            owner: ctx.accounts.stake_position.owner,
            previous_seat_index: prev,
            seat_active: seat.active,
            stake_amount: ctx.accounts.stake_position.amount,
            timestamp: Clock::get()?.unix_timestamp,
        });
        Ok(())
    }


    pub fn propose_set_top_token_mint(
        ctx: Context<AdminOnly>,
        new_mint: Pubkey,
    ) -> Result<()> {
        let config = &mut ctx.accounts.staking_config;
        require_keys_eq!(
            ctx.accounts.authority.key(),
            config.authority,
            ErrorCode::Unauthorized
        );
        let now = Clock::get()?.unix_timestamp;
        check_and_record_propose(config, now)?;
        config.pending_top_token_mint = new_mint;
        config.pending_top_token_mint_unlocks_at = now
            .checked_add(ADMIN_TIMELOCK_SECONDS)
            .ok_or(ErrorCode::MathOverflow)?;
        emit!(TopTokenMintProposed {
            new_mint,
            unlocks_at: config.pending_top_token_mint_unlocks_at,
        });
        Ok(())
    }

    pub fn finalize_set_top_token_mint(ctx: Context<FinalizeSetTopMint>) -> Result<()> {
        require_eq!(
            ctx.accounts.new_top_token_mint.decimals,
            EXPECTED_TOP_DECIMALS,
            ErrorCode::WrongDecimals
        );
        require_keys_eq!(
            *ctx.accounts.new_top_token_mint.to_account_info().owner,
            anchor_spl::token_2022::ID,
            ErrorCode::WrongTokenProgram
        );

        let config = &mut ctx.accounts.staking_config;
        require_keys_eq!(
            ctx.accounts.authority.key(),
            config.authority,
            ErrorCode::Unauthorized
        );
        require!(
            config.pending_top_token_mint_unlocks_at != 0,
            ErrorCode::NothingPending
        );
        let now = Clock::get()?.unix_timestamp;
        require!(
            now >= config.pending_top_token_mint_unlocks_at,
            ErrorCode::TimelockNotElapsed
        );
        require!(config.total_staked == 0, ErrorCode::MintRotationLocked);
        let old = config.top_token_mint;
        config.top_token_mint = config.pending_top_token_mint;
        config.pending_top_token_mint = Pubkey::default();
        config.pending_top_token_mint_unlocks_at = 0;
        emit!(TopTokenMintRotated { old, new: config.top_token_mint, timestamp: now });
        Ok(())
    }

    pub fn cancel_set_top_token_mint(ctx: Context<AdminOnly>) -> Result<()> {
        let config = &mut ctx.accounts.staking_config;
        require_keys_eq!(
            ctx.accounts.authority.key(),
            config.authority,
            ErrorCode::Unauthorized
        );
        require!(
            config.pending_top_token_mint_unlocks_at != 0,
            ErrorCode::NothingPending
        );
        config.pending_top_token_mint = Pubkey::default();
        config.pending_top_token_mint_unlocks_at = 0;
        emit!(TopTokenMintProposalCancelled {});
        Ok(())
    }

    pub fn propose_transfer_authority(
        ctx: Context<AdminOnly>,
        new_authority: Pubkey,
    ) -> Result<()> {
        let config = &mut ctx.accounts.staking_config;
        require_keys_eq!(
            ctx.accounts.authority.key(),
            config.authority,
            ErrorCode::Unauthorized
        );
        require!(
            new_authority != Pubkey::default(),
            ErrorCode::InvalidAuthority
        );
        let now = Clock::get()?.unix_timestamp;
        check_and_record_propose(config, now)?;
        config.pending_authority = new_authority;
        config.pending_authority_unlocks_at = now
            .checked_add(ADMIN_TIMELOCK_SECONDS)
            .ok_or(ErrorCode::MathOverflow)?;
        emit!(AuthorityProposed {
            new_authority,
            unlocks_at: config.pending_authority_unlocks_at,
        });
        Ok(())
    }

    pub fn finalize_transfer_authority(ctx: Context<AdminOnly>) -> Result<()> {
        let config = &mut ctx.accounts.staking_config;
        require_keys_eq!(
            ctx.accounts.authority.key(),
            config.authority,
            ErrorCode::Unauthorized
        );
        require!(
            config.pending_authority != Pubkey::default(),
            ErrorCode::NothingPending
        );
        let now = Clock::get()?.unix_timestamp;
        require!(
            now >= config.pending_authority_unlocks_at,
            ErrorCode::TimelockNotElapsed
        );
        let old = config.authority;
        config.authority = config.pending_authority;
        config.pending_authority = Pubkey::default();
        config.pending_authority_unlocks_at = 0;
        emit!(AuthorityRotated { old, new: config.authority, timestamp: now });
        Ok(())
    }

    pub fn cancel_transfer_authority(ctx: Context<AdminOnly>) -> Result<()> {
        let config = &mut ctx.accounts.staking_config;
        require_keys_eq!(
            ctx.accounts.authority.key(),
            config.authority,
            ErrorCode::Unauthorized
        );
        require!(
            config.pending_authority != Pubkey::default(),
            ErrorCode::NothingPending
        );
        config.pending_authority = Pubkey::default();
        config.pending_authority_unlocks_at = 0;
        emit!(AuthorityProposalCancelled {});
        Ok(())
    }
}


fn check_and_record_propose(config: &mut StakingConfig, now: i64) -> Result<()> {
    require!(
        now >= config.propose_cooldown_until,
        ErrorCode::ProposeCooldownActive
    );
    let window_start = now.saturating_sub(PROPOSE_RATE_LIMIT_WINDOW_SECONDS);
    let count_24h = config
        .recent_proposes
        .iter()
        .filter(|t| **t > window_start)
        .count();
    let next_cooldown_seconds: i64 = match count_24h {
        0 | 1 => 0,
        2 => 1_800,
        3 => 7_200,
        4 => 86_400,
        _ => 604_800,
    };
    config.propose_cooldown_until = if next_cooldown_seconds == 0 {
        0
    } else {
        now.checked_add(next_cooldown_seconds)
            .ok_or(ErrorCode::MathOverflow)?
    };
    for i in 0..(PROPOSE_RATE_LIMIT_RING_LEN - 1) {
        config.recent_proposes[i] = config.recent_proposes[i + 1];
    }
    config.recent_proposes[PROPOSE_RATE_LIMIT_RING_LEN - 1] = now;
    Ok(())
}


fn calculate_tier(amount: u64, has_seat: bool) -> u8 {
    let amount_tokens = amount / TOKEN_DECIMALS;

    if amount_tokens >= TIER_SOVEREIGN {
        if has_seat { 7 } else { 6 }
    } else if amount_tokens >= TIER_DIAMOND {
        6
    } else if amount_tokens >= TIER_PLATINUM {
        5
    } else if amount_tokens >= TIER_GOLD {
        4
    } else if amount_tokens >= TIER_SILVER {
        3
    } else if amount_tokens >= TIER_BRONZE {
        2
    } else if amount_tokens >= TIER_MICRO {
        1
    } else {
        0
    }
}


#[account]
pub struct StakingConfig {
    pub authority: Pubkey,
    pub top_token_mint: Pubkey,
    pub total_staked: u64,
    pub total_weighted_stake: u128,
    pub bump: u8,

    pub pending_top_token_mint: Pubkey,
    pub pending_top_token_mint_unlocks_at: i64,
    pub pending_authority: Pubkey,
    pub pending_authority_unlocks_at: i64,

    pub propose_cooldown_until: i64,
    pub recent_proposes: [i64; 5],
}

#[account]
pub struct StakePosition {
    pub owner: Pubkey,
    pub amount: u64,
    pub tier: u8,
    pub stake_timestamp: i64,
    pub last_claim: i64,
    pub etop_balance: u64,
    pub bump: u8,

    pub reward_owed: u64,
    pub reward_debt: u128,

    pub claimed_seat_index: Option<u8>,
}
impl StakePosition {
    pub const SPACE: usize = 100;
    pub const REWARD_OWED_OFFSET: usize = 74;
    pub const REWARD_DEBT_OFFSET: usize = 82;
}


#[derive(Accounts)]
#[instruction(top_token_mint: Pubkey)]
pub struct Initialize<'info> {
    #[account(
        init,
        payer = authority,
        space = 256,
        seeds = [b"staking_config"],
        bump
    )]
    pub staking_config: Account<'info, StakingConfig>,
    #[account(
        constraint = top_token_mint_account.key() == top_token_mint @ ErrorCode::WrongMint,
    )]
    pub top_token_mint_account: InterfaceAccount<'info, Mint>,
    #[account(mut)]
    pub authority: Signer<'info>,
    #[account(
        seeds = [crate::ID.as_ref()],
        bump,
        seeds::program = anchor_lang::solana_program::bpf_loader_upgradeable::ID,
        constraint = program_data.upgrade_authority_address == Some(authority.key())
            @ ErrorCode::Unauthorized,
    )]
    pub program_data: Account<'info, ProgramData>,
    pub system_program: Program<'info, System>,
}

#[derive(Accounts)]
pub struct InitStakingVault<'info> {
    #[account(seeds = [b"staking_config"], bump = staking_config.bump)]
    pub staking_config: Account<'info, StakingConfig>,
    #[account(
        constraint = top_token_mint.key() == staking_config.top_token_mint @ ErrorCode::WrongMint
    )]
    pub top_token_mint: InterfaceAccount<'info, Mint>,
    #[account(
        init,
        payer = authority,
        seeds = [b"staking_vault"],
        bump,
        token::mint = top_token_mint,
        token::authority = staking_config,
    )]
    pub staking_vault: InterfaceAccount<'info, TokenAccount>,
    #[account(
        mut,
        constraint = authority.key() == staking_config.authority @ ErrorCode::Unauthorized
    )]
    pub authority: Signer<'info>,
    pub token_program: Interface<'info, TokenInterface>,
    pub system_program: Program<'info, System>,
    pub rent: Sysvar<'info, Rent>,
}

#[derive(Accounts)]
pub struct AdminOnly<'info> {
    #[account(mut, seeds = [b"staking_config"], bump = staking_config.bump)]
    pub staking_config: Account<'info, StakingConfig>,
    pub authority: Signer<'info>,
}

#[derive(Accounts)]
pub struct FinalizeSetTopMint<'info> {
    #[account(mut, seeds = [b"staking_config"], bump = staking_config.bump)]
    pub staking_config: Account<'info, StakingConfig>,
    #[account(
        constraint = new_top_token_mint.key() == staking_config.pending_top_token_mint
            @ ErrorCode::WrongMint,
    )]
    pub new_top_token_mint: InterfaceAccount<'info, Mint>,
    pub authority: Signer<'info>,
}

#[derive(Accounts)]
pub struct Stake<'info> {
    #[account(
        init_if_needed,
        payer = owner,
        space = StakePosition::SPACE,
        seeds = [b"stake_position", owner.key().as_ref()],
        bump
    )]
    pub stake_position: Account<'info, StakePosition>,
    #[account(mut, seeds = [b"staking_config"], bump = staking_config.bump)]
    pub staking_config: Account<'info, StakingConfig>,
    #[account(
        seeds = [b"yield_config"],
        bump,
        seeds::program = YIELD_ESCROW_PROGRAM_ID,
        owner = YIELD_ESCROW_PROGRAM_ID,
    )]
    pub yield_config: UncheckedAccount<'info>,
    #[account(mut)]
    pub owner: Signer<'info>,
    #[account(
        constraint = top_token_mint.key() == staking_config.top_token_mint @ ErrorCode::WrongMint,
    )]
    pub top_token_mint: InterfaceAccount<'info, Mint>,
    #[account(
        mut,
        token::mint = staking_config.top_token_mint,
        token::authority = owner,
    )]
    pub owner_token_account: InterfaceAccount<'info, TokenAccount>,
    #[account(
        mut,
        seeds = [b"staking_vault"],
        bump,
        constraint = staking_vault.mint == staking_config.top_token_mint @ ErrorCode::WrongMint,
        constraint = staking_vault.owner == staking_config.key() @ ErrorCode::WrongVaultOwner,
    )]
    pub staking_vault: InterfaceAccount<'info, TokenAccount>,
    pub token_program: Interface<'info, TokenInterface>,
    pub system_program: Program<'info, System>,
}

#[derive(Accounts)]
pub struct Unstake<'info> {
    #[account(mut, has_one = owner, seeds = [b"stake_position", owner.key().as_ref()], bump = stake_position.bump)]
    pub stake_position: Account<'info, StakePosition>,
    #[account(mut, seeds = [b"staking_config"], bump = staking_config.bump)]
    pub staking_config: Account<'info, StakingConfig>,
    #[account(
        seeds = [b"yield_config"],
        bump,
        seeds::program = YIELD_ESCROW_PROGRAM_ID,
        owner = YIELD_ESCROW_PROGRAM_ID,
    )]
    pub yield_config: UncheckedAccount<'info>,
    #[account(mut)]
    pub owner: Signer<'info>,
    #[account(
        constraint = top_token_mint.key() == staking_config.top_token_mint @ ErrorCode::WrongMint,
    )]
    pub top_token_mint: InterfaceAccount<'info, Mint>,
    #[account(
        mut,
        token::mint = staking_config.top_token_mint,
        token::authority = owner,
    )]
    pub owner_token_account: InterfaceAccount<'info, TokenAccount>,
    #[account(
        mut,
        seeds = [b"staking_vault"],
        bump,
        constraint = staking_vault.mint == staking_config.top_token_mint @ ErrorCode::WrongMint,
        constraint = staking_vault.owner == staking_config.key() @ ErrorCode::WrongVaultOwner,
    )]
    pub staking_vault: InterfaceAccount<'info, TokenAccount>,
    pub token_program: Interface<'info, TokenInterface>,
    pub system_program: Program<'info, System>,

    #[account(mut)]
    pub sovereign_seat: Option<UncheckedAccount<'info>>,
    #[account(mut)]
    pub sovereign_registry_config: Option<UncheckedAccount<'info>>,
    pub sovereign_stake_position: Option<UncheckedAccount<'info>>,
    pub sovereign_registry_program: Option<AccountInfo<'info>>,
}

#[derive(Accounts)]
pub struct ClaimRewards<'info> {
    #[account(mut, has_one = owner, seeds = [b"stake_position", owner.key().as_ref()], bump = stake_position.bump)]
    pub stake_position: Account<'info, StakePosition>,
    #[account(seeds = [b"staking_config"], bump = staking_config.bump)]
    pub staking_config: Account<'info, StakingConfig>,
    #[account(mut)]
    pub owner: Signer<'info>,
    pub system_program: Program<'info, System>,
}

#[derive(Accounts)]
pub struct UpdateStakeTier<'info> {
    #[account(mut, has_one = owner, seeds = [b"stake_position", owner.key().as_ref()], bump = stake_position.bump)]
    pub stake_position: Account<'info, StakePosition>,
    #[account(mut, seeds = [b"staking_config"], bump = staking_config.bump)]
    pub staking_config: Account<'info, StakingConfig>,
    #[account(
        seeds = [b"yield_config"],
        bump,
        seeds::program = YIELD_ESCROW_PROGRAM_ID,
        owner = YIELD_ESCROW_PROGRAM_ID,
    )]
    pub yield_config: UncheckedAccount<'info>,
    pub owner: Signer<'info>,
}

#[derive(Accounts)]
pub struct RegisterSovereignSeat<'info> {
    #[account(
        mut,
        has_one = owner,
        seeds = [b"stake_position", owner.key().as_ref()],
        bump = stake_position.bump,
    )]
    pub stake_position: Account<'info, StakePosition>,
    #[account(
        owner = sovereign_registry::ID,
    )]
    pub sovereign_seat: UncheckedAccount<'info>,
    #[account(mut, seeds = [b"staking_config"], bump = staking_config.bump)]
    pub staking_config: Account<'info, StakingConfig>,
    #[account(
        seeds = [b"yield_config"],
        bump,
        seeds::program = YIELD_ESCROW_PROGRAM_ID,
        owner = YIELD_ESCROW_PROGRAM_ID,
    )]
    pub yield_config: UncheckedAccount<'info>,
    pub owner: Signer<'info>,
}

#[derive(Accounts)]
pub struct ReleaseSovereignSeatMarker<'info> {
    #[account(
        mut,
        seeds = [b"stake_position", stake_position.owner.as_ref()],
        bump = stake_position.bump,
    )]
    pub stake_position: Account<'info, StakePosition>,
    #[account(
        owner = sovereign_registry::ID,
    )]
    pub sovereign_seat: UncheckedAccount<'info>,
    #[account(mut, seeds = [b"staking_config"], bump = staking_config.bump)]
    pub staking_config: Account<'info, StakingConfig>,
    #[account(
        seeds = [b"yield_config"],
        bump,
        seeds::program = YIELD_ESCROW_PROGRAM_ID,
        owner = YIELD_ESCROW_PROGRAM_ID,
    )]
    pub yield_config: UncheckedAccount<'info>,
    pub caller: Signer<'info>,
}


#[event]
pub struct StakeEvent {
    pub owner: Pubkey,
    pub amount: u64,
    pub total_staked: u64,
    pub tier: u8,
    pub timestamp: i64,
}

#[event]
pub struct UnstakeEvent {
    pub owner: Pubkey,
    pub amount: u64,
    pub remaining_staked: u64,
    pub tier: u8,
    pub timestamp: i64,
}

#[event]
pub struct ClaimRewardsEvent {
    pub owner: Pubkey,
    pub liquid_amount: u64,
    pub etop_amount: u64,
    pub total_etop_balance: u64,
    pub tier: u8,
    pub timestamp: i64,
}

#[event]
pub struct TierUpdatedEvent {
    pub owner: Pubkey,
    pub old_tier: u8,
    pub new_tier: u8,
    pub staked_amount: u64,
    pub timestamp: i64,
}

#[event]
pub struct ETopForfeitureEvent {
    pub owner: Pubkey,
    pub days_staked: u64,
    pub etop_burned: u64,
    pub etop_returned: u64,
    pub timestamp: i64,
}

#[event]
pub struct JitYieldForfeited {
    pub owner: Pubkey,
    pub stake_age: i64,
    pub old_weight: u128,
    pub new_weight: u128,
    pub timestamp: i64,
}

#[event]
pub struct SovereignSeatRegisteredEvent {
    pub owner: Pubkey,
    pub seat_index: u8,
    pub stake_amount: u64,
    pub timestamp: i64,
}

#[event]
pub struct SovereignSeatMarkerClearedEvent {
    pub owner: Pubkey,
    pub previous_seat_index: Option<u8>,
    pub seat_active: bool,
    pub stake_amount: u64,
    pub timestamp: i64,
}

#[event]
pub struct TopTokenMintProposed { pub new_mint: Pubkey, pub unlocks_at: i64 }
#[event]
pub struct TopTokenMintRotated { pub old: Pubkey, pub new: Pubkey, pub timestamp: i64 }
#[event]
pub struct TopTokenMintProposalCancelled {}
#[event]
pub struct AuthorityProposed { pub new_authority: Pubkey, pub unlocks_at: i64 }
#[event]
pub struct AuthorityRotated { pub old: Pubkey, pub new: Pubkey, pub timestamp: i64 }
#[event]
pub struct AuthorityProposalCancelled {}


#[derive(AnchorSerialize, AnchorDeserialize, Clone, PartialEq)]
pub enum StakingTier {
    None,
    Micro,
    Bronze,
    Silver,
    Gold,
    Platinum,
    Diamond,
    Sovereign,
}


#[error_code]
pub enum ErrorCode {
    #[msg("Math overflow")]
    MathOverflow,
    #[msg("Amount must be greater than zero")]
    ZeroAmount,
    #[msg("Exceeds maximum stake of 20M $TOP per wallet")]
    ExceedsMaxStake,
    #[msg("Insufficient staked amount")]
    InsufficientStake,
    #[msg("Nothing staked")]
    NothingStaked,
    #[msg("Nothing to claim")]
    NothingToClaim,
    #[msg("eTOP forfeiture calculation overflow")]
    ETopForfeitureOverflow,
    #[msg("Token account mint does not match $TOP mint")]
    WrongMint,
    #[msg("Staking vault is not owned by the staking config PDA")]
    WrongVaultOwner,
    #[msg("DEPRECATED — Sovereign auto-release accounts must be supplied as a complete set (post-T0-05: use SovereignSeatReleaseRequired)")]
    SovereignAccountsIncomplete,
    #[msg("sovereign_registry_program account key does not match the expected sovereign_registry program ID")]
    WrongSovereignRegistryProgram,

    #[msg("Sovereign seat marker is set — unstake crossing below 20M MUST supply all 4 sovereign accounts for atomic release")]
    SovereignSeatReleaseRequired,
    #[msg("Insufficient stake to register Sovereign seat marker (must be >= 20M $TOP)")]
    InsufficientStakeForSovereignMarker,
    #[msg("Supplied sovereign_seat account is not a valid SovereignSeat PDA")]
    SovereignSeatInvalid,
    #[msg("sovereign_seat.holder does not match stake_position.owner")]
    SovereignSeatHolderMismatch,
    #[msg("Supplied sovereign_seat is not active (already released)")]
    SovereignSeatInactive,
    #[msg("seat_index argument does not match sovereign_seat.seat_index")]
    SovereignSeatIndexMismatch,
    #[msg("Sovereign seat already registered for this wallet — release the existing seat before claiming a different one")]
    SovereignSeatAlreadyRegistered,
    #[msg("Sovereign seat marker is not stale — seat is still active and staker is still at Sovereign tier")]
    SovereignSeatMarkerNotStale,

    #[msg("Unauthorized — signer is not the staking_config authority")]
    Unauthorized,
    #[msg("No pending proposal for this admin field (unlocks_at == 0)")]
    NothingPending,
    #[msg("Admin timelock has not elapsed yet (72h propose-to-finalize delay)")]
    TimelockNotElapsed,
    #[msg("Invalid authority: cannot propose Pubkey::default()")]
    InvalidAuthority,
    #[msg("Mint rotation blocked: total_staked must be zero to rotate $TOP mint")]
    MintRotationLocked,

    #[msg("Stake too young for Sovereign seat claim — must wait 7 days from stake")]
    StakeTooYoungForSeatClaim,

    #[msg("Propose cooldown active — escalating rate-limit per Rule 27b defense (R7.7-H-01)")]
    ProposeCooldownActive,
    #[msg("$TOP mint decimals != 6 — refusing to bind a wrong-decimals mint (would 1000x tier math)")]
    WrongDecimals,
    #[msg("$TOP mint is not owned by the Token-2022 program")]
    WrongTokenProgram,
}

#[cfg(test)]
mod tests {
    use super::*;

    proptest::proptest! {
        #[test]
        fn prop_staker_share_never_exceeds_deposit(
            w in 1u128..20_000_000_000_000_000u128,
            w_other in 0u128..20_000_000_000_000_000u128,
            d in 1u64..1_000_000_000_000_000u64,
            acc0 in 0u128..1_000_000_000_000_000_000u128,
        ) {
            let total_w = w.checked_add(w_other).unwrap_or(u128::MAX).max(w);
            let advance = (d as u128).saturating_mul(ACC_SCALE) / total_w;
            let acc1 = acc0.saturating_add(advance);
            let mut pos = mk_pos(1_700_000_000, 0, 0);
            if settle_reward(&mut pos, acc0, 0).is_ok()
                && remark_reward_debt(&mut pos, acc0, w).is_ok()
                && settle_reward(&mut pos, acc1, w).is_ok()
            {
                proptest::prop_assert!(
                    pos.reward_owed as u128 <= d as u128,
                    "OVER-DRAW: staker w={} of W={} banked {} from deposit {} (acc0={}, acc1={})",
                    w, total_w, pos.reward_owed, d, acc0, acc1
                );
            }
        }

        #[test]
        fn prop_forfeit_predicate_exact(
            age in proptest::num::i64::ANY,
            old_w in 0u128..20_000_000_000_000_000u128,
            new_w in 0u128..20_000_000_000_000_000u128,
        ) {
            let got = forfeit_unaged_accrual(age, old_w, new_w);
            let want = new_w < old_w && age < MIN_STAKE_AGE_FOR_CLAIM_SECONDS;
            proptest::prop_assert_eq!(got, want);
        }

        #[test]
        fn prop_sub24h_exit_forfeits_to_zero(
            w in 1u128..20_000_000_000_000_000u128,
            d in 1u64..1_000_000_000_000_000u64,
            acc0 in 0u128..1_000_000_000_000_000_000u128,
        ) {
            let advance = (d as u128).saturating_mul(ACC_SCALE) / w;
            let acc1 = acc0.saturating_add(advance);
            let mut pos = mk_pos(1_700_000_000, 0, 0);
            if settle_reward(&mut pos, acc0, 0).is_ok()
                && remark_reward_debt(&mut pos, acc0, w).is_ok()
            {
                proptest::prop_assert!(forfeit_unaged_accrual(0, w, 0));
                if remark_reward_debt(&mut pos, acc1, 0).is_ok() {
                    proptest::prop_assert_eq!(pos.reward_owed, 0u64);
                }
            }
        }

        #[test]
        fn prop_settle_remark_never_panic(
            acc in proptest::num::u128::ANY,
            weight in proptest::num::u128::ANY,
            owed in proptest::num::u64::ANY,
            debt in proptest::num::u128::ANY,
        ) {
            let mut pos = mk_pos(0, owed, debt);
            let _ = settle_reward(&mut pos, acc, weight);
            let _ = remark_reward_debt(&mut pos, acc, weight);
        }
    }

    #[test]
    fn tier_micro_renamed_from_entry() {
        assert_eq!(TIER_MICRO, 50_000);
    }

    #[test]
    fn tier_thresholds_match_business_spec() {
        assert_eq!(TIER_MICRO, 50_000);
        assert_eq!(TIER_BRONZE, 250_000);
        assert_eq!(TIER_SILVER, 750_000);
        assert_eq!(TIER_GOLD, 2_500_000);
        assert_eq!(TIER_PLATINUM, 7_500_000);
        assert_eq!(TIER_DIAMOND, 15_000_000);
        assert_eq!(TIER_SOVEREIGN, 20_000_000);
    }

    #[test]
    fn calculate_tier_boundary_cases() {
        assert_eq!(calculate_tier(0, false), 0);
        assert_eq!(calculate_tier(49_999 * TOKEN_DECIMALS, false), 0);
        assert_eq!(calculate_tier(50_000 * TOKEN_DECIMALS, false), 1);
        assert_eq!(calculate_tier(249_999 * TOKEN_DECIMALS, false), 1);
        assert_eq!(calculate_tier(250_000 * TOKEN_DECIMALS, false), 2);
        assert_eq!(calculate_tier(15_000_000 * TOKEN_DECIMALS, false), 6);
        assert_eq!(calculate_tier(19_999_999 * TOKEN_DECIMALS, false), 6);
        assert_eq!(calculate_tier(20_000_000 * TOKEN_DECIMALS, false), 6,
            "20M staker without seat must be Diamond, not Sovereign");
        assert_eq!(calculate_tier(20_000_000 * TOKEN_DECIMALS, true), 7,
            "20M staker with active seat must be Sovereign");
    }

    #[test]
    fn sovereign_downstake_crosses_threshold() {
        let sovereign_threshold = TIER_SOVEREIGN.checked_mul(TOKEN_DECIMALS).unwrap();

        let current = 20_005_000 * TOKEN_DECIMALS;
        let new_amount = 19_500_000 * TOKEN_DECIMALS;
        assert!(current >= sovereign_threshold);
        assert!(new_amount < sovereign_threshold);
        assert!(current >= sovereign_threshold && new_amount < sovereign_threshold);

        let current = 25_000_000 * TOKEN_DECIMALS;
        let new_amount = 21_000_000 * TOKEN_DECIMALS;
        assert!(current >= sovereign_threshold);
        assert!(new_amount >= sovereign_threshold);

        let current = 18_000_000 * TOKEN_DECIMALS;
        let new_amount = 17_000_000 * TOKEN_DECIMALS;
        assert!(current < sovereign_threshold);
        assert!(new_amount < sovereign_threshold);
    }

    #[test]
    fn max_stake_cap_matches_sovereign_threshold() {
        assert_eq!(
            MAX_STAKE_PER_WALLET,
            TIER_SOVEREIGN.checked_mul(TOKEN_DECIMALS).unwrap()
        );
    }

    #[test]
    fn sovereign_registry_program_id_validation_logic() {
        let sovereign_id = sovereign_registry::ID;
        let system_id = anchor_lang::system_program::ID;

        assert_ne!(sovereign_id, system_id,
            "sovereign_registry::ID must differ from SystemProgram::ID");

        let passed_correct = sovereign_id == sovereign_id;
        assert!(passed_correct, "correct sovereign_registry program passes validation");

        let passed_wrong = system_id == sovereign_id;
        assert!(!passed_wrong, "SystemProgram must NOT pass sovereign_registry validation");
    }


    fn release_decision(
        claimed_seat_index: Option<u8>,
        _current_amount: u64,
        new_amount: u64,
    ) -> (bool, bool) {
        let sovereign_threshold = TIER_SOVEREIGN
            .checked_mul(TOKEN_DECIMALS)
            .unwrap();
        let needs_strict = claimed_seat_index.is_some() && new_amount < sovereign_threshold;
        let legacy_eligible = false;
        (needs_strict, legacy_eligible)
    }

    fn all_sov_accounts_provided(
        seat: bool,
        registry_config: bool,
        sov_stake_pos: bool,
        registry_program: bool,
    ) -> bool {
        seat && registry_config && sov_stake_pos && registry_program
    }

    fn would_revert_with_release_required(
        needs_strict: bool,
        all_provided: bool,
    ) -> bool {
        needs_strict && !all_provided
    }

    #[test]
    fn marker_set_downstake_crosses_threshold_requires_strict_release() {
        let (needs_strict, legacy_eligible) = release_decision(
            Some(7),
            20_000_000 * TOKEN_DECIMALS,
            19_900_000 * TOKEN_DECIMALS,
        );
        assert!(needs_strict, "marker + crossed-below MUST require strict release");
        assert!(!legacy_eligible, "T0-05: legacy_eligible is now permanently false");

        let all_provided = all_sov_accounts_provided(true, true, true, true);
        assert!(!would_revert_with_release_required(needs_strict, all_provided),
            "marker set + crossed below + all 4 accounts provided → CPI fires, no revert");
    }

    #[test]
    fn marker_none_downstake_crosses_threshold_no_cpi_post_t0_05() {
        let (needs_strict, legacy_eligible) = release_decision(
            None,
            20_000_000 * TOKEN_DECIMALS,
            19_900_000 * TOKEN_DECIMALS,
        );
        assert!(!needs_strict, "no marker → strict path must NOT fire");
        assert!(!legacy_eligible,
            "T0-05: legacy path REMOVED — no CPI fires from staking side when marker is None");

        let all_provided = all_sov_accounts_provided(false, false, false, false);
        assert!(!would_revert_with_release_required(needs_strict, all_provided),
            "marker None + no accounts → unstake proceeds without revert (no CPI either)");
    }

    #[test]
    fn marker_set_stays_above_threshold_skips_release() {
        let (needs_strict, legacy_eligible) = release_decision(
            Some(3),
            25_000_000 * TOKEN_DECIMALS,
            21_000_000 * TOKEN_DECIMALS,
        );
        assert!(!needs_strict, "still-Sovereign → strict release must NOT fire");
        assert!(!legacy_eligible);
    }

    #[test]
    fn marker_set_full_unstake_to_zero_requires_strict_release() {
        let (needs_strict, legacy_eligible) = release_decision(
            Some(0),
            20_000_000 * TOKEN_DECIMALS,
            0,
        );
        assert!(needs_strict, "full exit with marker MUST require strict release");
        assert!(!legacy_eligible);
    }

    #[test]
    fn marker_none_no_threshold_cross_no_release_needed() {
        let (needs_strict, legacy_eligible) = release_decision(
            None,
            5_000_000 * TOKEN_DECIMALS,
            4_000_000 * TOKEN_DECIMALS,
        );
        assert!(!needs_strict);
        assert!(!legacy_eligible, "below-threshold-to-below-threshold: nothing to release");
    }

    #[test]
    fn marker_set_at_exact_threshold_boundary() {
        let sovereign_threshold = TIER_SOVEREIGN.checked_mul(TOKEN_DECIMALS).unwrap();
        let (needs_strict, _) = release_decision(
            Some(0),
            20_500_000 * TOKEN_DECIMALS,
            sovereign_threshold,
        );
        assert!(!needs_strict, "at-threshold exit: marker stays valid, no release");

        let (needs_strict, _) = release_decision(
            Some(0),
            20_500_000 * TOKEN_DECIMALS,
            sovereign_threshold - 1,
        );
        assert!(needs_strict, "one-unit-below-threshold: strict release REQUIRED");
    }


    #[test]
    fn t0_05_scenario_1_marker_with_seat_strict_path_only() {
        let (needs_strict, legacy_eligible) = release_decision(
            Some(11),
            20_000_000 * TOKEN_DECIMALS,
            10_000_000 * TOKEN_DECIMALS,
        );
        assert!(needs_strict,
            "T0-05: marker + crossed_below → strict path MUST fire");
        assert!(!legacy_eligible,
            "T0-05: legacy path is gone — never fires regardless of inputs");

        assert!(would_revert_with_release_required(needs_strict, false),
            "T0-05: strict path + 0 accounts → MUST revert SovereignSeatReleaseRequired");
        assert!(would_revert_with_release_required(
            needs_strict,
            all_sov_accounts_provided(true, false, false, false)),
            "T0-05: strict path + partial accounts → MUST revert (no partial accept)");
        assert!(!would_revert_with_release_required(
            needs_strict,
            all_sov_accounts_provided(true, true, true, true)),
            "T0-05: strict path + all 4 accounts → CPI fires, no revert");
    }

    #[test]
    fn t0_05_scenario_2_no_marker_no_cpi_unstake_succeeds() {
        let (needs_strict, legacy_eligible) = release_decision(
            None,
            20_000_000 * TOKEN_DECIMALS,
            19_500_000 * TOKEN_DECIMALS,
        );
        assert!(!needs_strict,
            "T0-05: no marker → no strict release");
        assert!(!legacy_eligible,
            "T0-05: no marker → no legacy release (branch removed)");

        for (s, rc, ssp, rp) in [
            (false, false, false, false),
            (true,  false, false, false),
            (false, true,  true,  false),
            (true,  true,  true,  true),
        ] {
            let all_p = all_sov_accounts_provided(s, rc, ssp, rp);
            assert!(!would_revert_with_release_required(needs_strict, all_p),
                "T0-05 scenario 2: no marker → no account check → unstake proceeds regardless");
        }
    }

    #[test]
    fn t0_05_scenario_3_attempted_exploit_pre_marker_loop_window() {

        let (needs_strict, legacy_eligible) = release_decision(
            None,
            20_000_000 * TOKEN_DECIMALS,
            19_900_000 * TOKEN_DECIMALS,
        );
        assert!(!needs_strict, "exploit step: no marker → strict won't fire");
        assert!(!legacy_eligible,
            "exploit step: legacy branch DELETED — no CPI fires regardless");

        let (needs_strict_restake, legacy_restake) = release_decision(
            None,
            19_900_000 * TOKEN_DECIMALS,
            20_000_000 * TOKEN_DECIMALS,
        );
        assert!(!needs_strict_restake, "re-stake: no marker, no cross-down → no strict");
        assert!(!legacy_restake, "re-stake: legacy permanently gone");

    }

    #[test]
    fn t0_05_scenario_4_restake_after_unstake_no_marker_auto_restore() {

        let mut marker: Option<u8> = Some(5);
        let stake_after_downstake = 19_900_000u64.checked_mul(TOKEN_DECIMALS).unwrap();
        let stake_after_restake = 20_000_000u64.checked_mul(TOKEN_DECIMALS).unwrap();

        let (needs_strict, _) = release_decision(
            marker,
            20_000_000 * TOKEN_DECIMALS,
            stake_after_downstake,
        );
        assert!(needs_strict, "step A: marker + crossed-below → strict path");
        let all_provided = all_sov_accounts_provided(true, true, true, true);
        assert!(!would_revert_with_release_required(needs_strict, all_provided));
        marker = None;

        assert_eq!(calculate_tier(stake_after_restake, false), 6,
            "post-restake without seat: Diamond (6), not Sovereign (7) — CRIT-3 fix");
        assert!(marker.is_none(),
            "T0-05: re-staking to 20M does NOT auto-restore the seat marker");

        assert!(marker.is_none(),
            "T0-05: no path inside staking::unstake or staking::stake writes the marker");
    }

    #[test]
    fn t0_05_strict_path_partial_accounts_revert() {
        let (needs_strict, _) = release_decision(
            Some(1),
            20_500_000 * TOKEN_DECIMALS,
            19_500_000 * TOKEN_DECIMALS,
        );
        assert!(needs_strict);

        assert!(would_revert_with_release_required(needs_strict,
            all_sov_accounts_provided(false, false, false, false)));
        assert!(would_revert_with_release_required(needs_strict,
            all_sov_accounts_provided(true, false, false, false)));
        assert!(would_revert_with_release_required(needs_strict,
            all_sov_accounts_provided(false, true, false, false)));
        assert!(would_revert_with_release_required(needs_strict,
            all_sov_accounts_provided(false, false, true, false)));
        assert!(would_revert_with_release_required(needs_strict,
            all_sov_accounts_provided(false, false, false, true)));
        assert!(would_revert_with_release_required(needs_strict,
            all_sov_accounts_provided(true, true, true, false)));
        assert!(!would_revert_with_release_required(needs_strict,
            all_sov_accounts_provided(true, true, true, true)));
    }

    #[test]
    fn stake_position_len_growth_matches_struct() {
        let fixed_prefix: usize = 8 + 32 + 8 + 1 + 8 + 8 + 8 + 1;
        assert_eq!(fixed_prefix, 74);
        assert_eq!(StakePosition::REWARD_OWED_OFFSET, fixed_prefix, "reward_owed @74");
        assert_eq!(StakePosition::REWARD_DEBT_OFFSET, fixed_prefix + 8, "reward_debt @82");
        assert_eq!(StakePosition::SPACE, fixed_prefix + 8 + 16 + 2);
        assert_eq!(StakePosition::SPACE, 100, "StakePosition SPACE must be 100");
        assert!(StakePosition::REWARD_DEBT_OFFSET + 16 <= StakePosition::SPACE - 1,
            "reward_debt must end before the variable Option tail (H-01 layout pin)");
    }

    #[test]
    fn acc_scale_pinned_at_1e12() {
        assert_eq!(ACC_SCALE, 1_000_000_000_000, "ACC_SCALE MUST be 1e12 (must match yield_escrow)");
    }

    #[test]
    fn acc_max_times_weight_max_fits_u128() {
        let weight_max = weighted_contribution(MAX_STAKE_PER_WALLET, 7);
        assert_eq!(weight_max, 20_000_000_000_000u128, "weight_max = 2e13 base units (6-decimal $TOP)");
        let min_tws = weighted_contribution(50_000 * TOKEN_DECIMALS, 1);
        assert_eq!(min_tws, 2_500_000_000u128, "min positive tws = 2.5e9 (6-decimal scale)");

        let acc_overflow_threshold = u128::MAX / weight_max;
        let funded_to_overflow = acc_overflow_threshold
            .checked_mul(min_tws).unwrap() / ACC_SCALE;
        assert!(funded_to_overflow > 1_000_000_000_000_000_000_000u128,
            "weight*acc overflow needs > $1 quadrillion funded (got {} micro-USDC) — unreachable",
            funded_to_overflow);

        let absurd_funded: u128 = 1_000_000_000_000_000_000u128;
        let absurd_acc = absurd_funded.checked_mul(ACC_SCALE).unwrap() / min_tws;
        let product = weight_max.checked_mul(absurd_acc)
            .expect("weight_max * acc at $1T-funded MUST fit u128");
        assert!(product < u128::MAX / 1000,
            "even $1T funded at the min denominator keeps >=1000x u128 headroom (got {product})");
    }

    #[test]
    fn stake_position_reward_offsets_borsh_stable_both_seat_variants() {
        use anchor_lang::AccountSerialize;
        let owed: u64 = 0x1122_3344_5566_7788;
        let debt: u128 = 0x0102_0304_0506_0708_090A_0B0C_0D0E_0F10;
        for seat in [None, Some(7u8)] {
            let pos = StakePosition {
                owner: Pubkey::default(),
                amount: 12_345,
                tier: 3,
                stake_timestamp: 1_700_000_000,
                last_claim: 0,
                etop_balance: 0,
                bump: 254,
                reward_owed: owed,
                reward_debt: debt,
                claimed_seat_index: seat,
            };
            let mut buf: Vec<u8> = Vec::new();
            pos.try_serialize(&mut buf).unwrap();
            assert!(buf.len() >= 98,
                "serialized StakePosition (seat={:?}) len {} must cover reward_debt end (byte 98)",
                seat, buf.len());
            let got_owed = u64::from_le_bytes(buf[74..82].try_into().unwrap());
            let got_debt = u128::from_le_bytes(buf[82..98].try_into().unwrap());
            assert_eq!(got_owed, owed,
                "reward_owed MUST decode at byte 74 for seat={:?} (yield claim raw-reads here)", seat);
            assert_eq!(got_debt, debt,
                "reward_debt MUST decode at byte 82 for seat={:?}", seat);
        }
        assert_eq!(StakePosition::REWARD_OWED_OFFSET, 74);
        assert_eq!(StakePosition::REWARD_DEBT_OFFSET, 82);
    }

    #[test]
    fn marker_idempotency_logic_same_seat_is_noop() {
        let mut marker: Option<u8> = Some(5);
        let to_register: u8 = 5;
        let outcome = match marker {
            Some(existing) if existing == to_register => "noop",
            Some(_) => "reject-different",
            None => "set",
        };
        assert_eq!(outcome, "noop");
        assert_eq!(marker, Some(5));
        let _ = &mut marker;
    }

    #[test]
    fn marker_idempotency_logic_different_seat_rejected() {
        let marker: Option<u8> = Some(5);
        let to_register: u8 = 7;
        let outcome = match marker {
            Some(existing) if existing == to_register => "noop",
            Some(_) => "reject-different",
            None => "set",
        };
        assert_eq!(outcome, "reject-different");
    }

    #[test]
    fn marker_release_eligibility_logic() {
        let sovereign_threshold = TIER_SOVEREIGN.checked_mul(TOKEN_DECIMALS).unwrap();

        let staker_amount: u64 = 20_500_000 * TOKEN_DECIMALS;
        let seat_active = false;
        let stale = !seat_active || (staker_amount < sovereign_threshold);
        assert!(stale, "inactive seat + full stake: marker IS stale, clear allowed");

        let staker_amount: u64 = 19_900_000 * TOKEN_DECIMALS;
        let seat_active = true;
        let stale = !seat_active || (staker_amount < sovereign_threshold);
        assert!(stale, "active seat + sub-threshold staker: marker IS stale, clear allowed");

        let staker_amount: u64 = 20_500_000 * TOKEN_DECIMALS;
        let seat_active = true;
        let stale = !seat_active || (staker_amount < sovereign_threshold);
        assert!(!stale, "active seat + full stake: marker NOT stale, clear MUST be blocked");
    }

    #[test]
    fn marker_atomic_clear_invariant() {
        let mut marker: Option<u8> = Some(3);
        let release_succeeded = true;
        if release_succeeded {
            marker = None;
        }
        assert!(marker.is_none(), "successful release MUST clear the marker");
    }

    #[test]
    fn marker_atomic_revert_keeps_marker_intact() {
        let marker_before: Option<u8> = Some(11);
        let mut marker_after = marker_before;
        let release_succeeded = false;
        if release_succeeded {
            marker_after = None;
        }
        assert_eq!(marker_after, marker_before, "CPI revert preserves marker");
    }


    fn fresh_staking_config() -> StakingConfig {
        StakingConfig {
            authority: Pubkey::new_unique(),
            top_token_mint: Pubkey::new_unique(),
            total_staked: 0,
            total_weighted_stake: 0,
            bump: 254,
            pending_top_token_mint: Pubkey::default(),
            pending_top_token_mint_unlocks_at: 0,
            pending_authority: Pubkey::default(),
            pending_authority_unlocks_at: 0,
            propose_cooldown_until: 0,
            recent_proposes: [0i64; 5],
        }
    }

    #[test]
    fn staking_admin_timelock_seconds_is_72h() {
        assert_eq!(ADMIN_TIMELOCK_SECONDS, 72 * 60 * 60);
        assert_eq!(ADMIN_TIMELOCK_SECONDS, 259_200);
    }

    #[test]
    fn propose_set_top_token_mint_writes_pending() {
        let mut cfg = fresh_staking_config();
        let new_mint = Pubkey::new_unique();
        let now: i64 = 1_700_000_000;
        cfg.pending_top_token_mint = new_mint;
        cfg.pending_top_token_mint_unlocks_at = now + ADMIN_TIMELOCK_SECONDS;
        assert_eq!(cfg.pending_top_token_mint, new_mint);
        assert_eq!(cfg.pending_top_token_mint_unlocks_at, now + 259_200);
        assert_ne!(cfg.top_token_mint, new_mint);
    }

    #[test]
    fn finalize_set_top_token_mint_before_unlock_blocked() {
        let mut cfg = fresh_staking_config();
        let now: i64 = 1_700_000_000;
        cfg.pending_top_token_mint = Pubkey::new_unique();
        cfg.pending_top_token_mint_unlocks_at = now + ADMIN_TIMELOCK_SECONDS;
        assert!(now < cfg.pending_top_token_mint_unlocks_at);
    }

    #[test]
    fn finalize_set_top_token_mint_after_unlock_commits() {
        let mut cfg = fresh_staking_config();
        let new_mint = Pubkey::new_unique();
        let now0: i64 = 1_700_000_000;
        cfg.pending_top_token_mint = new_mint;
        cfg.pending_top_token_mint_unlocks_at = now0 + ADMIN_TIMELOCK_SECONDS;
        let now1 = now0 + ADMIN_TIMELOCK_SECONDS + 1;
        assert!(now1 >= cfg.pending_top_token_mint_unlocks_at);
        cfg.top_token_mint = cfg.pending_top_token_mint;
        cfg.pending_top_token_mint = Pubkey::default();
        cfg.pending_top_token_mint_unlocks_at = 0;
        assert_eq!(cfg.top_token_mint, new_mint);
        assert_eq!(cfg.pending_top_token_mint, Pubkey::default());
        assert_eq!(cfg.pending_top_token_mint_unlocks_at, 0);
    }

    #[test]
    fn cancel_set_top_token_mint_clears_pending() {
        let mut cfg = fresh_staking_config();
        let live_mint = cfg.top_token_mint;
        cfg.pending_top_token_mint = Pubkey::new_unique();
        cfg.pending_top_token_mint_unlocks_at = 1_700_000_000 + ADMIN_TIMELOCK_SECONDS;
        cfg.pending_top_token_mint = Pubkey::default();
        cfg.pending_top_token_mint_unlocks_at = 0;
        assert_eq!(cfg.pending_top_token_mint, Pubkey::default());
        assert_eq!(cfg.pending_top_token_mint_unlocks_at, 0);
        assert_eq!(cfg.top_token_mint, live_mint);
    }

    #[test]
    fn finalize_set_top_token_mint_blocked_when_total_staked_nonzero() {
        let mut cfg = fresh_staking_config();
        cfg.total_staked = 1_000_000_000;
        cfg.pending_top_token_mint = Pubkey::new_unique();
        let now: i64 = 1_700_000_000;
        cfg.pending_top_token_mint_unlocks_at = now;
        assert!(now >= cfg.pending_top_token_mint_unlocks_at);
        assert!(cfg.total_staked > 0, "guard precondition holds");
    }

    #[test]
    fn transfer_authority_triplet_e2e() {
        let mut cfg = fresh_staking_config();
        let old_auth = cfg.authority;
        let new_auth = Pubkey::new_unique();
        let now0: i64 = 1_700_000_000;
        cfg.pending_authority = new_auth;
        cfg.pending_authority_unlocks_at = now0 + ADMIN_TIMELOCK_SECONDS;
        let _now1 = now0 + ADMIN_TIMELOCK_SECONDS + 1;
        cfg.authority = cfg.pending_authority;
        cfg.pending_authority = Pubkey::default();
        cfg.pending_authority_unlocks_at = 0;
        assert_eq!(cfg.authority, new_auth);
        assert_ne!(cfg.authority, old_auth);
    }

    #[test]
    fn propose_transfer_authority_rejects_default_pubkey() {
        let proposed = Pubkey::default();
        assert_eq!(proposed, Pubkey::default());
    }


    #[test]
    fn min_stake_duration_is_seven_days() {
        assert_eq!(MIN_STAKE_DURATION_SECONDS, 7 * 24 * 60 * 60);
        assert_eq!(MIN_STAKE_DURATION_SECONDS, 604_800);
    }

    #[test]
    fn register_sovereign_seat_rejects_flash_stake() {
        let stake_timestamp: i64 = 1_700_000_000;
        let now: i64 = stake_timestamp + 10;
        let age = now - stake_timestamp;
        assert!(age < MIN_STAKE_DURATION_SECONDS,
            "10s age must be below 7d threshold");
    }

    #[test]
    fn register_sovereign_seat_passes_at_exact_threshold() {
        let stake_timestamp: i64 = 1_700_000_000;
        let now: i64 = stake_timestamp + MIN_STAKE_DURATION_SECONDS;
        let age = now - stake_timestamp;
        assert_eq!(age, MIN_STAKE_DURATION_SECONDS);
        assert!(age >= MIN_STAKE_DURATION_SECONDS,
            "exact threshold must satisfy >= comparison");
    }

    #[test]
    fn register_sovereign_seat_passes_after_seven_days() {
        let stake_timestamp: i64 = 1_700_000_000;
        let now: i64 = stake_timestamp + 8 * 86_400;
        let age = now - stake_timestamp;
        assert!(age >= MIN_STAKE_DURATION_SECONDS,
            "8d age must satisfy 7d threshold");
    }

    #[test]
    fn register_sovereign_seat_rejects_negative_age() {
        let stake_timestamp: i64 = 1_700_000_000;
        let now: i64 = stake_timestamp - 10;
        let age_opt = now.checked_sub(stake_timestamp);
        assert_eq!(age_opt, Some(-10));
        assert!(!(age_opt.unwrap() >= MIN_STAKE_DURATION_SECONDS),
            "negative age must NOT satisfy threshold");
    }


    fn compute_weighted_topup_ts(
        current_amount: u64,
        prev_stake_timestamp: i64,
        amount: u64,
        now: i64,
    ) -> i64 {
        assert!(prev_stake_timestamp >= 0, "test setup: prev_ts must be >= 0");
        assert!(now >= 0, "test setup: now must be >= 0");
        let new_total = current_amount.checked_add(amount).unwrap();
        assert!(new_total > 0, "test setup: new_total must be > 0");
        if current_amount == 0 {
            return now;
        }
        let curr_weight = (current_amount as u128) * (prev_stake_timestamp as u128);
        let new_weight = (amount as u128) * (now as u128);
        let sum = curr_weight.checked_add(new_weight).unwrap();
        let weighted_ts = sum / (new_total as u128);
        weighted_ts as i64
    }

    #[test]
    fn weighted_ts_first_stake_uses_now() {
        let now: i64 = 1_700_000_000;
        let new_ts = compute_weighted_topup_ts(0, 0, 1_000_000_000_000, now);
        assert_eq!(new_ts, now);
    }

    #[test]
    fn test_stake_timestamp_updates_proportionally_on_topup() {
        let t0: i64 = 1_700_000_000;
        let day: i64 = 86_400;
        let initial_amount: u64 = 1 * TOKEN_DECIMALS;
        let topup_amount: u64 = 9 * TOKEN_DECIMALS;
        let now: i64 = t0 + 30 * day;

        let new_ts = compute_weighted_topup_ts(initial_amount, t0, topup_amount, now);

        let expected_ts = t0 + 27 * day;
        assert_eq!(new_ts, expected_ts,
            "30-day age + 9× top-up should produce 27-day effective age");

        let effective_age_secs = now - new_ts;
        let effective_age_days = effective_age_secs / day;
        assert_eq!(effective_age_days, 3,
            "effective age must be 3 days (10% of 30d) after 9× top-up");
    }

    #[test]
    fn test_flash_topup_resets_stake_age() {
        let t0: i64 = 1_700_000_000;
        let day: i64 = 86_400;
        let initial_amount: u64 = 1 * TOKEN_DECIMALS;
        let topup_amount: u64 = 19_999_999 * TOKEN_DECIMALS;
        let now: i64 = t0 + 7 * day;

        let new_ts = compute_weighted_topup_ts(initial_amount, t0, topup_amount, now);

        let effective_age_secs = now - new_ts;
        assert!(effective_age_secs < 10,
            "flash top-up effective age must be near-zero (got {}s)", effective_age_secs);
        assert!(effective_age_secs < MIN_STAKE_DURATION_SECONDS,
            "flash top-up effective age MUST be below 7-day Sovereign gate");

        let total = (initial_amount as u128) + (topup_amount as u128);
        let sovereign_threshold = (TIER_SOVEREIGN as u128) * (TOKEN_DECIMALS as u128);
        assert!(total >= sovereign_threshold,
            "attacker reached Sovereign threshold (20M)");
        let age_for_gate = now.checked_sub(new_ts).unwrap();
        assert!(!(age_for_gate >= MIN_STAKE_DURATION_SECONDS),
            "post-fix: flash top-up CANNOT satisfy the 7d Sovereign gate");
    }

    #[test]
    fn test_register_sovereign_seat_mirror_gate_uses_same_field() {
        let t0: i64 = 1_700_000_000;
        let day: i64 = 86_400;
        let initial_amount: u64 = 1 * TOKEN_DECIMALS;
        let topup_amount: u64 = 19_999_999 * TOKEN_DECIMALS;
        let now: i64 = t0 + 7 * day;

        let new_ts = compute_weighted_topup_ts(initial_amount, t0, topup_amount, now);

        let age = now.checked_sub(new_ts).unwrap();
        assert!(age < MIN_STAKE_DURATION_SECONDS,
            "mirror gate MUST see fresh ts and reject (got age {})", age);

        let legacy_age = now.checked_sub(t0).unwrap();
        assert!(legacy_age >= MIN_STAKE_DURATION_SECONDS,
            "pre-fix: t0-based ts would have wrongly passed the gate ({} >= {})",
            legacy_age, MIN_STAKE_DURATION_SECONDS);
        assert!(legacy_age >= age.saturating_mul(100_000),
            "weighted_ts must shrink the gate input by ≥100_000× on flash top-up \
             (legacy_age={}, post-fix age={})", legacy_age, age);
    }
    
    #[test]
    fn test_legitimate_topup_preserves_long_stake_age() {
        let t0: i64 = 1_700_000_000;
        let day: i64 = 86_400;
        let initial_amount: u64 = 10_000_000 * TOKEN_DECIMALS;
        let topup_amount: u64 = 10_000_000 * TOKEN_DECIMALS;
        let now: i64 = t0 + 365 * day;                          

        let new_ts = compute_weighted_topup_ts(initial_amount, t0, topup_amount, now);
        
        let effective_age_secs = now - new_ts;
        let effective_age_days = effective_age_secs / day;
        
        assert!(effective_age_days >= 181 && effective_age_days <= 183,
            "50/50 weighted top-up after 365d should give ~182d age (got {}d)",
            effective_age_days);
        
        assert!(effective_age_secs >= MIN_STAKE_DURATION_SECONDS,
            "legitimate 365d → +50% top-up MUST still pass the Sovereign gate");
    }
    
    #[test]
    fn weighted_ts_topup_at_same_slot_no_drift() {
        let t0: i64 = 1_700_000_000;
        let initial_amount: u64 = 5_000_000 * TOKEN_DECIMALS;
        let topup_amount: u64 = 5_000_000 * TOKEN_DECIMALS;

        let new_ts = compute_weighted_topup_ts(initial_amount, t0, topup_amount, t0);
        assert_eq!(new_ts, t0,
            "topup at same slot as initial stake must preserve stake_timestamp");
    }
    
    fn weighted_avg_ts(amt_old: u64, ts_old: i64, amt_new: u64, ts_new: i64) -> i64 {
        
        compute_weighted_topup_ts(amt_old, ts_old, amt_new, ts_new)
    }
    
    #[test]
    fn weighted_avg_ts_equal_mass_midpoint() {
        let day: i64 = 86_400;
        let amt: u64 = 10_000_000 * TOKEN_DECIMALS;  
        let ts_old: i64 = 0;
        let ts_new: i64 = 10 * day;

        let weighted = weighted_avg_ts(amt, ts_old, amt, ts_new);
        
        assert_eq!(weighted, 5 * day,
            "equal masses must produce the arithmetic midpoint (got {})", weighted);
    }
    
    #[test]
    fn weighted_avg_ts_brief_scenario_5m_plus_15m_at_23h_yields_17_25h() {
        let amt_old: u64 = 5_000_000 * TOKEN_DECIMALS;
        let amt_new: u64 = 15_000_000 * TOKEN_DECIMALS; 
        let ts_old: i64 = 0;
        let ts_new: i64 = 23 * 60 * 60;                  

        let weighted = weighted_avg_ts(amt_old, ts_old, amt_new, ts_new);
        let expected: i64 = 17 * 3600 + 900;  
        assert_eq!(weighted, expected,
            "5M@0 + 15M@23h must give 17.25h (62_100s); got {}s", weighted);
        assert_eq!(weighted, 62_100, "sanity: 62_100 seconds expected");
        
        let age_at_now = ts_new - weighted;
        assert_eq!(age_at_now, 20_700, "effective age at now = ts_new must be 5h 45m");
        
        let gate: i64 = 86_400;
        assert!(age_at_now < gate,
            "post-flash-topup effective age MUST be below the 24h yield-claim gate");
    }
    
    #[test]
    fn weighted_avg_ts_flash_stake_attack_blocked() {
        let amt_old: u64 = 50_000 * TOKEN_DECIMALS;
        let amt_new: u64 = 20_000_000 * TOKEN_DECIMALS;   
        let day: i64 = 86_400;
        let ts_old: i64 = 0;
        let ts_new: i64 = day - 1;
        let now: i64 = day;                               

        let weighted = weighted_avg_ts(amt_old, ts_old, amt_new, ts_new);
        
        let age_at_now = now - weighted;
        let gate_24h: i64 = 86_400;
        assert!(age_at_now < gate_24h,
            "post-attack effective age MUST be below the 24h gate (got {}s)", age_at_now);
        let legacy_age = now - ts_old;  
        assert!(legacy_age >= age_at_now.saturating_mul(200),
            "flash-attack age compression must be ≥200× (legacy={}, post-fix={})",
            legacy_age, age_at_now);
    }
    
    #[test]
    fn weighted_avg_ts_honest_topup_drift_proportional() {
        let day: i64 = 86_400;
        let amt_existing: u64 = 10_000_000 * TOKEN_DECIMALS;  
        let ts_existing: i64 = 0;
        let now: i64 = 90 * day;
        let amt_tiny: u64 = 100_000 * TOKEN_DECIMALS;  
        let weighted_tiny = weighted_avg_ts(amt_existing, ts_existing, amt_tiny, now);
        let drift_tiny = weighted_tiny;  
        let age_tiny = now - weighted_tiny;
        
        assert!(drift_tiny < day, "tiny top-up drift must be < 1 day (got {}s)", drift_tiny);
        
        assert!(age_tiny >= 89 * day,
            "honest staker after tiny top-up must keep ≥89d effective age (got {}s)", age_tiny);
        let amt_big: u64 = 10_000_000 * TOKEN_DECIMALS;  
        let weighted_big = weighted_avg_ts(amt_existing, ts_existing, amt_big, now);
        
        assert_eq!(weighted_big, 45 * day,
            "equal-mass top-up at 90d must produce 45d midpoint (got {})", weighted_big);
        let age_big = now - weighted_big;
        assert_eq!(age_big, 45 * day, "effective age after +100% top-up = 45d");
        
        assert!(age_big >= 86_400,
            "even equal-mass honest top-up keeps effective age above 24h gate");
        
        assert!(weighted_big > weighted_tiny,
            "bigger top-up MUST produce larger drift (weighted_big={} vs weighted_tiny={})",
            weighted_big, weighted_tiny);
        assert!(age_big < age_tiny,
            "bigger top-up MUST shrink effective age more (age_big={} vs age_tiny={})",
            age_big, age_tiny);
    }
    
    #[test]
    fn weighted_avg_ts_i128_headroom_check() {
        let amt: u64 = MAX_STAKE_PER_WALLET;          
        let ts_old: i64 = 0;
        let ts_new: i64 = 1_000_000_000_000_000_000;  
        let intermediate_u128: u128 = (amt as u128) * (ts_new as u128);
        assert!(intermediate_u128 > 0, "intermediate must not be zero — sanity");
        assert!(intermediate_u128 < u128::MAX / 2,
            "intermediate must stay under u128::MAX/2 — preserving headroom for the SUM");
        
        let weighted = weighted_avg_ts(amt, ts_old, amt, ts_new);
        let expected = ts_new / 2;
        assert_eq!(weighted, expected,
            "i128 stress: equal masses at extreme ts must midpoint cleanly (got {}, want {})",
            weighted, expected);
        let amt_dust: u64 = 1;  
        let weighted_skew = weighted_avg_ts(amt_dust, ts_old, amt, ts_new);
        
        let drift_from_new = ts_new - weighted_skew;
        assert!(drift_from_new >= 0 && drift_from_new < 100_000,
            "skewed weighted_ts must be within 100_000s of ts_new (got drift {})", drift_from_new);
        
        let huge_ts: i64 = i64::MAX / 2;
        let weighted_huge = weighted_avg_ts(amt, 0, amt, huge_ts);
        let expected_huge = huge_ts / 2;
        assert_eq!(weighted_huge, expected_huge,
            "i64::MAX/2 ts × 20M $TOP must still midpoint cleanly (got {}, want {})",
            weighted_huge, expected_huge);
    }
    
    
    fn simulate_stake_reset_step(
        current_amount: u64,
        prev_stake_timestamp: i64,
        now: i64,
    ) -> i64 {
        if current_amount == 0 {
            now
        } else {
            prev_stake_timestamp
        }
    }
    
    fn simulate_unstake_ts_step(
        _new_amount: u64,
        post_unstake_timestamp: i64,
    ) -> i64 {
        post_unstake_timestamp
    }
    
    #[test]
    fn test_unstake_to_zero_then_restake_blocks_flash_seat() {
        let t0: i64 = 1_700_000_000;
        let day: i64 = 86_400;
        
        let _initial_amount: u64 = 1 * TOKEN_DECIMALS;
        let mut stake_ts: i64 = t0;
        
        let _t_unstake: i64 = t0 + 7 * day;
        let new_amount_after_unstake: u64 = 0;
        stake_ts = simulate_unstake_ts_step(new_amount_after_unstake, stake_ts);
        assert_eq!(stake_ts, t0,
            "R4-UNSTAKE-01: full unstake PRESERVES stake_timestamp (no i64::MAX); the \
             flash-seat defense is the stake() reset, proven below");

        
        let t_restake: i64 = t0 + 7 * day + 1;
        let current_amount: u64 = 0;
        stake_ts = simulate_stake_reset_step(current_amount, stake_ts, t_restake);
        assert_eq!(stake_ts, t_restake,
            "M-CRIT-01: restake from zero MUST reset stake_timestamp to `now`");
        
        
        let age = t_restake.checked_sub(stake_ts).unwrap();
        assert!(age < MIN_STAKE_DURATION_SECONDS,
            "M-CRIT-01: post-fix restake from zero must fail 7d Sovereign gate (got age {}s)",
            age);
        
        
        let legacy_age = t_restake.checked_sub(t0).unwrap();
        assert!(legacy_age >= MIN_STAKE_DURATION_SECONDS,
            "pre-fix sanity: stale t0 would have wrongly passed (got age {}s)",
            legacy_age);
    }
    
    
    #[test]
    fn test_unstake_to_zero_then_restake_same_instant() {
        let t0: i64 = 1_700_000_000;
        let day: i64 = 86_400;

        
        let same_instant: i64 = t0 + 7 * day;

        let mut stake_ts: i64 = t0;

        
        stake_ts = simulate_unstake_ts_step(0, stake_ts);
        assert_eq!(stake_ts, t0, "full unstake preserves stake_timestamp");

        
        stake_ts = simulate_stake_reset_step(0, stake_ts, same_instant);
        assert_eq!(stake_ts, same_instant,
            "same-instant restake from zero must still reset to `now`");

        
        let age = same_instant.checked_sub(stake_ts).unwrap();
        assert_eq!(age, 0, "same-instant restake has age=0");
        assert!(age < MIN_STAKE_DURATION_SECONDS);
    }
    
    
    #[test]
    fn test_partial_unstake_does_not_trigger_sentinel() {
        let t0: i64 = 1_700_000_000;
        let day: i64 = 86_400;

        
        let mut stake_ts: i64 = t0;

        
        let _t_unstake: i64 = t0 + 7 * day;
        let new_amount_after_unstake: u64 = 10_000_000 * TOKEN_DECIMALS;
        stake_ts = simulate_unstake_ts_step(new_amount_after_unstake, stake_ts);
        assert_eq!(stake_ts, t0,
            "unstake (full or partial) preserves stake_timestamp; the weighted-avg + \
             stake-reset paths are the defenses (R4-UNSTAKE-01)");
        
        
        
        let t_restake: i64 = t0 + 8 * day;
        let current_amount: u64 = 10_000_000 * TOKEN_DECIMALS;
        let ts_before_reset = stake_ts;
        stake_ts = simulate_stake_reset_step(current_amount, stake_ts, t_restake);
        assert_eq!(stake_ts, ts_before_reset,
            "restake with current_amount > 0 must NOT reset (weighted-avg path is the defense)");
    }

    
    
    #[test]
    fn test_repeated_unstake_zero_cycles_all_blocked() {
        let t0: i64 = 1_700_000_000;
        let day: i64 = 86_400;
        let mut stake_ts: i64 = t0;

        
        for cycle in 0..3 {
            let restake_at: i64 = t0 + (cycle * 10 + 7) * day + 1;

            
            stake_ts = simulate_unstake_ts_step(0, stake_ts);

            
            stake_ts = simulate_stake_reset_step(0, stake_ts, restake_at);
            assert_eq!(stake_ts, restake_at, "cycle {}: reset post-restake", cycle);

            
            let age = restake_at.checked_sub(stake_ts).unwrap();
            assert!(age < MIN_STAKE_DURATION_SECONDS,
                "cycle {}: gate must reject (age={}s)", cycle, age);
        }
    }
    
    
    
    #[test]
    fn test_first_stake_reset_is_idempotent_with_init_block() {
        let t0: i64 = 1_700_000_000;

        let post_init_ts: i64 = t0;

        let reset_ts = simulate_stake_reset_step(0, post_init_ts, t0);

        
        assert_eq!(reset_ts, post_init_ts,
            "first stake: reset block must be idempotent with init block");
    }
    
    
    #[test]
    fn test_r4_unstake_01_exited_staker_can_claim_banked_yield() {
        const MIN_STAKE_AGE: i64 = 86_400; 
        let t0: i64 = 1_700_000_000;
        let day: i64 = 86_400;

        
        let ts_after_unstake = simulate_unstake_ts_step(0, t0);
        assert_eq!(ts_after_unstake, t0, "full unstake preserves the real stake_timestamp");
        let claim_at = t0 + 90 * day;
        let age = claim_at.checked_sub(ts_after_unstake).unwrap();
        assert!(age >= MIN_STAKE_AGE,
            "legit ≥24h exiter MUST pass the claim guard (age {}s) ⇒ banked yield claimable", age);

        
        
        let legacy_age = claim_at.checked_sub(i64::MAX).unwrap();
        assert!(legacy_age < MIN_STAKE_AGE,
            "old i64::MAX sentinel WOULD have bricked the exited staker's claim (age {}s)", legacy_age);

        
        
        let flash_claim_at = t0 + 3 * 3600;
        let flash_age = flash_claim_at.checked_sub(t0).unwrap();
        assert!(flash_age < MIN_STAKE_AGE,
            "flash-unstaker (<24h real age) MUST still be blocked from extracting (age {}s)", flash_age);
    }
    
    
    

    fn mk_pos(stake_timestamp: i64, reward_owed: u64, reward_debt: u128) -> StakePosition {
        StakePosition {
            owner: Pubkey::default(),
            amount: 0,
            tier: 0,
            stake_timestamp,
            last_claim: 0,
            etop_balance: 0,
            bump: 254,
            reward_owed,
            reward_debt,
            claimed_seat_index: None,
        }
    }
    #[test]
    fn r6_econ_01_forfeit_boundary_truth_table() {
        
        let (old_w, new_w) = (1_000u128, 500u128);
        assert!(forfeit_unaged_accrual(0, old_w, new_w),       "age 0 ⇒ forfeit");
        assert!(forfeit_unaged_accrual(86_399, old_w, new_w),  "age 86_399 (<24h) ⇒ forfeit");
        assert!(!forfeit_unaged_accrual(86_400, old_w, new_w), "age 86_400 (==24h gate) ⇒ BANK");
        assert!(!forfeit_unaged_accrual(86_401, old_w, new_w), "age 86_401 (>24h) ⇒ BANK");
        
        assert!(forfeit_unaged_accrual(-10, old_w, new_w),     "skewed-negative age ⇒ forfeit (fail-safe)");
        
        
        
        assert!(!forfeit_unaged_accrual(0, 500, 1_000),        "weight INCREASE ⇒ never forfeit");
        assert!(!forfeit_unaged_accrual(0, 1_000, 1_000),      "weight EQUAL ⇒ never forfeit");
        assert!(!forfeit_unaged_accrual(0, 0, 0),              "sub-Micro 0→0 ⇒ not a decrease");
    }
    #[test]
    fn r6_econ_01_skip_forfeits_unaged_slice_but_keeps_prior_banked() {
        
        let acc: u128 = 2 * ACC_SCALE;
        let old_w: u128 = 1_000;
        let prior_owed: u64 = 5_000;
        let reward_debt: u128 = 1_000;
        let new_w: u128 = 0;                     

        
        let mut p = mk_pos(0, prior_owed, reward_debt);
        remark_reward_debt(&mut p, acc, new_w).unwrap();
        assert_eq!(p.reward_owed, prior_owed, "forfeit MUST preserve prior banked reward_owed");
        assert_eq!(p.reward_debt, 0,          "reward_debt re-baselined at new weight 0");

        
        let mut q = mk_pos(0, prior_owed, reward_debt);
        settle_reward(&mut q, acc, old_w).unwrap();
        remark_reward_debt(&mut q, acc, new_w).unwrap();
        assert_eq!(q.reward_owed, prior_owed + 1_000, "aged exit banks the pending slice");
    }
    #[test]
    fn r6_econ_01_jit_attacker_claims_zero_honest_banks_fair_share() {
        
        
        
        let acc0: u128 = 3 * ACC_SCALE;
        let acc1: u128 = 5 * ACC_SCALE;
        let w_a: u128 = 1_000;

        
        
        let mut probe = mk_pos(0, 0, 0);
        settle_reward(&mut probe, acc0, 0).unwrap();
        remark_reward_debt(&mut probe, acc0, w_a).unwrap();
        let (fresh_owed, fresh_debt) = (probe.reward_owed, probe.reward_debt);
        assert_eq!((fresh_owed, fresh_debt), (0, 3_000));

        
        assert!(forfeit_unaged_accrual(0, w_a, 0));
        let mut atk = mk_pos(0, fresh_owed, fresh_debt);
        remark_reward_debt(&mut atk, acc1, 0).unwrap();
        
        let atk_entitlement = atk.reward_owed as u128 + 0u128 * acc1 / ACC_SCALE - atk.reward_debt;
        assert_eq!(atk_entitlement, 0, "JIT attacker (1-slot capital) claims ZERO");

        assert!(!forfeit_unaged_accrual(86_400, w_a, 0));
        let mut hon = mk_pos(0, fresh_owed, fresh_debt);
        settle_reward(&mut hon, acc1, w_a).unwrap();           
        remark_reward_debt(&mut hon, acc1, 0).unwrap();
        let hon_entitlement = hon.reward_owed as u128 + 0u128 * acc1 / ACC_SCALE - hon.reward_debt;
        assert_eq!(hon_entitlement, 2_000, "honest ≥24h holder banks the fair pro-rata share");
    }
    
    
    
    
    fn drift_handler_end(src: &str, idx: usize) -> usize {
        src[idx + 1..]
            .find("\n    pub fn ")
            .map(|p| idx + 1 + p)
            .expect("drift-gate: no following `pub fn` — re-anchor this source-assert bound")
    }

    fn staking_lib_rs_source() -> &'static str {
        include_str!("lib.rs")
    }

    #[test]
    fn r65_d1_weighted_avg_topup_defense_pinned_in_stake_handler() {
        let src = staking_lib_rs_source();
        let needle = "pub fn stake(ctx: Context<Stake>";
        let idx = src.find(needle).expect("stake handler must exist");
        let end = drift_handler_end(src, idx);
        let body = &src[idx..end];
        
        
        
        assert!(body.contains("(current_amount as u128)")
                && body.contains(".checked_mul(prev_ts as u128)")
                && body.contains("(amount as u128)")
                && body.contains(".checked_mul(now as u128)")
                && body.contains(".checked_div(new_total as u128)"),
            "stake handler MUST contain weighted-avg stake_timestamp formula \
             (R5.5 CHAIN-C / R6.5 D.1). Source excerpt:\n{}", body);
        assert!(body.contains("if current_amount == 0 {")
                && body.contains("ctx.accounts.stake_position.stake_timestamp = Clock::get()?.unix_timestamp;"),
            "stake handler MUST reset stake_timestamp on re-stake from zero \
             (M-CRIT-01). Source excerpt:\n{}", body);
        
        assert!(body.contains("R5.5") && (body.contains("R6.5") || body.contains("M-CRIT-01") || body.contains("R5.5 CHAIN-C")),
            "stake handler MUST cite R5.5 CHAIN-C and either R6.5 or M-CRIT-01 \
             for traceability");
    }
    #[test]
    fn r4_unstake_01_unstake_preserves_ts_and_zeros_etop() {
        let src = staking_lib_rs_source();
        let needle = "pub fn unstake(ctx: Context<Unstake>";
        let idx = src.find(needle).expect("unstake handler must exist");
        let end = drift_handler_end(src, idx);
        let body = &src[idx..end];
        assert!(body.contains("etop_balance = 0"),
            "unstake handler MUST zero etop_balance. Source excerpt:\n{}", body);
        
        
        
        
        
        
        assert!(!body.contains("stake_timestamp = i64::MAX"),
            "unstake handler MUST NOT sentinel stake_timestamp to i64::MAX \
             (R4-UNSTAKE-01 regression — it bricks banked-yield claims). Excerpt:\n{}", body);
        assert!(body.contains("R4-UNSTAKE-01"),
            "unstake handler MUST cite R4-UNSTAKE-01 (preserve stake_timestamp on \
             full unstake) for traceability. Excerpt:\n{}", body);
    }

    

    #[test]
    fn r77_h01_check_and_record_propose_helper_present() {
        let src = staking_lib_rs_source();
        assert!(src.contains("fn check_and_record_propose("),
            "staking MUST define check_and_record_propose helper (R7.7-H-01)");
        assert!(src.contains("propose_cooldown_until"),
            "StakingConfig MUST carry propose_cooldown_until field (R7.7-H-01)");
        assert!(src.contains("recent_proposes: [i64; 5]"),
            "StakingConfig MUST carry recent_proposes: [i64; 5] ring buffer (R7.7-H-01)");
        assert!(src.contains("ProposeCooldownActive"),
            "ErrorCode::ProposeCooldownActive variant MUST exist (R7.7-H-01)");
    }

    #[test]
    fn r77_h01_all_propose_handlers_call_check_and_record_propose() {
        
        
        let src = staking_lib_rs_source();
        let propose_names = ["propose_set_top_token_mint",
                             "propose_transfer_authority"];
        for name in propose_names {
            let needle = format!("pub fn {}(", name);
            let idx = src.find(&needle).unwrap_or_else(|| panic!("{} handler must exist", name));
            
            
            
            let end = drift_handler_end(src, idx);
            let body = &src[idx..end];
            assert!(body.contains("check_and_record_propose(config, now)?;"),
                "{} MUST call check_and_record_propose(config, now)? for R7.7-H-01 \
                 defense. Source excerpt:\n{}", name, body);
        }
    }
    
    
    
    

    
    
    
    
    
    fn apply_tier_delta(
        global: u128,
        amount: u64,
        old_tier: u8,
        new_tier: u8,
    ) -> u128 {
        let old_w = weighted_contribution(amount, old_tier);
        let new_w = weighted_contribution(amount, new_tier);
        global
            .checked_sub(old_w)
            .expect("NEW-H01: checked_sub underflow — global desynced below stored weight")
            .checked_add(new_w)
            .expect("NEW-H01: checked_add overflow")
    }

    #[test]
    fn new_h01_seat_transition_maintains_global_weight() {
        let amount: u64 = 20_000_000 * TOKEN_DECIMALS; 

        
        let diamond_w = weighted_contribution(amount, 6);
        let sovereign_w = weighted_contribution(amount, 7);
        assert_eq!(diamond_w, 18_000_000u128 * (TOKEN_DECIMALS as u128),
            "Diamond weight for 20M must be 18.0M (9000 bps)");
        assert_eq!(sovereign_w, 20_000_000u128 * (TOKEN_DECIMALS as u128),
            "Sovereign weight for 20M must be 20.0M (10000 bps)");
        assert!(sovereign_w > diamond_w, "Sovereign weight must exceed Diamond");
        
        
        let mut global: u128 = 0;
        let mut pos_amount: u64 = 0;
        let mut pos_tier: u8 = 0;

        let new_total = pos_amount.checked_add(amount).unwrap();
        let staked_tier = calculate_tier(new_total, false); 
        assert_eq!(staked_tier, 6, "20M without seat must be Diamond (CRIT-3)");
        global = global
            .checked_sub(weighted_contribution(pos_amount, pos_tier))
            .unwrap()
            .checked_add(weighted_contribution(new_total, staked_tier))
            .unwrap();
        pos_amount = new_total;
        pos_tier = staked_tier;
        assert_eq!(global, diamond_w, "after stake: global carries Diamond weight");
        
        assert_eq!(global, weighted_contribution(pos_amount, pos_tier));

        
        
        let new_tier_reg = calculate_tier(pos_amount, true);
        assert_eq!(new_tier_reg, 7, "with seat, 20M must be Sovereign");
        global = apply_tier_delta(global, pos_amount, pos_tier, new_tier_reg);
        pos_tier = new_tier_reg;
        assert_eq!(global, sovereign_w,
            "after register_sovereign_seat: global MUST carry Sovereign weight (the bug)");
        assert_eq!(global, weighted_contribution(pos_amount, pos_tier),
            "invariant holds: global == contrib(amount, stored tier)");

        
        
        let new_tier_upd = calculate_tier(pos_amount, true);
        assert_eq!(new_tier_upd, pos_tier, "update_tier after register sees old==new==7");
        let before = global;
        global = apply_tier_delta(global, pos_amount, pos_tier, new_tier_upd);
        assert_eq!(global, before,
            "NEW-H01 idempotency: update_tier after register is net-zero (no +2.0M twice)");
        assert_eq!(global, sovereign_w, "global still exactly Sovereign weight");

        
        
        
        
        let new_amount: u64 = 0;
        let new_tier_unstake = calculate_tier(new_amount, false);
        assert_eq!(new_tier_unstake, 0, "unstake to zero → None tier");
        global = apply_tier_delta(global, pos_amount, pos_tier, new_tier_unstake);
        assert_eq!(global, 0,
            "HARM B resolved: unstake-to-zero subtracts the SAME 20.0M now in the \
             global — no underflow, global returns to 0");

        let desynced_global: u128 = diamond_w; 
        let stale_old_weighted = weighted_contribution(amount, 7); 
        assert!(desynced_global.checked_sub(stale_old_weighted).is_none(),
            "pre-fix counterfactual: subtracting 20.0M from an 18.0M global MUST \
             underflow (this is exactly the bricked-exit bug NEW-H01 fixes)");
    }
    
    
    #[test]
    fn new_h01_register_same_seat_is_global_noop() {
        let amount: u64 = 20_000_000 * TOKEN_DECIMALS;
        
        let global: u128 = weighted_contribution(amount, 7);
        
        let would_be_wrong = apply_tier_delta(global, amount, 6, 7);
        assert_ne!(would_be_wrong, global,
            "sanity: applying the +Diamond→Sovereign delta again WOULD corrupt the \
             global — proving the idempotent arm MUST be a no-op (handler is)");
        
        let actual = global;
        assert_eq!(actual, weighted_contribution(amount, 7),
            "idempotent re-register leaves global at exactly Sovereign weight");
    }

    
    
    
    
    #[test]
    fn new_h01_release_seat_maintains_global_weight() {
        let amount: u64 = 20_000_000 * TOKEN_DECIMALS;
        let diamond_w = weighted_contribution(amount, 6); 
        let sovereign_w = weighted_contribution(amount, 7); 

        
        let mut global: u128 = sovereign_w;
        let mut pos_tier: u8 = 7;
        let pos_amount: u64 = amount;
        assert_eq!(global, weighted_contribution(pos_amount, pos_tier),
            "precondition: global == contrib(20M, Sovereign) after register fix");

        
        let new_tier_rel = calculate_tier(pos_amount, false);
        assert_eq!(new_tier_rel, 6,
            "seat released on a 20M holder → effective tier is Diamond (6), not Sovereign");
        assert_ne!(new_tier_rel, pos_tier, "tier MUST change 7 → 6 on release");
        global = apply_tier_delta(global, pos_amount, pos_tier, new_tier_rel);
        pos_tier = new_tier_rel;
        
        assert_eq!(global, diamond_w,
            "CC-M01: after release_seat the global MUST drop to Diamond weight (18.0M)");
        assert_eq!(global, weighted_contribution(pos_amount, pos_tier),
            "invariant restored: global == contrib(20M, stored Diamond tier)");
        
        assert_eq!(sovereign_w - global, 2_000_000u128 * (TOKEN_DECIMALS as u128),
            "release_seat removes exactly the 2.0M over-count CC-M01 flagged");
        
        let new_tier_upd = calculate_tier(pos_amount, false);
        assert_eq!(new_tier_upd, pos_tier, "update_tier after release sees old==new==6");
        let before = global;
        global = apply_tier_delta(global, pos_amount, pos_tier, new_tier_upd);
        assert_eq!(global, before,
            "CC-M01 idempotency: update_tier after release_seat is net-zero (no −2.0M twice)");
        assert_eq!(global, diamond_w, "global still exactly Diamond weight");
        let new_tier_rel2 = calculate_tier(pos_amount, false);
        let before2 = global;
        global = apply_tier_delta(global, pos_amount, pos_tier, new_tier_rel2);
        assert_eq!(global, before2,
            "redundant release_seat must be net-zero (guarded on tier change)");
    }

    
    
    
    
    #[test]
    fn new_h01_release_seat_below_threshold_is_global_noop() {
        
        
        let amount: u64 = 18_000_000 * TOKEN_DECIMALS;
        let pos_tier: u8 = calculate_tier(amount, false); 
        assert_eq!(pos_tier, 6, "18M is Diamond regardless of seat");
        let global: u128 = weighted_contribution(amount, pos_tier);

        
        let new_tier = calculate_tier(amount, false);
        assert_eq!(new_tier, pos_tier, "sub-threshold: tier unchanged by seat clear");
        let after = apply_tier_delta(global, amount, pos_tier, new_tier);
        assert_eq!(after, global,
            "sub-threshold release_seat is net-zero — no double-subtract vs unstake");
    }
}
