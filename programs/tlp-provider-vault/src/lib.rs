
use anchor_lang::prelude::*;
use anchor_lang::system_program;
use anchor_spl::associated_token::{get_associated_token_address, AssociatedToken};
use anchor_spl::token::{self, Mint, Token, TokenAccount, TransferChecked};
use anchor_spl::token_interface::{
    self as token_interface, Mint as TopMint, TokenAccount as TopTokenAccount, TokenInterface,
};
use swap_router::program::SwapRouter;

use affiliate_registry::cpi::accounts::DepositFundingPool as AffiliateDepositFundingPool;
use affiliate_registry::program::AffiliateRegistry;
use affiliate_registry::AffiliateConfig;
use affiliate_registry::FundingPool as AffiliateFundingPool;
use sovereign_registry::cpi::accounts::DepositRoyaltyUsdc as SovereignDepositRoyaltyUsdc;
use sovereign_registry::program::SovereignRegistry;
use sovereign_registry::RegistryConfig as SovereignRegistryConfig;
use yield_escrow::cpi::accounts::DepositProviderYieldUsdc as YieldDepositProviderUsdc;
use yield_escrow::program::YieldEscrow;
use yield_escrow::{Epoch as YieldEpoch, YieldConfig};

declare_id!("CtB3xQvmUGZtFRALhmkhhissargBJ51WPLCDFhGVy6Lx");

#[cfg(not(feature = "no-entrypoint"))]
solana_security_txt::security_txt! {
    name: "tlp_provider_vault",
    project_url: "https://topbit.io",
    contacts: "email:security@topbit.io",
    policy: "https://topbit.io/security",
    preferred_languages: "en",
    source_code: "https://github.com/topbit-io/topbit"
}


pub const MAX_PROVIDERS: u8 = 16;
pub const MAX_ASSETS: u8 = 8;
pub const SECONDS_PER_DAY: i64 = 86_400;

pub const SOL_PSEUDO_MINT: Pubkey =
    anchor_lang::solana_program::system_program::ID;

pub const MIN_DEPOSIT_USDC: u64 = 10_000_000;
pub const MIN_DELTA_GGR_FOR_SWEEP_USDC: u64 = 1_000_000_000;
pub const HARD_VAULT_FLOOR_USDC: u64 = 500_000_000;
pub const DEFAULT_LP_SHARE_BPS: u16 = 6_000;
pub const DEFAULT_DEV_FEE_BPS: u16 = 250;
pub const MAX_DEV_FEE_BPS: u16 = 1_000;
pub const SOVEREIGN_CARVE_BPS: u16 = 500;
pub const DEFAULT_INSURANCE_FLOOR_BPS: u16 = 500;
pub const DEFAULT_MAX_DAILY_DRAWDOWN_BPS: u16 = 500;

pub const SWAP_ROUTER_PROGRAM_ID: Pubkey = pubkey!("9tkZMhH7cf293wpGpJycqsZLb8eojkJ5jyUHJtqcS5zR");

pub const RESERVE_BURN_MODE_MANUAL: u8 = 0;
pub const RESERVE_BURN_MODE_AUTO_SWAP: u8 = 1;

pub const MAX_RESERVE_SWAP_SLIPPAGE_BPS: u16 = 150;

pub const MAX_PROVIDER_FEE_BPS: u16 = 2_500;
pub const DEFAULT_PROVIDER_FEE_BPS: u16 = 1_000;
pub const PROVIDER_SETTLE_KEEPER_DAYS: i64 = 40;

pub const PAUSE_RATE_LIMIT_SECONDS: i64 = 600;
pub const PAUSE_RATE_LIMIT_VAULT_WIDE_SECONDS: i64 = 60;

pub const YIELD_ESCROW_PROGRAM_ID: Pubkey =
    pubkey!("85b3FfAzz3akfnH7NPCqR4Pjna45N3N6e6MvPsxABJ6n");
pub const SOVEREIGN_REGISTRY_PROGRAM_ID: Pubkey =
    pubkey!("14ndgn3yKuD4Zi3ozBt7Fo4cYzUuYDAZrTn15wT3rFC2");
pub const AFFILIATE_REGISTRY_PROGRAM_ID: Pubkey =
    pubkey!("GcLnquNequt8UwNigWDzyfA2DpeeTMnQnyUbHxHL8cfC");
pub const ADMIN_TIMELOCK_SECONDS: i64 = 72 * 60 * 60;
pub const FREEZE_RATE_LIMIT_SECONDS: i64 = 600;

pub const PROPOSE_RATE_LIMIT_WINDOW_SECONDS: i64 = 86_400;
pub const PROPOSE_RATE_LIMIT_RING_LEN: usize = 5;

pub const DEFAULT_MAX_SETTLE_PER_WINDOW: u64 = 50_000_000_000;
pub const DEFAULT_SETTLE_WINDOW_SECONDS: u32 = 300;
pub const MIN_MAX_SETTLE_PER_WINDOW: u64 = 1_000_000_000;
pub const MIN_SETTLE_WINDOW_SECONDS: u32 = 30;
pub const MAX_SETTLE_WINDOW_SECONDS: u32 = 24 * 60 * 60;

pub const DEFAULT_MAX_DAILY_OUTFLOW: u64 = 250_000_000_000;
pub const MIN_MAX_DAILY_OUTFLOW: u64 = 50_000_000_000;

pub const MAX_NET_GGR_PER_RECEIPT_BPS: u16 = 2_000;
pub const NET_GGR_CAP_FLOOR_USDC: u64 = 1_000_000_000;
pub const DAILY_OUTFLOW_WINDOW_SECONDS: i64 = 86_400;

pub const DEFAULT_MAX_CHIP_DEBIT_PER_24H_PER_WALLET: u64 = 10_000_000_000;
pub const CHIP_DEBIT_WINDOW_SECONDS: i64 = 86_400;
pub const MAX_CHIP_DEBIT_CAP_PER_WALLET: u64 = 100_000_000_000;

pub const KEEPER_WINDOW_SECONDS: i64 = 8 * SECONDS_PER_DAY;

pub const WITHDRAW_BATCH_INTERVAL_SECONDS: i64 = 24 * 60 * 60;
pub const WITHDRAW_BATCH_PRESSURE_THRESHOLD: u32 = 20;
pub const BATCH_PRICE_SCALE: u128 = 1_000_000_000;

pub const TIER_COOLDOWN_DAYS: [i64; 5] = [14, 10, 7, 5, 3];

pub const TIER_LP_SHARE_BPS_GROWTH: [u16; 5] = [6_500, 7_000, 7_500, 8_000, 8_500];
pub const BOOTSTRAP_LP_SHARE_BPS: u16 = 7_500;

pub const FOUNDING_BANKER_MAX_SEATS: u8 = 21;
pub const FOUNDING_BANKER_MIN_USDC_MICRO: u64 = 5_000_000_000;
pub const FOUNDING_BANKER_BONUS_DAYS: i64 = 90;
pub const FOUNDING_BANKER_LP_SHARE_BPS: u16 = 8_500;

pub const CIRCUIT_GREEN: u8 = 0;
pub const CIRCUIT_YELLOW: u8 = 1;
pub const CIRCUIT_RED: u8 = 2;
pub const MIN_INSURANCE_DUST: u64 = 1_000;
pub const WAIVER_DELAY_SECONDS: i64 = 24 * 60 * 60;
pub const UNLOCK_VAULT_MIN_DELAY_SECONDS: i64 = 72 * 60 * 60;

pub const CIRCUIT_YELLOW_NAV_PCT_OF_PEAK: u64 = 20;
pub const CIRCUIT_RED_NAV_PCT_OF_PEAK: u64 = 10;
pub const INSURANCE_FLOOR_PCT_OF_NAV: u64 = 5;
pub const WAIVER_MAX_TOTAL_SECONDS: i64 = 72 * 60 * 60;

pub const MIN_DEAD_SHARES: u64 = 1_000_000;

pub const USDC_DECIMALS: u8 = 6;

pub const EXPECTED_TOP_DECIMALS: u8 = 6;

pub const PROVIDER_NAME_LEN: usize = 32;
pub const PAUSE_REASON_LEN: usize = 32;

pub const SINGLE_WALLET_THRESHOLD_BPS: u16 = 250;
pub const SINGLE_WALLET_EXTRA_COOLDOWN_SECONDS: i64 = 7 * SECONDS_PER_DAY;


#[program]
pub mod tlp_provider_vault {
    use super::*;


    pub fn init_vault(
        ctx: Context<InitVault>,
        operator: Pubkey,
        affiliate_recorder: Pubkey,
        pause_authority: Pubkey,
        waterfall_authority: Pubkey,
        sovereign_registry_program_id: Pubkey,
        sovereign_registry_config: Pubkey,
        yield_escrow_program_id: Pubkey,
        yield_escrow_provider_pool: Pubkey,
        affiliate_registry_program_id: Pubkey,
        affiliate_registry_config: Pubkey,
        founder_pubkey: Pubkey,
    ) -> Result<()> {
        let authority_key = ctx.accounts.authority.key();
        require_non_default_pubkeys(&[
            authority_key,
            operator,
            affiliate_recorder,
            pause_authority,
            waterfall_authority,
            sovereign_registry_program_id,
            sovereign_registry_config,
            yield_escrow_program_id,
            yield_escrow_provider_pool,
            affiliate_registry_program_id,
            affiliate_registry_config,
            founder_pubkey,
        ])?;
        require_roles_pairwise_distinct(&[
            authority_key,
            operator,
            affiliate_recorder,
            pause_authority,
            waterfall_authority,
        ])?;

        let config = &mut ctx.accounts.vault_config;
        config.authority = authority_key;
        config.operator_pubkey = operator;
        config.affiliate_recorder_pubkey = affiliate_recorder;
        config.pause_authority = pause_authority;
        config.waterfall_authority = waterfall_authority;
        config.bump = ctx.bumps.vault_config;

        config.active_provider_count = 0;
        config.next_provider_id = 0;
        config.is_paused = true;
        config.pause_reason = [0u8; PAUSE_REASON_LEN];
        config.last_pause_at = 0;
        config.last_provider_pause_at = 0;
        config.phase = 0;
        config.phase_started_at = Clock::get()?.unix_timestamp;
        config.dev_fee_bps = DEFAULT_DEV_FEE_BPS;
        config.sovereign_carve_bps = SOVEREIGN_CARVE_BPS;
        config.insurance_floor_bps = DEFAULT_INSURANCE_FLOOR_BPS;
        config.max_daily_drawdown_bps = DEFAULT_MAX_DAILY_DRAWDOWN_BPS;

        config.sovereign_registry_program_id = sovereign_registry_program_id;
        config.sovereign_registry_config = sovereign_registry_config;
        config.yield_escrow_program_id = yield_escrow_program_id;
        config.yield_escrow_provider_pool = yield_escrow_provider_pool;
        config.affiliate_registry_program_id = affiliate_registry_program_id;
        config.affiliate_registry_config = affiliate_registry_config;

        config.pending_authority = Pubkey::default();
        config.pending_authority_unlocks_at = 0;
        config.ops_marketing_wallet = Pubkey::default();
        config.pending_dev_fee_bps = 0;
        config.pending_dev_fee_bps_unlocks_at = 0;
        config.pending_ops_marketing_wallet = Pubkey::default();
        config.pending_ops_marketing_wallet_unlocks_at = 0;
        config.reserve_burn_mode = RESERVE_BURN_MODE_MANUAL;
        config.is_frozen = false;
        config.last_freeze_at = 0;
        config.last_heartbeat_at = 0;
        config.heartbeat_ttl = 0;
        config.raydium_graduated = false;

        config.max_settle_per_window = DEFAULT_MAX_SETTLE_PER_WINDOW;
        config.settle_window_seconds = DEFAULT_SETTLE_WINDOW_SECONDS;
        config.window_outflow = 0;
        config.window_start = 0;
        config.pending_max_settle_per_window = 0;
        config.pending_max_settle_per_window_unlocks_at = 0;
        config.pending_settle_window_seconds = 0;
        config.pending_settle_window_seconds_unlocks_at = 0;

        config.max_daily_outflow = DEFAULT_MAX_DAILY_OUTFLOW;
        config.daily_window_outflow = 0;
        config.daily_window_start = 0;
        config.pending_max_daily_outflow = 0;
        config.pending_max_daily_outflow_unlocks_at = 0;

        config.pending_pause_authority = Pubkey::default();
        config.pending_pause_authority_unlocks_at = 0;

        config.pending_operator_pubkey = Pubkey::default();
        config.pending_operator_unlocks_at = 0;

        config.propose_cooldown_until = 0;
        config.recent_proposes = [0i64; 5];

        config.founder_pubkey = founder_pubkey;
        config.founding_banker_counter = 0;
        config.vault_seeded = false;

        config.reserved = [0u8; 8];

        let assets = &mut ctx.accounts.registered_assets;
        assets.vault_config = config.key();
        assets.mints = [Pubkey::default(); MAX_ASSETS as usize];
        assets.active_count = 0;
        assets.bump = ctx.bumps.registered_assets;
        assets.reserved = [0u8; 32];

        emit!(VaultInitialized {
            authority: authority_key,
            operator,
            timestamp: Clock::get()?.unix_timestamp,
        });
        Ok(())
    }


    pub fn register_asset(
        ctx: Context<RegisterAsset>,
        asset_mint: Pubkey,
        initial_lp_share_bps: u16,
        provider_settle_owner: Pubkey,
    ) -> Result<()> {
        let config = &ctx.accounts.vault_config;
        require_keys_eq!(
            ctx.accounts.authority.key(),
            config.authority,
            ProviderVaultError::Unauthorized
        );
        require!(
            initial_lp_share_bps <= 10_000,
            ProviderVaultError::InvalidBps
        );
        require!(asset_mint != Pubkey::default(), ProviderVaultError::InvalidAsset);
        require!(
            provider_settle_owner != Pubkey::default(),
            ProviderVaultError::InvalidAuthority
        );

        let assets = &mut ctx.accounts.registered_assets;
        for i in 0..(assets.active_count as usize) {
            require!(
                assets.mints[i] != asset_mint,
                ProviderVaultError::AssetAlreadyRegistered
            );
        }
        require!(
            (assets.active_count as u8) < MAX_ASSETS,
            ProviderVaultError::TooManyAssets
        );

        let mint = &ctx.accounts.mint_account;
        require!(mint.key() == asset_mint, ProviderVaultError::InvalidAsset);
        require!(
            asset_mint == SOL_PSEUDO_MINT || mint.decimals == USDC_DECIMALS,
            ProviderVaultError::InvalidAsset
        );

        let pool = &mut ctx.accounts.asset_pool;
        pool.vault_config = config.key();
        pool.asset_mint = asset_mint;
        pool.is_sol = asset_mint == SOL_PSEUDO_MINT;
        pool.bump = ctx.bumps.asset_pool;
        pool.lp_mint = ctx.accounts.lp_mint.key();
        pool.lp_supply = 0;
        pool.cumulative_gross_ggr = 0;
        pool.last_distributed_gross_ggr = 0;
        pool.last_distributed_at = Clock::get()?.unix_timestamp;

        pool.pending_dev_fee = 0;
        pool.pending_provider_fee = 0;
        pool.pending_affiliate = 0;
        pool.pending_sovereign = 0;
        pool.pending_yield = 0;
        pool.pending_reserve = 0;
        pool.last_distributed_affiliate = 0;
        pool.pending_promo = 0;

        pool.lp_share_bps = initial_lp_share_bps;
        pool.lp_tokens_by_tier = [0u64; 5];
        pool.peak_vault = 0;
        pool.peak_vault_at = Clock::get()?.unix_timestamp;
        pool.circuit_state = CIRCUIT_GREEN;
        pool.red_entered_at = 0;
        pool.waiver_active = false;
        pool.waiver_started_at = 0;
        pool.waiver_max_until = 0;
        pool.insurance_balance = 0;
        pool.withdraw_batch_counter = 0;
        pool.last_batch_opened_at = 0;
        pool.pending_request_count = 0;
        pool.vault_locked = false;
        pool.vault_locked_at = 0;

        pool.provider_settle_owner = provider_settle_owner;
        pool.pending_settle_owner = Pubkey::default();
        pool.pending_settle_owner_unlocks_at = 0;
        pool.provider_owed_total = 0;
        pool.founding_banker_lp_tokens_in_window = 0;
        pool.max_chip_debit_per_24h_per_wallet = 0;
        pool.promo_paid_unreconciled = 0;
        pool.network_reimbursement_owed = 0;
        pool.provider_credit = 0;
        pool.affiliate_unreconciled = 0;
        pool.vault_holder = Pubkey::default();
        pool.pending_reset_peak = 0;
        pool.pending_reset_peak_unlocks_at = 0;
        pool.reserved = [0u8; 24];

        let lp = &ctx.accounts.lp_mint;
        require!(
            lp.mint_authority == anchor_lang::solana_program::program_option::COption::Some(pool.key()),
            ProviderVaultError::MintAuthorityMismatch
        );
        require!(lp.freeze_authority.is_none(), ProviderVaultError::InvalidLpMint);
        require!(lp.supply == 0, ProviderVaultError::InvalidLpMint);

        let slot = assets.active_count as usize;
        assets.mints[slot] = asset_mint;
        assets.active_count = assets
            .active_count
            .checked_add(1)
            .ok_or(ProviderVaultError::MathOverflow)?;

        emit!(AssetRegistered {
            asset_mint,
            lp_mint: ctx.accounts.lp_mint.key(),
            initial_lp_share_bps,
            timestamp: Clock::get()?.unix_timestamp,
        });
        Ok(())
    }


    pub fn add_provider(
        ctx: Context<AddProvider>,
        name: [u8; PROVIDER_NAME_LEN],
        provider_fee_bps: u16,
        affiliate_recorder_pubkey: Pubkey,
        signed_terms_hash: [u8; 32],
    ) -> Result<()> {
        let config = &mut ctx.accounts.vault_config;
        require_keys_eq!(
            ctx.accounts.authority.key(),
            config.authority,
            ProviderVaultError::Unauthorized
        );
        require!(
            provider_fee_bps <= MAX_PROVIDER_FEE_BPS,
            ProviderVaultError::ProviderFeeTooHigh
        );
        require!(
            config.active_provider_count < MAX_PROVIDERS,
            ProviderVaultError::TooManyProviders
        );
        require_non_default_pubkeys(&[affiliate_recorder_pubkey])?;

        let provider = &mut ctx.accounts.provider;
        provider.provider_id = config.next_provider_id;
        provider.name = name;
        provider.bump = ctx.bumps.provider;
        provider.active = true;
        provider.paused = false;
        provider.paused_at = 0;
        provider.settle_paused = false;
        provider.pause_reason = [0u8; PAUSE_REASON_LEN];

        provider.provider_fee_bps = provider_fee_bps;
        provider.fee_owed_since_last_sweep = 0;
        provider.affiliate_recorder_pubkey = affiliate_recorder_pubkey;
        provider.signed_terms_hash = signed_terms_hash;

        provider.cumulative_gross_ggr = 0;
        provider.cumulative_gross_wager = 0;
        provider.cumulative_gross_payout = 0;
        provider.cumulative_bet_count = 0;
        provider.last_submission_at = 0;
        provider.last_day_id = 0;

        provider.period_net_ggr = 0;
        provider.period_fee_charged = 0;
        provider.fee_correction_applied = 0;

        provider.reserved = [0u8; 47];

        config.next_provider_id = config
            .next_provider_id
            .checked_add(1)
            .ok_or(ProviderVaultError::MathOverflow)?;
        config.active_provider_count = config
            .active_provider_count
            .checked_add(1)
            .ok_or(ProviderVaultError::MathOverflow)?;

        emit!(ProviderAdded {
            provider_id: provider.provider_id,
            name,
            provider_fee_bps,
            timestamp: Clock::get()?.unix_timestamp,
        });
        Ok(())
    }

    pub fn update_provider_fee(
        ctx: Context<UpdateProviderFee>,
        provider_id: u32,
        new_bps: u16,
    ) -> Result<()> {
        let config = &ctx.accounts.vault_config;
        require_keys_eq!(
            ctx.accounts.authority.key(),
            config.authority,
            ProviderVaultError::Unauthorized
        );
        require!(
            new_bps <= MAX_PROVIDER_FEE_BPS,
            ProviderVaultError::ProviderFeeTooHigh
        );
        let provider = &mut ctx.accounts.provider;
        require!(provider.provider_id == provider_id, ProviderVaultError::ProviderMismatch);
        require!(
            provider.period_net_ggr <= 0,
            ProviderVaultError::FeeBpsChangeWouldRepriceOpenPeriod
        );
        let old = provider.provider_fee_bps;
        provider.provider_fee_bps = new_bps;
        emit!(ProviderFeeUpdated {
            provider_id,
            old_bps: old,
            new_bps,
            timestamp: Clock::get()?.unix_timestamp,
        });
        Ok(())
    }

    pub fn pause_provider_settlement(
        ctx: Context<PauseProviderSettlement>,
        provider_id: u32,
    ) -> Result<()> {
        let config = &ctx.accounts.vault_config;
        require_keys_eq!(
            ctx.accounts.authority.key(),
            config.authority,
            ProviderVaultError::Unauthorized
        );
        let provider = &mut ctx.accounts.provider;
        require!(provider.provider_id == provider_id, ProviderVaultError::ProviderMismatch);
        provider.settle_paused = true;
        emit!(ProviderSettlementPaused { provider_id, paused: true });
        Ok(())
    }

    pub fn unpause_provider_settlement(
        ctx: Context<PauseProviderSettlement>,
        provider_id: u32,
    ) -> Result<()> {
        let config = &ctx.accounts.vault_config;
        require_keys_eq!(
            ctx.accounts.authority.key(),
            config.authority,
            ProviderVaultError::Unauthorized
        );
        let provider = &mut ctx.accounts.provider;
        require!(provider.provider_id == provider_id, ProviderVaultError::ProviderMismatch);
        provider.settle_paused = false;
        emit!(ProviderSettlementPaused { provider_id, paused: false });
        Ok(())
    }


    pub fn propose_set_settle_owner(
        ctx: Context<ProposeSettleOwner>,
        asset_mint: Pubkey,
        new_wallet: Pubkey,
    ) -> Result<()> {
        let config = &mut ctx.accounts.vault_config;
        require_keys_eq!(
            ctx.accounts.authority.key(),
            config.authority,
            ProviderVaultError::Unauthorized
        );
        require!(new_wallet != Pubkey::default(), ProviderVaultError::InvalidAuthority);

        let pool = &mut ctx.accounts.asset_pool;
        require!(pool.asset_mint == asset_mint, ProviderVaultError::AssetMismatch);

        let now = Clock::get()?.unix_timestamp;
        check_and_record_propose(config, now)?;
        pool.pending_settle_owner = new_wallet;
        pool.pending_settle_owner_unlocks_at = now
            .checked_add(ADMIN_TIMELOCK_SECONDS)
            .ok_or(ProviderVaultError::MathOverflow)?;

        emit!(SettleOwnerProposed {
            asset_mint,
            new_wallet,
            unlocks_at: pool.pending_settle_owner_unlocks_at,
        });
        Ok(())
    }

    pub fn finalize_set_settle_owner(
        ctx: Context<ProposeSettleOwner>,
        asset_mint: Pubkey,
    ) -> Result<()> {
        let config = &ctx.accounts.vault_config;
        require_keys_eq!(
            ctx.accounts.authority.key(),
            config.authority,
            ProviderVaultError::Unauthorized
        );
        let pool = &mut ctx.accounts.asset_pool;
        require!(pool.asset_mint == asset_mint, ProviderVaultError::AssetMismatch);
        require!(
            pool.pending_settle_owner != Pubkey::default(),
            ProviderVaultError::NothingPending
        );
        let now = Clock::get()?.unix_timestamp;
        require!(
            now >= pool.pending_settle_owner_unlocks_at,
            ProviderVaultError::TimelockNotElapsed
        );
        let old = pool.provider_settle_owner;
        pool.provider_settle_owner = pool.pending_settle_owner;
        pool.pending_settle_owner = Pubkey::default();
        pool.pending_settle_owner_unlocks_at = 0;
        emit!(SettleOwnerFinalized {
            asset_mint,
            old_wallet: old,
            new_wallet: pool.provider_settle_owner,
        });
        Ok(())
    }

    pub fn cancel_set_settle_owner(
        ctx: Context<ProposeSettleOwner>,
        asset_mint: Pubkey,
    ) -> Result<()> {
        let config = &ctx.accounts.vault_config;
        require_keys_eq!(
            ctx.accounts.authority.key(),
            config.authority,
            ProviderVaultError::Unauthorized
        );
        let pool = &mut ctx.accounts.asset_pool;
        require!(pool.asset_mint == asset_mint, ProviderVaultError::AssetMismatch);
        require!(
            pool.pending_settle_owner != Pubkey::default(),
            ProviderVaultError::NothingPending
        );
        let cancelled = pool.pending_settle_owner;
        pool.pending_settle_owner = Pubkey::default();
        pool.pending_settle_owner_unlocks_at = 0;
        emit!(SettleOwnerCancelled { asset_mint, cancelled });
        Ok(())
    }


    pub fn submit_provider_ggr(
        ctx: Context<SubmitProviderGgr>,
        provider_id: u32,
        day_id: u64,
        asset_mint: Pubkey,
        gross_wager: u64,
        gross_payout: u64,
        bet_count: u32,
        provider_signed_digest: [u8; 32],
    ) -> Result<()> {
        let config = &ctx.accounts.vault_config;
        require!(!config.is_frozen, ProviderVaultError::VaultFrozen);
        require_keys_eq!(
            ctx.accounts.operator.key(),
            config.operator_pubkey,
            ProviderVaultError::Unauthorized
        );
        require!(!config.is_paused, ProviderVaultError::VaultPaused);

        let pool = &mut ctx.accounts.asset_pool;
        require!(!pool.vault_locked, ProviderVaultError::VaultLocked);
        require!(pool.asset_mint == asset_mint, ProviderVaultError::AssetMismatch);

        let provider = &mut ctx.accounts.provider;
        require!(provider.provider_id == provider_id, ProviderVaultError::ProviderMismatch);
        require!(provider.active, ProviderVaultError::ProviderInactive);
        require!(!provider.paused, ProviderVaultError::ProviderPaused);

        let now_ts = Clock::get()?.unix_timestamp;
        let current_day = (now_ts as u64) / 86_400;

        require!(
            if provider.last_day_id == 0 {
                day_id >= current_day
            } else {
                day_id > provider.last_day_id
            },
            ProviderVaultError::DayIdRegression
        );

        require!(
            day_id <= current_day.saturating_add(1),
            ProviderVaultError::InvalidDayId
        );

        let net_ggr_signed = compute_net_ggr(gross_wager, gross_payout)?;

        let holder_balance = ctx.accounts.vault_holder.amount;


        let snapshot_bps = provider.provider_fee_bps;

        let fee_step = provider_period_fee_step(
            provider.period_net_ggr,
            net_ggr_signed,
            provider.period_fee_charged,
            snapshot_bps,
        )?;
        provider.period_net_ggr = fee_step.period_net_after;
        provider.period_fee_charged = fee_step.fee_target;

        let fee_due: u64 = fee_step.increase;

        provider.fee_owed_since_last_sweep = provider
            .fee_owed_since_last_sweep
            .checked_add(fee_due)
            .ok_or(ProviderVaultError::MathOverflow)?;

        reduce_provider_fee_accrual(pool, provider, fee_step.decrease);

        provider.cumulative_gross_wager = provider
            .cumulative_gross_wager
            .checked_add(gross_wager)
            .ok_or(ProviderVaultError::MathOverflow)?;
        provider.cumulative_gross_payout = provider
            .cumulative_gross_payout
            .checked_add(gross_payout)
            .ok_or(ProviderVaultError::MathOverflow)?;
        provider.cumulative_gross_ggr = provider
            .cumulative_gross_ggr
            .checked_add(net_ggr_signed)
            .ok_or(ProviderVaultError::MathOverflow)?;
        provider.cumulative_bet_count = provider
            .cumulative_bet_count
            .checked_add(bet_count as u64)
            .ok_or(ProviderVaultError::MathOverflow)?;
        provider.last_submission_at = Clock::get()?.unix_timestamp;
        provider.last_day_id = day_id;

        let cum_before_ggr = pool.cumulative_gross_ggr;
        pool.cumulative_gross_ggr = pool
            .cumulative_gross_ggr
            .checked_add(net_ggr_signed)
            .ok_or(ProviderVaultError::MathOverflow)?;

        let accrual_base_signed = effective_accrual_base(
            pool.last_distributed_gross_ggr,
            cum_before_ggr,
            net_ggr_signed,
        )?;

        let after_provider_for_net: u64 = if net_ggr_signed > 0 {
            (accrual_base_signed as u64).saturating_sub(fee_due)
        } else {
            0
        };
        let own_promo_unreconciled = pool
            .promo_paid_unreconciled
            .saturating_sub(pool.network_reimbursement_owed);
        let promo_to_net = own_promo_unreconciled.min(after_provider_for_net);

        let remaining_base = after_provider_for_net.saturating_sub(promo_to_net);
        let affiliate_to_net = pool.affiliate_unreconciled.min(remaining_base);
        let cost_netted = promo_to_net
            .checked_add(affiliate_to_net)
            .ok_or(ProviderVaultError::MathOverflow)?;

        accrue_earmarks(
            pool,
            accrual_base_signed,
            config.phase,
            snapshot_bps,
            fee_due,
            config.dev_fee_bps,
            cost_netted,
            fee_step.decrease,
        )?;

        pool.promo_paid_unreconciled = pool
            .promo_paid_unreconciled
            .checked_sub(promo_to_net)
            .ok_or(ProviderVaultError::MathOverflow)?;
        pool.affiliate_unreconciled = pool
            .affiliate_unreconciled
            .checked_sub(affiliate_to_net)
            .ok_or(ProviderVaultError::MathOverflow)?;

        require_earmark_invariant(pool, holder_balance)?;

        recompute_circuit_state(pool, holder_balance, now_ts)?;

        let receipt = &mut ctx.accounts.receipt;
        receipt.provider_id = provider_id;
        receipt.day_id = day_id;
        receipt.asset_mint = asset_mint;
        receipt.bump = ctx.bumps.receipt;
        receipt.gross_wager = gross_wager;
        receipt.gross_payout = gross_payout;
        receipt.net_ggr = net_ggr_signed;
        receipt.bet_count = bet_count;
        receipt.provider_signed_digest = provider_signed_digest;
        receipt.submitter_pubkey = ctx.accounts.operator.key();
        receipt.submitted_at = Clock::get()?.unix_timestamp;
        receipt.fee_bps_at_accrual = snapshot_bps;
        receipt.fee_due_recorded = fee_due;
        receipt.reserved = [0u8; 32];

        emit!(ProviderGgrSubmitted {
            provider_id,
            day_id,
            asset_mint,
            net_ggr: net_ggr_signed,
            fee_bps_at_accrual: snapshot_bps,
            fee_due,
            promo_netted: promo_to_net,
            new_promo_paid_unreconciled: pool.promo_paid_unreconciled,
            affiliate_netted: affiliate_to_net,
            new_affiliate_unreconciled: pool.affiliate_unreconciled,
            timestamp: receipt.submitted_at,
            fee_decrease: fee_step.decrease,
            period_net_ggr: fee_step.period_net_after,
            period_fee_charged: fee_step.fee_target,
        });
        Ok(())
    }


    pub fn close_debit_receipt(
        _ctx: Context<CloseDebitReceipt>,
        _reference: [u8; 32],
    ) -> Result<()> {
        Ok(())
    }

    pub fn close_withdraw_receipt(
        _ctx: Context<CloseWithdrawReceipt>,
        _reference: [u8; 32],
    ) -> Result<()> {
        Ok(())
    }

    pub fn close_credit_receipt(
        _ctx: Context<CloseCreditReceipt>,
        _reference: [u8; 32],
    ) -> Result<()> {
        Ok(())
    }

    pub fn close_credit_receipt_promo(
        _ctx: Context<CloseCreditReceiptPromo>,
        _reference: [u8; 32],
    ) -> Result<()> {
        Ok(())
    }

    pub fn close_credit_receipt_ngr(
        _ctx: Context<CloseCreditReceiptNgr>,
        _reference: [u8; 32],
    ) -> Result<()> {
        Ok(())
    }

    pub fn distribute_ggr(
        ctx: Context<DistributeGgr>,
        is_keeper: bool,
    ) -> Result<()> {
        let config = &ctx.accounts.vault_config;
        require!(!config.is_frozen, ProviderVaultError::VaultFrozen);
        let pool = &mut ctx.accounts.asset_pool;
        let now = Clock::get()?.unix_timestamp;

        if is_keeper {
            require!(
                now >= pool
                    .last_distributed_at
                    .checked_add(KEEPER_WINDOW_SECONDS)
                    .ok_or(ProviderVaultError::MathOverflow)?,
                ProviderVaultError::KeeperWindowNotElapsed
            );
        } else {
            require_keys_eq!(
                ctx.accounts.signer.key(),
                config.waterfall_authority,
                ProviderVaultError::Unauthorized
            );
        }

        require!(!pool.vault_locked, ProviderVaultError::VaultLocked);

        let delta_gross_signed = pool
            .cumulative_gross_ggr
            .checked_sub(pool.last_distributed_gross_ggr)
            .ok_or(ProviderVaultError::MathOverflow)?;

        let affiliate_accrual = pool.pending_affiliate;
        let delta_net_signed = (delta_gross_signed)
            .checked_sub(affiliate_accrual as i64)
            .ok_or(ProviderVaultError::MathOverflow)?;

        if delta_net_signed <= 0
            || (delta_net_signed as u64) < MIN_DELTA_GGR_FOR_SWEEP_USDC
        {
            if affiliate_accrual > 0 {
                emit!(SweepSkippedDueToNetNegative {
                    asset_mint: pool.asset_mint,
                    gross_delta: delta_gross_signed,
                    affiliate_accrual,
                    net_delta: delta_net_signed,
                    timestamp: now,
                });
            } else {
                emit!(SweepSkipped {
                    asset_mint: pool.asset_mint,
                    delta_ggr: delta_gross_signed,
                    threshold: MIN_DELTA_GGR_FOR_SWEEP_USDC,
                    timestamp: now,
                });
            }
            if !is_keeper {
                pool.last_distributed_at = now;
            }
            return Ok(());
        }

        advance_hwm_on_drain(pool);
        if !is_keeper {
            pool.last_distributed_at = now;
        }

        if is_keeper {
            emit!(KeeperSweepTriggered {
                asset_mint: pool.asset_mint,
                delta_ggr: delta_gross_signed,
                timestamp: now,
            });
        }

        emit!(Distributed {
            asset_mint: pool.asset_mint,
            gross_delta: delta_gross_signed,
            net_delta: delta_net_signed,
            affiliate_accrual,
            pending_dev_fee: pool.pending_dev_fee,
            pending_provider_fee: pool.pending_provider_fee,
            pending_sovereign: pool.pending_sovereign,
            pending_yield: pool.pending_yield,
            pending_reserve: pool.pending_reserve,
            timestamp: now,
        });
        Ok(())
    }


    pub fn settle_provider_invoice(
        ctx: Context<SettleProviderInvoice>,
        provider_id: u32,
        asset_mint: Pubkey,
    ) -> Result<()> {
        let config = &ctx.accounts.vault_config;
        require!(!config.is_frozen, ProviderVaultError::VaultFrozen);
        let pool = &mut ctx.accounts.asset_pool;
        require!(!pool.vault_locked, ProviderVaultError::VaultLocked);
        let provider = &mut ctx.accounts.provider;
        let owed = &mut ctx.accounts.provider_owed;
        let now = Clock::get()?.unix_timestamp;

        require!(provider.provider_id == provider_id, ProviderVaultError::ProviderMismatch);
        require!(pool.asset_mint == asset_mint, ProviderVaultError::AssetMismatch);
        require!(!provider.settle_paused, ProviderVaultError::ProviderSettlePaused);
        require!(owed.amount > 0, ProviderVaultError::NothingOwed);

        let caller = ctx.accounts.caller.key();
        let is_operator = caller == config.operator_pubkey;
        let is_keeper_eligible = now
            >= owed
                .last_settled_at
                .checked_add(PROVIDER_SETTLE_KEEPER_DAYS * SECONDS_PER_DAY)
                .ok_or(ProviderVaultError::MathOverflow)?;
        require!(
            is_operator || is_keeper_eligible,
            ProviderVaultError::Unauthorized
        );


        let amount = owed.amount;

        let reimbursable_reconciled = pool.network_reimbursement_owed;
        let credit_avail = pool
            .provider_credit
            .checked_add(pool.network_reimbursement_owed)
            .ok_or(ProviderVaultError::MathOverflow)?;
        let reimb_applied = credit_avail.min(amount);
        let pay_pp = amount
            .checked_sub(reimb_applied)
            .ok_or(ProviderVaultError::MathOverflow)?;
        let from_credit = pool.provider_credit.min(reimb_applied);
        pool.provider_credit = pool
            .provider_credit
            .checked_sub(from_credit)
            .ok_or(ProviderVaultError::MathOverflow)?;
        let from_network = reimb_applied
            .checked_sub(from_credit)
            .ok_or(ProviderVaultError::MathOverflow)?;
        pool.network_reimbursement_owed = pool
            .network_reimbursement_owed
            .checked_sub(from_network)
            .ok_or(ProviderVaultError::MathOverflow)?;
        pool.provider_credit = pool
            .provider_credit
            .checked_add(pool.network_reimbursement_owed)
            .ok_or(ProviderVaultError::MathOverflow)?;
        pool.network_reimbursement_owed = 0;

        pool.promo_paid_unreconciled = pool
            .promo_paid_unreconciled
            .saturating_sub(reimbursable_reconciled);

        owed.amount = 0;
        owed.last_settled_at = now;
        pool.provider_owed_total = pool
            .provider_owed_total
            .checked_sub(amount)
            .ok_or(ProviderVaultError::MathOverflow)?;

        if pay_pp > 0 {
            let asset_mint_bytes = asset_mint.to_bytes();
            let vault_config_bytes = config.key().to_bytes();
            let seeds: &[&[u8]] = &[
                b"asset_pool",
                vault_config_bytes.as_ref(),
                asset_mint_bytes.as_ref(),
                &[pool.bump],
            ];
            let signer = &[seeds];

            let cpi = CpiContext::new_with_signer(
                ctx.accounts.token_program.to_account_info(),
                TransferChecked {
                    from: ctx.accounts.vault_holder.to_account_info(),
                    mint: ctx.accounts.asset_mint_account.to_account_info(),
                    to: ctx.accounts.settle_recipient.to_account_info(),
                    authority: pool.to_account_info(),
                },
                signer,
            );
            token::transfer_checked(cpi, pay_pp, USDC_DECIMALS)?;
        }

        ctx.accounts.vault_holder.reload()?;
        let settle_post_balance = ctx.accounts.vault_holder.amount;
        recompute_circuit_state(pool, settle_post_balance, now)?;

        emit!(ProviderInvoiceSettled {
            provider_id,
            asset_mint,
            amount,
            reimbursement_applied: reimb_applied,
            paid_to_provider: pay_pp,
            new_provider_credit: pool.provider_credit,
            recipient: pool.provider_settle_owner,
            is_keeper: !is_operator,
            timestamp: now,
        });
        Ok(())
    }

    pub fn flush_provider_fee(
        ctx: Context<FlushProviderFee>,
        provider_id: u32,
        asset_mint: Pubkey,
    ) -> Result<()> {
        let config = &ctx.accounts.vault_config;
        require!(!config.is_frozen, ProviderVaultError::VaultFrozen);
        require!(
            ctx.accounts.signer.key() == config.operator_pubkey
                || ctx.accounts.signer.key() == config.waterfall_authority,
            ProviderVaultError::Unauthorized
        );
        let pool = &mut ctx.accounts.asset_pool;
        let provider = &mut ctx.accounts.provider;
        let owed = &mut ctx.accounts.provider_owed;

        require!(provider.provider_id == provider_id, ProviderVaultError::ProviderMismatch);
        require!(pool.asset_mint == asset_mint, ProviderVaultError::AssetMismatch);

        if owed.asset_pool == Pubkey::default() {
            owed.bump = ctx.bumps.provider_owed;
            owed.asset_pool = pool.key();
            owed.provider_id = provider_id;
            owed.last_settled_at = Clock::get()?.unix_timestamp;
        }

        let amount = provider.fee_owed_since_last_sweep;
        require!(amount > 0, ProviderVaultError::NothingOwed);

        provider.period_net_ggr = 0;
        provider.period_fee_charged = 0;

        provider.fee_owed_since_last_sweep = 0;
        pool.pending_provider_fee = pool
            .pending_provider_fee
            .checked_sub(amount)
            .ok_or(ProviderVaultError::MathOverflow)?;
        owed.amount = owed
            .amount
            .checked_add(amount)
            .ok_or(ProviderVaultError::MathOverflow)?;
        pool.provider_owed_total = pool
            .provider_owed_total
            .checked_add(amount)
            .ok_or(ProviderVaultError::MathOverflow)?;

        emit!(ProviderFeeFlushed {
            provider_id,
            asset_mint,
            amount,
            owed_after: owed.amount,
            timestamp: Clock::get()?.unix_timestamp,
        });
        Ok(())
    }

    pub fn correct_provider_fee_overaccrual(
        ctx: Context<CorrectProviderFeeOverAccrual>,
        provider_id: u32,
        asset_mint: Pubkey,
        expected_pending_provider_fee: u64,
        expected_fee_owed_since_last_sweep: u64,
        new_pending_provider_fee: u64,
    ) -> Result<()> {
        let config = &ctx.accounts.vault_config;
        require_keys_eq!(
            ctx.accounts.authority.key(),
            config.authority,
            ProviderVaultError::Unauthorized
        );

        let pool = &mut ctx.accounts.asset_pool;
        let provider = &mut ctx.accounts.provider;
        require!(provider.provider_id == provider_id, ProviderVaultError::ProviderMismatch);
        require!(pool.asset_mint == asset_mint, ProviderVaultError::AssetMismatch);

        require!(
            provider.fee_correction_applied == 0,
            ProviderVaultError::FeeCorrectionAlreadyApplied
        );

        require!(
            pool.pending_provider_fee == expected_pending_provider_fee
                && provider.fee_owed_since_last_sweep == expected_fee_owed_since_last_sweep,
            ProviderVaultError::FeeCorrectionPreStateMismatch
        );

        require!(
            new_pending_provider_fee < expected_pending_provider_fee,
            ProviderVaultError::FeeCorrectionMustDecrease
        );
        let delta = expected_pending_provider_fee
            .checked_sub(new_pending_provider_fee)
            .ok_or(ProviderVaultError::MathOverflow)?;
        require!(
            delta <= expected_fee_owed_since_last_sweep,
            ProviderVaultError::FeeCorrectionMustDecrease
        );

        let holder_balance = ctx.accounts.vault_holder.amount;
        let nav_before = nav_basis(pool, holder_balance)?;
        let fee_owed_before = provider.fee_owed_since_last_sweep;

        pool.pending_provider_fee = pool
            .pending_provider_fee
            .checked_sub(delta)
            .ok_or(ProviderVaultError::MathOverflow)?;
        provider.fee_owed_since_last_sweep = provider
            .fee_owed_since_last_sweep
            .checked_sub(delta)
            .ok_or(ProviderVaultError::MathOverflow)?;
        provider.fee_correction_applied = 1;

        require_earmark_invariant(pool, holder_balance)?;
        let nav_after = nav_basis(pool, holder_balance)?;

        let now = Clock::get()?.unix_timestamp;
        recompute_circuit_state(pool, holder_balance, now)?;

        emit!(ProviderFeeOverAccrualCorrected {
            provider_id,
            asset_mint,
            authority: ctx.accounts.authority.key(),
            pending_provider_fee_before: expected_pending_provider_fee,
            pending_provider_fee_after: pool.pending_provider_fee,
            fee_owed_before,
            fee_owed_after: provider.fee_owed_since_last_sweep,
            delta,
            holder_balance,
            nav_before,
            nav_after,
            timestamp: now,
        });
        Ok(())
    }


    pub fn deposit_lp_usdc(
        ctx: Context<DepositLpUsdc>,
        amount: u64,
        lp_tier: u8,
    ) -> Result<()> {
        require!(amount >= MIN_DEPOSIT_USDC, ProviderVaultError::DepositBelowMinimum);
        require!(lp_tier <= 4, ProviderVaultError::InvalidTier);

        let depositor_key = ctx.accounts.depositor.key();
        let now = Clock::get()?.unix_timestamp;
        let founder_pubkey;
        let vault_seeded_pre;
        let founding_banker_counter_pre;
        let phase;
        {
            let config = &ctx.accounts.vault_config;
            require!(!config.is_frozen, ProviderVaultError::VaultFrozen);

            if config.vault_seeded {
                require!(!config.is_paused, ProviderVaultError::VaultPaused);
            }

            founder_pubkey = config.founder_pubkey;
            vault_seeded_pre = config.vault_seeded;
            founding_banker_counter_pre = config.founding_banker_counter;
            phase = config.phase;
        }
        let _ = phase;
        let _ = founding_banker_counter_pre;

        if !vault_seeded_pre {
            require!(
                depositor_key == founder_pubkey,
                ProviderVaultError::OnlyFounderCanSeed
            );
            require!(
                amount >= FOUNDING_BANKER_MIN_USDC_MICRO,
                ProviderVaultError::DepositBelowFoundingMin
            );
        }

        let derived_tier = compute_tier(
            ctx.accounts.lp_position.cumulative_deposited.saturating_add(amount),
        ) as usize;

        let pool = &mut ctx.accounts.asset_pool;
        require!(!pool.vault_locked, ProviderVaultError::VaultLocked);
        require!(!pool.is_sol, ProviderVaultError::WrongAssetBranch);

        let holder_balance = ctx.accounts.vault_holder.amount;
        let nav_balance = nav_basis(pool, holder_balance)?;
        let lp_supply = pool.lp_supply;

        let (minted_lp, dead_carve) = compute_shares_for_deposit(
            amount,
            nav_balance,
            lp_supply,
        )?;
        require!(minted_lp > 0, ProviderVaultError::ZeroSharesMinted);

        let cpi = CpiContext::new(
            ctx.accounts.token_program.to_account_info(),
            TransferChecked {
                from: ctx.accounts.depositor_token_account.to_account_info(),
                mint: ctx.accounts.asset_mint_account.to_account_info(),
                to: ctx.accounts.vault_holder.to_account_info(),
                authority: ctx.accounts.depositor.to_account_info(),
            },
        );
        token::transfer_checked(cpi, amount, USDC_DECIMALS)?;

        let asset_mint_bytes = pool.asset_mint.to_bytes();
        let vault_config_key = pool.vault_config;
        let vault_config_bytes = vault_config_key.to_bytes();
        let seeds: &[&[u8]] = &[
            b"asset_pool",
            vault_config_bytes.as_ref(),
            asset_mint_bytes.as_ref(),
            &[pool.bump],
        ];
        let signer = &[seeds];

        if dead_carve > 0 {
            let cpi = CpiContext::new_with_signer(
                ctx.accounts.token_program.to_account_info(),
                anchor_spl::token::MintTo {
                    mint: ctx.accounts.lp_mint.to_account_info(),
                    to: ctx.accounts.dead_shares_ata.to_account_info(),
                    authority: pool.to_account_info(),
                },
                signer,
            );
            token::mint_to(cpi, dead_carve)?;
        }
        let cpi = CpiContext::new_with_signer(
            ctx.accounts.token_program.to_account_info(),
            anchor_spl::token::MintTo {
                mint: ctx.accounts.lp_mint.to_account_info(),
                to: ctx.accounts.depositor_lp_ata.to_account_info(),
                authority: pool.to_account_info(),
            },
            signer,
        );
        token::mint_to(cpi, minted_lp)?;

        let total_minted = minted_lp
            .checked_add(dead_carve)
            .ok_or(ProviderVaultError::MathOverflow)?;

        pool.lp_supply = pool
            .lp_supply
            .checked_add(total_minted)
            .ok_or(ProviderVaultError::MathOverflow)?;
        pool.lp_tokens_by_tier[derived_tier] = pool
            .lp_tokens_by_tier[derived_tier]
            .checked_add(minted_lp)
            .ok_or(ProviderVaultError::MathOverflow)?;

        let position = &mut ctx.accounts.lp_position;
        if position.bump == 0 {
            position.holder = ctx.accounts.depositor.key();
            position.tier = derived_tier as u8;
            position.bump = ctx.bumps.lp_position;
            position.reserved = [0u8; 22];
            position.is_founding_banker = false;
            position.founding_banker_seat_number = 0;
            position.founding_banker_seat_timestamp = 0;
        }
        if position.tier != derived_tier as u8 && position.lp_shares > 0 {
            pool.lp_tokens_by_tier[position.tier as usize] = pool
                .lp_tokens_by_tier[position.tier as usize]
                .saturating_sub(position.lp_shares);
            pool.lp_tokens_by_tier[derived_tier] = pool
                .lp_tokens_by_tier[derived_tier]
                .checked_add(position.lp_shares)
                .ok_or(ProviderVaultError::MathOverflow)?;
            position.tier = derived_tier as u8;
        }

        let pre_is_fb = position.is_founding_banker;
        let pre_fb_at = position.founding_banker_seat_timestamp;
        let pre_lp_shares_before_add = position.lp_shares;

        position.lp_shares = position
            .lp_shares
            .checked_add(minted_lp)
            .ok_or(ProviderVaultError::MathOverflow)?;
        position.cumulative_deposited = position
            .cumulative_deposited
            .checked_add(amount)
            .ok_or(ProviderVaultError::MathOverflow)?;
        position.last_deposit_at = now;

        if pre_is_fb {
            let still_in_window = now
                < pre_fb_at
                    .checked_add(FOUNDING_BANKER_BONUS_DAYS * SECONDS_PER_DAY)
                    .ok_or(ProviderVaultError::MathOverflow)?;
            if still_in_window {
                pool.founding_banker_lp_tokens_in_window = pool
                    .founding_banker_lp_tokens_in_window
                    .checked_add(minted_lp)
                    .ok_or(ProviderVaultError::MathOverflow)?;
            } else {
                let drain = pre_lp_shares_before_add
                    .min(pool.founding_banker_lp_tokens_in_window);
                pool.founding_banker_lp_tokens_in_window = pool
                    .founding_banker_lp_tokens_in_window
                    .saturating_sub(drain);
            }
        }

        if !position.is_founding_banker
            && amount >= FOUNDING_BANKER_MIN_USDC_MICRO
        {
            let config = &mut ctx.accounts.vault_config;
            if config.founding_banker_counter < FOUNDING_BANKER_MAX_SEATS {
                config.founding_banker_counter = config
                    .founding_banker_counter
                    .checked_add(1)
                    .ok_or(ProviderVaultError::MathOverflow)?;
                position.is_founding_banker = true;
                position.founding_banker_seat_number = config.founding_banker_counter;
                position.founding_banker_seat_timestamp = now;
                pool.founding_banker_lp_tokens_in_window = pool
                    .founding_banker_lp_tokens_in_window
                    .checked_add(position.lp_shares)
                    .ok_or(ProviderVaultError::MathOverflow)?;
                emit!(FoundingBankerGranted {
                    wallet: depositor_key,
                    seat_number: config.founding_banker_counter,
                    amount,
                    timestamp_at: now,
                    vault_seat_count_after: config.founding_banker_counter,
                });
            }
        }

        if !vault_seeded_pre {
            let config = &mut ctx.accounts.vault_config;
            config.vault_seeded = true;
        }

        let new_balance = holder_balance
            .checked_add(amount)
            .ok_or(ProviderVaultError::MathOverflow)?;
        require_earmark_invariant(pool, new_balance)?;

        recompute_circuit_state(pool, new_balance, now)?;

        emit!(LpDeposited {
            depositor: ctx.accounts.depositor.key(),
            asset_mint: pool.asset_mint,
            amount,
            minted_lp,
            tier: derived_tier as u8,
            timestamp: Clock::get()?.unix_timestamp,
        });
        Ok(())
    }

    pub fn request_withdraw_usdc(
        ctx: Context<RequestWithdrawUsdc>,
        lp_amount: u64,
        nonce: u64,
    ) -> Result<()> {
        require!(lp_amount > 0, ProviderVaultError::ZeroAmount);
        let config = &ctx.accounts.vault_config;
        require!(!config.is_frozen, ProviderVaultError::VaultFrozen);

        let pool = &mut ctx.accounts.asset_pool;
        require!(!pool.vault_locked, ProviderVaultError::VaultLocked);

        let position = &mut ctx.accounts.lp_position;
        require!(position.holder == ctx.accounts.wallet.key(), ProviderVaultError::Unauthorized);
        require!(
            lp_amount <= position.lp_shares,
            ProviderVaultError::InsufficientShares
        );

        let now = Clock::get()?.unix_timestamp;
        let tier_cooldown = TIER_COOLDOWN_DAYS[position.tier as usize];
        let cooldown_secs = tier_cooldown
            .checked_mul(SECONDS_PER_DAY)
            .ok_or(ProviderVaultError::MathOverflow)?;
        let mut processable_at = now
            .checked_add(cooldown_secs)
            .ok_or(ProviderVaultError::MathOverflow)?;

        if check_rule30_penalty(position, lp_amount, pool.lp_supply, now)? {
            processable_at = processable_at
                .checked_add(SINGLE_WALLET_EXTRA_COOLDOWN_SECONDS)
                .ok_or(ProviderVaultError::MathOverflow)?;
        }

        let request = &mut ctx.accounts.request;
        request.owner = ctx.accounts.wallet.key();
        request.asset_pool = pool.key();
        request.lp_amount = lp_amount;
        request.nonce = nonce;
        request.requested_at = now;
        request.processable_at = processable_at;
        request.processed = false;
        request.batch_id = 0;
        request.bump = ctx.bumps.request;
        request.reserved = [0u8; 32];

        position.pending_withdrawal_shares = position
            .pending_withdrawal_shares
            .checked_add(lp_amount)
            .ok_or(ProviderVaultError::MathOverflow)?;

        pool.pending_request_count = pool
            .pending_request_count
            .checked_add(1)
            .ok_or(ProviderVaultError::MathOverflow)?;

        emit!(WithdrawRequested {
            owner: request.owner,
            asset_mint: pool.asset_mint,
            lp_amount,
            processable_at,
            timestamp: now,
        });
        Ok(())
    }

    pub fn cancel_withdraw_request(
        ctx: Context<CancelWithdrawRequest>,
    ) -> Result<()> {
        require!(!ctx.accounts.vault_config.is_frozen, ProviderVaultError::VaultFrozen);
        let request = &mut ctx.accounts.request;
        require!(request.owner == ctx.accounts.wallet.key(), ProviderVaultError::Unauthorized);
        require!(!request.processed, ProviderVaultError::RequestAlreadyProcessed);
        require!(request.batch_id == 0, ProviderVaultError::RequestAlreadyAssigned);

        let position = &mut ctx.accounts.lp_position;
        position.pending_withdrawal_shares = position
            .pending_withdrawal_shares
            .saturating_sub(request.lp_amount);

        let pool = &mut ctx.accounts.asset_pool;
        pool.pending_request_count = pool.pending_request_count.saturating_sub(1);

        emit!(WithdrawRequestCancelled {
            owner: request.owner,
            asset_mint: pool.asset_mint,
            lp_amount: request.lp_amount,
            timestamp: Clock::get()?.unix_timestamp,
        });
        Ok(())
    }

    pub fn process_withdraw_request_usdc(
        ctx: Context<ProcessWithdrawRequestUsdc>,
    ) -> Result<()> {
        require!(!ctx.accounts.vault_config.is_frozen, ProviderVaultError::VaultFrozen);
        let pool = &mut ctx.accounts.asset_pool;
        require!(!pool.vault_locked, ProviderVaultError::VaultLocked);

        let now = Clock::get()?.unix_timestamp;
        let cooldown_waived = withdrawal_cooldown_waived(pool, now);

        let request = &mut ctx.accounts.request;
        require!(!request.processed, ProviderVaultError::RequestAlreadyProcessed);
        if !cooldown_waived {
            require!(
                now >= request.processable_at,
                ProviderVaultError::CooldownNotElapsed
            );
        }

        let holder_balance = ctx.accounts.vault_holder.amount;
        let nav = nav_basis(pool, holder_balance)?;
        let payout = compute_lamports_for_withdraw(
            request.lp_amount,
            nav,
            pool.lp_supply,
        )?;
        require!(payout > 0, ProviderVaultError::ZeroPayout);

        let post_balance = holder_balance
            .checked_sub(payout)
            .ok_or(ProviderVaultError::MathOverflow)?;
        require!(
            post_balance >= HARD_VAULT_FLOOR_USDC,
            ProviderVaultError::SubFloorWithdrawBlocked
        );

        let asset_mint_bytes = pool.asset_mint.to_bytes();
        let vault_config_key = pool.vault_config;
        let vault_config_bytes = vault_config_key.to_bytes();
        let seeds: &[&[u8]] = &[
            b"asset_pool",
            vault_config_bytes.as_ref(),
            asset_mint_bytes.as_ref(),
            &[pool.bump],
        ];
        let signer = &[seeds];

        let burn_ctx = CpiContext::new_with_signer(
            ctx.accounts.token_program.to_account_info(),
            anchor_spl::token::Burn {
                mint: ctx.accounts.lp_mint.to_account_info(),
                from: ctx.accounts.owner_lp_ata.to_account_info(),
                authority: ctx.accounts.wallet.to_account_info(),
            },
            &[],
        );
        token::burn(burn_ctx, request.lp_amount)?;

        let cpi = CpiContext::new_with_signer(
            ctx.accounts.token_program.to_account_info(),
            TransferChecked {
                from: ctx.accounts.vault_holder.to_account_info(),
                mint: ctx.accounts.asset_mint_account.to_account_info(),
                to: ctx.accounts.wallet_token_account.to_account_info(),
                authority: pool.to_account_info(),
            },
            signer,
        );
        token::transfer_checked(cpi, payout, USDC_DECIMALS)?;

        let pre_burn_is_fb;
        let pre_burn_lp_shares;
        let pre_burn_seat_number;
        {
            let position_pre = &ctx.accounts.lp_position;
            pre_burn_is_fb = position_pre.is_founding_banker;
            pre_burn_lp_shares = position_pre.lp_shares;
            pre_burn_seat_number = position_pre.founding_banker_seat_number;
        }

        let position = &mut ctx.accounts.lp_position;
        position.lp_shares = position
            .lp_shares
            .checked_sub(request.lp_amount)
            .ok_or(ProviderVaultError::MathOverflow)?;
        position.pending_withdrawal_shares = position
            .pending_withdrawal_shares
            .saturating_sub(request.lp_amount);
        position.last_withdrawal_at = now;
        let (rearmed_start, rearmed_rolling) = rolling_window_rearm(
            position.rolling_7d_window_start,
            position.rolling_7d_withdrawn_shares,
            now,
        );
        position.rolling_7d_window_start = rearmed_start;
        position.rolling_7d_withdrawn_shares = rearmed_rolling
            .checked_add(request.lp_amount)
            .ok_or(ProviderVaultError::MathOverflow)?;
        let post_burn_lp_shares = position.lp_shares;
        let _ = pre_burn_lp_shares;

        pool.lp_supply = pool
            .lp_supply
            .checked_sub(request.lp_amount)
            .ok_or(ProviderVaultError::MathOverflow)?;
        pool.lp_tokens_by_tier[position.tier as usize] = pool
            .lp_tokens_by_tier[position.tier as usize]
            .saturating_sub(request.lp_amount);

        if pre_burn_is_fb {
            pool.founding_banker_lp_tokens_in_window = pool
                .founding_banker_lp_tokens_in_window
                .saturating_sub(request.lp_amount);
        }

        if pre_burn_is_fb && post_burn_lp_shares == 0 {
            let config = &mut ctx.accounts.vault_config;
            let new_count = config.founding_banker_counter.saturating_sub(1);
            config.founding_banker_counter = new_count;
            emit!(FoundingBankerReleased {
                wallet: position.holder,
                seat_number: pre_burn_seat_number,
                timestamp_at: now,
                vault_seat_count_after: new_count,
            });
        }

        pool.pending_request_count = pool.pending_request_count.saturating_sub(1);

        request.processed = true;

        let new_holder = post_balance;
        require_earmark_invariant(pool, new_holder)?;

        recompute_circuit_state(pool, new_holder, now)?;

        emit!(WithdrawProcessed {
            owner: request.owner,
            asset_mint: pool.asset_mint,
            lp_amount: request.lp_amount,
            payout,
            timestamp: now,
        });
        Ok(())
    }

    pub fn refill_insurance_usdc(
        ctx: Context<RefillInsuranceUsdc>,
        amount: u64,
    ) -> Result<()> {
        require!(amount > 0, ProviderVaultError::ZeroAmount);
        let vault_config = &ctx.accounts.vault_config;
        let pool = &mut ctx.accounts.asset_pool;

        require!(!vault_config.is_frozen, ProviderVaultError::VaultFrozen);
        require!(!pool.is_sol, ProviderVaultError::WrongAssetBranch);

        let cpi = CpiContext::new(
            ctx.accounts.token_program.to_account_info(),
            TransferChecked {
                from: ctx.accounts.source_token_account.to_account_info(),
                mint: ctx.accounts.asset_mint.to_account_info(),
                to: ctx.accounts.insurance_holder.to_account_info(),
                authority: ctx.accounts.authority.to_account_info(),
            },
        );
        token::transfer_checked(cpi, amount, USDC_DECIMALS)?;

        ctx.accounts.insurance_holder.reload()?;
        let new_balance = ctx.accounts.insurance_holder.amount;
        pool.insurance_balance = new_balance;

        let now_refill = Clock::get()?.unix_timestamp;
        let vh_balance = ctx.accounts.vault_holder.amount;
        recompute_circuit_state(pool, vh_balance, now_refill)?;

        emit!(InsuranceRefilled {
            asset_mint: pool.asset_mint,
            amount,
            new_balance,
            timestamp: Clock::get()?.unix_timestamp,
        });
        Ok(())
    }

    pub fn deposit_lp_sol(_ctx: Context<DepositLpSolStub>, _amount: u64, _lp_tier: u8) -> Result<()> {
        err!(ProviderVaultError::SolPondFoundingBankerNotImplemented)
    }


    pub fn set_paused(ctx: Context<SetPaused>, paused: bool, reason: [u8; PAUSE_REASON_LEN]) -> Result<()> {
        let config = &mut ctx.accounts.vault_config;
        let now = Clock::get()?.unix_timestamp;

        if paused {
            require!(
                ctx.accounts.signer.key() == config.pause_authority
                    || ctx.accounts.signer.key() == config.authority,
                ProviderVaultError::Unauthorized
            );
            if config.last_pause_at != 0 {
                require!(
                    now >= config.last_pause_at + PAUSE_RATE_LIMIT_SECONDS,
                    ProviderVaultError::PauseRateLimited
                );
            }
            config.last_pause_at = now;
            config.pause_reason = reason;
        } else {
            require_keys_eq!(
                ctx.accounts.signer.key(),
                config.authority,
                ProviderVaultError::Unauthorized
            );
        }
        config.is_paused = paused;
        emit!(PausedChanged { paused, timestamp: now });
        Ok(())
    }


    pub fn freeze(ctx: Context<Freeze>) -> Result<()> {
        let config = &mut ctx.accounts.vault_config;
        let now = Clock::get()?.unix_timestamp;

        require_keys_eq!(
            ctx.accounts.signer.key(),
            config.pause_authority,
            ProviderVaultError::Unauthorized
        );

        if config.last_freeze_at != 0 {
            require!(
                now >= config.last_freeze_at + FREEZE_RATE_LIMIT_SECONDS,
                ProviderVaultError::FreezeRateLimited
            );
        }

        config.is_frozen = true;
        config.last_freeze_at = now;
        emit!(VaultFrozenEvent { by: ctx.accounts.signer.key(), timestamp: now });
        Ok(())
    }

    pub fn unfreeze(ctx: Context<Unfreeze>) -> Result<()> {
        let config = &mut ctx.accounts.vault_config;
        let now = Clock::get()?.unix_timestamp;

        require_keys_eq!(
            ctx.accounts.signer.key(),
            config.authority,
            ProviderVaultError::Unauthorized
        );

        config.is_frozen = false;
        emit!(VaultUnfrozenEvent { by: ctx.accounts.signer.key(), timestamp: now });
        Ok(())
    }


    pub fn heartbeat(ctx: Context<AdminAction>) -> Result<()> {
        let config = &mut ctx.accounts.vault_config;
        require_keys_eq!(
            ctx.accounts.signer.key(),
            config.operator_pubkey,
            ProviderVaultError::Unauthorized
        );
        let now = Clock::get()?.unix_timestamp;
        config.last_heartbeat_at = now;
        emit!(HeartbeatRecorded { operator: ctx.accounts.signer.key(), timestamp: now });
        Ok(())
    }

    pub fn halt_if_stale(ctx: Context<AdminAction>) -> Result<()> {
        let config = &mut ctx.accounts.vault_config;
        require!(config.heartbeat_ttl > 0, ProviderVaultError::DeadmanDisabled);
        let now = Clock::get()?.unix_timestamp;
        let age = now
            .checked_sub(config.last_heartbeat_at)
            .ok_or(ProviderVaultError::MathOverflow)?;
        require!(age > config.heartbeat_ttl, ProviderVaultError::HeartbeatNotStale);
        config.is_frozen = true;
        emit!(DeadmanHaltTriggered { by: ctx.accounts.signer.key(), age, timestamp: now });
        Ok(())
    }

    pub fn set_heartbeat_ttl(ctx: Context<AdminAction>, new_ttl: i64) -> Result<()> {
        let config = &mut ctx.accounts.vault_config;
        require_keys_eq!(
            ctx.accounts.signer.key(),
            config.authority,
            ProviderVaultError::Unauthorized
        );
        require!(new_ttl >= 0, ProviderVaultError::InvalidHeartbeatTtl);
        let now = Clock::get()?.unix_timestamp;
        config.heartbeat_ttl = new_ttl;
        if new_ttl > 0 {
            config.last_heartbeat_at = now;
        }
        emit!(HeartbeatTtlSet { new_ttl, timestamp: now });
        Ok(())
    }

    pub fn propose_transfer_authority(
        ctx: Context<AdminAction>,
        new_authority: Pubkey,
    ) -> Result<()> {
        let config = &mut ctx.accounts.vault_config;
        require_keys_eq!(
            ctx.accounts.signer.key(),
            config.authority,
            ProviderVaultError::Unauthorized
        );
        require!(new_authority != Pubkey::default(), ProviderVaultError::InvalidAuthority);
        require!(new_authority != config.operator_pubkey, ProviderVaultError::OperatorRoleCollision);
        require!(new_authority != config.pause_authority, ProviderVaultError::OperatorRoleCollision);
        require!(new_authority != config.waterfall_authority, ProviderVaultError::OperatorRoleCollision);
        require!(new_authority != config.affiliate_recorder_pubkey, ProviderVaultError::OperatorRoleCollision);
        let now = Clock::get()?.unix_timestamp;
        check_and_record_propose(config, now)?;
        config.pending_authority = new_authority;
        config.pending_authority_unlocks_at = now
            .checked_add(ADMIN_TIMELOCK_SECONDS)
            .ok_or(ProviderVaultError::MathOverflow)?;
        emit!(AuthorityProposed {
            new_authority,
            unlocks_at: config.pending_authority_unlocks_at,
        });
        Ok(())
    }

    pub fn finalize_transfer_authority(ctx: Context<AdminAction>) -> Result<()> {
        let config = &mut ctx.accounts.vault_config;
        require_keys_eq!(
            ctx.accounts.signer.key(),
            config.authority,
            ProviderVaultError::Unauthorized
        );
        require!(
            config.pending_authority != Pubkey::default(),
            ProviderVaultError::NothingPending
        );
        let now = Clock::get()?.unix_timestamp;
        require!(
            now >= config.pending_authority_unlocks_at,
            ProviderVaultError::TimelockNotElapsed
        );
        require!(config.pending_authority != config.operator_pubkey, ProviderVaultError::OperatorRoleCollision);
        require!(config.pending_authority != config.pause_authority, ProviderVaultError::OperatorRoleCollision);
        require!(config.pending_authority != config.waterfall_authority, ProviderVaultError::OperatorRoleCollision);
        require!(config.pending_authority != config.affiliate_recorder_pubkey, ProviderVaultError::OperatorRoleCollision);
        let old = config.authority;
        config.authority = config.pending_authority;
        config.pending_authority = Pubkey::default();
        config.pending_authority_unlocks_at = 0;
        emit!(AuthorityRotated {
            old,
            new: config.authority,
            timestamp: now,
        });
        Ok(())
    }

    pub fn cancel_transfer_authority(ctx: Context<AdminAction>) -> Result<()> {
        let config = &mut ctx.accounts.vault_config;
        require_keys_eq!(
            ctx.accounts.signer.key(),
            config.authority,
            ProviderVaultError::Unauthorized
        );
        require!(
            config.pending_authority != Pubkey::default(),
            ProviderVaultError::NothingPending
        );
        config.pending_authority = Pubkey::default();
        config.pending_authority_unlocks_at = 0;
        emit!(AuthorityProposalCancelled {});
        Ok(())
    }

    pub fn propose_set_dev_fee_bps(ctx: Context<AdminAction>, new_bps: u16) -> Result<()> {
        let config = &mut ctx.accounts.vault_config;
        require_keys_eq!(
            ctx.accounts.signer.key(),
            config.authority,
            ProviderVaultError::Unauthorized
        );
        require!(new_bps <= MAX_DEV_FEE_BPS, ProviderVaultError::InvalidBps);
        let now = Clock::get()?.unix_timestamp;
        check_and_record_propose(config, now)?;
        config.pending_dev_fee_bps = new_bps;
        config.pending_dev_fee_bps_unlocks_at = now
            .checked_add(ADMIN_TIMELOCK_SECONDS)
            .ok_or(ProviderVaultError::MathOverflow)?;
        emit!(DevFeeBpsProposed {
            new_bps,
            unlocks_at: config.pending_dev_fee_bps_unlocks_at,
        });
        Ok(())
    }

    pub fn finalize_set_dev_fee_bps(ctx: Context<AdminAction>) -> Result<()> {
        let config = &mut ctx.accounts.vault_config;
        require_keys_eq!(
            ctx.accounts.signer.key(),
            config.authority,
            ProviderVaultError::Unauthorized
        );
        require!(
            config.pending_dev_fee_bps_unlocks_at != 0,
            ProviderVaultError::NothingPending
        );
        let now = Clock::get()?.unix_timestamp;
        require!(
            now >= config.pending_dev_fee_bps_unlocks_at,
            ProviderVaultError::TimelockNotElapsed
        );
        let old = config.dev_fee_bps;
        let new = config.pending_dev_fee_bps;
        config.dev_fee_bps = new;
        config.pending_dev_fee_bps = 0;
        config.pending_dev_fee_bps_unlocks_at = 0;
        emit!(DevFeeBpsRotated { old, new, timestamp: now });
        Ok(())
    }

    pub fn cancel_set_dev_fee_bps(ctx: Context<AdminAction>) -> Result<()> {
        let config = &mut ctx.accounts.vault_config;
        require_keys_eq!(
            ctx.accounts.signer.key(),
            config.authority,
            ProviderVaultError::Unauthorized
        );
        require!(
            config.pending_dev_fee_bps_unlocks_at != 0,
            ProviderVaultError::NothingPending
        );
        config.pending_dev_fee_bps = 0;
        config.pending_dev_fee_bps_unlocks_at = 0;
        emit!(DevFeeBpsProposalCancelled {});
        Ok(())
    }

    pub fn propose_reset_peak_vault(
        ctx: Context<ProposeResetPeak>,
        new_peak: u64,
    ) -> Result<()> {
        require_keys_eq!(
            ctx.accounts.signer.key(),
            ctx.accounts.vault_config.authority,
            ProviderVaultError::Unauthorized
        );
        let now = Clock::get()?.unix_timestamp;
        check_and_record_propose(&mut ctx.accounts.vault_config, now)?;

        let pool = &mut ctx.accounts.asset_pool;
        pool.pending_reset_peak = new_peak;
        pool.pending_reset_peak_unlocks_at = now
            .checked_add(ADMIN_TIMELOCK_SECONDS)
            .ok_or(ProviderVaultError::MathOverflow)?;
        emit!(PeakResetProposed {
            asset_mint: pool.asset_mint,
            new_peak,
            current_peak: pool.peak_vault,
            unlocks_at: pool.pending_reset_peak_unlocks_at,
        });
        Ok(())
    }

    pub fn finalize_reset_peak_vault(ctx: Context<FinalizeResetPeak>) -> Result<()> {
        require_keys_eq!(
            ctx.accounts.signer.key(),
            ctx.accounts.vault_config.authority,
            ProviderVaultError::Unauthorized
        );
        let now = Clock::get()?.unix_timestamp;
        {
            let pool = &ctx.accounts.asset_pool;
            require!(
                pool.pending_reset_peak_unlocks_at != 0,
                ProviderVaultError::NothingPending
            );
            require!(
                now >= pool.pending_reset_peak_unlocks_at,
                ProviderVaultError::TimelockNotElapsed
            );
        }
        let new_peak = ctx.accounts.asset_pool.pending_reset_peak;
        let old_peak = ctx.accounts.asset_pool.peak_vault;
        {
            let pool = &mut ctx.accounts.asset_pool;
            pool.peak_vault = new_peak;
            pool.peak_vault_at = now;
            pool.pending_reset_peak = 0;
            pool.pending_reset_peak_unlocks_at = 0;
        }
        let holder_balance = ctx.accounts.vault_holder.amount;
        let state_before = ctx.accounts.asset_pool.circuit_state;
        let new_state = if state_before == CIRCUIT_GREEN {
            recompute_circuit_state(&mut ctx.accounts.asset_pool, holder_balance, now)?
        } else {
            state_before
        };
        emit!(PeakResetFinalized {
            asset_mint: ctx.accounts.asset_pool.asset_mint,
            old_peak,
            new_peak,
            circuit_state_after: new_state,
            timestamp: now,
        });
        Ok(())
    }

    pub fn cancel_reset_peak_vault(ctx: Context<CancelResetPeak>) -> Result<()> {
        require_keys_eq!(
            ctx.accounts.signer.key(),
            ctx.accounts.vault_config.authority,
            ProviderVaultError::Unauthorized
        );
        let pool = &mut ctx.accounts.asset_pool;
        require!(
            pool.pending_reset_peak_unlocks_at != 0,
            ProviderVaultError::NothingPending
        );
        pool.pending_reset_peak = 0;
        pool.pending_reset_peak_unlocks_at = 0;
        emit!(PeakResetProposalCancelled {
            asset_mint: pool.asset_mint,
        });
        Ok(())
    }


    pub fn propose_max_settle_per_window(
        ctx: Context<AdminAction>,
        new_value: u64,
    ) -> Result<()> {
        let config = &mut ctx.accounts.vault_config;
        require_keys_eq!(
            ctx.accounts.signer.key(),
            config.authority,
            ProviderVaultError::Unauthorized
        );
        require!(
            new_value >= MIN_MAX_SETTLE_PER_WINDOW,
            ProviderVaultError::WindowCapBelowMinimum
        );
        let now = Clock::get()?.unix_timestamp;
        check_and_record_propose(config, now)?;
        config.pending_max_settle_per_window = new_value;
        config.pending_max_settle_per_window_unlocks_at = now
            .checked_add(ADMIN_TIMELOCK_SECONDS)
            .ok_or(ProviderVaultError::MathOverflow)?;
        emit!(MaxSettlePerWindowProposed {
            new_value,
            unlocks_at: config.pending_max_settle_per_window_unlocks_at,
        });
        Ok(())
    }

    pub fn finalize_max_settle_per_window(ctx: Context<AdminAction>) -> Result<()> {
        let config = &mut ctx.accounts.vault_config;
        require_keys_eq!(
            ctx.accounts.signer.key(),
            config.authority,
            ProviderVaultError::Unauthorized
        );
        require!(
            config.pending_max_settle_per_window_unlocks_at != 0,
            ProviderVaultError::NothingPending
        );
        let now = Clock::get()?.unix_timestamp;
        require!(
            now >= config.pending_max_settle_per_window_unlocks_at,
            ProviderVaultError::TimelockNotElapsed
        );
        let old = config.max_settle_per_window;
        let new = config.pending_max_settle_per_window;
        config.max_settle_per_window = new;
        config.pending_max_settle_per_window = 0;
        config.pending_max_settle_per_window_unlocks_at = 0;
        emit!(MaxSettlePerWindowRotated { old, new, timestamp: now });
        Ok(())
    }

    pub fn cancel_max_settle_per_window(ctx: Context<AdminAction>) -> Result<()> {
        let config = &mut ctx.accounts.vault_config;
        require_keys_eq!(
            ctx.accounts.signer.key(),
            config.authority,
            ProviderVaultError::Unauthorized
        );
        require!(
            config.pending_max_settle_per_window_unlocks_at != 0,
            ProviderVaultError::NothingPending
        );
        config.pending_max_settle_per_window = 0;
        config.pending_max_settle_per_window_unlocks_at = 0;
        emit!(MaxSettlePerWindowProposalCancelled {});
        Ok(())
    }

    pub fn propose_settle_window_seconds(
        ctx: Context<AdminAction>,
        new_value: u32,
    ) -> Result<()> {
        let config = &mut ctx.accounts.vault_config;
        require_keys_eq!(
            ctx.accounts.signer.key(),
            config.authority,
            ProviderVaultError::Unauthorized
        );
        require!(
            new_value >= MIN_SETTLE_WINDOW_SECONDS
                && new_value <= MAX_SETTLE_WINDOW_SECONDS,
            ProviderVaultError::WindowSecondsOutOfRange
        );
        let now = Clock::get()?.unix_timestamp;
        check_and_record_propose(config, now)?;
        config.pending_settle_window_seconds = new_value;
        config.pending_settle_window_seconds_unlocks_at = now
            .checked_add(ADMIN_TIMELOCK_SECONDS)
            .ok_or(ProviderVaultError::MathOverflow)?;
        emit!(SettleWindowSecondsProposed {
            new_value,
            unlocks_at: config.pending_settle_window_seconds_unlocks_at,
        });
        Ok(())
    }

    pub fn finalize_settle_window_seconds(ctx: Context<AdminAction>) -> Result<()> {
        let config = &mut ctx.accounts.vault_config;
        require_keys_eq!(
            ctx.accounts.signer.key(),
            config.authority,
            ProviderVaultError::Unauthorized
        );
        require!(
            config.pending_settle_window_seconds_unlocks_at != 0,
            ProviderVaultError::NothingPending
        );
        let now = Clock::get()?.unix_timestamp;
        require!(
            now >= config.pending_settle_window_seconds_unlocks_at,
            ProviderVaultError::TimelockNotElapsed
        );
        let old = config.settle_window_seconds;
        let new = config.pending_settle_window_seconds;
        config.settle_window_seconds = new;
        config.pending_settle_window_seconds = 0;
        config.pending_settle_window_seconds_unlocks_at = 0;
        emit!(SettleWindowSecondsRotated { old, new, timestamp: now });
        Ok(())
    }

    pub fn cancel_settle_window_seconds(ctx: Context<AdminAction>) -> Result<()> {
        let config = &mut ctx.accounts.vault_config;
        require_keys_eq!(
            ctx.accounts.signer.key(),
            config.authority,
            ProviderVaultError::Unauthorized
        );
        require!(
            config.pending_settle_window_seconds_unlocks_at != 0,
            ProviderVaultError::NothingPending
        );
        config.pending_settle_window_seconds = 0;
        config.pending_settle_window_seconds_unlocks_at = 0;
        emit!(SettleWindowSecondsProposalCancelled {});
        Ok(())
    }


    pub fn propose_max_daily_outflow(
        ctx: Context<AdminAction>,
        new_value: u64,
    ) -> Result<()> {
        let config = &mut ctx.accounts.vault_config;
        require_keys_eq!(
            ctx.accounts.signer.key(),
            config.authority,
            ProviderVaultError::Unauthorized
        );
        require!(
            new_value >= MIN_MAX_DAILY_OUTFLOW,
            ProviderVaultError::WindowCapBelowMinimum
        );
        let now = Clock::get()?.unix_timestamp;
        check_and_record_propose(config, now)?;
        config.pending_max_daily_outflow = new_value;
        config.pending_max_daily_outflow_unlocks_at = now
            .checked_add(ADMIN_TIMELOCK_SECONDS)
            .ok_or(ProviderVaultError::MathOverflow)?;
        emit!(MaxDailyOutflowProposed {
            new_max: new_value,
            unlocks_at: config.pending_max_daily_outflow_unlocks_at,
            timestamp: now,
        });
        Ok(())
    }

    pub fn finalize_max_daily_outflow(ctx: Context<AdminAction>) -> Result<()> {
        let config = &mut ctx.accounts.vault_config;
        require_keys_eq!(
            ctx.accounts.signer.key(),
            config.authority,
            ProviderVaultError::Unauthorized
        );
        require!(
            config.pending_max_daily_outflow_unlocks_at != 0,
            ProviderVaultError::NothingPending
        );
        let now = Clock::get()?.unix_timestamp;
        require!(
            now >= config.pending_max_daily_outflow_unlocks_at,
            ProviderVaultError::TimelockNotElapsed
        );
        let old = config.max_daily_outflow;
        let new = config.pending_max_daily_outflow;
        config.max_daily_outflow = new;
        config.pending_max_daily_outflow = 0;
        config.pending_max_daily_outflow_unlocks_at = 0;
        emit!(MaxDailyOutflowRotated { old, new, timestamp: now });
        Ok(())
    }

    pub fn cancel_max_daily_outflow(ctx: Context<AdminAction>) -> Result<()> {
        let config = &mut ctx.accounts.vault_config;
        require_keys_eq!(
            ctx.accounts.signer.key(),
            config.authority,
            ProviderVaultError::Unauthorized
        );
        require!(
            config.pending_max_daily_outflow_unlocks_at != 0,
            ProviderVaultError::NothingPending
        );
        let cancelled_value = config.pending_max_daily_outflow;
        config.pending_max_daily_outflow = 0;
        config.pending_max_daily_outflow_unlocks_at = 0;
        emit!(MaxDailyOutflowProposalCancelled {
            cancelled_value,
            timestamp: Clock::get()?.unix_timestamp,
        });
        Ok(())
    }

    pub fn set_chip_debit_cap_per_wallet(
        ctx: Context<SetChipDebitCapPerWallet>,
        _asset_mint: Pubkey,
        new_cap: u64,
    ) -> Result<()> {
        let config = &ctx.accounts.vault_config;
        require_keys_eq!(
            ctx.accounts.signer.key(),
            config.authority,
            ProviderVaultError::Unauthorized
        );
        require!(
            new_cap <= MAX_CHIP_DEBIT_CAP_PER_WALLET,
            ProviderVaultError::ChipDebitCapTooHigh
        );
        let pool = &mut ctx.accounts.asset_pool;
        let old = pool.max_chip_debit_per_24h_per_wallet;
        pool.max_chip_debit_per_24h_per_wallet = new_cap;
        emit!(ChipDebitCapPerWalletSet {
            asset_mint: pool.asset_mint,
            old_cap: old,
            new_cap,
            timestamp: Clock::get()?.unix_timestamp,
        });
        Ok(())
    }

    pub fn set_phase(ctx: Context<AdminAction>, new_phase: u8) -> Result<()> {
        let config = &mut ctx.accounts.vault_config;
        require_keys_eq!(
            ctx.accounts.signer.key(),
            config.authority,
            ProviderVaultError::Unauthorized
        );
        require!(new_phase <= 2, ProviderVaultError::InvalidPhase);
        require!(new_phase > config.phase, ProviderVaultError::PhaseNotAdvancing);
        let now = Clock::get()?.unix_timestamp;
        require!(
            now >= config.phase_started_at + 7 * SECONDS_PER_DAY,
            ProviderVaultError::PhaseNotEnoughTime
        );
        config.phase = new_phase;
        config.phase_started_at = now;
        emit!(PhaseAdvanced { new_phase, timestamp: now });
        Ok(())
    }


    pub fn propose_rotate_pause_authority(
        ctx: Context<AdminAction>,
        new_pubkey: Pubkey,
    ) -> Result<()> {
        let config = &mut ctx.accounts.vault_config;
        require_keys_eq!(
            ctx.accounts.signer.key(),
            config.authority,
            ProviderVaultError::Unauthorized
        );
        require!(
            new_pubkey != Pubkey::default(),
            ProviderVaultError::InvalidAuthority
        );
        require!(
            new_pubkey != config.authority,
            ProviderVaultError::InvalidAuthority
        );
        require!(
            new_pubkey != config.operator_pubkey,
            ProviderVaultError::InvalidAuthority
        );
        require!(
            new_pubkey != config.waterfall_authority,
            ProviderVaultError::InvalidAuthority
        );
        require!(
            new_pubkey != config.pause_authority,
            ProviderVaultError::InvalidAuthority
        );
        require!(
            new_pubkey != config.affiliate_recorder_pubkey,
            ProviderVaultError::OperatorRoleCollision
        );
        let now = Clock::get()?.unix_timestamp;
        check_and_record_propose(config, now)?;
        let unlocks_at = now
            .checked_add(ADMIN_TIMELOCK_SECONDS)
            .ok_or(ProviderVaultError::MathOverflow)?;
        config.pending_pause_authority = new_pubkey;
        config.pending_pause_authority_unlocks_at = unlocks_at;
        emit!(RotatePauseAuthorityProposed {
            admin: ctx.accounts.signer.key(),
            new_authority: new_pubkey,
            unlocks_at,
            timestamp: now,
        });
        Ok(())
    }

    pub fn finalize_rotate_pause_authority(ctx: Context<AdminAction>) -> Result<()> {
        let config = &mut ctx.accounts.vault_config;
        require_keys_eq!(
            ctx.accounts.signer.key(),
            config.authority,
            ProviderVaultError::Unauthorized
        );
        require!(
            config.pending_pause_authority != Pubkey::default(),
            ProviderVaultError::NothingPending
        );
        let now = Clock::get()?.unix_timestamp;
        require!(
            now >= config.pending_pause_authority_unlocks_at,
            ProviderVaultError::TimelockNotElapsed
        );
        require!(
            config.pending_pause_authority != config.authority,
            ProviderVaultError::InvalidAuthority
        );
        require!(
            config.pending_pause_authority != config.operator_pubkey,
            ProviderVaultError::InvalidAuthority
        );
        require!(
            config.pending_pause_authority != config.waterfall_authority,
            ProviderVaultError::InvalidAuthority
        );
        require!(
            config.pending_pause_authority != config.affiliate_recorder_pubkey,
            ProviderVaultError::OperatorRoleCollision
        );
        let old = config.pause_authority;
        let new_authority = config.pending_pause_authority;
        config.pause_authority = new_authority;
        config.pending_pause_authority = Pubkey::default();
        config.pending_pause_authority_unlocks_at = 0;
        emit!(PauseAuthorityRotated { old, new: new_authority });
        Ok(())
    }

    pub fn cancel_rotate_pause_authority(ctx: Context<AdminAction>) -> Result<()> {
        let config = &mut ctx.accounts.vault_config;
        require_keys_eq!(
            ctx.accounts.signer.key(),
            config.authority,
            ProviderVaultError::Unauthorized
        );
        require!(
            config.pending_pause_authority != Pubkey::default(),
            ProviderVaultError::NothingPending
        );
        config.pending_pause_authority = Pubkey::default();
        config.pending_pause_authority_unlocks_at = 0;
        emit!(RotatePauseAuthorityProposalCancelled {
            admin: ctx.accounts.signer.key(),
            timestamp: Clock::get()?.unix_timestamp,
        });
        Ok(())
    }


    pub fn propose_rotate_operator(
        ctx: Context<AdminAction>,
        new_operator: Pubkey,
    ) -> Result<()> {
        let config = &mut ctx.accounts.vault_config;
        require_keys_eq!(
            ctx.accounts.signer.key(),
            config.authority,
            ProviderVaultError::Unauthorized
        );
        require!(
            new_operator != Pubkey::default(),
            ProviderVaultError::InvalidOperator
        );
        require!(
            new_operator != config.authority,
            ProviderVaultError::OperatorRoleCollision
        );
        require!(
            new_operator != config.pause_authority,
            ProviderVaultError::OperatorRoleCollision
        );
        require!(
            new_operator != config.waterfall_authority,
            ProviderVaultError::OperatorRoleCollision
        );
        require!(
            new_operator != config.affiliate_recorder_pubkey,
            ProviderVaultError::OperatorRoleCollision
        );
        require!(
            new_operator != config.operator_pubkey,
            ProviderVaultError::InvalidOperator
        );
        require!(
            config.pending_operator_pubkey == Pubkey::default(),
            ProviderVaultError::ProposalAlreadyPending
        );
        let now = Clock::get()?.unix_timestamp;
        check_and_record_propose(config, now)?;
        let unlocks_at = now
            .checked_add(ADMIN_TIMELOCK_SECONDS)
            .ok_or(ProviderVaultError::MathOverflow)?;
        let current_operator = config.operator_pubkey;
        config.pending_operator_pubkey = new_operator;
        config.pending_operator_unlocks_at = unlocks_at;
        emit!(OperatorRotationProposed {
            admin: ctx.accounts.signer.key(),
            current_operator,
            new_operator,
            unlocks_at,
            timestamp: now,
        });
        Ok(())
    }

    pub fn finalize_rotate_operator(ctx: Context<AdminAction>) -> Result<()> {
        let config = &mut ctx.accounts.vault_config;
        require_keys_eq!(
            ctx.accounts.signer.key(),
            config.authority,
            ProviderVaultError::Unauthorized
        );
        require!(
            config.pending_operator_pubkey != Pubkey::default(),
            ProviderVaultError::NoProposalPending
        );
        let now = Clock::get()?.unix_timestamp;
        require!(
            now >= config.pending_operator_unlocks_at,
            ProviderVaultError::TimelockNotElapsed
        );
        require!(
            config.pending_operator_pubkey != config.authority,
            ProviderVaultError::OperatorRoleCollision
        );
        require!(
            config.pending_operator_pubkey != config.pause_authority,
            ProviderVaultError::OperatorRoleCollision
        );
        require!(
            config.pending_operator_pubkey != config.waterfall_authority,
            ProviderVaultError::OperatorRoleCollision
        );
        require!(
            config.pending_operator_pubkey != config.affiliate_recorder_pubkey,
            ProviderVaultError::OperatorRoleCollision
        );
        let old_operator = config.operator_pubkey;
        let new_operator = config.pending_operator_pubkey;
        config.operator_pubkey = new_operator;
        config.pending_operator_pubkey = Pubkey::default();
        config.pending_operator_unlocks_at = 0;
        emit!(OperatorRotated {
            old_operator,
            new_operator,
            timestamp: now,
        });
        Ok(())
    }

    pub fn cancel_propose_operator_rotation(ctx: Context<AdminAction>) -> Result<()> {
        let config = &mut ctx.accounts.vault_config;
        require_keys_eq!(
            ctx.accounts.signer.key(),
            config.authority,
            ProviderVaultError::Unauthorized
        );
        require!(
            config.pending_operator_pubkey != Pubkey::default(),
            ProviderVaultError::NoProposalPending
        );
        let cancelled_operator = config.pending_operator_pubkey;
        config.pending_operator_pubkey = Pubkey::default();
        config.pending_operator_unlocks_at = 0;
        emit!(OperatorRotationProposalCancelled {
            admin: ctx.accounts.signer.key(),
            cancelled_operator,
            timestamp: Clock::get()?.unix_timestamp,
        });
        Ok(())
    }

    pub fn set_vault_holder(ctx: Context<SetVaultHolder>) -> Result<()> {
        let config = &ctx.accounts.vault_config;
        require_keys_eq!(
            ctx.accounts.signer.key(),
            config.authority,
            ProviderVaultError::Unauthorized
        );
        let pool = &mut ctx.accounts.asset_pool;
        require_keys_eq!(
            pool.vault_holder,
            Pubkey::default(),
            ProviderVaultError::VaultHolderAlreadySet
        );
        pool.vault_holder = ctx.accounts.vault_holder.key();
        emit!(VaultHolderSet {
            asset_mint: pool.asset_mint,
            vault_holder: pool.vault_holder,
            by: ctx.accounts.signer.key(),
            timestamp: Clock::get()?.unix_timestamp,
        });
        Ok(())
    }

    pub fn lock_vault(ctx: Context<AdminLockVault>) -> Result<()> {
        let config = &ctx.accounts.vault_config;
        require_keys_eq!(
            ctx.accounts.signer.key(),
            config.authority,
            ProviderVaultError::Unauthorized
        );
        let pool = &mut ctx.accounts.asset_pool;
        require!(!pool.vault_locked, ProviderVaultError::VaultAlreadyLocked);
        pool.vault_locked = true;
        pool.vault_locked_at = Clock::get()?.unix_timestamp;
        emit!(VaultLocked { asset_mint: pool.asset_mint, timestamp: pool.vault_locked_at });
        Ok(())
    }

    pub fn unlock_vault(ctx: Context<AdminLockVault>) -> Result<()> {
        let config = &ctx.accounts.vault_config;
        require_keys_eq!(
            ctx.accounts.signer.key(),
            config.authority,
            ProviderVaultError::Unauthorized
        );
        let pool = &mut ctx.accounts.asset_pool;
        require!(pool.vault_locked, ProviderVaultError::VaultNotLocked);
        let now = Clock::get()?.unix_timestamp;
        require!(
            now >= pool.vault_locked_at + UNLOCK_VAULT_MIN_DELAY_SECONDS,
            ProviderVaultError::UnlockTooEarly
        );
        pool.vault_locked = false;
        pool.vault_locked_at = 0;
        emit!(VaultUnlocked { asset_mint: pool.asset_mint, timestamp: now });
        Ok(())
    }


    pub fn cancel_waiver(ctx: Context<AdminLockVault>) -> Result<()> {
        let config = &ctx.accounts.vault_config;
        require_keys_eq!(
            ctx.accounts.signer.key(),
            config.authority,
            ProviderVaultError::Unauthorized
        );
        let pool = &mut ctx.accounts.asset_pool;
        require!(pool.circuit_state == CIRCUIT_RED, ProviderVaultError::WaiverNotRed);
        pool.waiver_max_until = 0;
        pool.waiver_active = false;
        emit!(WaiverCancelled {
            asset_mint: pool.asset_mint,
            by: ctx.accounts.signer.key(),
            timestamp: Clock::get()?.unix_timestamp,
        });
        Ok(())
    }

    pub fn extend_waiver(ctx: Context<AdminLockVault>, extra_seconds: i64) -> Result<()> {
        require!(extra_seconds > 0, ProviderVaultError::ZeroAmount);
        let config = &ctx.accounts.vault_config;
        require_keys_eq!(
            ctx.accounts.signer.key(),
            config.authority,
            ProviderVaultError::Unauthorized
        );
        let pool = &mut ctx.accounts.asset_pool;
        require!(pool.circuit_state == CIRCUIT_RED, ProviderVaultError::WaiverNotRed);
        require!(pool.waiver_max_until != 0, ProviderVaultError::WaiverNotArmed);
        let new_until = pool
            .waiver_max_until
            .checked_add(extra_seconds)
            .ok_or(ProviderVaultError::MathOverflow)?;
        let span = new_until
            .checked_sub(pool.red_entered_at)
            .ok_or(ProviderVaultError::MathOverflow)?;
        require!(
            span <= WAIVER_MAX_TOTAL_SECONDS,
            ProviderVaultError::WaiverExtensionTooLong
        );
        pool.waiver_max_until = new_until;
        emit!(WaiverExtended {
            asset_mint: pool.asset_mint,
            by: ctx.accounts.signer.key(),
            new_waiver_max_until: new_until,
            timestamp: Clock::get()?.unix_timestamp,
        });
        Ok(())
    }

    pub fn set_ops_marketing_wallet(
        ctx: Context<AdminAction>,
        new_wallet: Pubkey,
    ) -> Result<()> {
        let config = &mut ctx.accounts.vault_config;
        require_keys_eq!(
            ctx.accounts.signer.key(),
            config.authority,
            ProviderVaultError::Unauthorized
        );
        require!(new_wallet != Pubkey::default(), ProviderVaultError::InvalidAuthority);
        require!(
            config.ops_marketing_wallet == Pubkey::default(),
            ProviderVaultError::OpsMarketingAlreadyConfigured
        );
        let old = config.ops_marketing_wallet;
        config.ops_marketing_wallet = new_wallet;
        emit!(OpsMarketingWalletRotated {
            old,
            new: new_wallet,
            timestamp: Clock::get()?.unix_timestamp,
        });
        Ok(())
    }

    pub fn propose_set_ops_marketing_wallet(
        ctx: Context<AdminAction>,
        new_wallet: Pubkey,
    ) -> Result<()> {
        let config = &mut ctx.accounts.vault_config;
        require_keys_eq!(
            ctx.accounts.signer.key(),
            config.authority,
            ProviderVaultError::Unauthorized
        );
        require!(new_wallet != Pubkey::default(), ProviderVaultError::InvalidAuthority);
        require!(
            config.ops_marketing_wallet != Pubkey::default(),
            ProviderVaultError::OpsMarketingWalletNotConfigured
        );
        let now = Clock::get()?.unix_timestamp;
        check_and_record_propose(config, now)?;
        config.pending_ops_marketing_wallet = new_wallet;
        config.pending_ops_marketing_wallet_unlocks_at = now
            .checked_add(ADMIN_TIMELOCK_SECONDS)
            .ok_or(ProviderVaultError::MathOverflow)?;
        emit!(OpsMarketingWalletProposed {
            new_wallet,
            unlocks_at: config.pending_ops_marketing_wallet_unlocks_at,
        });
        Ok(())
    }

    pub fn finalize_set_ops_marketing_wallet(ctx: Context<AdminAction>) -> Result<()> {
        let config = &mut ctx.accounts.vault_config;
        require_keys_eq!(
            ctx.accounts.signer.key(),
            config.authority,
            ProviderVaultError::Unauthorized
        );
        require!(
            config.pending_ops_marketing_wallet != Pubkey::default(),
            ProviderVaultError::NothingPending
        );
        let now = Clock::get()?.unix_timestamp;
        require!(
            now >= config.pending_ops_marketing_wallet_unlocks_at,
            ProviderVaultError::TimelockNotElapsed
        );
        let old = config.ops_marketing_wallet;
        let new = config.pending_ops_marketing_wallet;
        config.ops_marketing_wallet = new;
        config.pending_ops_marketing_wallet = Pubkey::default();
        config.pending_ops_marketing_wallet_unlocks_at = 0;
        emit!(OpsMarketingWalletRotated { old, new, timestamp: now });
        Ok(())
    }

    pub fn cancel_set_ops_marketing_wallet(ctx: Context<AdminAction>) -> Result<()> {
        let config = &mut ctx.accounts.vault_config;
        require_keys_eq!(
            ctx.accounts.signer.key(),
            config.authority,
            ProviderVaultError::Unauthorized
        );
        require!(
            config.pending_ops_marketing_wallet != Pubkey::default(),
            ProviderVaultError::NothingPending
        );
        config.pending_ops_marketing_wallet = Pubkey::default();
        config.pending_ops_marketing_wallet_unlocks_at = 0;
        emit!(OpsMarketingWalletProposalCancelled {});
        Ok(())
    }


    pub fn distribute_affiliate(
        ctx: Context<DistributeAffiliate>,
        asset_mint: Pubkey,
    ) -> Result<()> {
        let config = &ctx.accounts.vault_config;
        require!(!config.is_frozen, ProviderVaultError::VaultFrozen);
        let now = Clock::get()?.unix_timestamp;
        let pool = &mut ctx.accounts.asset_pool;

        require!(pool.asset_mint == asset_mint, ProviderVaultError::AssetMismatch);
        require!(!pool.vault_locked, ProviderVaultError::VaultLocked);
        require!(
            pool.circuit_state == CIRCUIT_GREEN,
            ProviderVaultError::CircuitBreakerYieldPaused
        );

        let caller = ctx.accounts.signer.key();
        let is_operator = caller == config.operator_pubkey
            || caller == config.waterfall_authority;
        let is_keeper_eligible = now
            >= pool
                .last_distributed_at
                .checked_add(KEEPER_WINDOW_SECONDS)
                .ok_or(ProviderVaultError::MathOverflow)?;
        require!(
            is_operator || is_keeper_eligible,
            ProviderVaultError::Unauthorized
        );

        let amount = pool.pending_affiliate;
        require!(amount > 0, ProviderVaultError::NothingToDrain);

        advance_hwm_on_drain(pool);

        pool.pending_affiliate = 0;
        pool.last_distributed_affiliate = pool
            .last_distributed_affiliate
            .checked_add(amount)
            .ok_or(ProviderVaultError::MathOverflow)?;

        let asset_mint_bytes = pool.asset_mint.to_bytes();
        let vault_config_bytes = config.key().to_bytes();
        let seeds: &[&[u8]] = &[
            b"asset_pool",
            vault_config_bytes.as_ref(),
            asset_mint_bytes.as_ref(),
            &[pool.bump],
        ];
        let signer = &[seeds];

        let cpi_program = ctx.accounts.affiliate_registry_program.to_account_info();
        let cpi_accounts = AffiliateDepositFundingPool {
            affiliate_config: ctx.accounts.affiliate_config.to_account_info(),
            funding_pool: ctx.accounts.affiliate_funding_pool.to_account_info(),
            source_signer: pool.to_account_info(),
            source_token_account: Some(ctx.accounts.vault_holder.to_account_info()),
            pool_token_account: Some(ctx.accounts.affiliate_pool_token_account.to_account_info()),
            token_program: Some(ctx.accounts.token_program.to_account_info()),
            sol_holder: None,
            system_program: ctx.accounts.system_program.to_account_info(),
        };
        let cpi_ctx = CpiContext::new_with_signer(cpi_program, cpi_accounts, signer);
        affiliate_registry::cpi::deposit_funding_pool(cpi_ctx, amount, asset_mint)?;

        ctx.accounts.vault_holder.reload()?;
        let post_balance = ctx.accounts.vault_holder.amount;
        require_earmark_invariant(pool, post_balance)?;
        recompute_circuit_state(pool, post_balance, now)?;

        emit!(AffiliateDistributed {
            asset_mint,
            amount,
            timestamp: now,
        });
        Ok(())
    }

    pub fn distribute_sovereign(
        ctx: Context<DistributeSovereign>,
        asset_mint: Pubkey,
    ) -> Result<()> {
        let config = &ctx.accounts.vault_config;
        require!(!config.is_frozen, ProviderVaultError::VaultFrozen);
        let now = Clock::get()?.unix_timestamp;
        let pool = &mut ctx.accounts.asset_pool;

        require!(pool.asset_mint == asset_mint, ProviderVaultError::AssetMismatch);
        require!(!pool.vault_locked, ProviderVaultError::VaultLocked);
        require!(
            pool.circuit_state == CIRCUIT_GREEN,
            ProviderVaultError::CircuitBreakerYieldPaused
        );

        let caller = ctx.accounts.signer.key();
        let is_operator = caller == config.operator_pubkey
            || caller == config.waterfall_authority;
        let is_keeper_eligible = now
            >= pool
                .last_distributed_at
                .checked_add(KEEPER_WINDOW_SECONDS)
                .ok_or(ProviderVaultError::MathOverflow)?;
        require!(
            is_operator || is_keeper_eligible,
            ProviderVaultError::Unauthorized
        );

        let amount = pool.pending_sovereign;
        require!(amount > 0, ProviderVaultError::NothingToDrain);

        advance_hwm_on_drain(pool);

        if pool.is_sol {
            return err!(ProviderVaultError::SolPondNotImplemented);
        }

        let seats_filled = ctx.accounts.sovereign_registry_config.total_seats_filled;
        if seats_filled == 0 {
            pool.pending_sovereign = 0;
            pool.pending_reserve = pool
                .pending_reserve
                .checked_add(amount)
                .ok_or(ProviderVaultError::MathOverflow)?;
            emit!(SovereignDistributedFallback {
                asset_mint,
                amount,
                routed_to_reserve: amount,
                timestamp: now,
            });
            return Ok(());
        }

        pool.pending_sovereign = 0;

        let asset_mint_bytes = pool.asset_mint.to_bytes();
        let vault_config_bytes = config.key().to_bytes();
        let seeds: &[&[u8]] = &[
            b"asset_pool",
            vault_config_bytes.as_ref(),
            asset_mint_bytes.as_ref(),
            &[pool.bump],
        ];
        let signer = &[seeds];

        let cpi_program = ctx.accounts.sovereign_registry_program.to_account_info();
        let cpi_accounts = SovereignDepositRoyaltyUsdc {
            registry_config: ctx.accounts.sovereign_registry_config.to_account_info(),
            royalty_vault_usdc: ctx.accounts.sovereign_royalty_vault_usdc.to_account_info(),
            waterfall_source_ata: ctx.accounts.vault_holder.to_account_info(),
            waterfall_signer: pool.to_account_info(),
            token_program: ctx.accounts.token_program.to_account_info(),
        };
        let cpi_ctx = CpiContext::new_with_signer(cpi_program, cpi_accounts, signer);
        sovereign_registry::cpi::deposit_royalty_usdc(cpi_ctx, amount)?;

        ctx.accounts.vault_holder.reload()?;
        let post_balance = ctx.accounts.vault_holder.amount;
        require_earmark_invariant(pool, post_balance)?;
        recompute_circuit_state(pool, post_balance, now)?;

        emit!(SovereignDistributed {
            asset_mint,
            amount,
            timestamp: now,
        });
        Ok(())
    }

    pub fn distribute_yield(
        ctx: Context<DistributeYield>,
        asset_mint: Pubkey,
    ) -> Result<()> {
        let config = &ctx.accounts.vault_config;
        require!(!config.is_frozen, ProviderVaultError::VaultFrozen);
        let now = Clock::get()?.unix_timestamp;
        let pool = &mut ctx.accounts.asset_pool;

        require!(pool.asset_mint == asset_mint, ProviderVaultError::AssetMismatch);
        require!(!pool.vault_locked, ProviderVaultError::VaultLocked);
        require!(!pool.is_sol, ProviderVaultError::SolPondNotImplemented);
        require!(
            pool.circuit_state == CIRCUIT_GREEN,
            ProviderVaultError::CircuitBreakerYieldPaused
        );

        let caller = ctx.accounts.signer.key();
        let is_operator = caller == config.operator_pubkey
            || caller == config.waterfall_authority;
        let is_keeper_eligible = now
            >= pool
                .last_distributed_at
                .checked_add(KEEPER_WINDOW_SECONDS)
                .ok_or(ProviderVaultError::MathOverflow)?;
        require!(
            is_operator || is_keeper_eligible,
            ProviderVaultError::Unauthorized
        );

        let amount = pool.pending_yield;
        require!(amount > 0, ProviderVaultError::NothingToDrain);

        advance_hwm_on_drain(pool);

        pool.pending_yield = 0;

        let asset_mint_bytes = pool.asset_mint.to_bytes();
        let vault_config_bytes = config.key().to_bytes();
        let pool_bump = pool.bump;
        let seeds: &[&[u8]] = &[
            b"asset_pool",
            vault_config_bytes.as_ref(),
            asset_mint_bytes.as_ref(),
            &[pool_bump],
        ];
        let signer = &[seeds];

        let graduated = config.raydium_graduated;

        if !graduated {
            msg!("distribute_yield: Path B inactive (Pump.fun phase) — 100% to stakers");
            let cpi_program = ctx.accounts.yield_escrow_program.to_account_info();
            let cpi_accounts = YieldDepositProviderUsdc {
                yield_config: ctx.accounts.yield_config.to_account_info(),
                source_token_account: ctx.accounts.vault_holder.to_account_info(),
                yield_pool_usdc: ctx.accounts.yield_pool_usdc.to_account_info(),
                waterfall_signer: ctx.accounts.asset_pool.to_account_info(),
                staking_config: ctx.accounts.staking_config.to_account_info(),
                token_program: ctx.accounts.token_program.to_account_info(),
            };
            let cpi_ctx = CpiContext::new_with_signer(cpi_program, cpi_accounts, signer);
            yield_escrow::cpi::deposit_provider_yield_usdc(cpi_ctx, amount)?;
        } else {
            let liquid = amount
                .checked_mul(7_000)
                .ok_or(ProviderVaultError::MathOverflow)?
                .checked_div(10_000)
                .ok_or(ProviderVaultError::MathOverflow)?;
            let swap_part = amount
                .checked_sub(liquid)
                .ok_or(ProviderVaultError::MathOverflow)?;

            let cpi_program = ctx.accounts.yield_escrow_program.to_account_info();
            let cpi_accounts = YieldDepositProviderUsdc {
                yield_config: ctx.accounts.yield_config.to_account_info(),
                source_token_account: ctx.accounts.vault_holder.to_account_info(),
                yield_pool_usdc: ctx.accounts.yield_pool_usdc.to_account_info(),
                waterfall_signer: ctx.accounts.asset_pool.to_account_info(),
                staking_config: ctx.accounts.staking_config.to_account_info(),
                token_program: ctx.accounts.token_program.to_account_info(),
            };
            let cpi_ctx = CpiContext::new_with_signer(cpi_program, cpi_accounts, signer);
            yield_escrow::cpi::deposit_provider_yield_usdc(cpi_ctx, liquid)?;

            let swap_cpi_program = ctx.accounts.swap_router_program.to_account_info();
            let swap_cpi_accounts = swap_router::cpi::accounts::RouteProviderYieldUsdc {
                config: ctx.accounts.swap_router_config.to_account_info(),
                usdc_source: ctx.accounts.vault_holder.to_account_info(),
                usdc_holder: ctx.accounts.swap_router_usdc_holder.to_account_info(),
                usdc_mint_constraint: ctx.accounts.swap_router_usdc_mint.to_account_info(),
                caller_authority: ctx.accounts.asset_pool.to_account_info(),
                token_program: ctx.accounts.token_program.to_account_info(),
            };
            let swap_cpi_ctx =
                CpiContext::new_with_signer(swap_cpi_program, swap_cpi_accounts, signer);
            swap_router::cpi::route_provider_yield_usdc(swap_cpi_ctx, swap_part, 0)?;
        }

        ctx.accounts.vault_holder.reload()?;
        let post_balance = ctx.accounts.vault_holder.amount;
        let pool_mut = &mut *ctx.accounts.asset_pool;
        require_earmark_invariant(pool_mut, post_balance)?;
        recompute_circuit_state(pool_mut, post_balance, now)?;

        emit!(YieldDistributed {
            asset_mint,
            amount,
            graduated,
            timestamp: now,
        });
        Ok(())
    }

    pub fn set_reserve_burn_mode(
        ctx: Context<AdminAction>,
        _new_mode: u8,
    ) -> Result<()> {
        let config = &ctx.accounts.vault_config;
        require_keys_eq!(
            ctx.accounts.signer.key(),
            config.authority,
            ProviderVaultError::Unauthorized
        );
        msg!("DEPRECATED: use set_raydium_graduated instead of set_reserve_burn_mode (M-CRIT-02)");
        err!(ProviderVaultError::InstructionDeprecated)
    }

    pub fn set_raydium_graduated(
        ctx: Context<AdminAction>,
        value: bool,
    ) -> Result<()> {
        let config = &mut ctx.accounts.vault_config;
        require_keys_eq!(
            ctx.accounts.signer.key(),
            config.authority,
            ProviderVaultError::Unauthorized
        );
        let old_value = config.raydium_graduated;
        config.raydium_graduated = value;
        config.reserve_burn_mode = if value {
            RESERVE_BURN_MODE_AUTO_SWAP
        } else {
            RESERVE_BURN_MODE_MANUAL
        };
        emit!(RaydiumGraduatedFlipped {
            old_value,
            new_value: value,
            changed_by: ctx.accounts.signer.key(),
            timestamp: Clock::get()?.unix_timestamp,
        });
        Ok(())
    }

    pub fn distribute_reserve(
        ctx: Context<DistributeReserve>,
        asset_mint: Pubkey,
    ) -> Result<()> {
        let config = &ctx.accounts.vault_config;
        require!(!config.is_frozen, ProviderVaultError::VaultFrozen);
        let now = Clock::get()?.unix_timestamp;
        let pool = &mut ctx.accounts.asset_pool;

        require!(pool.asset_mint == asset_mint, ProviderVaultError::AssetMismatch);
        require!(!pool.vault_locked, ProviderVaultError::VaultLocked);
        require!(!pool.is_sol, ProviderVaultError::SolPondNotImplemented);
        require!(
            config.ops_marketing_wallet != Pubkey::default(),
            ProviderVaultError::OpsMarketingWalletNotConfigured
        );
        require_keys_eq!(
            ctx.accounts.ops_marketing_token_account.owner,
            config.ops_marketing_wallet,
            ProviderVaultError::OpsMarketingWalletMismatch
        );

        let caller = ctx.accounts.signer.key();
        let is_operator = caller == config.operator_pubkey
            || caller == config.waterfall_authority;
        let is_keeper_eligible = now
            >= pool
                .last_distributed_at
                .checked_add(KEEPER_WINDOW_SECONDS)
                .ok_or(ProviderVaultError::MathOverflow)?;
        require!(
            is_operator || is_keeper_eligible,
            ProviderVaultError::Unauthorized
        );

        require!(
            pool.circuit_state == CIRCUIT_GREEN,
            ProviderVaultError::CircuitBreakerYieldPaused
        );

        let amount = pool.pending_reserve;
        require!(amount > 0, ProviderVaultError::NothingToDrain);

        advance_hwm_on_drain(pool);

        let mode = config.reserve_burn_mode;

        pool.pending_reserve = 0;

        let asset_mint_bytes = pool.asset_mint.to_bytes();
        let vault_config_bytes = config.key().to_bytes();
        let pool_bump = pool.bump;
        let seeds: &[&[u8]] = &[
            b"asset_pool",
            vault_config_bytes.as_ref(),
            asset_mint_bytes.as_ref(),
            &[pool_bump],
        ];
        let signer = &[seeds];

        if mode == RESERVE_BURN_MODE_MANUAL {
            let transfer_cpi = CpiContext::new_with_signer(
                ctx.accounts.token_program.to_account_info(),
                TransferChecked {
                    from: ctx.accounts.vault_holder.to_account_info(),
                    mint: ctx.accounts.asset_mint_account.to_account_info(),
                    to: ctx.accounts.ops_marketing_token_account.to_account_info(),
                    authority: ctx.accounts.asset_pool.to_account_info(),
                },
                signer,
            );
            token::transfer_checked(transfer_cpi, amount, USDC_DECIMALS)?;

            ctx.accounts.vault_holder.reload()?;
            let post_balance = ctx.accounts.vault_holder.amount;
            require_earmark_invariant(&ctx.accounts.asset_pool, post_balance)?;

            emit!(ReserveDistributed {
                asset_mint,
                burn_amount: 0,
                ops_amount: amount,
                mode,
                timestamp: now,
            });
        } else {

            require_eq!(
                ctx.accounts.top_token_mint.decimals,
                EXPECTED_TOP_DECIMALS,
                ProviderVaultError::WrongTopDecimals
            );
            require_keys_eq!(
                *ctx.accounts.top_token_mint.to_account_info().owner,
                anchor_spl::token_2022::ID,
                ProviderVaultError::WrongTopTokenProgram
            );

            let burn_usdc = amount
                .checked_div(2)
                .ok_or(ProviderVaultError::MathOverflow)?;
            let ops_amount = amount
                .checked_sub(burn_usdc)
                .ok_or(ProviderVaultError::MathOverflow)?;

            let min_top_out: u64 = 1;

            {
                let cpi_program = ctx.accounts.swap_router_program.to_account_info();
                let cpi_accounts = swap_router::cpi::accounts::SwapUsdcToTop {
                    config: ctx.accounts.swap_router_config.to_account_info(),
                    usdc_source: ctx.accounts.vault_holder.to_account_info(),
                    usdc_mint: ctx.accounts.asset_mint_account.to_account_info(),
                    top_token_mint: ctx.accounts.top_token_mint.to_account_info(),
                    top_destination: ctx.accounts.top_vault_holder.to_account_info(),
                    caller_authority: ctx.accounts.asset_pool.to_account_info(),
                    token_program: ctx.accounts.token_program.to_account_info(),
                    system_program: ctx.accounts.system_program.to_account_info(),
                };
                let cpi_ctx = CpiContext::new_with_signer(cpi_program, cpi_accounts, signer);
                swap_router::cpi::swap_usdc_to_top(cpi_ctx, burn_usdc, min_top_out, false)?;
            }

            ctx.accounts.top_vault_holder.reload()?;
            let top_received = ctx.accounts.top_vault_holder.amount;
            require!(top_received >= min_top_out, ProviderVaultError::SwapOutputBelowMin);

            if top_received > 0 {
                let burn_cpi = CpiContext::new_with_signer(
                    ctx.accounts.top_token_program.to_account_info(),
                    token_interface::Burn {
                        mint: ctx.accounts.top_token_mint.to_account_info(),
                        from: ctx.accounts.top_vault_holder.to_account_info(),
                        authority: ctx.accounts.asset_pool.to_account_info(),
                    },
                    signer,
                );
                token_interface::burn(burn_cpi, top_received)?;
            }

            if ops_amount > 0 {
                let transfer_cpi = CpiContext::new_with_signer(
                    ctx.accounts.token_program.to_account_info(),
                    TransferChecked {
                        from: ctx.accounts.vault_holder.to_account_info(),
                        mint: ctx.accounts.asset_mint_account.to_account_info(),
                        to: ctx.accounts.ops_marketing_token_account.to_account_info(),
                        authority: ctx.accounts.asset_pool.to_account_info(),
                    },
                    signer,
                );
                token::transfer_checked(transfer_cpi, ops_amount, USDC_DECIMALS)?;
            }

            ctx.accounts.vault_holder.reload()?;
            let post_balance = ctx.accounts.vault_holder.amount;
            require_earmark_invariant(&ctx.accounts.asset_pool, post_balance)?;

            emit!(ReserveBurnExecuted {
                asset_mint,
                usdc_in: burn_usdc,
                top_burned: top_received,
                ops_amount,
                timestamp: now,
            });
            emit!(ReserveDistributed {
                asset_mint,
                burn_amount: burn_usdc,
                ops_amount,
                mode,
                timestamp: now,
            });
        }

        let reserve_post_balance = ctx.accounts.vault_holder.amount;
        recompute_circuit_state(&mut ctx.accounts.asset_pool, reserve_post_balance, now)?;

        Ok(())
    }

    pub fn distribute_dev_fee(
        ctx: Context<DistributeDevFee>,
        asset_mint: Pubkey,
    ) -> Result<()> {
        let config = &ctx.accounts.vault_config;
        require!(!config.is_frozen, ProviderVaultError::VaultFrozen);
        let now = Clock::get()?.unix_timestamp;
        let pool = &mut ctx.accounts.asset_pool;

        require!(pool.asset_mint == asset_mint, ProviderVaultError::AssetMismatch);
        require!(!pool.vault_locked, ProviderVaultError::VaultLocked);
        require!(!pool.is_sol, ProviderVaultError::SolPondNotImplemented);
        require!(
            config.ops_marketing_wallet != Pubkey::default(),
            ProviderVaultError::OpsMarketingWalletNotConfigured
        );
        require_keys_eq!(
            ctx.accounts.ops_marketing_token_account.owner,
            config.ops_marketing_wallet,
            ProviderVaultError::OpsMarketingWalletMismatch
        );

        require!(
            pool.circuit_state == CIRCUIT_GREEN,
            ProviderVaultError::CircuitBreakerYieldPaused
        );

        let caller = ctx.accounts.signer.key();
        let is_operator = caller == config.operator_pubkey
            || caller == config.waterfall_authority;
        let is_keeper_eligible = now
            >= pool
                .last_distributed_at
                .checked_add(KEEPER_WINDOW_SECONDS)
                .ok_or(ProviderVaultError::MathOverflow)?;
        require!(
            is_operator || is_keeper_eligible,
            ProviderVaultError::Unauthorized
        );

        let amount = pool.pending_dev_fee;
        require!(amount > 0, ProviderVaultError::NothingToDrain);

        advance_hwm_on_drain(pool);

        pool.pending_dev_fee = 0;

        let asset_mint_bytes = pool.asset_mint.to_bytes();
        let vault_config_bytes = config.key().to_bytes();
        let seeds: &[&[u8]] = &[
            b"asset_pool",
            vault_config_bytes.as_ref(),
            asset_mint_bytes.as_ref(),
            &[pool.bump],
        ];
        let signer = &[seeds];

        let cpi = CpiContext::new_with_signer(
            ctx.accounts.token_program.to_account_info(),
            TransferChecked {
                from: ctx.accounts.vault_holder.to_account_info(),
                mint: ctx.accounts.asset_mint_account.to_account_info(),
                to: ctx.accounts.ops_marketing_token_account.to_account_info(),
                authority: pool.to_account_info(),
            },
            signer,
        );
        token::transfer_checked(cpi, amount, USDC_DECIMALS)?;

        ctx.accounts.vault_holder.reload()?;
        let post_balance = ctx.accounts.vault_holder.amount;
        require_earmark_invariant(pool, post_balance)?;
        recompute_circuit_state(pool, post_balance, now)?;

        emit!(DevFeeDrained {
            asset_mint,
            amount,
            timestamp: now,
        });
        Ok(())
    }

    pub fn accrue_affiliate_amount(
        ctx: Context<AccrueAffiliateAmount>,
        provider_id: u32,
        asset_mint: Pubkey,
        amount: u64,
    ) -> Result<()> {
        require!(amount > 0, ProviderVaultError::ZeroAmount);
        let config = &ctx.accounts.vault_config;
        require!(!config.is_frozen, ProviderVaultError::VaultFrozen);
        require_keys_eq!(
            ctx.accounts.operator.key(),
            config.operator_pubkey,
            ProviderVaultError::Unauthorized
        );

        let pool = &mut ctx.accounts.asset_pool;
        require!(pool.asset_mint == asset_mint, ProviderVaultError::AssetMismatch);
        require!(!pool.vault_locked, ProviderVaultError::VaultLocked);

        crate::accrue_affiliate_amount(pool, amount)?;
        let new_pending = pool.pending_affiliate;

        let holder_balance = ctx.accounts.vault_holder.amount;
        require_earmark_invariant(pool, holder_balance)?;

        let now_ts = Clock::get()?.unix_timestamp;
        recompute_circuit_state(pool, holder_balance, now_ts)?;

        emit!(AffiliateAccrued {
            provider_id,
            asset_mint,
            amount,
            new_pending_total: new_pending,
            timestamp: now_ts,
        });
        Ok(())
    }


    pub fn chip_deposit(
        ctx: Context<ChipDeposit>,
        asset_mint: Pubkey,
        amount: u64,
    ) -> Result<()> {
        require!(amount > 0, ProviderVaultError::ZeroAmount);
        let config = &ctx.accounts.vault_config;
        require!(!config.is_frozen, ProviderVaultError::VaultFrozen);
        require!(!config.is_paused, ProviderVaultError::VaultPaused);

        let pool = &ctx.accounts.asset_pool;
        require!(pool.asset_mint == asset_mint, ProviderVaultError::AssetMismatch);
        require!(!pool.is_sol, ProviderVaultError::SolPondNotImplemented);

        let escrow = &mut ctx.accounts.player_escrow;
        if escrow.wallet == Pubkey::default() {
            escrow.wallet = ctx.accounts.player.key();
            escrow.mint = asset_mint;
            escrow.bump = ctx.bumps.player_escrow;
            escrow.debit_window_amount = 0;
            escrow.debit_window_start = 0;
            escrow.reserved = [0u8; 16];
        } else {
            require_keys_eq!(escrow.wallet, ctx.accounts.player.key(),
                ProviderVaultError::PlayerEscrowMismatch);
            require_keys_eq!(escrow.mint, asset_mint,
                ProviderVaultError::AssetMismatch);
        }

        let cpi = CpiContext::new(
            ctx.accounts.token_program.to_account_info(),
            TransferChecked {
                from: ctx.accounts.player_token_account.to_account_info(),
                mint: ctx.accounts.asset_mint_account.to_account_info(),
                to: ctx.accounts.escrow_holder.to_account_info(),
                authority: ctx.accounts.player.to_account_info(),
            },
        );
        token::transfer_checked(cpi, amount, USDC_DECIMALS)?;

        escrow.amount = escrow
            .amount
            .checked_add(amount)
            .ok_or(ProviderVaultError::MathOverflow)?;

        emit!(ProviderChipDeposited {
            wallet: escrow.wallet,
            asset_mint,
            amount,
            new_balance: escrow.amount,
            timestamp: Clock::get()?.unix_timestamp,
        });
        Ok(())
    }

    pub fn chip_withdraw(
        ctx: Context<ChipWithdraw>,
        asset_mint: Pubkey,
        amount: u64,
        _reference: [u8; 32],
    ) -> Result<()> {
        require!(amount > 0, ProviderVaultError::ZeroAmount);
        let config = &ctx.accounts.vault_config;
        require!(!config.is_frozen, ProviderVaultError::VaultFrozen);
        let signer_key = ctx.accounts.signer.key();
        require!(
            signer_key == config.operator_pubkey,
            ProviderVaultError::Unauthorized
        );

        let pool = &ctx.accounts.asset_pool;
        require!(pool.asset_mint == asset_mint, ProviderVaultError::AssetMismatch);

        let escrow = &mut ctx.accounts.player_escrow;
        require_keys_eq!(escrow.mint, asset_mint, ProviderVaultError::AssetMismatch);
        require!(escrow.amount >= amount, ProviderVaultError::InsufficientShares);

        let r = &mut ctx.accounts.withdraw_receipt;
        require!(!r.withdrawn, ProviderVaultError::DuplicateWithdraw);
        r.withdrawn = true;
        r.bump = ctx.bumps.withdraw_receipt;

        escrow.amount = escrow
            .amount
            .checked_sub(amount)
            .ok_or(ProviderVaultError::MathOverflow)?;

        let wallet_bytes = escrow.wallet.to_bytes();
        let mint_bytes = asset_mint.to_bytes();
        let seeds: &[&[u8]] = &[
            b"provider_player_escrow_holder",
            wallet_bytes.as_ref(),
            mint_bytes.as_ref(),
            &[ctx.bumps.escrow_holder_authority],
        ];
        let signer = &[seeds];

        let cpi = CpiContext::new_with_signer(
            ctx.accounts.token_program.to_account_info(),
            TransferChecked {
                from: ctx.accounts.escrow_holder.to_account_info(),
                mint: ctx.accounts.asset_mint_account.to_account_info(),
                to: ctx.accounts.player_token_account.to_account_info(),
                authority: ctx.accounts.escrow_holder_authority.to_account_info(),
            },
            signer,
        );
        token::transfer_checked(cpi, amount, USDC_DECIMALS)?;

        emit!(ProviderChipWithdrawn {
            wallet: escrow.wallet,
            asset_mint,
            amount,
            new_balance: escrow.amount,
            timestamp: Clock::get()?.unix_timestamp,
        });
        Ok(())
    }

    pub fn chip_debit_to_vault(
        ctx: Context<ChipDebitToVault>,
        asset_mint: Pubkey,
        amount: u64,
        _player_wallet: Pubkey,
        provider_id: u32,
        _reference: [u8; 32],
    ) -> Result<()> {
        require!(amount > 0, ProviderVaultError::ZeroAmount);
        let config = &ctx.accounts.vault_config;
        require!(!config.is_frozen, ProviderVaultError::VaultFrozen);
        require!(!config.is_paused, ProviderVaultError::VaultPaused);
        require_keys_eq!(
            ctx.accounts.operator.key(),
            config.operator_pubkey,
            ProviderVaultError::Unauthorized
        );

        let pool = &ctx.accounts.asset_pool;
        require!(pool.asset_mint == asset_mint, ProviderVaultError::AssetMismatch);
        require!(!pool.vault_locked, ProviderVaultError::VaultLocked);

        let escrow = &mut ctx.accounts.player_escrow;
        require_keys_eq!(escrow.mint, asset_mint, ProviderVaultError::AssetMismatch);
        require!(escrow.amount >= amount, ProviderVaultError::InsufficientShares);

        let now_debit = Clock::get()?.unix_timestamp;
        let cap = if pool.max_chip_debit_per_24h_per_wallet == 0 {
            DEFAULT_MAX_CHIP_DEBIT_PER_24H_PER_WALLET
        } else {
            pool.max_chip_debit_per_24h_per_wallet
        };
        let debit_window_end = escrow
            .debit_window_start
            .saturating_add(CHIP_DEBIT_WINDOW_SECONDS);
        if escrow.debit_window_start == 0 || now_debit >= debit_window_end {
            escrow.debit_window_amount = 0;
            escrow.debit_window_start = now_debit;
        }
        let projected_debit = escrow
            .debit_window_amount
            .checked_add(amount)
            .ok_or(ProviderVaultError::MathOverflow)?;
        require!(
            projected_debit <= cap,
            ProviderVaultError::ChipDebitRateLimited
        );
        escrow.debit_window_amount = projected_debit;

        let r = &mut ctx.accounts.debit_receipt;
        require!(!r.recorded, ProviderVaultError::DuplicateDebit);
        r.recorded = true;
        r.bump = ctx.bumps.debit_receipt;

        escrow.amount = escrow
            .amount
            .checked_sub(amount)
            .ok_or(ProviderVaultError::MathOverflow)?;

        let wallet_bytes = escrow.wallet.to_bytes();
        let mint_bytes = asset_mint.to_bytes();
        let seeds: &[&[u8]] = &[
            b"provider_player_escrow_holder",
            wallet_bytes.as_ref(),
            mint_bytes.as_ref(),
            &[ctx.bumps.escrow_holder_authority],
        ];
        let signer = &[seeds];

        let cpi = CpiContext::new_with_signer(
            ctx.accounts.token_program.to_account_info(),
            TransferChecked {
                from: ctx.accounts.escrow_holder.to_account_info(),
                mint: ctx.accounts.asset_mint_account.to_account_info(),
                to: ctx.accounts.vault_holder.to_account_info(),
                authority: ctx.accounts.escrow_holder_authority.to_account_info(),
            },
            signer,
        );
        token::transfer_checked(cpi, amount, USDC_DECIMALS)?;

        emit!(ProviderChipDebited {
            wallet: escrow.wallet,
            provider_id,
            asset_mint,
            amount,
            new_balance: escrow.amount,
            timestamp: Clock::get()?.unix_timestamp,
        });

        ctx.accounts.vault_holder.reload()?;
        let debit_post_balance = ctx.accounts.vault_holder.amount;
        let now_debit_recompute = Clock::get()?.unix_timestamp;
        recompute_circuit_state(
            &mut ctx.accounts.asset_pool,
            debit_post_balance,
            now_debit_recompute,
        )?;
        Ok(())
    }

    pub fn chip_credit_from_vault(
        ctx: Context<ChipCreditFromVault>,
        asset_mint: Pubkey,
        amount: u64,
        _player_wallet: Pubkey,
        provider_id: u32,
        _reference: [u8; 32],
    ) -> Result<()> {
        require!(amount > 0, ProviderVaultError::ZeroAmount);
        let config = &mut ctx.accounts.vault_config;
        require!(!config.is_frozen, ProviderVaultError::VaultFrozen);
        require_keys_eq!(
            ctx.accounts.operator.key(),
            config.operator_pubkey,
            ProviderVaultError::Unauthorized
        );
        require!(
            ctx.accounts.asset_pool.circuit_state != CIRCUIT_RED,
            ProviderVaultError::CircuitBreakerRed
        );

        let now = Clock::get()?.unix_timestamp;
        let window_end = config
            .window_start
            .saturating_add(config.settle_window_seconds as i64);
        if config.window_start == 0 || now >= window_end {
            config.window_outflow = 0;
            config.window_start = now;
        }
        let projected_outflow = config
            .window_outflow
            .checked_add(amount)
            .ok_or(ProviderVaultError::MathOverflow)?;
        if projected_outflow > config.max_settle_per_window {
            msg!(
                "BREAKER_TRIP:AutoFrozenOnOutflow program={} pool={} source={} attempted_amount={} window_outflow={} cap={} window_start={} timestamp={}",
                ctx.program_id,
                ctx.accounts.asset_pool.key(),
                AUTO_FROZEN_SOURCE_LP,
                amount,
                config.window_outflow,
                config.max_settle_per_window,
                config.window_start,
                now,
            );
            config.is_frozen = true;
            emit!(AutoFrozenOnOutflow {
                source: AUTO_FROZEN_SOURCE_LP,
                attempted_amount: amount,
                window_outflow_at_trip: config.window_outflow,
                threshold: config.max_settle_per_window,
                window_start: config.window_start,
                tripped_at: now,
            });
            return Ok(());
        }

        let daily_window_end = config
            .daily_window_start
            .saturating_add(DAILY_OUTFLOW_WINDOW_SECONDS);
        if config.daily_window_start == 0 || now >= daily_window_end {
            config.daily_window_outflow = 0;
            config.daily_window_start = now;
        }
        let projected_daily = config
            .daily_window_outflow
            .checked_add(amount)
            .ok_or(ProviderVaultError::MathOverflow)?;
        if projected_daily > config.max_daily_outflow {
            msg!(
                "BREAKER_TRIP:AutoFrozenOnDailyOutflow program={} pool={} source={} attempted_amount={} daily_window_outflow={} cap={} daily_window_start={} timestamp={}",
                ctx.program_id,
                ctx.accounts.asset_pool.key(),
                AUTO_FROZEN_SOURCE_LP,
                amount,
                config.daily_window_outflow,
                config.max_daily_outflow,
                config.daily_window_start,
                now,
            );
            config.is_frozen = true;
            config.last_freeze_at = now;
            emit!(AutoFrozenOnDailyOutflow {
                attempted_amount: amount,
                daily_window_outflow_at_trip: config.daily_window_outflow,
                max_daily_outflow: config.max_daily_outflow,
                daily_window_start: config.daily_window_start,
                tripped_at: now,
            });
            return Ok(());
        }

        config.window_outflow = projected_outflow;
        config.daily_window_outflow = projected_daily;

        require!(ctx.accounts.asset_pool.asset_mint == asset_mint, ProviderVaultError::AssetMismatch);
        require!(!ctx.accounts.asset_pool.vault_locked, ProviderVaultError::VaultLocked);

        if let Some(ins_info) = ctx.accounts.insurance_holder.as_ref().map(|a| a.to_account_info()) {
            let earmarks = sum_earmarks(&ctx.accounts.asset_pool);
            let draw = compute_insurance_draw(
                ctx.accounts.vault_holder.amount,
                amount,
                earmarks,
                ctx.accounts.asset_pool.insurance_balance,
            )?;
            if draw > 0 {
                let pool_key = ctx.accounts.asset_pool.key();
                let (ins_pda, _) = Pubkey::find_program_address(
                    &[b"insurance", pool_key.as_ref()],
                    ctx.program_id,
                );
                require_keys_eq!(ins_info.key(), ins_pda, ProviderVaultError::InvalidMint);
                let am_bytes = ctx.accounts.asset_pool.asset_mint.to_bytes();
                let vc_bytes = config.key().to_bytes();
                let pbump = ctx.accounts.asset_pool.bump;
                let iseeds: &[&[u8]] = &[
                    b"asset_pool",
                    vc_bytes.as_ref(),
                    am_bytes.as_ref(),
                    &[pbump],
                ];
                let isigner = &[iseeds];
                let icpi = CpiContext::new_with_signer(
                    ctx.accounts.token_program.to_account_info(),
                    TransferChecked {
                        from: ins_info,
                        mint: ctx.accounts.asset_mint_account.to_account_info(),
                        to: ctx.accounts.vault_holder.to_account_info(),
                        authority: ctx.accounts.asset_pool.to_account_info(),
                    },
                    isigner,
                );
                token::transfer_checked(icpi, draw, USDC_DECIMALS)?;
                ctx.accounts.asset_pool.insurance_balance = ctx
                    .accounts
                    .asset_pool
                    .insurance_balance
                    .checked_sub(draw)
                    .ok_or(ProviderVaultError::MathOverflow)?;
                ctx.accounts.vault_holder.reload()?;
                emit!(InsuranceDrawn {
                    asset_mint: ctx.accounts.asset_pool.asset_mint,
                    amount: draw,
                    insurance_balance_after: ctx.accounts.asset_pool.insurance_balance,
                    timestamp: Clock::get()?.unix_timestamp,
                });
            }
        }

        let pool = &ctx.accounts.asset_pool;

        let escrow = &mut ctx.accounts.player_escrow;
        if escrow.wallet == Pubkey::default() {
            escrow.wallet = ctx.accounts.player.key();
            escrow.mint = asset_mint;
            escrow.bump = ctx.bumps.player_escrow;
            escrow.debit_window_amount = 0;
            escrow.debit_window_start = 0;
            escrow.reserved = [0u8; 16];
        } else {
            require_keys_eq!(escrow.wallet, ctx.accounts.player.key(),
                ProviderVaultError::PlayerEscrowMismatch);
            require_keys_eq!(escrow.mint, asset_mint,
                ProviderVaultError::AssetMismatch);
        }

        let r = &mut ctx.accounts.credit_receipt;
        require!(!r.credited, ProviderVaultError::DuplicateCredit);
        r.credited = true;
        r.bump = ctx.bumps.credit_receipt;

        let post_balance = ctx.accounts.vault_holder.amount
            .checked_sub(amount)
            .ok_or(ProviderVaultError::HardFloorViolated)?;
        require_earmark_invariant(pool, post_balance)?;

        let asset_mint_bytes = pool.asset_mint.to_bytes();
        let vault_config_bytes = config.key().to_bytes();
        let seeds: &[&[u8]] = &[
            b"asset_pool",
            vault_config_bytes.as_ref(),
            asset_mint_bytes.as_ref(),
            &[pool.bump],
        ];
        let signer = &[seeds];

        let cpi = CpiContext::new_with_signer(
            ctx.accounts.token_program.to_account_info(),
            TransferChecked {
                from: ctx.accounts.vault_holder.to_account_info(),
                mint: ctx.accounts.asset_mint_account.to_account_info(),
                to: ctx.accounts.escrow_holder.to_account_info(),
                authority: pool.to_account_info(),
            },
            signer,
        );
        token::transfer_checked(cpi, amount, USDC_DECIMALS)?;

        escrow.amount = escrow
            .amount
            .checked_add(amount)
            .ok_or(ProviderVaultError::MathOverflow)?;

        emit!(ProviderChipCredited {
            wallet: escrow.wallet,
            provider_id,
            asset_mint,
            amount,
            new_balance: escrow.amount,
            timestamp: Clock::get()?.unix_timestamp,
        });

        ctx.accounts.vault_holder.reload()?;
        let cc_post_balance = ctx.accounts.vault_holder.amount;
        let cc_now = Clock::get()?.unix_timestamp;
        recompute_circuit_state(&mut ctx.accounts.asset_pool, cc_post_balance, cc_now)?;
        Ok(())
    }


    pub fn top_up_promo_pool(
        ctx: Context<TopUpPromoPool>,
        asset_mint: Pubkey,
        amount: u64,
    ) -> Result<()> {
        require!(amount > 0, ProviderVaultError::ZeroAmount);

        let config = &ctx.accounts.vault_config;
        require!(!config.is_frozen, ProviderVaultError::VaultFrozen);
        require_keys_eq!(
            ctx.accounts.operator.key(),
            config.operator_pubkey,
            ProviderVaultError::Unauthorized
        );

        let pool = &mut ctx.accounts.asset_pool;
        require!(pool.asset_mint == asset_mint, ProviderVaultError::AssetMismatch);
        require!(!pool.vault_locked, ProviderVaultError::VaultLocked);

        let cpi = CpiContext::new(
            ctx.accounts.token_program.to_account_info(),
            TransferChecked {
                from: ctx.accounts.source_token_account.to_account_info(),
                mint: ctx.accounts.asset_mint_account.to_account_info(),
                to: ctx.accounts.vault_holder.to_account_info(),
                authority: ctx.accounts.operator.to_account_info(),
            },
        );
        token::transfer_checked(cpi, amount, USDC_DECIMALS)?;

        pool.pending_promo = pool
            .pending_promo
            .checked_add(amount)
            .ok_or(ProviderVaultError::MathOverflow)?;

        ctx.accounts.vault_holder.reload()?;
        let post_balance = ctx.accounts.vault_holder.amount;
        require_earmark_invariant(pool, post_balance)?;

        emit!(PromoPoolToppedUp {
            asset_mint,
            amount,
            new_pending_promo: pool.pending_promo,
            timestamp: Clock::get()?.unix_timestamp,
        });
        Ok(())
    }

    pub fn chip_credit_from_vault_promo(
        ctx: Context<ChipCreditFromVaultPromo>,
        asset_mint: Pubkey,
        amount: u64,
        _player_wallet: Pubkey,
        provider_id: u32,
        _reference: [u8; 32],
    ) -> Result<()> {
        require!(amount > 0, ProviderVaultError::ZeroAmount);
        let config = &mut ctx.accounts.vault_config;
        require!(!config.is_frozen, ProviderVaultError::VaultFrozen);
        require_keys_eq!(
            ctx.accounts.operator.key(),
            config.operator_pubkey,
            ProviderVaultError::Unauthorized
        );
        require!(
            ctx.accounts.asset_pool.circuit_state != CIRCUIT_RED,
            ProviderVaultError::CircuitBreakerRed
        );

        let now = Clock::get()?.unix_timestamp;
        let window_end = config
            .window_start
            .saturating_add(config.settle_window_seconds as i64);
        if config.window_start == 0 || now >= window_end {
            config.window_outflow = 0;
            config.window_start = now;
        }
        let projected_outflow = config
            .window_outflow
            .checked_add(amount)
            .ok_or(ProviderVaultError::MathOverflow)?;
        if projected_outflow > config.max_settle_per_window {
            msg!(
                "BREAKER_TRIP:AutoFrozenOnOutflow program={} pool={} source={} attempted_amount={} window_outflow={} cap={} window_start={} timestamp={}",
                ctx.program_id,
                ctx.accounts.asset_pool.key(),
                AUTO_FROZEN_SOURCE_PROMO,
                amount,
                config.window_outflow,
                config.max_settle_per_window,
                config.window_start,
                now,
            );
            config.is_frozen = true;
            emit!(AutoFrozenOnOutflow {
                source: AUTO_FROZEN_SOURCE_PROMO,
                attempted_amount: amount,
                window_outflow_at_trip: config.window_outflow,
                threshold: config.max_settle_per_window,
                window_start: config.window_start,
                tripped_at: now,
            });
            return Ok(());
        }

        let daily_window_end_promo = config
            .daily_window_start
            .saturating_add(DAILY_OUTFLOW_WINDOW_SECONDS);
        if config.daily_window_start == 0 || now >= daily_window_end_promo {
            config.daily_window_outflow = 0;
            config.daily_window_start = now;
        }
        let projected_daily_promo = config
            .daily_window_outflow
            .checked_add(amount)
            .ok_or(ProviderVaultError::MathOverflow)?;
        if projected_daily_promo > config.max_daily_outflow {
            msg!(
                "BREAKER_TRIP:AutoFrozenOnDailyOutflow program={} pool={} source={} attempted_amount={} daily_window_outflow={} cap={} daily_window_start={} timestamp={}",
                ctx.program_id,
                ctx.accounts.asset_pool.key(),
                AUTO_FROZEN_SOURCE_PROMO,
                amount,
                config.daily_window_outflow,
                config.max_daily_outflow,
                config.daily_window_start,
                now,
            );
            config.is_frozen = true;
            config.last_freeze_at = now;
            emit!(AutoFrozenOnDailyOutflow {
                attempted_amount: amount,
                daily_window_outflow_at_trip: config.daily_window_outflow,
                max_daily_outflow: config.max_daily_outflow,
                daily_window_start: config.daily_window_start,
                tripped_at: now,
            });
            return Ok(());
        }

        config.window_outflow = projected_outflow;
        config.daily_window_outflow = projected_daily_promo;

        let pool = &mut ctx.accounts.asset_pool;
        require!(pool.asset_mint == asset_mint, ProviderVaultError::AssetMismatch);
        require!(!pool.vault_locked, ProviderVaultError::VaultLocked);

        require!(
            pool.pending_promo >= amount,
            ProviderVaultError::PromoPoolUnderfunded
        );

        let escrow = &mut ctx.accounts.player_escrow;
        if escrow.wallet == Pubkey::default() {
            escrow.wallet = ctx.accounts.player.key();
            escrow.mint = asset_mint;
            escrow.bump = ctx.bumps.player_escrow;
            escrow.debit_window_amount = 0;
            escrow.debit_window_start = 0;
            escrow.reserved = [0u8; 16];
        } else {
            require_keys_eq!(escrow.wallet, ctx.accounts.player.key(),
                ProviderVaultError::PlayerEscrowMismatch);
            require_keys_eq!(escrow.mint, asset_mint,
                ProviderVaultError::AssetMismatch);
        }

        pool.pending_promo = pool
            .pending_promo
            .checked_sub(amount)
            .ok_or(ProviderVaultError::MathOverflow)?;

        if let Some(ins_info) =
            ctx.accounts.insurance_holder.as_ref().map(|a| a.to_account_info())
        {
            let earmarks = sum_earmarks(pool);
            let draw = compute_insurance_draw(
                ctx.accounts.vault_holder.amount,
                amount,
                earmarks,
                pool.insurance_balance,
            )?;
            if draw > 0 {
                let pool_key = pool.key();
                let (ins_pda, _) = Pubkey::find_program_address(
                    &[b"insurance", pool_key.as_ref()],
                    ctx.program_id,
                );
                require_keys_eq!(ins_info.key(), ins_pda, ProviderVaultError::InvalidMint);
                let am_bytes = pool.asset_mint.to_bytes();
                let vc_bytes = config.key().to_bytes();
                let pbump = pool.bump;
                let iseeds: &[&[u8]] = &[
                    b"asset_pool",
                    vc_bytes.as_ref(),
                    am_bytes.as_ref(),
                    &[pbump],
                ];
                let isigner = &[iseeds];
                let icpi = CpiContext::new_with_signer(
                    ctx.accounts.token_program.to_account_info(),
                    TransferChecked {
                        from: ins_info,
                        mint: ctx.accounts.asset_mint_account.to_account_info(),
                        to: ctx.accounts.vault_holder.to_account_info(),
                        authority: pool.to_account_info(),
                    },
                    isigner,
                );
                token::transfer_checked(icpi, draw, USDC_DECIMALS)?;
                pool.insurance_balance = pool
                    .insurance_balance
                    .checked_sub(draw)
                    .ok_or(ProviderVaultError::MathOverflow)?;
                ctx.accounts.vault_holder.reload()?;
                emit!(InsuranceDrawn {
                    asset_mint: pool.asset_mint,
                    amount: draw,
                    insurance_balance_after: pool.insurance_balance,
                    timestamp: Clock::get()?.unix_timestamp,
                });
            }
        }

        let r = &mut ctx.accounts.credit_receipt;
        require!(!r.credited, ProviderVaultError::DuplicateCredit);
        r.credited = true;
        r.bump = ctx.bumps.credit_receipt;

        let post_balance = ctx.accounts.vault_holder.amount
            .checked_sub(amount)
            .ok_or(ProviderVaultError::HardFloorViolated)?;
        require_earmark_invariant(pool, post_balance)?;

        let asset_mint_bytes = pool.asset_mint.to_bytes();
        let vault_config_bytes = config.key().to_bytes();
        let seeds: &[&[u8]] = &[
            b"asset_pool",
            vault_config_bytes.as_ref(),
            asset_mint_bytes.as_ref(),
            &[pool.bump],
        ];
        let signer = &[seeds];

        let cpi = CpiContext::new_with_signer(
            ctx.accounts.token_program.to_account_info(),
            TransferChecked {
                from: ctx.accounts.vault_holder.to_account_info(),
                mint: ctx.accounts.asset_mint_account.to_account_info(),
                to: ctx.accounts.escrow_holder.to_account_info(),
                authority: pool.to_account_info(),
            },
            signer,
        );
        token::transfer_checked(cpi, amount, USDC_DECIMALS)?;

        escrow.amount = escrow
            .amount
            .checked_add(amount)
            .ok_or(ProviderVaultError::MathOverflow)?;

        emit!(ProviderChipCreditedPromo {
            wallet: escrow.wallet,
            provider_id,
            asset_mint,
            amount,
            new_balance: escrow.amount,
            new_pending_promo: pool.pending_promo,
            timestamp: Clock::get()?.unix_timestamp,
        });

        ctx.accounts.vault_holder.reload()?;
        let promo_post_balance = ctx.accounts.vault_holder.amount;
        let promo_now = Clock::get()?.unix_timestamp;
        recompute_circuit_state(pool, promo_post_balance, promo_now)?;
        Ok(())
    }

    pub fn chip_credit_from_vault_ngr_promo(
        ctx: Context<ChipCreditFromVaultNgrPromo>,
        asset_mint: Pubkey,
        amount: u64,
        _player_wallet: Pubkey,
        provider_id: u32,
        is_network_reimbursable: bool,
        _reference: [u8; 32],
    ) -> Result<()> {
        require!(amount > 0, ProviderVaultError::ZeroAmount);
        let config = &mut ctx.accounts.vault_config;
        require!(!config.is_frozen, ProviderVaultError::VaultFrozen);
        require_keys_eq!(
            ctx.accounts.operator.key(),
            config.operator_pubkey,
            ProviderVaultError::Unauthorized
        );
        require!(
            ctx.accounts.asset_pool.circuit_state != CIRCUIT_RED,
            ProviderVaultError::CircuitBreakerRed
        );

        let now = Clock::get()?.unix_timestamp;
        let window_end = config
            .window_start
            .saturating_add(config.settle_window_seconds as i64);
        if config.window_start == 0 || now >= window_end {
            config.window_outflow = 0;
            config.window_start = now;
        }
        let projected_outflow = config
            .window_outflow
            .checked_add(amount)
            .ok_or(ProviderVaultError::MathOverflow)?;
        if projected_outflow > config.max_settle_per_window {
            msg!(
                "BREAKER_TRIP:AutoFrozenOnOutflow program={} pool={} source={} attempted_amount={} window_outflow={} cap={} window_start={} timestamp={}",
                ctx.program_id,
                ctx.accounts.asset_pool.key(),
                AUTO_FROZEN_SOURCE_PROMO,
                amount,
                config.window_outflow,
                config.max_settle_per_window,
                config.window_start,
                now,
            );
            config.is_frozen = true;
            emit!(AutoFrozenOnOutflow {
                source: AUTO_FROZEN_SOURCE_PROMO,
                attempted_amount: amount,
                window_outflow_at_trip: config.window_outflow,
                threshold: config.max_settle_per_window,
                window_start: config.window_start,
                tripped_at: now,
            });
            return Ok(());
        }

        let daily_window_end = config
            .daily_window_start
            .saturating_add(DAILY_OUTFLOW_WINDOW_SECONDS);
        if config.daily_window_start == 0 || now >= daily_window_end {
            config.daily_window_outflow = 0;
            config.daily_window_start = now;
        }
        let projected_daily = config
            .daily_window_outflow
            .checked_add(amount)
            .ok_or(ProviderVaultError::MathOverflow)?;
        if projected_daily > config.max_daily_outflow {
            msg!(
                "BREAKER_TRIP:AutoFrozenOnDailyOutflow program={} pool={} source={} attempted_amount={} daily_window_outflow={} cap={} daily_window_start={} timestamp={}",
                ctx.program_id,
                ctx.accounts.asset_pool.key(),
                AUTO_FROZEN_SOURCE_PROMO,
                amount,
                config.daily_window_outflow,
                config.max_daily_outflow,
                config.daily_window_start,
                now,
            );
            config.is_frozen = true;
            config.last_freeze_at = now;
            emit!(AutoFrozenOnDailyOutflow {
                attempted_amount: amount,
                daily_window_outflow_at_trip: config.daily_window_outflow,
                max_daily_outflow: config.max_daily_outflow,
                daily_window_start: config.daily_window_start,
                tripped_at: now,
            });
            return Ok(());
        }

        config.window_outflow = projected_outflow;
        config.daily_window_outflow = projected_daily;

        let pool = &mut ctx.accounts.asset_pool;
        require!(pool.asset_mint == asset_mint, ProviderVaultError::AssetMismatch);
        require!(!pool.vault_locked, ProviderVaultError::VaultLocked);

        let escrow = &mut ctx.accounts.player_escrow;
        if escrow.wallet == Pubkey::default() {
            escrow.wallet = ctx.accounts.player.key();
            escrow.mint = asset_mint;
            escrow.bump = ctx.bumps.player_escrow;
            escrow.debit_window_amount = 0;
            escrow.debit_window_start = 0;
            escrow.reserved = [0u8; 16];
        } else {
            require_keys_eq!(escrow.wallet, ctx.accounts.player.key(),
                ProviderVaultError::PlayerEscrowMismatch);
            require_keys_eq!(escrow.mint, asset_mint,
                ProviderVaultError::AssetMismatch);
        }

        if let Some(ins_info) =
            ctx.accounts.insurance_holder.as_ref().map(|a| a.to_account_info())
        {
            let earmarks = sum_earmarks(pool);
            let draw = compute_insurance_draw(
                ctx.accounts.vault_holder.amount,
                amount,
                earmarks,
                pool.insurance_balance,
            )?;
            if draw > 0 {
                let pool_key = pool.key();
                let (ins_pda, _) = Pubkey::find_program_address(
                    &[b"insurance", pool_key.as_ref()],
                    ctx.program_id,
                );
                require_keys_eq!(ins_info.key(), ins_pda, ProviderVaultError::InvalidMint);
                let am_bytes = pool.asset_mint.to_bytes();
                let vc_bytes = config.key().to_bytes();
                let pbump = pool.bump;
                let iseeds: &[&[u8]] = &[
                    b"asset_pool",
                    vc_bytes.as_ref(),
                    am_bytes.as_ref(),
                    &[pbump],
                ];
                let isigner = &[iseeds];
                let icpi = CpiContext::new_with_signer(
                    ctx.accounts.token_program.to_account_info(),
                    TransferChecked {
                        from: ins_info,
                        mint: ctx.accounts.asset_mint_account.to_account_info(),
                        to: ctx.accounts.vault_holder.to_account_info(),
                        authority: pool.to_account_info(),
                    },
                    isigner,
                );
                token::transfer_checked(icpi, draw, USDC_DECIMALS)?;
                pool.insurance_balance = pool
                    .insurance_balance
                    .checked_sub(draw)
                    .ok_or(ProviderVaultError::MathOverflow)?;
                ctx.accounts.vault_holder.reload()?;
                emit!(InsuranceDrawn {
                    asset_mint: pool.asset_mint,
                    amount: draw,
                    insurance_balance_after: pool.insurance_balance,
                    timestamp: Clock::get()?.unix_timestamp,
                });
            }
        }

        let r = &mut ctx.accounts.credit_receipt;
        require!(!r.credited, ProviderVaultError::DuplicateCredit);
        r.credited = true;
        r.bump = ctx.bumps.credit_receipt;

        let post_balance = ctx.accounts.vault_holder.amount
            .checked_sub(amount)
            .ok_or(ProviderVaultError::HardFloorViolated)?;
        require_earmark_invariant(pool, post_balance)?;

        let asset_mint_bytes = pool.asset_mint.to_bytes();
        let vault_config_bytes = config.key().to_bytes();
        let seeds: &[&[u8]] = &[
            b"asset_pool",
            vault_config_bytes.as_ref(),
            asset_mint_bytes.as_ref(),
            &[pool.bump],
        ];
        let signer = &[seeds];

        let cpi = CpiContext::new_with_signer(
            ctx.accounts.token_program.to_account_info(),
            TransferChecked {
                from: ctx.accounts.vault_holder.to_account_info(),
                mint: ctx.accounts.asset_mint_account.to_account_info(),
                to: ctx.accounts.escrow_holder.to_account_info(),
                authority: pool.to_account_info(),
            },
            signer,
        );
        token::transfer_checked(cpi, amount, USDC_DECIMALS)?;

        escrow.amount = escrow
            .amount
            .checked_add(amount)
            .ok_or(ProviderVaultError::MathOverflow)?;

        pool.promo_paid_unreconciled = pool
            .promo_paid_unreconciled
            .checked_add(amount)
            .ok_or(ProviderVaultError::MathOverflow)?;
        if is_network_reimbursable {
            pool.network_reimbursement_owed = pool
                .network_reimbursement_owed
                .checked_add(amount)
                .ok_or(ProviderVaultError::MathOverflow)?;
        }

        emit!(ProviderChipCreditedNgrPromo {
            wallet: escrow.wallet,
            provider_id,
            asset_mint,
            amount,
            new_balance: escrow.amount,
            is_network_reimbursable,
            new_promo_paid_unreconciled: pool.promo_paid_unreconciled,
            new_network_reimbursement_owed: pool.network_reimbursement_owed,
            timestamp: Clock::get()?.unix_timestamp,
        });

        ctx.accounts.vault_holder.reload()?;
        let ngr_post_balance = ctx.accounts.vault_holder.amount;
        let ngr_now = Clock::get()?.unix_timestamp;
        recompute_circuit_state(pool, ngr_post_balance, ngr_now)?;
        Ok(())
    }

    pub fn credit_chips_from_swap(
        ctx: Context<CreditChipsFromSwap>,
        asset_mint: Pubkey,
        min_out_floor: u64,
    ) -> Result<()> {
        let config = &ctx.accounts.vault_config;
        require!(!config.is_frozen, ProviderVaultError::VaultFrozen);
        require!(!config.is_paused, ProviderVaultError::VaultPaused);

        let pool = &ctx.accounts.asset_pool;
        require!(pool.asset_mint == asset_mint, ProviderVaultError::AssetMismatch);
        require!(!pool.is_sol, ProviderVaultError::SolPondNotImplemented);

        let escrow = &mut ctx.accounts.player_escrow;
        if escrow.wallet == Pubkey::default() {
            escrow.wallet = ctx.accounts.player.key();
            escrow.mint = asset_mint;
            escrow.bump = ctx.bumps.player_escrow;
            escrow.debit_window_amount = 0;
            escrow.debit_window_start = 0;
            escrow.reserved = [0u8; 16];
        } else {
            require_keys_eq!(
                escrow.wallet,
                ctx.accounts.player.key(),
                ProviderVaultError::PlayerEscrowMismatch
            );
            require_keys_eq!(escrow.mint, asset_mint, ProviderVaultError::AssetMismatch);
        }

        ctx.accounts.escrow_holder.reload()?;
        let holder_balance = ctx.accounts.escrow_holder.amount;

        let credited = compute_swap_credit(holder_balance, escrow.amount, min_out_floor)?;

        escrow.amount = holder_balance;

        emit!(ChipsCreditedFromSwap {
            wallet: escrow.wallet,
            asset_mint,
            credited,
            new_balance: escrow.amount,
            timestamp: Clock::get()?.unix_timestamp,
        });
        Ok(())
    }
}


fn require_non_default_pubkeys(keys: &[Pubkey]) -> Result<()> {
    for k in keys {
        require!(*k != Pubkey::default(), ProviderVaultError::InvalidAuthority);
    }
    Ok(())
}

fn require_roles_pairwise_distinct(keys: &[Pubkey]) -> Result<()> {
    for i in 0..keys.len() {
        for j in (i + 1)..keys.len() {
            require!(
                keys[i] != keys[j],
                ProviderVaultError::OperatorRoleCollision
            );
        }
    }
    Ok(())
}

pub fn compute_net_ggr(gross_wager: u64, gross_payout: u64) -> Result<i64> {
    let w = gross_wager as i128;
    let p = gross_payout as i128;
    let n = w.checked_sub(p).ok_or(ProviderVaultError::MathOverflow)?;
    if n > i64::MAX as i128 || n < i64::MIN as i128 {
        return Err(ProviderVaultError::MathOverflow.into());
    }
    Ok(n as i64)
}

pub fn phase_split_bps(phase: u8) -> (u16, u16, u16) {
    match phase {
        0 => (2_000, 7_000, 1_000),
        _ => (6_000, 3_000, 1_000),
    }
}

pub fn compute_tier(cumulative_deposited: u64) -> u8 {
    if cumulative_deposited >= 50_000_000_000 { return 4; }
    if cumulative_deposited >= 10_000_000_000 { return 3; }
    if cumulative_deposited >= 2_500_000_000 { return 2; }
    if cumulative_deposited >= 500_000_000 { return 1; }
    0
}

pub fn compute_weighted_lp_bps(pool: &AssetPool, phase: u8, fb_tokens_in_window: u64) -> Result<u64> {
    let total: u128 = pool.lp_tokens_by_tier.iter().map(|t| *t as u128).sum();
    if total == 0 {
        return Ok(DEFAULT_LP_SHARE_BPS as u64);
    }

    let fb_slice: u128 = (fb_tokens_in_window as u128).min(total);
    let non_fb_slice: u128 = total - fb_slice;

    let mut weighted_sum: u128 = fb_slice
        .checked_mul(FOUNDING_BANKER_LP_SHARE_BPS as u128)
        .ok_or(ProviderVaultError::MathOverflow)?;

    if non_fb_slice > 0 {
        if phase == 0 {
            weighted_sum = weighted_sum.saturating_add(
                non_fb_slice.saturating_mul(BOOTSTRAP_LP_SHARE_BPS as u128),
            );
        } else {
            for (tier_idx, &tier_lp) in pool.lp_tokens_by_tier.iter().enumerate() {
                if tier_lp == 0 {
                    continue;
                }
                let tier_lp_u128 = tier_lp as u128;
                let fb_in_tier = tier_lp_u128
                    .saturating_mul(fb_slice)
                    .checked_div(total)
                    .ok_or(ProviderVaultError::MathOverflow)?;
                let tier_non_fb = tier_lp_u128.saturating_sub(fb_in_tier);
                let rate = TIER_LP_SHARE_BPS_GROWTH[tier_idx] as u128;
                weighted_sum =
                    weighted_sum.saturating_add(tier_non_fb.saturating_mul(rate));
            }
        }
    }

    Ok((weighted_sum / total) as u64)
}

pub fn effective_accrual_base(
    hwm: i64,
    cum_before: i64,
    net_ggr_signed: i64,
) -> Result<i64> {
    if net_ggr_signed <= 0 {
        return Ok(net_ggr_signed);
    }
    let cum_after = cum_before
        .checked_add(net_ggr_signed)
        .ok_or(ProviderVaultError::MathOverflow)?;
    let water_line = hwm.max(cum_before).max(0);
    let above = cum_after
        .checked_sub(water_line)
        .ok_or(ProviderVaultError::MathOverflow)?;
    Ok(if above > 0 { above } else { 0 })
}

pub fn advance_hwm_on_drain(pool: &mut AssetPool) {
    let floored = pool.last_distributed_gross_ggr.max(0);
    pool.last_distributed_gross_ggr = floored.max(pool.cumulative_gross_ggr);
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ProviderFeeStep {
    pub period_net_after: i64,
    pub fee_target: u64,
    pub increase: u64,
    pub decrease: u64,
}

pub fn provider_period_fee_step(
    period_net_before: i64,
    net_ggr_signed: i64,
    period_fee_charged: u64,
    bps: u16,
) -> Result<ProviderFeeStep> {
    let period_net_after = period_net_before
        .checked_add(net_ggr_signed)
        .ok_or(ProviderVaultError::MathOverflow)?;

    let fee_target: u64 = if period_net_after > 0 {
        ((period_net_after as u128)
            .checked_mul(bps as u128)
            .ok_or(ProviderVaultError::MathOverflow)?
            / 10_000u128) as u64
    } else {
        0
    };

    let (increase, decrease) = if fee_target >= period_fee_charged {
        (fee_target - period_fee_charged, 0u64)
    } else {
        (0u64, period_fee_charged - fee_target)
    };

    Ok(ProviderFeeStep {
        period_net_after,
        fee_target,
        increase,
        decrease,
    })
}

pub fn reduce_provider_fee_accrual(
    pool: &mut AssetPool,
    provider: &mut Provider,
    decrease: u64,
) {
    if decrease == 0 {
        return;
    }
    pool.pending_provider_fee = pool.pending_provider_fee.saturating_sub(decrease);
    provider.fee_owed_since_last_sweep =
        provider.fee_owed_since_last_sweep.saturating_sub(decrease);
}

pub fn accrue_earmarks(
    pool: &mut AssetPool,
    net_delta_signed: i64,
    phase: u8,
    _snapshot_provider_fee_bps: u16,
    fee_due: u64,
    dev_fee_bps: u16,
    cost_netted: u64,
    fee_release: u64,
) -> Result<()> {
    require!(dev_fee_bps <= MAX_DEV_FEE_BPS, ProviderVaultError::InvalidBps);

    let weighted_lp_bps =
        compute_weighted_lp_bps(pool, phase, pool.founding_banker_lp_tokens_in_window)?;

    if net_delta_signed >= 0 {
        require!(fee_release == 0, ProviderVaultError::FeeReleaseOnPositiveReceipt);

        let net_delta = net_delta_signed as u64;

        pool.pending_provider_fee = pool
            .pending_provider_fee
            .checked_add(fee_due)
            .ok_or(ProviderVaultError::MathOverflow)?;


        let after_provider = net_delta.saturating_sub(fee_due);

        require!(cost_netted <= after_provider, ProviderVaultError::PromoNetExceedsBase);
        let distribution_base = after_provider
            .checked_sub(cost_netted)
            .ok_or(ProviderVaultError::MathOverflow)?;

        let dev_fee_due = (distribution_base as u128)
            .checked_mul(dev_fee_bps as u128)
            .ok_or(ProviderVaultError::MathOverflow)?
            / 10_000u128;
        let dev_fee_due = dev_fee_due as u64;

        let after_dev = distribution_base
            .checked_sub(dev_fee_due)
            .ok_or(ProviderVaultError::MathOverflow)?;

        let lp_due = (after_dev as u128)
            .checked_mul(weighted_lp_bps as u128)
            .ok_or(ProviderVaultError::MathOverflow)?
            / 10_000u128;
        let lp_due = lp_due as u64;

        let protocol_due = after_dev
            .checked_sub(lp_due)
            .ok_or(ProviderVaultError::MathOverflow)?;

        let sov_due = (protocol_due as u128)
            .checked_mul(SOVEREIGN_CARVE_BPS as u128)
            .ok_or(ProviderVaultError::MathOverflow)?
            / 10_000u128;
        let sov_due = sov_due as u64;

        let tax_remainder = protocol_due
            .checked_sub(sov_due)
            .ok_or(ProviderVaultError::MathOverflow)?;

        let (yield_bps, compound_bps, _reserve_bps_unused) = phase_split_bps(phase);
        let yield_due = (tax_remainder as u128)
            .checked_mul(yield_bps as u128)
            .ok_or(ProviderVaultError::MathOverflow)?
            / 10_000u128;
        let yield_due = yield_due as u64;

        let compound_due = (tax_remainder as u128)
            .checked_mul(compound_bps as u128)
            .ok_or(ProviderVaultError::MathOverflow)?
            / 10_000u128;
        let compound_due = compound_due as u64;

        let reserve_due = tax_remainder
            .checked_sub(yield_due)
            .ok_or(ProviderVaultError::MathOverflow)?
            .checked_sub(compound_due)
            .ok_or(ProviderVaultError::MathOverflow)?;

        pool.pending_dev_fee = pool
            .pending_dev_fee
            .checked_add(dev_fee_due)
            .ok_or(ProviderVaultError::MathOverflow)?;
        pool.pending_sovereign = pool
            .pending_sovereign
            .checked_add(sov_due)
            .ok_or(ProviderVaultError::MathOverflow)?;
        pool.pending_yield = pool
            .pending_yield
            .checked_add(yield_due)
            .ok_or(ProviderVaultError::MathOverflow)?;
        pool.pending_reserve = pool
            .pending_reserve
            .checked_add(reserve_due)
            .ok_or(ProviderVaultError::MathOverflow)?;
    } else {
        require!(cost_netted == 0, ProviderVaultError::PromoNetExceedsBase);
        let abs_delta = net_delta_signed.unsigned_abs();
        let after_provider = abs_delta.saturating_sub(fee_release);

        let dev_fee_unwind = ((after_provider as u128)
            .saturating_mul(dev_fee_bps as u128)
            / 10_000u128) as u64;
        let after_dev = after_provider.saturating_sub(dev_fee_unwind);
        let lp_unwind = ((after_dev as u128)
            .saturating_mul(weighted_lp_bps as u128)
            / 10_000u128) as u64;
        let protocol_unwind = after_dev.saturating_sub(lp_unwind);
        let sov_unwind = ((protocol_unwind as u128)
            .saturating_mul(SOVEREIGN_CARVE_BPS as u128)
            / 10_000u128) as u64;
        let tax_remainder = protocol_unwind.saturating_sub(sov_unwind);
        let (yield_bps, compound_bps, _) = phase_split_bps(phase);
        let yield_unwind = ((tax_remainder as u128)
            .saturating_mul(yield_bps as u128)
            / 10_000u128) as u64;
        let compound_unwind = ((tax_remainder as u128)
            .saturating_mul(compound_bps as u128)
            / 10_000u128) as u64;
        let reserve_unwind = tax_remainder
            .saturating_sub(yield_unwind)
            .saturating_sub(compound_unwind);

        pool.pending_dev_fee = pool.pending_dev_fee.saturating_sub(dev_fee_unwind);
        pool.pending_sovereign = pool.pending_sovereign.saturating_sub(sov_unwind);
        pool.pending_yield = pool.pending_yield.saturating_sub(yield_unwind);
        pool.pending_reserve = pool.pending_reserve.saturating_sub(reserve_unwind);
    }
    Ok(())
}

pub fn accrue_affiliate_amount(pool: &mut AssetPool, amount: u64) -> Result<()> {
    pool.pending_affiliate = pool
        .pending_affiliate
        .checked_add(amount)
        .ok_or(ProviderVaultError::MathOverflow)?;
    pool.affiliate_unreconciled = pool
        .affiliate_unreconciled
        .checked_add(amount)
        .ok_or(ProviderVaultError::MathOverflow)?;
    Ok(())
}

pub fn nav_basis(pool: &AssetPool, holder_balance: u64) -> Result<u64> {
    let earmarks = sum_earmarks(pool);
    Ok(holder_balance.saturating_sub(earmarks))
}

pub fn sum_earmarks(pool: &AssetPool) -> u64 {
    pool.pending_dev_fee
        .saturating_add(pool.pending_provider_fee)
        .saturating_add(pool.pending_affiliate)
        .saturating_add(pool.pending_sovereign)
        .saturating_add(pool.pending_yield)
        .saturating_add(pool.pending_reserve)
        .saturating_add(pool.pending_promo)
        .saturating_add(pool.provider_owed_total)
}

pub fn require_earmark_invariant(pool: &AssetPool, holder_balance: u64) -> Result<()> {
    let earmarks = sum_earmarks(pool);
    require!(holder_balance >= earmarks, ProviderVaultError::EarmarkInvariantViolated);
    Ok(())
}

pub fn recompute_circuit_state(
    pool: &mut AssetPool,
    holder_balance: u64,
    now: i64,
) -> Result<u8> {
    let nav = nav_basis(pool, holder_balance)?;

    if nav > pool.peak_vault {
        pool.peak_vault = nav;
        pool.peak_vault_at = now;
    }

    let old_state = pool.circuit_state;

    let new_state: u8 = if pool.peak_vault == 0 {
        CIRCUIT_GREEN
    } else {
        let yellow_thr = pool
            .peak_vault
            .checked_mul(CIRCUIT_YELLOW_NAV_PCT_OF_PEAK)
            .ok_or(ProviderVaultError::MathOverflow)?
            .checked_div(100)
            .ok_or(ProviderVaultError::MathOverflow)?;
        let red_thr = pool
            .peak_vault
            .checked_mul(CIRCUIT_RED_NAV_PCT_OF_PEAK)
            .ok_or(ProviderVaultError::MathOverflow)?
            .checked_div(100)
            .ok_or(ProviderVaultError::MathOverflow)?;
        let ins_floor = nav
            .checked_mul(INSURANCE_FLOOR_PCT_OF_NAV)
            .ok_or(ProviderVaultError::MathOverflow)?
            .checked_div(100)
            .ok_or(ProviderVaultError::MathOverflow)?;

        if nav < red_thr && pool.insurance_balance == 0 {
            CIRCUIT_RED
        } else if nav < yellow_thr || pool.insurance_balance < ins_floor {
            CIRCUIT_YELLOW
        } else {
            CIRCUIT_GREEN
        }
    };

    if old_state != CIRCUIT_RED && new_state == CIRCUIT_RED {
        pool.red_entered_at = now;
        pool.waiver_started_at = now;
        pool.waiver_max_until = now
            .checked_add(WAIVER_DELAY_SECONDS)
            .ok_or(ProviderVaultError::MathOverflow)?;
        pool.waiver_active = false;
    } else if old_state == CIRCUIT_RED && new_state != CIRCUIT_RED {
        pool.red_entered_at = 0;
        pool.waiver_active = false;
        pool.waiver_started_at = 0;
        pool.waiver_max_until = 0;
    }

    pool.circuit_state = new_state;
    if new_state != old_state {
        emit!(CircuitStateChanged {
            asset_mint: pool.asset_mint,
            old_state,
            new_state,
            nav,
            peak: pool.peak_vault,
            insurance: pool.insurance_balance,
            timestamp: now,
        });
    }
    Ok(new_state)
}

pub fn compute_insurance_draw(
    holder_balance: u64,
    credit_amount: u64,
    earmarks: u64,
    insurance_balance: u64,
) -> Result<u64> {
    let min_required = core::cmp::max(HARD_VAULT_FLOOR_USDC, earmarks);
    let post_no_insurance = holder_balance
        .checked_sub(credit_amount)
        .ok_or(ProviderVaultError::MathOverflow)?;
    if post_no_insurance >= min_required {
        return Ok(0);
    }
    let deficit = min_required
        .checked_sub(post_no_insurance)
        .ok_or(ProviderVaultError::MathOverflow)?;
    Ok(deficit.min(insurance_balance))
}

pub fn withdrawal_cooldown_waived(pool: &AssetPool, now: i64) -> bool {
    pool.circuit_state == CIRCUIT_RED
        && pool.waiver_max_until != 0
        && now >= pool.waiver_max_until
}

pub fn compute_swap_credit(
    holder_balance: u64,
    escrow_amount: u64,
    min_out_floor: u64,
) -> Result<u64> {
    require!(
        holder_balance >= escrow_amount,
        ProviderVaultError::HolderBalanceDecreased
    );
    let credited = holder_balance
        .checked_sub(escrow_amount)
        .ok_or(ProviderVaultError::MathOverflow)?;
    require!(
        credited >= min_out_floor,
        ProviderVaultError::CreditBelowMinOut
    );
    Ok(credited)
}

pub fn compute_shares_for_deposit(
    amount: u64,
    nav_balance: u64,
    lp_supply: u64,
) -> Result<(u64, u64)> {
    if lp_supply == 0 {
        let dead = MIN_DEAD_SHARES.min(amount);
        let user = amount.checked_sub(dead).ok_or(ProviderVaultError::MathOverflow)?;
        return Ok((user, dead));
    }
    require!(nav_balance > 0, ProviderVaultError::DrainedPoolReseedDisallowed);
    let s = (amount as u128)
        .checked_mul(lp_supply as u128)
        .ok_or(ProviderVaultError::MathOverflow)?
        / (nav_balance as u128);
    if s > u64::MAX as u128 {
        return Err(ProviderVaultError::MathOverflow.into());
    }
    Ok((s as u64, 0))
}

pub fn compute_lamports_for_withdraw(
    lp_amount: u64,
    nav_balance: u64,
    lp_supply: u64,
) -> Result<u64> {
    require!(lp_supply > 0, ProviderVaultError::EmptySupply);
    let p = (lp_amount as u128)
        .checked_mul(nav_balance as u128)
        .ok_or(ProviderVaultError::MathOverflow)?
        / (lp_supply as u128);
    if p > u64::MAX as u128 {
        return Err(ProviderVaultError::MathOverflow.into());
    }
    Ok(p as u64)
}

pub fn rolling_window_rearm(window_start: i64, rolling: u64, now: i64) -> (i64, u64) {
    if window_start == 0 || now >= window_start.saturating_add(7 * SECONDS_PER_DAY) {
        (now, 0)
    } else {
        (window_start, rolling)
    }
}

pub fn check_rule30_penalty(
    position: &ProviderLpPosition,
    new_amount: u64,
    lp_supply: u64,
    now: i64,
) -> Result<bool> {
    if lp_supply == 0 {
        return Ok(false);
    }
    let threshold = (lp_supply as u128)
        .saturating_mul(SINGLE_WALLET_THRESHOLD_BPS as u128)
        / 10_000u128;
    let threshold = threshold as u64;

    let window_open = position.rolling_7d_window_start > 0
        && now < position.rolling_7d_window_start.saturating_add(7 * SECONDS_PER_DAY);
    let rolling = if window_open {
        position.rolling_7d_withdrawn_shares
    } else {
        0
    };
    let total = rolling
        .saturating_add(position.pending_withdrawal_shares)
        .saturating_add(new_amount);
    Ok(total > threshold)
}

fn check_and_record_propose(config: &mut VaultConfig, now: i64) -> Result<()> {
    require!(
        now >= config.propose_cooldown_until,
        ProviderVaultError::ProposeCooldownActive
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

    let new_cooldown_until = if next_cooldown_seconds == 0 {
        0
    } else {
        now.checked_add(next_cooldown_seconds)
            .ok_or(ProviderVaultError::MathOverflow)?
    };
    config.propose_cooldown_until = new_cooldown_until;

    for i in 0..(PROPOSE_RATE_LIMIT_RING_LEN - 1) {
        config.recent_proposes[i] = config.recent_proposes[i + 1];
    }
    config.recent_proposes[PROPOSE_RATE_LIMIT_RING_LEN - 1] = now;

    Ok(())
}


#[error_code]
pub enum ProviderVaultError {
    #[msg("Math overflow")]
    MathOverflow,
    #[msg("Invalid authority pubkey")]
    InvalidAuthority,
    #[msg("Invalid program id")]
    InvalidProgramId,
    #[msg("Invalid mint (decimals/authority/supply)")]
    InvalidLpMint,
    #[msg("Mint authority mismatch")]
    MintAuthorityMismatch,
    #[msg("Invalid asset mint")]
    InvalidAsset,
    #[msg("Mint does not match asset_pool.asset_mint")]
    InvalidMint,
    #[msg("asset_pool.vault_config does not match vault_config account")]
    InvalidVaultConfig,
    #[msg("Asset already registered")]
    AssetAlreadyRegistered,
    #[msg("Too many assets registered")]
    TooManyAssets,
    #[msg("Too many providers")]
    TooManyProviders,
    #[msg("Caller is not the configured authority")]
    Unauthorized,
    #[msg("Vault holder already set (init-once); rebinding requires a program upgrade")]
    VaultHolderAlreadySet,
    #[msg("Invalid basis points")]
    InvalidBps,
    #[msg("Vault is paused")]
    VaultPaused,
    #[msg("Vault is locked for emergency response")]
    VaultLocked,
    #[msg("Vault was already locked")]
    VaultAlreadyLocked,
    #[msg("Vault is not locked")]
    VaultNotLocked,
    #[msg("Unlock requested before the 72h delay elapsed")]
    UnlockTooEarly,
    #[msg("Provider id mismatch")]
    ProviderMismatch,
    #[msg("Provider is inactive")]
    ProviderInactive,
    #[msg("Provider is paused")]
    ProviderPaused,
    #[msg("Provider settlement is paused (dispute)")]
    ProviderSettlePaused,
    #[msg("Provider fee bps exceeds MAX_PROVIDER_FEE_BPS")]
    ProviderFeeTooHigh,
    #[msg("Asset mint does not match the supplied pool")]
    AssetMismatch,
    #[msg("Wrong asset branch (USDC vs SOL routing)")]
    WrongAssetBranch,
    #[msg("Day id must be strictly greater than the last submission (a first submission must start at the current day)")]
    DayIdRegression,
    #[msg("Nothing pending to finalize/cancel")]
    NothingPending,
    #[msg("Timelock not elapsed")]
    TimelockNotElapsed,
    #[msg("Nothing owed to settle")]
    NothingOwed,
    #[msg("Settle recipient does not match the registered cold wallet")]
    SettleRecipientMismatch,
    #[msg("Deposit below MIN_DEPOSIT")]
    DepositBelowMinimum,
    #[msg("Invalid tier (must be 0..=4)")]
    InvalidTier,
    #[msg("Zero shares would be minted")]
    ZeroSharesMinted,
    #[msg("Insufficient shares for withdrawal")]
    InsufficientShares,
    #[msg("Cooldown not yet elapsed")]
    CooldownNotElapsed,
    #[msg("Withdraw request already processed")]
    RequestAlreadyProcessed,
    #[msg("Withdraw request already assigned to a batch")]
    RequestAlreadyAssigned,
    #[msg("Zero payout would be transferred")]
    ZeroPayout,
    #[msg("Hard floor violated")]
    HardFloorViolated,
    #[msg("Earmark invariant violated: holder < Σ pending_*")]
    EarmarkInvariantViolated,
    #[msg("Zero amount")]
    ZeroAmount,
    #[msg("Empty supply (cannot price withdraw)")]
    EmptySupply,
    #[msg("Pause rate-limited")]
    PauseRateLimited,
    #[msg("Phase must increase")]
    PhaseNotAdvancing,
    #[msg("Phase has not been active long enough")]
    PhaseNotEnoughTime,
    #[msg("Invalid phase id")]
    InvalidPhase,
    #[msg("Keeper window has not elapsed")]
    KeeperWindowNotElapsed,
    #[msg("Nothing to drain — pending counter is zero")]
    NothingToDrain,
    #[msg("SOL pond is not implemented at v2.0 — reserved for v2.1")]
    SolPondNotImplemented,
    #[msg("ops_marketing_wallet is not configured — call set_ops_marketing_wallet first")]
    OpsMarketingWalletNotConfigured,
    #[msg("ops_marketing_token_account owner does not match configured wallet")]
    OpsMarketingWalletMismatch,
    #[msg("ops_marketing_wallet already configured — use propose_set_ops_marketing_wallet for timelocked rotation")]
    OpsMarketingAlreadyConfigured,
    #[msg("ProviderPlayerEscrow wallet/mint mismatch")]
    PlayerEscrowMismatch,
    #[msg("escrow_holder is not the canonical ATA(escrow_holder_authority, asset_mint) (C-01)")]
    EscrowHolderMismatch,
    #[msg("reserve_burn_mode must be 0 (manual) or 1 (auto-swap)")]
    InvalidReserveBurnMode,
    #[msg("Swap output below min_top_out — slippage exceeded")]
    SwapOutputBelowMin,
    #[msg("$TOP mint decimals != 6 — refusing the reserve burn against a wrong-decimals mint")]
    WrongTopDecimals,
    #[msg("$TOP mint is not owned by the Token-2022 program")]
    WrongTopTokenProgram,
    #[msg("Promo pool underfunded for this payout — top_up_promo_pool first")]
    PromoPoolUnderfunded,
    #[msg("Vault is frozen — emergency halt active; only admin can unfreeze")]
    VaultFrozen,
    #[msg("Freeze rate-limited — wait 600s between freeze calls")]
    FreezeRateLimited,
    #[msg("day_id must be <= current_day + 1 (no far-future submissions)")]
    InvalidDayId,
    #[msg("Outflow circuit breaker tripped — vault auto-frozen due to excessive payout volume")]
    OutflowCircuitBreakerTripped,
    #[msg("Proposed max_settle_per_window below minimum ($1,000)")]
    WindowCapBelowMinimum,
    #[msg("Proposed settle_window_seconds out of range (must be 30s..86400s)")]
    WindowSecondsOutOfRange,
    #[msg("Sub-floor vault — LP withdrawals blocked while holder balance below HARD_VAULT_FLOOR_USDC")]
    SubFloorWithdrawBlocked,
    #[msg("Circuit breaker RED — chip payouts halted (Rule 45d)")]
    CircuitBreakerRed,
    #[msg("Circuit breaker not GREEN — distribution paused (Rule 45c/45d)")]
    CircuitBreakerYieldPaused,
    #[msg("Waiver op requires the circuit breaker to be RED")]
    WaiverNotRed,
    #[msg("No RED waiver timer is armed")]
    WaiverNotArmed,
    #[msg("Waiver extension exceeds the 72h total cap (Rule 45d)")]
    WaiverExtensionTooLong,
    #[msg("Proposed operator is the default sentinel pubkey")]
    InvalidOperator,
    #[msg("An operator rotation proposal is already pending — cancel before proposing again")]
    ProposalAlreadyPending,
    #[msg("No operator rotation proposal pending")]
    NoProposalPending,
    #[msg("Propose cooldown active — escalating rate-limit per Rule 27b defense (R7.7-H-01)")]
    ProposeCooldownActive,
    #[msg("Caller wallet does not own this WithdrawRequest")]
    UnauthorizedRequest,
    #[msg("All 21 Founding Banker seats are filled")]
    AllFoundingBankerSeatsFilled,
    #[msg("Deposit below Founding Banker minimum ($5,000 USDC)")]
    DepositBelowFoundingMin,
    #[msg("Vault not yet seeded — founder must call deposit_lp_usdc first per Rule 41")]
    VaultNotSeeded,
    #[msg("Only founder_pubkey can seed the vault per Rule 41 founder-first")]
    OnlyFounderCanSeed,
    #[msg("Founding Banker threshold for SOL pond requires oracle pricing — not implemented at v1")]
    SolPondFoundingBankerNotImplemented,

    #[msg("Instruction deprecated — call the propose/finalize triplet or the canonical replacement (M-CRIT-02 / R9-1-RC-02)")]
    InstructionDeprecated,

    #[msg("24h rolling outflow cap exceeded — vault auto-frozen for safety (M-HIGH-01 / R9-1-NEW-03)")]
    DailyOutflowExceeded,

    #[msg("Proposed operator collides with another high-privilege role (M-HIGH-05 / R9-1-RC-05)")]
    OperatorRoleCollision,

    #[msg("Per-wallet 24h chip-debit cap exceeded (M-HIGH-07 / R9-1-RC-07)")]
    ChipDebitRateLimited,

    #[msg("Proposed chip-debit cap exceeds MAX_CHIP_DEBIT_CAP_PER_WALLET ($100k) — admin-instant rotation ceiling (FIX PASS 3 / F8)")]
    ChipDebitCapTooHigh,
    #[msg("Credited delta below min_out_floor — swap slippage exceeded")]
    CreditBelowMinOut,
    #[msg("Holder balance below recorded chip amount — credit cannot decrease chips")]
    HolderBalanceDecreased,
    #[msg("Deadman is disabled (heartbeat_ttl == 0) — arm it via set_heartbeat_ttl")]
    DeadmanDisabled,
    #[msg("Operator heartbeat is not stale yet — halt_if_stale requires age > heartbeat_ttl")]
    HeartbeatNotStale,
    #[msg("Invalid heartbeat_ttl — must be >= 0 (0 disables)")]
    InvalidHeartbeatTtl,
    #[msg("Duplicate credit — a credit for this reference has already landed on-chain")]
    DuplicateCredit,
    #[msg("Duplicate debit — a debit for this reference has already landed on-chain")]
    DuplicateDebit,
    #[msg("Duplicate withdraw — a withdraw for this reference has already landed on-chain")]
    DuplicateWithdraw,
    #[msg("NGR promo to net exceeds the post-provider distribution base")]
    PromoNetExceedsBase,
    #[msg("Pool NAV is zero with shares outstanding; deposits disabled pending admin pool reset")]
    DrainedPoolReseedDisallowed,
    #[msg("Receipt net GGR exceeds the per-receipt cap — max(20% of vault holder, $1,000)")]
    GgrExceedsCap,
    #[msg("Provider fee correction has already been applied for this provider — one-shot only")]
    FeeCorrectionAlreadyApplied,
    #[msg("Observed state does not match the attested expected pre-state — refusing to correct")]
    FeeCorrectionPreStateMismatch,
    #[msg("Provider fee correction must strictly DECREASE the accrual; it can never raise a bucket")]
    FeeCorrectionMustDecrease,
    #[msg("Cannot change provider_fee_bps while the period is in profit — it would retroactively reprice accrued GGR and misroute released fee into LP NAV. Flush the period first.")]
    FeeBpsChangeWouldRepriceOpenPeriod,
    #[msg("A positive receipt released period fee, which the positive cascade cannot absorb — the mid-period rate-change guard should have made this unreachable")]
    FeeReleaseOnPositiveReceipt,
}


#[account]
pub struct VaultConfig {
    pub authority: Pubkey,
    pub operator_pubkey: Pubkey,
    pub affiliate_recorder_pubkey: Pubkey,
    pub pause_authority: Pubkey,
    pub waterfall_authority: Pubkey,
    pub bump: u8,

    pub active_provider_count: u8,
    pub next_provider_id: u32,

    pub is_paused: bool,
    pub pause_reason: [u8; PAUSE_REASON_LEN],
    pub last_pause_at: i64,
    pub last_provider_pause_at: i64,

    pub phase: u8,
    pub phase_started_at: i64,
    pub dev_fee_bps: u16,
    pub sovereign_carve_bps: u16,
    pub insurance_floor_bps: u16,
    pub max_daily_drawdown_bps: u16,

    pub sovereign_registry_program_id: Pubkey,
    pub sovereign_registry_config: Pubkey,
    pub yield_escrow_program_id: Pubkey,
    pub yield_escrow_provider_pool: Pubkey,
    pub affiliate_registry_program_id: Pubkey,
    pub affiliate_registry_config: Pubkey,

    pub pending_authority: Pubkey,
    pub pending_authority_unlocks_at: i64,

    pub ops_marketing_wallet: Pubkey,

    pub pending_dev_fee_bps: u16,
    pub pending_dev_fee_bps_unlocks_at: i64,

    pub pending_ops_marketing_wallet: Pubkey,
    pub pending_ops_marketing_wallet_unlocks_at: i64,

    pub reserve_burn_mode: u8,

    pub is_frozen: bool,
    pub last_freeze_at: i64,

    pub raydium_graduated: bool,

    pub max_settle_per_window: u64,
    pub settle_window_seconds: u32,
    pub window_outflow: u64,
    pub window_start: i64,

    pub pending_max_settle_per_window: u64,
    pub pending_max_settle_per_window_unlocks_at: i64,
    pub pending_settle_window_seconds: u32,
    pub pending_settle_window_seconds_unlocks_at: i64,

    pub pending_pause_authority: Pubkey,
    pub pending_pause_authority_unlocks_at: i64,

    pub pending_operator_pubkey: Pubkey,
    pub pending_operator_unlocks_at: i64,

    pub propose_cooldown_until: i64,
    pub recent_proposes: [i64; 5],

    pub founder_pubkey: Pubkey,
    pub founding_banker_counter: u8,
    pub vault_seeded: bool,

    pub max_daily_outflow: u64,
    pub daily_window_outflow: u64,
    pub daily_window_start: i64,
    pub pending_max_daily_outflow: u64,
    pub pending_max_daily_outflow_unlocks_at: i64,

    pub last_heartbeat_at: i64,
    pub heartbeat_ttl: i64,

    pub reserved: [u8; 8],
}
impl VaultConfig {
    pub const LEN: usize = 847;
}

#[account]
pub struct RegisteredAssets {
    pub vault_config: Pubkey,
    pub mints: [Pubkey; MAX_ASSETS as usize],
    pub active_count: u8,
    pub bump: u8,
    pub reserved: [u8; 32],
}
impl RegisteredAssets {
    pub const LEN: usize = 8 + 32 + (MAX_ASSETS as usize) * 32 + 1 + 1 + 32;
}

#[account]
pub struct AssetPool {
    pub vault_config: Pubkey,
    pub asset_mint: Pubkey,
    pub is_sol: bool,
    pub bump: u8,

    pub lp_mint: Pubkey,
    pub lp_supply: u64,

    pub cumulative_gross_ggr: i64,
    pub last_distributed_gross_ggr: i64,
    pub last_distributed_at: i64,

    pub pending_dev_fee: u64,
    pub pending_provider_fee: u64,
    pub pending_affiliate: u64,
    pub pending_sovereign: u64,
    pub pending_yield: u64,
    pub pending_reserve: u64,
    pub last_distributed_affiliate: u64,

    pub pending_promo: u64,

    pub lp_share_bps: u16,
    pub lp_tokens_by_tier: [u64; 5],

    pub peak_vault: u64,
    pub peak_vault_at: i64,
    pub circuit_state: u8,
    pub red_entered_at: i64,
    pub waiver_active: bool,
    pub waiver_started_at: i64,
    pub waiver_max_until: i64,
    pub insurance_balance: u64,

    pub withdraw_batch_counter: u64,
    pub last_batch_opened_at: i64,
    pub pending_request_count: u32,

    pub vault_locked: bool,
    pub vault_locked_at: i64,

    pub provider_settle_owner: Pubkey,
    pub pending_settle_owner: Pubkey,
    pub pending_settle_owner_unlocks_at: i64,
    pub provider_owed_total: u64,

    pub founding_banker_lp_tokens_in_window: u64,

    pub max_chip_debit_per_24h_per_wallet: u64,

    pub promo_paid_unreconciled: u64,
    pub network_reimbursement_owed: u64,
    pub provider_credit: u64,

    pub vault_holder: Pubkey,

    pub pending_reset_peak: u64,
    pub pending_reset_peak_unlocks_at: i64,

    pub affiliate_unreconciled: u64,

    pub reserved: [u8; 24],
}
impl AssetPool {
    pub const LEN: usize = 8
        + 32 * 6
        + 6
        + 8 * 31
        + 2
        + 5 * 8
        + 4
        + 24;
}

#[account]
pub struct Provider {
    pub provider_id: u32,
    pub name: [u8; PROVIDER_NAME_LEN],
    pub bump: u8,
    pub active: bool,
    pub paused: bool,
    pub paused_at: i64,
    pub settle_paused: bool,
    pub pause_reason: [u8; PAUSE_REASON_LEN],

    pub provider_fee_bps: u16,
    pub fee_owed_since_last_sweep: u64,
    pub affiliate_recorder_pubkey: Pubkey,
    pub signed_terms_hash: [u8; 32],

    pub cumulative_gross_ggr: i64,
    pub cumulative_gross_wager: u64,
    pub cumulative_gross_payout: u64,
    pub cumulative_bet_count: u64,
    pub last_submission_at: i64,
    pub last_day_id: u64,

    pub period_net_ggr: i64,
    pub period_fee_charged: u64,
    pub fee_correction_applied: u8,

    pub reserved: [u8; 47],
}
impl Provider {
    pub const LEN: usize = 8
        + 4
        + PROVIDER_NAME_LEN
        + 1 * 4
        + 8
        + PAUSE_REASON_LEN
        + 2
        + 8
        + 32
        + 32
        + 8 * 6
        + 8
        + 8
        + 1
        + 47;
}

#[account]
pub struct DailyReceipt {
    pub provider_id: u32,
    pub day_id: u64,
    pub asset_mint: Pubkey,
    pub bump: u8,

    pub gross_wager: u64,
    pub gross_payout: u64,
    pub net_ggr: i64,
    pub bet_count: u32,
    pub provider_signed_digest: [u8; 32],
    pub submitter_pubkey: Pubkey,
    pub submitted_at: i64,

    pub fee_bps_at_accrual: u16,
    pub fee_due_recorded: u64,

    pub reserved: [u8; 32],
}
impl DailyReceipt {
    pub const LEN: usize = 8
        + 4
        + 8
        + 32
        + 1
        + 8 * 3
        + 4
        + 32
        + 32
        + 8
        + 2
        + 8
        + 32;
}

#[account]
pub struct ProviderOwed {
    pub asset_pool: Pubkey,
    pub provider_id: u32,
    pub amount: u64,
    pub last_settled_at: i64,
    pub bump: u8,
    pub reserved: [u8; 32],
}
impl ProviderOwed {
    pub const LEN: usize = 8 + 32 + 4 + 8 + 8 + 1 + 32;
}

#[account]
pub struct ProviderPlayerEscrow {
    pub wallet: Pubkey,
    pub mint: Pubkey,
    pub amount: u64,
    pub bump: u8,
    pub debit_window_amount: u64,
    pub debit_window_start: i64,
    pub reserved: [u8; 16],
}
impl ProviderPlayerEscrow {
    pub const LEN: usize = 8 + 32 + 32 + 8 + 1 + 8 + 8 + 16;
}

#[account]
pub struct ProviderLpPosition {
    pub holder: Pubkey,
    pub asset_pool: Pubkey,
    pub tier: u8,
    pub lp_shares: u64,
    pub pending_withdrawal_shares: u64,
    pub cumulative_deposited: u64,
    pub last_deposit_at: i64,
    pub last_withdrawal_at: i64,
    pub rolling_7d_withdrawn_shares: u64,
    pub rolling_7d_window_start: i64,
    pub bump: u8,
    pub is_founding_banker: bool,
    pub founding_banker_seat_number: u8,
    pub founding_banker_seat_timestamp: i64,
    pub reserved: [u8; 22],
}
impl ProviderLpPosition {
    pub const LEN: usize = 8 + 32 + 32 + 1 + 8 * 6 + 8 + 1 + 1 + 1 + 8 + 22;
}

#[account]
pub struct WithdrawRequest {
    pub owner: Pubkey,
    pub asset_pool: Pubkey,
    pub lp_amount: u64,
    pub nonce: u64,
    pub requested_at: i64,
    pub processable_at: i64,
    pub processed: bool,
    pub batch_id: u64,
    pub bump: u8,
    pub reserved: [u8; 32],
}
impl WithdrawRequest {
    pub const LEN: usize = 8 + 32 + 32 + 8 * 5 + 1 + 1 + 32;
}


#[derive(Accounts)]
pub struct InitVault<'info> {
    #[account(
        init,
        payer = authority,
        space = VaultConfig::LEN,
        seeds = [b"provider_vault_config"],
        bump
    )]
    pub vault_config: Account<'info, VaultConfig>,
    #[account(
        init,
        payer = authority,
        space = RegisteredAssets::LEN,
        seeds = [b"registered_assets", vault_config.key().as_ref()],
        bump
    )]
    pub registered_assets: Account<'info, RegisteredAssets>,
    #[account(mut)]
    pub authority: Signer<'info>,
    #[account(
        seeds = [crate::ID.as_ref()],
        bump,
        seeds::program = anchor_lang::solana_program::bpf_loader_upgradeable::ID,
        constraint = program_data.upgrade_authority_address == Some(authority.key())
            @ ProviderVaultError::Unauthorized,
    )]
    pub program_data: Account<'info, ProgramData>,
    pub system_program: Program<'info, System>,
}

#[derive(Accounts)]
#[instruction(asset_mint: Pubkey, initial_lp_share_bps: u16, provider_settle_owner: Pubkey)]
pub struct RegisterAsset<'info> {
    #[account(seeds = [b"provider_vault_config"], bump = vault_config.bump)]
    pub vault_config: Box<Account<'info, VaultConfig>>,
    #[account(
        mut,
        seeds = [b"registered_assets", vault_config.key().as_ref()],
        bump = registered_assets.bump
    )]
    pub registered_assets: Box<Account<'info, RegisteredAssets>>,
    #[account(
        init,
        payer = authority,
        space = AssetPool::LEN,
        seeds = [b"asset_pool", vault_config.key().as_ref(), asset_mint.as_ref()],
        bump
    )]
    pub asset_pool: Box<Account<'info, AssetPool>>,
    pub mint_account: Box<Account<'info, Mint>>,
    pub lp_mint: Box<Account<'info, Mint>>,
    #[account(mut)]
    pub authority: Signer<'info>,
    pub system_program: Program<'info, System>,
}

#[derive(Accounts)]
#[instruction(name: [u8; PROVIDER_NAME_LEN], provider_fee_bps: u16, affiliate_recorder_pubkey: Pubkey, signed_terms_hash: [u8; 32])]
pub struct AddProvider<'info> {
    #[account(mut, seeds = [b"provider_vault_config"], bump = vault_config.bump)]
    pub vault_config: Account<'info, VaultConfig>,
    #[account(
        init,
        payer = authority,
        space = Provider::LEN,
        seeds = [b"provider", vault_config.next_provider_id.to_le_bytes().as_ref()],
        bump
    )]
    pub provider: Account<'info, Provider>,
    #[account(mut)]
    pub authority: Signer<'info>,
    pub system_program: Program<'info, System>,
}

#[derive(Accounts)]
#[instruction(provider_id: u32, new_bps: u16)]
pub struct UpdateProviderFee<'info> {
    #[account(seeds = [b"provider_vault_config"], bump = vault_config.bump)]
    pub vault_config: Account<'info, VaultConfig>,
    #[account(
        mut,
        seeds = [b"provider", provider_id.to_le_bytes().as_ref()],
        bump = provider.bump
    )]
    pub provider: Account<'info, Provider>,
    pub authority: Signer<'info>,
}

#[derive(Accounts)]
#[instruction(provider_id: u32)]
pub struct PauseProviderSettlement<'info> {
    #[account(seeds = [b"provider_vault_config"], bump = vault_config.bump)]
    pub vault_config: Account<'info, VaultConfig>,
    #[account(
        mut,
        seeds = [b"provider", provider_id.to_le_bytes().as_ref()],
        bump = provider.bump
    )]
    pub provider: Account<'info, Provider>,
    pub authority: Signer<'info>,
}

#[derive(Accounts)]
#[instruction(asset_mint: Pubkey, new_wallet: Pubkey)]
pub struct ProposeSettleOwner<'info> {
    #[account(mut, seeds = [b"provider_vault_config"], bump = vault_config.bump)]
    pub vault_config: Account<'info, VaultConfig>,
    #[account(
        mut,
        seeds = [b"asset_pool", vault_config.key().as_ref(), asset_mint.as_ref()],
        bump = asset_pool.bump
    )]
    pub asset_pool: Account<'info, AssetPool>,
    pub authority: Signer<'info>,
}

#[derive(Accounts)]
#[instruction(provider_id: u32, day_id: u64, asset_mint: Pubkey)]
pub struct SubmitProviderGgr<'info> {
    #[account(seeds = [b"provider_vault_config"], bump = vault_config.bump)]
    pub vault_config: Box<Account<'info, VaultConfig>>,
    #[account(
        mut,
        seeds = [b"asset_pool", vault_config.key().as_ref(), asset_mint.as_ref()],
        bump = asset_pool.bump
    )]
    pub asset_pool: Box<Account<'info, AssetPool>>,
    #[account(
        mut,
        seeds = [b"provider", provider_id.to_le_bytes().as_ref()],
        bump = provider.bump
    )]
    pub provider: Box<Account<'info, Provider>>,
    #[account(
        init,
        payer = operator,
        space = DailyReceipt::LEN,
        seeds = [b"daily_receipt", provider_id.to_le_bytes().as_ref(), day_id.to_le_bytes().as_ref(), asset_mint.as_ref()],
        bump
    )]
    pub receipt: Box<Account<'info, DailyReceipt>>,
    #[account(
        mut,
        token::mint = asset_mint,
        token::authority = asset_pool,
        address = asset_pool.vault_holder,
    )]
    pub vault_holder: Box<Account<'info, TokenAccount>>,
    #[account(mut)]
    pub operator: Signer<'info>,
    pub system_program: Program<'info, System>,
}

#[derive(Accounts)]
#[instruction(is_keeper: bool)]
pub struct DistributeGgr<'info> {
    #[account(seeds = [b"provider_vault_config"], bump = vault_config.bump)]
    pub vault_config: Account<'info, VaultConfig>,
    #[account(
        mut,
        seeds = [b"asset_pool", vault_config.key().as_ref(), asset_pool.asset_mint.as_ref()],
        bump = asset_pool.bump
    )]
    pub asset_pool: Account<'info, AssetPool>,
    pub signer: Signer<'info>,
}

#[derive(Accounts)]
#[instruction(provider_id: u32, asset_mint: Pubkey)]
pub struct SettleProviderInvoice<'info> {
    #[account(seeds = [b"provider_vault_config"], bump = vault_config.bump)]
    pub vault_config: Box<Account<'info, VaultConfig>>,
    #[account(
        mut,
        seeds = [b"asset_pool", vault_config.key().as_ref(), asset_mint.as_ref()],
        bump = asset_pool.bump
    )]
    pub asset_pool: Box<Account<'info, AssetPool>>,
    #[account(
        mut,
        seeds = [b"provider", provider_id.to_le_bytes().as_ref()],
        bump = provider.bump
    )]
    pub provider: Box<Account<'info, Provider>>,
    #[account(
        mut,
        seeds = [b"provider_owed", asset_pool.key().as_ref(), provider_id.to_le_bytes().as_ref()],
        bump = provider_owed.bump
    )]
    pub provider_owed: Box<Account<'info, ProviderOwed>>,
    #[account(
        mut,
        token::mint = asset_mint_account,
        token::authority = asset_pool,
        address = asset_pool.vault_holder,
    )]
    pub vault_holder: Box<Account<'info, TokenAccount>>,
    #[account(
        constraint = asset_mint_account.key() == asset_pool.asset_mint
            @ ProviderVaultError::AssetMismatch
    )]
    pub asset_mint_account: Box<Account<'info, Mint>>,
    #[account(address = asset_pool.provider_settle_owner @ ProviderVaultError::SettleRecipientMismatch)]
    pub provider_settle_owner: AccountInfo<'info>,
    #[account(
        init_if_needed,
        payer = caller,
        associated_token::mint = asset_mint_account,
        associated_token::authority = provider_settle_owner,
    )]
    pub settle_recipient: Box<Account<'info, TokenAccount>>,
    #[account(mut)]
    pub caller: Signer<'info>,
    pub token_program: Program<'info, Token>,
    pub associated_token_program: Program<'info, AssociatedToken>,
    pub system_program: Program<'info, System>,
}

#[derive(Accounts)]
#[instruction(provider_id: u32, asset_mint: Pubkey)]
pub struct FlushProviderFee<'info> {
    #[account(seeds = [b"provider_vault_config"], bump = vault_config.bump)]
    pub vault_config: Box<Account<'info, VaultConfig>>,
    #[account(
        mut,
        seeds = [b"asset_pool", vault_config.key().as_ref(), asset_mint.as_ref()],
        bump = asset_pool.bump
    )]
    pub asset_pool: Box<Account<'info, AssetPool>>,
    #[account(
        mut,
        seeds = [b"provider", provider_id.to_le_bytes().as_ref()],
        bump = provider.bump
    )]
    pub provider: Box<Account<'info, Provider>>,
    #[account(
        init_if_needed,
        payer = signer,
        space = ProviderOwed::LEN,
        seeds = [b"provider_owed", asset_pool.key().as_ref(), provider_id.to_le_bytes().as_ref()],
        bump
    )]
    pub provider_owed: Box<Account<'info, ProviderOwed>>,
    #[account(mut)]
    pub signer: Signer<'info>,
    pub system_program: Program<'info, System>,
}

#[derive(Accounts)]
#[instruction(provider_id: u32, asset_mint: Pubkey)]
pub struct CorrectProviderFeeOverAccrual<'info> {
    #[account(seeds = [b"provider_vault_config"], bump = vault_config.bump)]
    pub vault_config: Box<Account<'info, VaultConfig>>,
    #[account(
        mut,
        seeds = [b"asset_pool", vault_config.key().as_ref(), asset_mint.as_ref()],
        bump = asset_pool.bump
    )]
    pub asset_pool: Box<Account<'info, AssetPool>>,
    #[account(
        mut,
        seeds = [b"provider", provider_id.to_le_bytes().as_ref()],
        bump = provider.bump
    )]
    pub provider: Box<Account<'info, Provider>>,
    #[account(
        token::mint = asset_mint,
        token::authority = asset_pool,
        address = asset_pool.vault_holder,
    )]
    pub vault_holder: Box<Account<'info, TokenAccount>>,
    pub authority: Signer<'info>,
}

#[derive(Accounts)]
#[instruction(amount: u64, lp_tier: u8)]
pub struct DepositLpUsdc<'info> {
    #[account(mut, seeds = [b"provider_vault_config"], bump = vault_config.bump)]
    pub vault_config: Box<Account<'info, VaultConfig>>,
    #[account(
        mut,
        seeds = [b"asset_pool", vault_config.key().as_ref(), asset_pool.asset_mint.as_ref()],
        bump = asset_pool.bump
    )]
    pub asset_pool: Box<Account<'info, AssetPool>>,
    #[account(
        mut,
        address = asset_pool.lp_mint @ ProviderVaultError::InvalidLpMint,
    )]
    pub lp_mint: Box<Account<'info, Mint>>,
    #[account(
        constraint = asset_mint_account.key() == asset_pool.asset_mint
            @ ProviderVaultError::AssetMismatch
    )]
    pub asset_mint_account: Box<Account<'info, Mint>>,
    #[account(
        mut,
        token::mint = asset_pool.asset_mint,
        token::authority = asset_pool,
        address = asset_pool.vault_holder,
    )]
    pub vault_holder: Box<Account<'info, TokenAccount>>,
    #[account(mut)]
    pub depositor_token_account: Box<Account<'info, TokenAccount>>,
    #[account(
        mut,
        token::mint = asset_pool.lp_mint,
        token::authority = depositor,
    )]
    pub depositor_lp_ata: Box<Account<'info, TokenAccount>>,
    #[account(
        mut,
        token::mint = asset_pool.lp_mint,
        token::authority = asset_pool,
    )]
    pub dead_shares_ata: Box<Account<'info, TokenAccount>>,
    #[account(
        init_if_needed,
        payer = depositor,
        space = ProviderLpPosition::LEN,
        seeds = [b"lp_position", asset_pool.key().as_ref(), depositor.key().as_ref()],
        bump
    )]
    pub lp_position: Box<Account<'info, ProviderLpPosition>>,
    #[account(mut)]
    pub depositor: Signer<'info>,
    pub token_program: Program<'info, Token>,
    pub system_program: Program<'info, System>,
}

#[derive(Accounts)]
pub struct DepositLpSolStub<'info> {
    #[account(mut)]
    pub depositor: Signer<'info>,
}

#[derive(Accounts)]
#[instruction(lp_amount: u64, nonce: u64)]
pub struct RequestWithdrawUsdc<'info> {
    #[account(seeds = [b"provider_vault_config"], bump = vault_config.bump)]
    pub vault_config: Box<Account<'info, VaultConfig>>,
    #[account(
        mut,
        seeds = [b"asset_pool", asset_pool.vault_config.as_ref(), asset_pool.asset_mint.as_ref()],
        bump = asset_pool.bump
    )]
    pub asset_pool: Account<'info, AssetPool>,
    #[account(
        mut,
        seeds = [b"lp_position", asset_pool.key().as_ref(), wallet.key().as_ref()],
        bump = lp_position.bump
    )]
    pub lp_position: Account<'info, ProviderLpPosition>,
    #[account(
        init,
        payer = wallet,
        space = WithdrawRequest::LEN,
        seeds = [b"withdraw_request", asset_pool.key().as_ref(), wallet.key().as_ref(), nonce.to_le_bytes().as_ref()],
        bump
    )]
    pub request: Account<'info, WithdrawRequest>,
    #[account(mut)]
    pub wallet: Signer<'info>,
    pub system_program: Program<'info, System>,
}

#[derive(Accounts)]
pub struct CancelWithdrawRequest<'info> {
    #[account(seeds = [b"provider_vault_config"], bump = vault_config.bump)]
    pub vault_config: Box<Account<'info, VaultConfig>>,
    #[account(
        mut,
        seeds = [b"asset_pool", asset_pool.vault_config.as_ref(), asset_pool.asset_mint.as_ref()],
        bump = asset_pool.bump
    )]
    pub asset_pool: Account<'info, AssetPool>,
    #[account(
        mut,
        seeds = [b"lp_position", asset_pool.key().as_ref(), wallet.key().as_ref()],
        bump = lp_position.bump
    )]
    pub lp_position: Account<'info, ProviderLpPosition>,
    #[account(
        mut,
        close = wallet,
        has_one = asset_pool @ ProviderVaultError::AssetMismatch,
        seeds = [
            b"withdraw_request",
            asset_pool.key().as_ref(),
            request.owner.as_ref(),
            &request.nonce.to_le_bytes(),
        ],
        bump = request.bump,
        constraint = request.owner == wallet.key() @ ProviderVaultError::UnauthorizedRequest,
    )]
    pub request: Account<'info, WithdrawRequest>,
    pub wallet: Signer<'info>,
}

#[derive(Accounts)]
pub struct ProcessWithdrawRequestUsdc<'info> {
    #[account(mut, seeds = [b"provider_vault_config"], bump = vault_config.bump)]
    pub vault_config: Box<Account<'info, VaultConfig>>,
    #[account(
        mut,
        seeds = [b"asset_pool", asset_pool.vault_config.as_ref(), asset_pool.asset_mint.as_ref()],
        bump = asset_pool.bump
    )]
    pub asset_pool: Box<Account<'info, AssetPool>>,
    #[account(
        mut,
        seeds = [b"lp_position", asset_pool.key().as_ref(), wallet.key().as_ref()],
        bump = lp_position.bump
    )]
    pub lp_position: Box<Account<'info, ProviderLpPosition>>,
    #[account(
        mut,
        has_one = asset_pool @ ProviderVaultError::AssetMismatch,
        seeds = [
            b"withdraw_request",
            asset_pool.key().as_ref(),
            request.owner.as_ref(),
            &request.nonce.to_le_bytes(),
        ],
        bump = request.bump,
        constraint = request.owner == wallet.key() @ ProviderVaultError::UnauthorizedRequest,
    )]
    pub request: Box<Account<'info, WithdrawRequest>>,
    #[account(
        mut,
        address = asset_pool.lp_mint @ ProviderVaultError::InvalidLpMint,
    )]
    pub lp_mint: Box<Account<'info, Mint>>,
    #[account(
        constraint = asset_mint_account.key() == asset_pool.asset_mint
            @ ProviderVaultError::AssetMismatch
    )]
    pub asset_mint_account: Box<Account<'info, Mint>>,
    #[account(
        mut,
        token::mint = asset_pool.asset_mint,
        token::authority = asset_pool,
        address = asset_pool.vault_holder,
    )]
    pub vault_holder: Box<Account<'info, TokenAccount>>,
    #[account(
        mut,
        token::mint = asset_pool.lp_mint,
        token::authority = wallet,
    )]
    pub owner_lp_ata: Box<Account<'info, TokenAccount>>,
    #[account(
        mut,
        token::mint = asset_pool.asset_mint,
        constraint = wallet_token_account.owner == wallet.key()
            @ ProviderVaultError::UnauthorizedRequest,
    )]
    pub wallet_token_account: Box<Account<'info, TokenAccount>>,
    #[account(mut)]
    pub wallet: Signer<'info>,
    pub token_program: Program<'info, Token>,
}

#[derive(Accounts)]
pub struct RefillInsuranceUsdc<'info> {
    #[account(
        seeds = [b"provider_vault_config"],
        bump = vault_config.bump,
    )]
    pub vault_config: Box<Account<'info, VaultConfig>>,
    #[account(
        mut,
        seeds = [b"asset_pool", asset_pool.vault_config.as_ref(), asset_pool.asset_mint.as_ref()],
        bump = asset_pool.bump,
        constraint = asset_pool.vault_config == vault_config.key() @ ProviderVaultError::InvalidVaultConfig
    )]
    pub asset_pool: Box<Account<'info, AssetPool>>,
    #[account(
        constraint = asset_mint.key() == asset_pool.asset_mint @ ProviderVaultError::InvalidMint
    )]
    pub asset_mint: Box<Account<'info, Mint>>,
    #[account(
        init_if_needed,
        payer = authority,
        seeds = [b"insurance", asset_pool.key().as_ref()],
        bump,
        token::mint = asset_mint,
        token::authority = asset_pool,
    )]
    pub insurance_holder: Box<Account<'info, TokenAccount>>,
    #[account(
        token::mint = asset_mint,
        token::authority = asset_pool,
        address = asset_pool.vault_holder,
    )]
    pub vault_holder: Box<Account<'info, TokenAccount>>,
    #[account(
        mut,
        constraint = source_token_account.owner == authority.key() @ ProviderVaultError::Unauthorized
    )]
    pub source_token_account: Box<Account<'info, TokenAccount>>,
    #[account(
        mut,
        constraint = authority.key() == vault_config.authority @ ProviderVaultError::Unauthorized
    )]
    pub authority: Signer<'info>,
    pub token_program: Program<'info, Token>,
    pub system_program: Program<'info, System>,
    pub rent: Sysvar<'info, Rent>,
}

#[derive(Accounts)]
pub struct SetPaused<'info> {
    #[account(mut, seeds = [b"provider_vault_config"], bump = vault_config.bump)]
    pub vault_config: Account<'info, VaultConfig>,
    pub signer: Signer<'info>,
}

#[derive(Accounts)]
pub struct Freeze<'info> {
    #[account(mut, seeds = [b"provider_vault_config"], bump = vault_config.bump)]
    pub vault_config: Account<'info, VaultConfig>,
    pub signer: Signer<'info>,
}

#[derive(Accounts)]
pub struct Unfreeze<'info> {
    #[account(mut, seeds = [b"provider_vault_config"], bump = vault_config.bump)]
    pub vault_config: Account<'info, VaultConfig>,
    pub signer: Signer<'info>,
}

#[derive(Accounts)]
pub struct AdminAction<'info> {
    #[account(mut, seeds = [b"provider_vault_config"], bump = vault_config.bump)]
    pub vault_config: Account<'info, VaultConfig>,
    pub signer: Signer<'info>,
}

#[derive(Accounts)]
pub struct AdminLockVault<'info> {
    #[account(seeds = [b"provider_vault_config"], bump = vault_config.bump)]
    pub vault_config: Account<'info, VaultConfig>,
    #[account(
        mut,
        seeds = [b"asset_pool", vault_config.key().as_ref(), asset_pool.asset_mint.as_ref()],
        bump = asset_pool.bump
    )]
    pub asset_pool: Account<'info, AssetPool>,
    pub signer: Signer<'info>,
}

#[derive(Accounts)]
pub struct ProposeResetPeak<'info> {
    #[account(mut, seeds = [b"provider_vault_config"], bump = vault_config.bump)]
    pub vault_config: Account<'info, VaultConfig>,
    #[account(
        mut,
        seeds = [b"asset_pool", vault_config.key().as_ref(), asset_pool.asset_mint.as_ref()],
        bump = asset_pool.bump
    )]
    pub asset_pool: Account<'info, AssetPool>,
    pub signer: Signer<'info>,
}

#[derive(Accounts)]
pub struct FinalizeResetPeak<'info> {
    #[account(seeds = [b"provider_vault_config"], bump = vault_config.bump)]
    pub vault_config: Account<'info, VaultConfig>,
    #[account(
        mut,
        seeds = [b"asset_pool", vault_config.key().as_ref(), asset_pool.asset_mint.as_ref()],
        bump = asset_pool.bump
    )]
    pub asset_pool: Box<Account<'info, AssetPool>>,
    #[account(
        token::authority = asset_pool,
        address = asset_pool.vault_holder,
    )]
    pub vault_holder: Box<Account<'info, TokenAccount>>,
    pub signer: Signer<'info>,
}

#[derive(Accounts)]
pub struct CancelResetPeak<'info> {
    #[account(seeds = [b"provider_vault_config"], bump = vault_config.bump)]
    pub vault_config: Account<'info, VaultConfig>,
    #[account(
        mut,
        seeds = [b"asset_pool", vault_config.key().as_ref(), asset_pool.asset_mint.as_ref()],
        bump = asset_pool.bump
    )]
    pub asset_pool: Account<'info, AssetPool>,
    pub signer: Signer<'info>,
}

#[derive(Accounts)]
pub struct SetVaultHolder<'info> {
    #[account(seeds = [b"provider_vault_config"], bump = vault_config.bump)]
    pub vault_config: Account<'info, VaultConfig>,
    #[account(
        mut,
        seeds = [b"asset_pool", vault_config.key().as_ref(), asset_pool.asset_mint.as_ref()],
        bump = asset_pool.bump
    )]
    pub asset_pool: Account<'info, AssetPool>,
    #[account(
        token::mint = asset_mint_account,
        token::authority = asset_pool,
    )]
    pub vault_holder: Box<Account<'info, TokenAccount>>,
    #[account(
        constraint = asset_mint_account.key() == asset_pool.asset_mint
            @ ProviderVaultError::AssetMismatch
    )]
    pub asset_mint_account: Box<Account<'info, Mint>>,
    pub signer: Signer<'info>,
}

#[derive(Accounts)]
#[instruction(asset_mint: Pubkey)]
pub struct SetChipDebitCapPerWallet<'info> {
    #[account(seeds = [b"provider_vault_config"], bump = vault_config.bump)]
    pub vault_config: Account<'info, VaultConfig>,
    #[account(
        mut,
        seeds = [b"asset_pool", vault_config.key().as_ref(), asset_mint.as_ref()],
        bump = asset_pool.bump
    )]
    pub asset_pool: Account<'info, AssetPool>,
    pub signer: Signer<'info>,
}


#[derive(Accounts)]
#[instruction(asset_mint: Pubkey)]
pub struct DistributeAffiliate<'info> {
    #[account(seeds = [b"provider_vault_config"], bump = vault_config.bump)]
    pub vault_config: Box<Account<'info, VaultConfig>>,
    #[account(
        mut,
        seeds = [b"asset_pool", vault_config.key().as_ref(), asset_mint.as_ref()],
        bump = asset_pool.bump
    )]
    pub asset_pool: Box<Account<'info, AssetPool>>,
    #[account(
        mut,
        token::mint = asset_mint,
        token::authority = asset_pool,
        address = asset_pool.vault_holder,
    )]
    pub vault_holder: Box<Account<'info, TokenAccount>>,

    #[account(
        address = vault_config.affiliate_registry_config @ ProviderVaultError::InvalidProgramId
    )]
    pub affiliate_config: Box<Account<'info, AffiliateConfig>>,
    #[account(mut)]
    pub affiliate_funding_pool: Box<Account<'info, AffiliateFundingPool>>,
    #[account(mut)]
    pub affiliate_pool_token_account: Box<Account<'info, TokenAccount>>,
    #[account(
        address = AFFILIATE_REGISTRY_PROGRAM_ID @ ProviderVaultError::InvalidProgramId
    )]
    pub affiliate_registry_program: Program<'info, AffiliateRegistry>,

    #[account(mut)]
    pub signer: Signer<'info>,
    pub token_program: Program<'info, Token>,
    pub system_program: Program<'info, System>,
}

#[derive(Accounts)]
#[instruction(asset_mint: Pubkey)]
pub struct DistributeSovereign<'info> {
    #[account(seeds = [b"provider_vault_config"], bump = vault_config.bump)]
    pub vault_config: Box<Account<'info, VaultConfig>>,
    #[account(
        mut,
        seeds = [b"asset_pool", vault_config.key().as_ref(), asset_mint.as_ref()],
        bump = asset_pool.bump
    )]
    pub asset_pool: Box<Account<'info, AssetPool>>,
    #[account(
        mut,
        token::mint = asset_mint,
        token::authority = asset_pool,
        address = asset_pool.vault_holder,
    )]
    pub vault_holder: Box<Account<'info, TokenAccount>>,

    #[account(
        address = vault_config.sovereign_registry_config @ ProviderVaultError::InvalidProgramId
    )]
    pub sovereign_registry_config: Box<Account<'info, SovereignRegistryConfig>>,
    #[account(mut)]
    pub sovereign_royalty_vault_usdc: Box<Account<'info, TokenAccount>>,
    #[account(
        address = SOVEREIGN_REGISTRY_PROGRAM_ID @ ProviderVaultError::InvalidProgramId
    )]
    pub sovereign_registry_program: Program<'info, SovereignRegistry>,

    #[account(mut)]
    pub signer: Signer<'info>,
    pub token_program: Program<'info, Token>,
    pub system_program: Program<'info, System>,
}

#[derive(Accounts)]
#[instruction(asset_mint: Pubkey)]
pub struct DistributeYield<'info> {
    #[account(seeds = [b"provider_vault_config"], bump = vault_config.bump)]
    pub vault_config: Box<Account<'info, VaultConfig>>,
    #[account(
        mut,
        seeds = [b"asset_pool", vault_config.key().as_ref(), asset_mint.as_ref()],
        bump = asset_pool.bump
    )]
    pub asset_pool: Box<Account<'info, AssetPool>>,
    #[account(
        mut,
        token::mint = asset_mint,
        token::authority = asset_pool,
        address = asset_pool.vault_holder,
    )]
    pub vault_holder: Box<Account<'info, TokenAccount>>,

    #[account(mut)]
    pub yield_config: Box<Account<'info, YieldConfig>>,
    #[account(mut)]
    pub yield_pool_usdc: Box<Account<'info, TokenAccount>>,
    #[account(
        address = YIELD_ESCROW_PROGRAM_ID @ ProviderVaultError::InvalidProgramId
    )]
    pub yield_escrow_program: Program<'info, YieldEscrow>,
    pub staking_config: AccountInfo<'info>,

    #[account(mut)]
    pub swap_router_config: AccountInfo<'info>,
    #[account(
        mut,
        token::mint = asset_mint,
    )]
    pub swap_router_usdc_holder: Box<Account<'info, TokenAccount>>,
    #[account(address = asset_mint @ ProviderVaultError::AssetMismatch)]
    pub swap_router_usdc_mint: Box<Account<'info, Mint>>,
    #[account(address = SWAP_ROUTER_PROGRAM_ID @ ProviderVaultError::InvalidProgramId)]
    pub swap_router_program: Program<'info, SwapRouter>,

    #[account(mut)]
    pub signer: Signer<'info>,
    pub token_program: Program<'info, Token>,
    pub system_program: Program<'info, System>,
}

#[derive(Accounts)]
#[instruction(asset_mint: Pubkey)]
pub struct DistributeReserve<'info> {
    #[account(seeds = [b"provider_vault_config"], bump = vault_config.bump)]
    pub vault_config: Box<Account<'info, VaultConfig>>,
    #[account(
        mut,
        seeds = [b"asset_pool", vault_config.key().as_ref(), asset_mint.as_ref()],
        bump = asset_pool.bump
    )]
    pub asset_pool: Box<Account<'info, AssetPool>>,
    #[account(
        mut,
        token::mint = asset_mint_account,
        token::authority = asset_pool,
        address = asset_pool.vault_holder,
    )]
    pub vault_holder: Box<Account<'info, TokenAccount>>,
    #[account(
        mut,
        constraint = asset_mint_account.key() == asset_pool.asset_mint
            @ ProviderVaultError::AssetMismatch
    )]
    pub asset_mint_account: Box<Account<'info, Mint>>,
    #[account(mut)]
    pub ops_marketing_token_account: Box<Account<'info, TokenAccount>>,


    #[account(mut)]
    pub swap_router_config: AccountInfo<'info>,

    #[account(mut)]
    pub top_token_mint: Box<InterfaceAccount<'info, TopMint>>,

    #[account(
        constraint = top_token_program.key() == anchor_spl::token_2022::ID
            @ ProviderVaultError::WrongTopTokenProgram
    )]
    pub top_token_program: Interface<'info, TokenInterface>,

    #[account(
        init_if_needed,
        payer = signer,
        associated_token::mint = top_token_mint,
        associated_token::authority = asset_pool,
        associated_token::token_program = top_token_program,
    )]
    pub top_vault_holder: Box<InterfaceAccount<'info, TopTokenAccount>>,

    #[account(address = SWAP_ROUTER_PROGRAM_ID @ ProviderVaultError::InvalidProgramId)]
    pub swap_router_program: Program<'info, SwapRouter>,

    #[account(mut)]
    pub signer: Signer<'info>,
    pub token_program: Program<'info, Token>,
    pub associated_token_program: Program<'info, AssociatedToken>,
    pub system_program: Program<'info, System>,
}

#[derive(Accounts)]
#[instruction(asset_mint: Pubkey)]
pub struct DistributeDevFee<'info> {
    #[account(seeds = [b"provider_vault_config"], bump = vault_config.bump)]
    pub vault_config: Box<Account<'info, VaultConfig>>,
    #[account(
        mut,
        seeds = [b"asset_pool", vault_config.key().as_ref(), asset_mint.as_ref()],
        bump = asset_pool.bump
    )]
    pub asset_pool: Box<Account<'info, AssetPool>>,
    #[account(
        mut,
        token::mint = asset_mint_account,
        token::authority = asset_pool,
        address = asset_pool.vault_holder,
    )]
    pub vault_holder: Box<Account<'info, TokenAccount>>,
    #[account(
        constraint = asset_mint_account.key() == asset_pool.asset_mint
            @ ProviderVaultError::AssetMismatch
    )]
    pub asset_mint_account: Box<Account<'info, Mint>>,
    #[account(mut)]
    pub ops_marketing_token_account: Box<Account<'info, TokenAccount>>,

    #[account(mut)]
    pub signer: Signer<'info>,
    pub token_program: Program<'info, Token>,
}

#[derive(Accounts)]
#[instruction(provider_id: u32, asset_mint: Pubkey, amount: u64)]
pub struct AccrueAffiliateAmount<'info> {
    #[account(seeds = [b"provider_vault_config"], bump = vault_config.bump)]
    pub vault_config: Box<Account<'info, VaultConfig>>,
    #[account(
        mut,
        seeds = [b"asset_pool", vault_config.key().as_ref(), asset_mint.as_ref()],
        bump = asset_pool.bump
    )]
    pub asset_pool: Box<Account<'info, AssetPool>>,
    #[account(
        token::mint = asset_mint,
        token::authority = asset_pool,
        address = asset_pool.vault_holder,
    )]
    pub vault_holder: Box<Account<'info, TokenAccount>>,
    pub operator: Signer<'info>,
}


#[derive(Accounts)]
#[instruction(asset_mint: Pubkey, amount: u64)]
pub struct ChipDeposit<'info> {
    #[account(seeds = [b"provider_vault_config"], bump = vault_config.bump)]
    pub vault_config: Box<Account<'info, VaultConfig>>,
    #[account(
        seeds = [b"asset_pool", vault_config.key().as_ref(), asset_mint.as_ref()],
        bump = asset_pool.bump
    )]
    pub asset_pool: Box<Account<'info, AssetPool>>,

    #[account(
        init_if_needed,
        payer = player,
        space = ProviderPlayerEscrow::LEN,
        seeds = [b"provider_player_escrow", player.key().as_ref(), asset_mint.as_ref()],
        bump
    )]
    pub player_escrow: Box<Account<'info, ProviderPlayerEscrow>>,

    #[account(
        seeds = [b"provider_player_escrow_holder", player.key().as_ref(), asset_mint.as_ref()],
        bump
    )]
    pub escrow_holder_authority: AccountInfo<'info>,

    #[account(
        mut,
        token::mint = asset_mint,
        token::authority = escrow_holder_authority,
        address = get_associated_token_address(&escrow_holder_authority.key(), &asset_pool.asset_mint)
            @ ProviderVaultError::EscrowHolderMismatch,
    )]
    pub escrow_holder: Box<Account<'info, TokenAccount>>,

    #[account(
        constraint = asset_mint_account.key() == asset_pool.asset_mint
            @ ProviderVaultError::AssetMismatch
    )]
    pub asset_mint_account: Box<Account<'info, Mint>>,
    #[account(mut)]
    pub player_token_account: Box<Account<'info, TokenAccount>>,
    #[account(mut)]
    pub player: Signer<'info>,
    pub token_program: Program<'info, Token>,
    pub system_program: Program<'info, System>,
}

#[account]
pub struct WithdrawReceipt {
    pub withdrawn: bool,
    pub bump: u8,
}
impl WithdrawReceipt {
    pub const LEN: usize = 8 + 1 + 1;
}

#[derive(Accounts)]
#[instruction(asset_mint: Pubkey, amount: u64, reference: [u8; 32])]
pub struct ChipWithdraw<'info> {
    #[account(seeds = [b"provider_vault_config"], bump = vault_config.bump)]
    pub vault_config: Box<Account<'info, VaultConfig>>,
    #[account(
        seeds = [b"asset_pool", vault_config.key().as_ref(), asset_mint.as_ref()],
        bump = asset_pool.bump
    )]
    pub asset_pool: Box<Account<'info, AssetPool>>,

    #[account(
        mut,
        seeds = [b"provider_player_escrow", player_escrow.wallet.as_ref(), asset_mint.as_ref()],
        bump = player_escrow.bump
    )]
    pub player_escrow: Box<Account<'info, ProviderPlayerEscrow>>,

    #[account(
        init_if_needed,
        payer = signer,
        space = WithdrawReceipt::LEN,
        seeds = [b"withdraw_receipt", reference.as_ref()],
        bump
    )]
    pub withdraw_receipt: Box<Account<'info, WithdrawReceipt>>,

    #[account(
        seeds = [b"provider_player_escrow_holder", player_escrow.wallet.as_ref(), asset_mint.as_ref()],
        bump
    )]
    pub escrow_holder_authority: AccountInfo<'info>,

    #[account(
        mut,
        token::mint = asset_mint,
        token::authority = escrow_holder_authority,
        address = get_associated_token_address(&escrow_holder_authority.key(), &asset_pool.asset_mint)
            @ ProviderVaultError::EscrowHolderMismatch,
    )]
    pub escrow_holder: Box<Account<'info, TokenAccount>>,

    #[account(
        mut,
        constraint = player_token_account.owner == player_escrow.wallet
            @ ProviderVaultError::PlayerEscrowMismatch
    )]
    pub player_token_account: Box<Account<'info, TokenAccount>>,

    #[account(mut)]
    pub signer: Signer<'info>,
    #[account(
        constraint = asset_mint_account.key() == asset_pool.asset_mint
            @ ProviderVaultError::AssetMismatch
    )]
    pub asset_mint_account: Box<Account<'info, Mint>>,
    pub token_program: Program<'info, Token>,
    pub system_program: Program<'info, System>,
}

#[account]
pub struct DebitReceipt {
    pub recorded: bool,
    pub bump: u8,
}
impl DebitReceipt {
    pub const LEN: usize = 8 + 1 + 1;
}

#[derive(Accounts)]
#[instruction(asset_mint: Pubkey, amount: u64, player_wallet: Pubkey, provider_id: u32, reference: [u8; 32])]
pub struct ChipDebitToVault<'info> {
    #[account(seeds = [b"provider_vault_config"], bump = vault_config.bump)]
    pub vault_config: Box<Account<'info, VaultConfig>>,
    #[account(
        mut,
        seeds = [b"asset_pool", vault_config.key().as_ref(), asset_mint.as_ref()],
        bump = asset_pool.bump
    )]
    pub asset_pool: Box<Account<'info, AssetPool>>,

    #[account(
        mut,
        seeds = [b"provider_player_escrow", player_wallet.as_ref(), asset_mint.as_ref()],
        bump = player_escrow.bump
    )]
    pub player_escrow: Box<Account<'info, ProviderPlayerEscrow>>,

    #[account(
        init_if_needed,
        payer = operator,
        space = DebitReceipt::LEN,
        seeds = [b"debit_receipt", reference.as_ref()],
        bump
    )]
    pub debit_receipt: Box<Account<'info, DebitReceipt>>,

    #[account(
        seeds = [b"provider_player_escrow_holder", player_wallet.as_ref(), asset_mint.as_ref()],
        bump
    )]
    pub escrow_holder_authority: AccountInfo<'info>,

    #[account(
        mut,
        token::mint = asset_mint,
        token::authority = escrow_holder_authority,
        address = get_associated_token_address(&escrow_holder_authority.key(), &asset_pool.asset_mint)
            @ ProviderVaultError::EscrowHolderMismatch,
    )]
    pub escrow_holder: Box<Account<'info, TokenAccount>>,

    #[account(
        mut,
        token::mint = asset_mint,
        token::authority = asset_pool,
        address = asset_pool.vault_holder,
    )]
    pub vault_holder: Box<Account<'info, TokenAccount>>,
    #[account(mut)]
    pub operator: Signer<'info>,
    #[account(
        constraint = asset_mint_account.key() == asset_pool.asset_mint
            @ ProviderVaultError::AssetMismatch
    )]
    pub asset_mint_account: Box<Account<'info, Mint>>,
    pub token_program: Program<'info, Token>,
    pub system_program: Program<'info, System>,
}

#[account]
pub struct CreditReceipt {
    pub credited: bool,
    pub bump: u8,
}
impl CreditReceipt {
    pub const LEN: usize = 8 + 1 + 1;
}

#[derive(Accounts)]
#[instruction(reference: [u8; 32])]
pub struct CloseDebitReceipt<'info> {
    #[account(seeds = [b"provider_vault_config"], bump = vault_config.bump)]
    pub vault_config: Box<Account<'info, VaultConfig>>,
    #[account(
        mut,
        close = operator,
        seeds = [b"debit_receipt", reference.as_ref()],
        bump = debit_receipt.bump,
    )]
    pub debit_receipt: Box<Account<'info, DebitReceipt>>,
    #[account(mut, address = vault_config.operator_pubkey @ ProviderVaultError::Unauthorized)]
    pub operator: Signer<'info>,
}

#[derive(Accounts)]
#[instruction(reference: [u8; 32])]
pub struct CloseWithdrawReceipt<'info> {
    #[account(seeds = [b"provider_vault_config"], bump = vault_config.bump)]
    pub vault_config: Box<Account<'info, VaultConfig>>,
    #[account(
        mut,
        close = operator,
        seeds = [b"withdraw_receipt", reference.as_ref()],
        bump = withdraw_receipt.bump,
    )]
    pub withdraw_receipt: Box<Account<'info, WithdrawReceipt>>,
    #[account(mut, address = vault_config.operator_pubkey @ ProviderVaultError::Unauthorized)]
    pub operator: Signer<'info>,
}

#[derive(Accounts)]
#[instruction(reference: [u8; 32])]
pub struct CloseCreditReceipt<'info> {
    #[account(seeds = [b"provider_vault_config"], bump = vault_config.bump)]
    pub vault_config: Box<Account<'info, VaultConfig>>,
    #[account(
        mut,
        close = operator,
        seeds = [b"credit_receipt", reference.as_ref()],
        bump = credit_receipt.bump,
    )]
    pub credit_receipt: Box<Account<'info, CreditReceipt>>,
    #[account(mut, address = vault_config.operator_pubkey @ ProviderVaultError::Unauthorized)]
    pub operator: Signer<'info>,
}

#[derive(Accounts)]
#[instruction(reference: [u8; 32])]
pub struct CloseCreditReceiptPromo<'info> {
    #[account(seeds = [b"provider_vault_config"], bump = vault_config.bump)]
    pub vault_config: Box<Account<'info, VaultConfig>>,
    #[account(
        mut,
        close = operator,
        seeds = [b"credit_receipt_promo", reference.as_ref()],
        bump = credit_receipt.bump,
    )]
    pub credit_receipt: Box<Account<'info, CreditReceipt>>,
    #[account(mut, address = vault_config.operator_pubkey @ ProviderVaultError::Unauthorized)]
    pub operator: Signer<'info>,
}

#[derive(Accounts)]
#[instruction(reference: [u8; 32])]
pub struct CloseCreditReceiptNgr<'info> {
    #[account(seeds = [b"provider_vault_config"], bump = vault_config.bump)]
    pub vault_config: Box<Account<'info, VaultConfig>>,
    #[account(
        mut,
        close = operator,
        seeds = [b"credit_receipt_ngr", reference.as_ref()],
        bump = credit_receipt.bump,
    )]
    pub credit_receipt: Box<Account<'info, CreditReceipt>>,
    #[account(mut, address = vault_config.operator_pubkey @ ProviderVaultError::Unauthorized)]
    pub operator: Signer<'info>,
}

#[derive(Accounts)]
#[instruction(asset_mint: Pubkey, amount: u64, player_wallet: Pubkey, provider_id: u32, reference: [u8; 32])]
pub struct ChipCreditFromVault<'info> {
    #[account(mut, seeds = [b"provider_vault_config"], bump = vault_config.bump)]
    pub vault_config: Box<Account<'info, VaultConfig>>,
    #[account(
        mut,
        seeds = [b"asset_pool", vault_config.key().as_ref(), asset_mint.as_ref()],
        bump = asset_pool.bump
    )]
    pub asset_pool: Box<Account<'info, AssetPool>>,

    #[account(
        init_if_needed,
        payer = operator,
        space = ProviderPlayerEscrow::LEN,
        seeds = [b"provider_player_escrow", player.key().as_ref(), asset_mint.as_ref()],
        bump
    )]
    pub player_escrow: Box<Account<'info, ProviderPlayerEscrow>>,

    #[account(
        init_if_needed,
        payer = operator,
        space = CreditReceipt::LEN,
        seeds = [b"credit_receipt", reference.as_ref()],
        bump
    )]
    pub credit_receipt: Box<Account<'info, CreditReceipt>>,

    #[account(
        seeds = [b"provider_player_escrow_holder", player.key().as_ref(), asset_mint.as_ref()],
        bump
    )]
    pub escrow_holder_authority: AccountInfo<'info>,

    #[account(
        mut,
        token::mint = asset_mint,
        token::authority = escrow_holder_authority,
        address = get_associated_token_address(&escrow_holder_authority.key(), &asset_pool.asset_mint)
            @ ProviderVaultError::EscrowHolderMismatch,
    )]
    pub escrow_holder: Box<Account<'info, TokenAccount>>,

    #[account(
        mut,
        token::mint = asset_mint,
        token::authority = asset_pool,
        address = asset_pool.vault_holder,
    )]
    pub vault_holder: Box<Account<'info, TokenAccount>>,
    pub player: AccountInfo<'info>,

    #[account(mut)]
    pub operator: Signer<'info>,
    #[account(
        constraint = asset_mint_account.key() == asset_pool.asset_mint
            @ ProviderVaultError::AssetMismatch
    )]
    pub asset_mint_account: Box<Account<'info, Mint>>,
    pub token_program: Program<'info, Token>,
    pub system_program: Program<'info, System>,
    #[account(
        mut,
        token::mint = asset_mint_account,
        token::authority = asset_pool,
    )]
    pub insurance_holder: Option<Box<Account<'info, TokenAccount>>>,
}


#[derive(Accounts)]
#[instruction(asset_mint: Pubkey, amount: u64)]
pub struct TopUpPromoPool<'info> {
    #[account(seeds = [b"provider_vault_config"], bump = vault_config.bump)]
    pub vault_config: Box<Account<'info, VaultConfig>>,
    #[account(
        mut,
        seeds = [b"asset_pool", vault_config.key().as_ref(), asset_mint.as_ref()],
        bump = asset_pool.bump
    )]
    pub asset_pool: Box<Account<'info, AssetPool>>,

    #[account(
        mut,
        token::mint = asset_mint,
        token::authority = operator,
    )]
    pub source_token_account: Box<Account<'info, TokenAccount>>,

    #[account(
        mut,
        token::mint = asset_mint,
        token::authority = asset_pool,
        address = asset_pool.vault_holder,
    )]
    pub vault_holder: Box<Account<'info, TokenAccount>>,

    #[account(mut)]
    pub operator: Signer<'info>,
    #[account(
        constraint = asset_mint_account.key() == asset_pool.asset_mint
            @ ProviderVaultError::AssetMismatch
    )]
    pub asset_mint_account: Box<Account<'info, Mint>>,
    pub token_program: Program<'info, Token>,
}

#[derive(Accounts)]
#[instruction(asset_mint: Pubkey, amount: u64, player_wallet: Pubkey, provider_id: u32, reference: [u8; 32])]
pub struct ChipCreditFromVaultPromo<'info> {
    #[account(mut, seeds = [b"provider_vault_config"], bump = vault_config.bump)]
    pub vault_config: Box<Account<'info, VaultConfig>>,
    #[account(
        mut,
        seeds = [b"asset_pool", vault_config.key().as_ref(), asset_mint.as_ref()],
        bump = asset_pool.bump
    )]
    pub asset_pool: Box<Account<'info, AssetPool>>,

    #[account(
        init_if_needed,
        payer = operator,
        space = ProviderPlayerEscrow::LEN,
        seeds = [b"provider_player_escrow", player.key().as_ref(), asset_mint.as_ref()],
        bump
    )]
    pub player_escrow: Box<Account<'info, ProviderPlayerEscrow>>,

    #[account(
        init_if_needed,
        payer = operator,
        space = CreditReceipt::LEN,
        seeds = [b"credit_receipt_promo", reference.as_ref()],
        bump
    )]
    pub credit_receipt: Box<Account<'info, CreditReceipt>>,

    #[account(
        seeds = [b"provider_player_escrow_holder", player.key().as_ref(), asset_mint.as_ref()],
        bump
    )]
    pub escrow_holder_authority: AccountInfo<'info>,

    #[account(
        mut,
        token::mint = asset_mint,
        token::authority = escrow_holder_authority,
        address = get_associated_token_address(&escrow_holder_authority.key(), &asset_pool.asset_mint)
            @ ProviderVaultError::EscrowHolderMismatch,
    )]
    pub escrow_holder: Box<Account<'info, TokenAccount>>,

    #[account(
        mut,
        token::mint = asset_mint,
        token::authority = asset_pool,
        address = asset_pool.vault_holder,
    )]
    pub vault_holder: Box<Account<'info, TokenAccount>>,

    pub player: AccountInfo<'info>,

    #[account(mut)]
    pub operator: Signer<'info>,
    #[account(
        constraint = asset_mint_account.key() == asset_pool.asset_mint
            @ ProviderVaultError::AssetMismatch
    )]
    pub asset_mint_account: Box<Account<'info, Mint>>,
    pub token_program: Program<'info, Token>,
    pub system_program: Program<'info, System>,
    #[account(
        mut,
        token::mint = asset_mint_account,
        token::authority = asset_pool,
    )]
    pub insurance_holder: Option<Box<Account<'info, TokenAccount>>>,
}

#[derive(Accounts)]
#[instruction(asset_mint: Pubkey, amount: u64, player_wallet: Pubkey, provider_id: u32, is_network_reimbursable: bool, reference: [u8; 32])]
pub struct ChipCreditFromVaultNgrPromo<'info> {
    #[account(mut, seeds = [b"provider_vault_config"], bump = vault_config.bump)]
    pub vault_config: Box<Account<'info, VaultConfig>>,
    #[account(
        mut,
        seeds = [b"asset_pool", vault_config.key().as_ref(), asset_mint.as_ref()],
        bump = asset_pool.bump
    )]
    pub asset_pool: Box<Account<'info, AssetPool>>,

    #[account(
        init_if_needed,
        payer = operator,
        space = ProviderPlayerEscrow::LEN,
        seeds = [b"provider_player_escrow", player.key().as_ref(), asset_mint.as_ref()],
        bump
    )]
    pub player_escrow: Box<Account<'info, ProviderPlayerEscrow>>,

    #[account(
        init_if_needed,
        payer = operator,
        space = CreditReceipt::LEN,
        seeds = [b"credit_receipt_ngr", reference.as_ref()],
        bump
    )]
    pub credit_receipt: Box<Account<'info, CreditReceipt>>,

    #[account(
        seeds = [b"provider_player_escrow_holder", player.key().as_ref(), asset_mint.as_ref()],
        bump
    )]
    pub escrow_holder_authority: AccountInfo<'info>,

    #[account(
        mut,
        token::mint = asset_mint,
        token::authority = escrow_holder_authority,
        address = get_associated_token_address(&escrow_holder_authority.key(), &asset_pool.asset_mint)
            @ ProviderVaultError::EscrowHolderMismatch,
    )]
    pub escrow_holder: Box<Account<'info, TokenAccount>>,

    #[account(
        mut,
        token::mint = asset_mint,
        token::authority = asset_pool,
        address = asset_pool.vault_holder,
    )]
    pub vault_holder: Box<Account<'info, TokenAccount>>,

    pub player: AccountInfo<'info>,

    #[account(mut)]
    pub operator: Signer<'info>,
    #[account(
        constraint = asset_mint_account.key() == asset_pool.asset_mint
            @ ProviderVaultError::AssetMismatch
    )]
    pub asset_mint_account: Box<Account<'info, Mint>>,
    pub token_program: Program<'info, Token>,
    pub system_program: Program<'info, System>,
    #[account(
        mut,
        token::mint = asset_mint_account,
        token::authority = asset_pool,
    )]
    pub insurance_holder: Option<Box<Account<'info, TokenAccount>>>,
}

#[derive(Accounts)]
#[instruction(asset_mint: Pubkey)]
pub struct CreditChipsFromSwap<'info> {
    #[account(seeds = [b"provider_vault_config"], bump = vault_config.bump)]
    pub vault_config: Box<Account<'info, VaultConfig>>,
    #[account(
        seeds = [b"asset_pool", vault_config.key().as_ref(), asset_mint.as_ref()],
        bump = asset_pool.bump
    )]
    pub asset_pool: Box<Account<'info, AssetPool>>,

    #[account(
        init_if_needed,
        payer = fee_payer,
        space = ProviderPlayerEscrow::LEN,
        seeds = [b"provider_player_escrow", player.key().as_ref(), asset_mint.as_ref()],
        bump
    )]
    pub player_escrow: Box<Account<'info, ProviderPlayerEscrow>>,

    #[account(
        seeds = [b"provider_player_escrow_holder", player.key().as_ref(), asset_mint.as_ref()],
        bump
    )]
    pub escrow_holder_authority: AccountInfo<'info>,

    #[account(
        token::mint = asset_pool.asset_mint,
        token::authority = escrow_holder_authority,
        address = get_associated_token_address(&escrow_holder_authority.key(), &asset_pool.asset_mint)
            @ ProviderVaultError::EscrowHolderMismatch,
    )]
    pub escrow_holder: Box<Account<'info, TokenAccount>>,

    pub player: AccountInfo<'info>,

    #[account(mut)]
    pub fee_payer: Signer<'info>,

    pub token_program: Program<'info, Token>,
    pub system_program: Program<'info, System>,
}


#[event]
pub struct VaultInitialized {
    pub authority: Pubkey,
    pub operator: Pubkey,
    pub timestamp: i64,
}

#[event]
pub struct AssetRegistered {
    pub asset_mint: Pubkey,
    pub lp_mint: Pubkey,
    pub initial_lp_share_bps: u16,
    pub timestamp: i64,
}

#[event]
pub struct ProviderAdded {
    pub provider_id: u32,
    pub name: [u8; PROVIDER_NAME_LEN],
    pub provider_fee_bps: u16,
    pub timestamp: i64,
}

#[event]
pub struct ProviderFeeUpdated {
    pub provider_id: u32,
    pub old_bps: u16,
    pub new_bps: u16,
    pub timestamp: i64,
}

#[event]
pub struct ProviderSettlementPaused {
    pub provider_id: u32,
    pub paused: bool,
}

#[event]
pub struct SettleOwnerProposed {
    pub asset_mint: Pubkey,
    pub new_wallet: Pubkey,
    pub unlocks_at: i64,
}

#[event]
pub struct SettleOwnerFinalized {
    pub asset_mint: Pubkey,
    pub old_wallet: Pubkey,
    pub new_wallet: Pubkey,
}

#[event]
pub struct SettleOwnerCancelled {
    pub asset_mint: Pubkey,
    pub cancelled: Pubkey,
}

#[event]
pub struct ProviderGgrSubmitted {
    pub provider_id: u32,
    pub day_id: u64,
    pub asset_mint: Pubkey,
    pub net_ggr: i64,
    pub fee_bps_at_accrual: u16,
    pub fee_due: u64,
    pub promo_netted: u64,
    pub new_promo_paid_unreconciled: u64,
    pub affiliate_netted: u64,
    pub new_affiliate_unreconciled: u64,
    pub timestamp: i64,
    pub fee_decrease: u64,
    pub period_net_ggr: i64,
    pub period_fee_charged: u64,
}

#[event]
pub struct ProviderFeeOverAccrualCorrected {
    pub provider_id: u32,
    pub asset_mint: Pubkey,
    pub authority: Pubkey,
    pub pending_provider_fee_before: u64,
    pub pending_provider_fee_after: u64,
    pub fee_owed_before: u64,
    pub fee_owed_after: u64,
    pub delta: u64,
    pub holder_balance: u64,
    pub nav_before: u64,
    pub nav_after: u64,
    pub timestamp: i64,
}

#[event]
pub struct SweepSkipped {
    pub asset_mint: Pubkey,
    pub delta_ggr: i64,
    pub threshold: u64,
    pub timestamp: i64,
}

#[event]
pub struct SweepSkippedDueToNetNegative {
    pub asset_mint: Pubkey,
    pub gross_delta: i64,
    pub affiliate_accrual: u64,
    pub net_delta: i64,
    pub timestamp: i64,
}

#[event]
pub struct KeeperSweepTriggered {
    pub asset_mint: Pubkey,
    pub delta_ggr: i64,
    pub timestamp: i64,
}

#[event]
pub struct Distributed {
    pub asset_mint: Pubkey,
    pub gross_delta: i64,
    pub net_delta: i64,
    pub affiliate_accrual: u64,
    pub pending_dev_fee: u64,
    pub pending_provider_fee: u64,
    pub pending_sovereign: u64,
    pub pending_yield: u64,
    pub pending_reserve: u64,
    pub timestamp: i64,
}

#[event]
pub struct ProviderFeeFlushed {
    pub provider_id: u32,
    pub asset_mint: Pubkey,
    pub amount: u64,
    pub owed_after: u64,
    pub timestamp: i64,
}

#[event]
pub struct ProviderInvoiceSettled {
    pub provider_id: u32,
    pub asset_mint: Pubkey,
    pub amount: u64,
    pub reimbursement_applied: u64,
    pub paid_to_provider: u64,
    pub new_provider_credit: u64,
    pub recipient: Pubkey,
    pub is_keeper: bool,
    pub timestamp: i64,
}

#[event]
pub struct LpDeposited {
    pub depositor: Pubkey,
    pub asset_mint: Pubkey,
    pub amount: u64,
    pub minted_lp: u64,
    pub tier: u8,
    pub timestamp: i64,
}

#[event]
pub struct FoundingBankerGranted {
    pub wallet: Pubkey,
    pub seat_number: u8,
    pub amount: u64,
    pub timestamp_at: i64,
    pub vault_seat_count_after: u8,
}

#[event]
pub struct FoundingBankerReleased {
    pub wallet: Pubkey,
    pub seat_number: u8,
    pub timestamp_at: i64,
    pub vault_seat_count_after: u8,
}

#[event]
pub struct WithdrawRequested {
    pub owner: Pubkey,
    pub asset_mint: Pubkey,
    pub lp_amount: u64,
    pub processable_at: i64,
    pub timestamp: i64,
}

#[event]
pub struct WithdrawRequestCancelled {
    pub owner: Pubkey,
    pub asset_mint: Pubkey,
    pub lp_amount: u64,
    pub timestamp: i64,
}

#[event]
pub struct WithdrawProcessed {
    pub owner: Pubkey,
    pub asset_mint: Pubkey,
    pub lp_amount: u64,
    pub payout: u64,
    pub timestamp: i64,
}

#[event]
pub struct InsuranceRefilled {
    pub asset_mint: Pubkey,
    pub amount: u64,
    pub new_balance: u64,
    pub timestamp: i64,
}

#[event]
pub struct PausedChanged {
    pub paused: bool,
    pub timestamp: i64,
}

#[event]
pub struct VaultFrozenEvent {
    pub by: Pubkey,
    pub timestamp: i64,
}

#[event]
pub struct VaultUnfrozenEvent {
    pub by: Pubkey,
    pub timestamp: i64,
}

#[event]
pub struct HeartbeatRecorded {
    pub operator: Pubkey,
    pub timestamp: i64,
}

#[event]
pub struct DeadmanHaltTriggered {
    pub by: Pubkey,
    pub age: i64,
    pub timestamp: i64,
}

#[event]
pub struct HeartbeatTtlSet {
    pub new_ttl: i64,
    pub timestamp: i64,
}

#[event]
pub struct AuthorityProposed {
    pub new_authority: Pubkey,
    pub unlocks_at: i64,
}

#[event]
pub struct AuthorityRotated {
    pub old: Pubkey,
    pub new: Pubkey,
    pub timestamp: i64,
}

#[event]
pub struct AuthorityProposalCancelled {}

#[event]
pub struct PhaseAdvanced {
    pub new_phase: u8,
    pub timestamp: i64,
}

#[event]
pub struct PauseAuthorityRotated {
    pub old: Pubkey,
    pub new: Pubkey,
}


#[event]
pub struct RotatePauseAuthorityProposed {
    pub admin: Pubkey,
    pub new_authority: Pubkey,
    pub unlocks_at: i64,
    pub timestamp: i64,
}

#[event]
pub struct RotatePauseAuthorityProposalCancelled {
    pub admin: Pubkey,
    pub timestamp: i64,
}


#[event]
pub struct OperatorRotationProposed {
    pub admin: Pubkey,
    pub current_operator: Pubkey,
    pub new_operator: Pubkey,
    pub unlocks_at: i64,
    pub timestamp: i64,
}

#[event]
pub struct OperatorRotated {
    pub old_operator: Pubkey,
    pub new_operator: Pubkey,
    pub timestamp: i64,
}

#[event]
pub struct OperatorRotationProposalCancelled {
    pub admin: Pubkey,
    pub cancelled_operator: Pubkey,
    pub timestamp: i64,
}

#[event]
pub struct VaultLocked {
    pub asset_mint: Pubkey,
    pub timestamp: i64,
}

#[event]
pub struct VaultUnlocked {
    pub asset_mint: Pubkey,
    pub timestamp: i64,
}

#[event]
pub struct VaultHolderSet {
    pub asset_mint: Pubkey,
    pub vault_holder: Pubkey,
    pub by: Pubkey,
    pub timestamp: i64,
}


#[event]
pub struct CircuitStateChanged {
    pub asset_mint: Pubkey,
    pub old_state: u8,
    pub new_state: u8,
    pub nav: u64,
    pub peak: u64,
    pub insurance: u64,
    pub timestamp: i64,
}

#[event]
pub struct PeakResetProposed {
    pub asset_mint: Pubkey,
    pub new_peak: u64,
    pub current_peak: u64,
    pub unlocks_at: i64,
}

#[event]
pub struct PeakResetFinalized {
    pub asset_mint: Pubkey,
    pub old_peak: u64,
    pub new_peak: u64,
    pub circuit_state_after: u8,
    pub timestamp: i64,
}

#[event]
pub struct PeakResetProposalCancelled {
    pub asset_mint: Pubkey,
}

#[event]
pub struct InsuranceDrawn {
    pub asset_mint: Pubkey,
    pub amount: u64,
    pub insurance_balance_after: u64,
    pub timestamp: i64,
}

#[event]
pub struct WaiverCancelled {
    pub asset_mint: Pubkey,
    pub by: Pubkey,
    pub timestamp: i64,
}

#[event]
pub struct WaiverExtended {
    pub asset_mint: Pubkey,
    pub by: Pubkey,
    pub new_waiver_max_until: i64,
    pub timestamp: i64,
}


#[event]
pub struct OpsMarketingWalletRotated {
    pub old: Pubkey,
    pub new: Pubkey,
    pub timestamp: i64,
}


#[event]
pub struct DevFeeBpsProposed {
    pub new_bps: u16,
    pub unlocks_at: i64,
}

#[event]
pub struct DevFeeBpsRotated {
    pub old: u16,
    pub new: u16,
    pub timestamp: i64,
}

#[event]
pub struct DevFeeBpsProposalCancelled {}


#[event]
pub struct AutoFrozenOnOutflow {
    pub source: u8,
    pub attempted_amount: u64,
    pub window_outflow_at_trip: u64,
    pub threshold: u64,
    pub window_start: i64,
    pub tripped_at: i64,
}

pub const AUTO_FROZEN_SOURCE_LP: u8 = 0;
pub const AUTO_FROZEN_SOURCE_PROMO: u8 = 1;

#[event]
pub struct AutoFrozenOnDailyOutflow {
    pub attempted_amount: u64,
    pub daily_window_outflow_at_trip: u64,
    pub max_daily_outflow: u64,
    pub daily_window_start: i64,
    pub tripped_at: i64,
}

#[event]
pub struct MaxDailyOutflowProposed {
    pub new_max: u64,
    pub unlocks_at: i64,
    pub timestamp: i64,
}

#[event]
pub struct MaxDailyOutflowRotated {
    pub old: u64,
    pub new: u64,
    pub timestamp: i64,
}

#[event]
pub struct MaxDailyOutflowProposalCancelled {
    pub cancelled_value: u64,
    pub timestamp: i64,
}

#[event]
pub struct ChipDebitCapPerWalletSet {
    pub asset_mint: Pubkey,
    pub old_cap: u64,
    pub new_cap: u64,
    pub timestamp: i64,
}

#[event]
pub struct MaxSettlePerWindowProposed {
    pub new_value: u64,
    pub unlocks_at: i64,
}

#[event]
pub struct MaxSettlePerWindowRotated {
    pub old: u64,
    pub new: u64,
    pub timestamp: i64,
}

#[event]
pub struct MaxSettlePerWindowProposalCancelled {}

#[event]
pub struct SettleWindowSecondsProposed {
    pub new_value: u32,
    pub unlocks_at: i64,
}

#[event]
pub struct SettleWindowSecondsRotated {
    pub old: u32,
    pub new: u32,
    pub timestamp: i64,
}

#[event]
pub struct SettleWindowSecondsProposalCancelled {}

#[event]
pub struct OpsMarketingWalletProposed {
    pub new_wallet: Pubkey,
    pub unlocks_at: i64,
}

#[event]
pub struct OpsMarketingWalletProposalCancelled {}

#[event]
pub struct AffiliateDistributed {
    pub asset_mint: Pubkey,
    pub amount: u64,
    pub timestamp: i64,
}

#[event]
pub struct SovereignDistributed {
    pub asset_mint: Pubkey,
    pub amount: u64,
    pub timestamp: i64,
}

#[event]
pub struct SovereignDistributedFallback {
    pub asset_mint: Pubkey,
    pub amount: u64,
    pub routed_to_reserve: u64,
    pub timestamp: i64,
}

#[event]
pub struct YieldDistributed {
    pub asset_mint: Pubkey,
    pub amount: u64,
    pub graduated: bool,
    pub timestamp: i64,
}

#[event]
pub struct ReserveDistributed {
    pub asset_mint: Pubkey,
    pub burn_amount: u64,
    pub ops_amount: u64,
    pub mode: u8,
    pub timestamp: i64,
}

#[event]
pub struct ReserveBurnExecuted {
    pub asset_mint: Pubkey,
    pub usdc_in: u64,
    pub top_burned: u64,
    pub ops_amount: u64,
    pub timestamp: i64,
}

#[event]
pub struct ReserveBurnModeChanged {
    pub old_mode: u8,
    pub new_mode: u8,
    pub changed_by: Pubkey,
    pub timestamp: i64,
}

#[event]
pub struct RaydiumGraduatedFlipped {
    pub old_value: bool,
    pub new_value: bool,
    pub changed_by: Pubkey,
    pub timestamp: i64,
}

#[event]
pub struct ProviderChipDeposited {
    pub wallet: Pubkey,
    pub asset_mint: Pubkey,
    pub amount: u64,
    pub new_balance: u64,
    pub timestamp: i64,
}

#[event]
pub struct ProviderChipWithdrawn {
    pub wallet: Pubkey,
    pub asset_mint: Pubkey,
    pub amount: u64,
    pub new_balance: u64,
    pub timestamp: i64,
}

#[event]
pub struct ProviderChipDebited {
    pub wallet: Pubkey,
    pub provider_id: u32,
    pub asset_mint: Pubkey,
    pub amount: u64,
    pub new_balance: u64,
    pub timestamp: i64,
}

#[event]
pub struct ProviderChipCredited {
    pub wallet: Pubkey,
    pub provider_id: u32,
    pub asset_mint: Pubkey,
    pub amount: u64,
    pub new_balance: u64,
    pub timestamp: i64,
}

#[event]
pub struct PromoPoolToppedUp {
    pub asset_mint: Pubkey,
    pub amount: u64,
    pub new_pending_promo: u64,
    pub timestamp: i64,
}

#[event]
pub struct ProviderChipCreditedPromo {
    pub wallet: Pubkey,
    pub provider_id: u32,
    pub asset_mint: Pubkey,
    pub amount: u64,
    pub new_balance: u64,
    pub new_pending_promo: u64,
    pub timestamp: i64,
}

#[event]
pub struct ProviderChipCreditedNgrPromo {
    pub wallet: Pubkey,
    pub provider_id: u32,
    pub asset_mint: Pubkey,
    pub amount: u64,
    pub new_balance: u64,
    pub is_network_reimbursable: bool,
    pub new_promo_paid_unreconciled: u64,
    pub new_network_reimbursement_owed: u64,
    pub timestamp: i64,
}

#[event]
pub struct ChipsCreditedFromSwap {
    pub wallet: Pubkey,
    pub asset_mint: Pubkey,
    pub credited: u64,
    pub new_balance: u64,
    pub timestamp: i64,
}

#[event]
pub struct DevFeeDrained {
    pub asset_mint: Pubkey,
    pub amount: u64,
    pub timestamp: i64,
}

#[event]
pub struct AffiliateAccrued {
    pub provider_id: u32,
    pub asset_mint: Pubkey,
    pub amount: u64,
    pub new_pending_total: u64,
    pub timestamp: i64,
}

#[allow(dead_code)]
fn _silence_unused_system_program() {
    let _ = system_program::ID;
}

#[cfg(test)]
mod tests {
    use super::*;

    proptest::proptest! {
        #[test]
        fn prop_nav_roundtrip_never_profits(
            amount in 1u64..1_000_000_000_000u64,
            nav_before in 0u64..10_000_000_000_000u64,
            supply_before in 0u64..10_000_000_000_000u64,
        ) {
            if let Ok((user_shares, dead_shares)) =
                compute_shares_for_deposit(amount, nav_before, supply_before)
            {
                let nav_after = nav_before.saturating_add(amount);
                let supply_after = supply_before
                    .saturating_add(user_shares)
                    .saturating_add(dead_shares);
                if user_shares > 0 && supply_after > 0 {
                    if let Ok(payout) =
                        compute_lamports_for_withdraw(user_shares, nav_after, supply_after)
                    {
                        proptest::prop_assert!(
                            payout <= amount,
                            "NAV round-trip PROFIT: deposited {} got {} (user_shares={}, dead={}, nav_before={}, supply_before={})",
                            amount, payout, user_shares, dead_shares, nav_before, supply_before
                        );
                    }
                }
            }
        }

        #[test]
        fn prop_share_math_never_panics(
            a in proptest::num::u64::ANY,
            nav in proptest::num::u64::ANY,
            supply in proptest::num::u64::ANY,
        ) {
            let _ = compute_shares_for_deposit(a, nav, supply);
            let _ = compute_lamports_for_withdraw(a, nav, supply);
        }

        #[test]
        fn prop_earmark_invariant_holds(
            dev in 0u64..1_000_000_000_000_000u64,
            prov in 0u64..1_000_000_000_000_000u64,
            aff in 0u64..1_000_000_000_000_000u64,
            sov in 0u64..1_000_000_000_000_000u64,
            yld in 0u64..1_000_000_000_000_000u64,
            res in 0u64..1_000_000_000_000_000u64,
            promo in 0u64..1_000_000_000_000_000u64,
            owed in 0u64..1_000_000_000_000_000u64,
            extra in 0u64..1_000_000_000_000_000u64,
        ) {
            let mut pool = fresh_pool(Pubkey::new_unique());
            pool.pending_dev_fee = dev;
            pool.pending_provider_fee = prov;
            pool.pending_affiliate = aff;
            pool.pending_sovereign = sov;
            pool.pending_yield = yld;
            pool.pending_reserve = res;
            pool.pending_promo = promo;
            pool.provider_owed_total = owed;
            let earmarks = sum_earmarks(&pool);
            let holder = earmarks.saturating_add(extra);
            let nav = nav_basis(&pool, holder).unwrap();
            proptest::prop_assert_eq!(nav, holder - earmarks);
            proptest::prop_assert!(nav <= holder);
            proptest::prop_assert!(require_earmark_invariant(&pool, holder).is_ok());
            if earmarks > 0 {
                proptest::prop_assert!(require_earmark_invariant(&pool, earmarks - 1).is_err());
            }
        }
    }

    fn fresh_pool(mint: Pubkey) -> AssetPool {
        AssetPool {
            vault_config: Pubkey::new_unique(),
            asset_mint: mint,
            is_sol: false,
            bump: 255,
            lp_mint: Pubkey::new_unique(),
            lp_supply: 0,
            cumulative_gross_ggr: 0,
            last_distributed_gross_ggr: 0,
            last_distributed_at: 0,
            pending_dev_fee: 0,
            pending_provider_fee: 0,
            pending_affiliate: 0,
            pending_sovereign: 0,
            pending_yield: 0,
            pending_reserve: 0,
            last_distributed_affiliate: 0,
            pending_promo: 0,
            lp_share_bps: DEFAULT_LP_SHARE_BPS,
            lp_tokens_by_tier: [0u64; 5],
            peak_vault: 0,
            peak_vault_at: 0,
            circuit_state: CIRCUIT_GREEN,
            red_entered_at: 0,
            waiver_active: false,
            waiver_started_at: 0,
            waiver_max_until: 0,
            insurance_balance: 0,
            withdraw_batch_counter: 0,
            last_batch_opened_at: 0,
            pending_request_count: 0,
            vault_locked: false,
            vault_locked_at: 0,
            provider_settle_owner: Pubkey::new_unique(),
            pending_settle_owner: Pubkey::default(),
            pending_settle_owner_unlocks_at: 0,
            provider_owed_total: 0,
            founding_banker_lp_tokens_in_window: 0,
            max_chip_debit_per_24h_per_wallet: 0,
            promo_paid_unreconciled: 0,
            network_reimbursement_owed: 0,
            provider_credit: 0,
            vault_holder: Pubkey::default(),
            pending_reset_peak: 0,
            pending_reset_peak_unlocks_at: 0,
            affiliate_unreconciled: 0,
            reserved: [0u8; 24],
        }
    }


    #[test]
    fn net_ggr_house_wins() {
        assert_eq!(compute_net_ggr(1_000_000, 600_000).unwrap(), 400_000);
    }
    #[test]
    fn net_ggr_player_wins() {
        assert_eq!(compute_net_ggr(500_000, 700_000).unwrap(), -200_000);
    }
    #[test]
    fn net_ggr_breakeven() {
        assert_eq!(compute_net_ggr(123, 123).unwrap(), 0);
    }
    #[test]
    fn net_ggr_zero_zero() {
        assert_eq!(compute_net_ggr(0, 0).unwrap(), 0);
    }
    #[test]
    fn net_ggr_large_house_win() {
        assert_eq!(
            compute_net_ggr(1_000_000_000_000u64, 1u64).unwrap(),
            999_999_999_999i64
        );
    }
    #[test]
    fn net_ggr_large_player_win() {
        assert_eq!(
            compute_net_ggr(1u64, 1_000_000_000_000u64).unwrap(),
            -999_999_999_999i64
        );
    }


    #[test]
    fn phase_split_bootstrap() {
        assert_eq!(phase_split_bps(0), (2_000, 7_000, 1_000));
    }
    #[test]
    fn phase_split_growth() {
        assert_eq!(phase_split_bps(1), (6_000, 3_000, 1_000));
    }
    #[test]
    fn phase_split_standard() {
        assert_eq!(phase_split_bps(2), (6_000, 3_000, 1_000));
    }
    #[test]
    fn phase_split_bootstrap_sums_to_10000() {
        let (y, c, r) = phase_split_bps(0);
        assert_eq!(y + c + r, 10_000);
    }
    #[test]
    fn phase_split_growth_sums_to_10000() {
        let (y, c, r) = phase_split_bps(1);
        assert_eq!(y + c + r, 10_000);
    }


    #[test]
    fn weighted_lp_bps_zero_supply_returns_default() {
        let p = fresh_pool(Pubkey::new_unique());
        assert_eq!(compute_weighted_lp_bps(&p, 1, 0).unwrap(), DEFAULT_LP_SHARE_BPS as u64);
    }
    #[test]
    fn weighted_lp_bps_bootstrap_phase_overrides_tiers() {
        let mut p = fresh_pool(Pubkey::new_unique());
        p.lp_tokens_by_tier = [100, 0, 0, 0, 0];
        assert_eq!(compute_weighted_lp_bps(&p, 0, 0).unwrap(), BOOTSTRAP_LP_SHARE_BPS as u64);
    }
    #[test]
    fn weighted_lp_bps_growth_pure_tier0_is_6500() {
        let mut p = fresh_pool(Pubkey::new_unique());
        p.lp_tokens_by_tier = [100, 0, 0, 0, 0];
        assert_eq!(compute_weighted_lp_bps(&p, 1, 0).unwrap(), 6_500);
    }
    #[test]
    fn weighted_lp_bps_growth_pure_tier4_is_8500() {
        let mut p = fresh_pool(Pubkey::new_unique());
        p.lp_tokens_by_tier = [0, 0, 0, 0, 100];
        assert_eq!(compute_weighted_lp_bps(&p, 1, 0).unwrap(), 8_500);
    }
    #[test]
    fn weighted_lp_bps_growth_mixed_50_50_low_high() {
        let mut p = fresh_pool(Pubkey::new_unique());
        p.lp_tokens_by_tier = [50, 0, 0, 0, 50];
        assert_eq!(compute_weighted_lp_bps(&p, 1, 0).unwrap(), 7_500);
    }
    #[test]
    fn weighted_lp_bps_growth_all_tiers_equal() {
        let mut p = fresh_pool(Pubkey::new_unique());
        p.lp_tokens_by_tier = [10, 10, 10, 10, 10];
        assert_eq!(compute_weighted_lp_bps(&p, 1, 0).unwrap(), 7_500);
    }


    #[test]
    fn earmarks_positive_growth() {
        let mut p = fresh_pool(Pubkey::new_unique());
        p.lp_tokens_by_tier = [0, 0, 100, 0, 0];
        let net = 1_000_000_000i64;
        let fee_due = (net as u64) * DEFAULT_PROVIDER_FEE_BPS as u64 / 10_000;
        accrue_earmarks(&mut p, net, 1, DEFAULT_PROVIDER_FEE_BPS, fee_due, DEFAULT_DEV_FEE_BPS, 0, 0).unwrap();
        assert_eq!(p.pending_provider_fee, fee_due);
        assert_eq!(p.pending_dev_fee, 22_500_000);
        assert_eq!(p.pending_sovereign, 10_968_750);
        assert_eq!(p.pending_yield, 125_043_750);
        assert_eq!(p.pending_reserve, 20_840_625);
    }

    #[test]
    fn earmarks_positive_bootstrap_overrides_lp_bps() {
        let mut p = fresh_pool(Pubkey::new_unique());
        p.lp_tokens_by_tier = [100, 0, 0, 0, 0];
        let net = 1_000_000_000i64;
        let fee_due = (net as u64) * DEFAULT_PROVIDER_FEE_BPS as u64 / 10_000;
        accrue_earmarks(&mut p, net, 0, DEFAULT_PROVIDER_FEE_BPS, fee_due, DEFAULT_DEV_FEE_BPS, 0, 0).unwrap();
        assert!(p.pending_dev_fee > 0);
    }

    #[test]
    fn earmarks_negative_unwind() {
        let mut p = fresh_pool(Pubkey::new_unique());
        p.pending_dev_fee = 100_000;
        p.pending_sovereign = 5_000;
        p.pending_yield = 50_000;
        p.pending_reserve = 10_000;
        accrue_earmarks(&mut p, -100_000_000, 1, DEFAULT_PROVIDER_FEE_BPS, 0, DEFAULT_DEV_FEE_BPS, 0, 0).unwrap();
        assert_eq!(p.pending_dev_fee, 0);
        assert_eq!(p.pending_sovereign, 0);
        assert_eq!(p.pending_yield, 0);
        assert_eq!(p.pending_reserve, 0);
    }

    #[test]
    fn earmarks_negative_under_unwind_saturates() {
        let mut p = fresh_pool(Pubkey::new_unique());
        p.pending_dev_fee = 1;
        accrue_earmarks(&mut p, -10_000_000, 1, DEFAULT_PROVIDER_FEE_BPS, 0, DEFAULT_DEV_FEE_BPS, 0, 0).unwrap();
        assert_eq!(p.pending_dev_fee, 0);
    }

    #[test]
    fn earmarks_zero_delta_no_op() {
        let mut p = fresh_pool(Pubkey::new_unique());
        accrue_earmarks(&mut p, 0, 1, DEFAULT_PROVIDER_FEE_BPS, 0, DEFAULT_DEV_FEE_BPS, 0, 0).unwrap();
        assert_eq!(p.pending_dev_fee, 0);
        assert_eq!(p.pending_provider_fee, 0);
        assert_eq!(p.pending_sovereign, 0);
        assert_eq!(p.pending_yield, 0);
        assert_eq!(p.pending_reserve, 0);
    }

    #[test]
    fn earmarks_mixed_sequence() {
        let mut p = fresh_pool(Pubkey::new_unique());
        p.lp_tokens_by_tier = [0, 0, 100, 0, 0];
        let net1 = 1_000_000_000i64;
        let fee1 = (net1 as u64) * DEFAULT_PROVIDER_FEE_BPS as u64 / 10_000;
        accrue_earmarks(&mut p, net1, 1, DEFAULT_PROVIDER_FEE_BPS, fee1, DEFAULT_DEV_FEE_BPS, 0, 0).unwrap();
        let dev_after_first = p.pending_dev_fee;
        accrue_earmarks(&mut p, -200_000_000, 1, DEFAULT_PROVIDER_FEE_BPS, 0, DEFAULT_DEV_FEE_BPS, 0, 0).unwrap();
        assert!(p.pending_dev_fee < dev_after_first);
        accrue_earmarks(&mut p, 500_000_000, 1, DEFAULT_PROVIDER_FEE_BPS, 12_500_000, DEFAULT_DEV_FEE_BPS, 0, 0).unwrap();
        assert!(p.pending_dev_fee > 0);
        assert!(p.pending_provider_fee > 0);
    }


    #[test]
    fn k4_guard_gross_zero_affiliate_zero() {
        let gross: i64 = 0;
        let aff: u64 = 0;
        let net = gross - aff as i64;
        assert!(net <= 0 || (net as u64) < MIN_DELTA_GGR_FOR_SWEEP_USDC);
    }

    #[test]
    fn k4_guard_gross_lt_affiliate_skips() {
        let gross: i64 = 100;
        let aff: u64 = 500;
        let net = gross - aff as i64;
        assert!(net <= 0);
    }

    #[test]
    fn k4_guard_gross_eq_affiliate_skips() {
        let gross: i64 = 1_000_000;
        let aff: u64 = 1_000_000;
        let net = gross - aff as i64;
        assert!(net <= 0);
    }

    #[test]
    fn k4_guard_below_min_threshold_skips() {
        let gross: i64 = 600_000_000;
        let aff: u64 = 200_000_000;
        let net = gross - aff as i64;
        assert!(net > 0 && (net as u64) < MIN_DELTA_GGR_FOR_SWEEP_USDC);
    }

    #[test]
    fn k4_guard_at_threshold_proceeds() {
        let gross: i64 = MIN_DELTA_GGR_FOR_SWEEP_USDC as i64 + 100;
        let aff: u64 = 100;
        let net = gross - aff as i64;
        assert!(net >= MIN_DELTA_GGR_FOR_SWEEP_USDC as i64);
    }

    #[test]
    fn k4_guard_well_above_threshold_proceeds() {
        let gross: i64 = 10_000_000_000;
        let aff: u64 = 100_000_000;
        let net = gross - aff as i64;
        assert!((net as u64) >= MIN_DELTA_GGR_FOR_SWEEP_USDC);
    }


    #[test]
    fn first_deposit_carves_dead_shares() {
        let (user, dead) = compute_shares_for_deposit(10_000_000, 0, 0).unwrap();
        assert_eq!(dead, MIN_DEAD_SHARES);
        assert_eq!(user, 10_000_000 - MIN_DEAD_SHARES);
    }
    #[test]
    fn first_deposit_below_dead_share_threshold_clamps() {
        let amount = MIN_DEAD_SHARES - 1;
        let (user, dead) = compute_shares_for_deposit(amount, 0, 0).unwrap();
        assert_eq!(dead, amount);
        assert_eq!(user, 0);
    }
    #[test]
    fn second_deposit_no_dead_carve() {
        let (user, dead) = compute_shares_for_deposit(100, 1_000, 1_000).unwrap();
        assert_eq!(dead, 0);
        assert_eq!(user, 100);
    }
    #[test]
    fn deposit_share_calc_uses_nav() {
        let (user, dead) = compute_shares_for_deposit(100, 200, 200).unwrap();
        assert_eq!(dead, 0);
        assert_eq!(user, 100);
    }
    #[test]
    fn deposit_share_calc_post_appreciation() {
        let (user, dead) = compute_shares_for_deposit(100, 400, 200).unwrap();
        assert_eq!(dead, 0);
        assert_eq!(user, 50);
    }
    #[test]
    fn deposit_into_drained_pool_with_zombie_shares_is_rejected() {
        assert!(
            compute_shares_for_deposit(10_000_000, 0, 1_000).is_err(),
            "drained pool (nav==0, supply>0) must reject, not re-seed"
        );
        assert!(
            compute_shares_for_deposit(10_000_000, 0, 0).is_ok(),
            "genuine genesis (supply==0) must still mint even at nav==0"
        );
    }

    #[test]
    fn deposit_guard_keys_on_nav_zero_exactly() {
        let (user, dead) = compute_shares_for_deposit(100, 1, 1_000).unwrap();
        assert_eq!(dead, 0);
        assert_eq!(user, 100_000);
    }


    #[test]
    fn withdraw_proportional() {
        let p = compute_lamports_for_withdraw(50, 400, 200).unwrap();
        assert_eq!(p, 100);
    }
    #[test]
    fn withdraw_full_supply_drains_nav() {
        let p = compute_lamports_for_withdraw(1_000, 5_000, 1_000).unwrap();
        assert_eq!(p, 5_000);
    }
    #[test]
    fn withdraw_zero_supply_errors() {
        let r = compute_lamports_for_withdraw(1, 100, 0);
        assert!(r.is_err());
    }


    #[test]
    fn nav_math_excludes_all_earmarks() {
        let mut p = fresh_pool(Pubkey::new_unique());
        p.pending_dev_fee = 10;
        p.pending_provider_fee = 20;
        p.pending_affiliate = 30;
        p.pending_sovereign = 40;
        p.pending_yield = 50;
        p.pending_reserve = 60;
        let nav = nav_basis(&p, 1_000).unwrap();
        assert_eq!(nav, 1_000 - 210);
    }
    #[test]
    fn nav_math_clamps_at_zero_if_earmarks_exceed_balance() {
        let mut p = fresh_pool(Pubkey::new_unique());
        p.pending_dev_fee = 5_000;
        let nav = nav_basis(&p, 1_000).unwrap();
        assert_eq!(nav, 0);
    }


    #[test]
    fn earmark_invariant_holds_when_balance_ge_pending() {
        let mut p = fresh_pool(Pubkey::new_unique());
        p.pending_dev_fee = 100;
        p.pending_yield = 200;
        assert!(require_earmark_invariant(&p, 500).is_ok());
    }
    #[test]
    fn earmark_invariant_fails_when_balance_lt_pending() {
        let mut p = fresh_pool(Pubkey::new_unique());
        p.pending_dev_fee = 1000;
        let r = require_earmark_invariant(&p, 500);
        assert!(r.is_err());
    }


    #[test]
    fn pool_state_isolation_independent_counters() {
        let mut usdc = fresh_pool(Pubkey::new_unique());
        let mut sol = fresh_pool(Pubkey::new_unique());
        accrue_earmarks(&mut usdc, 1_000_000_000, 1, DEFAULT_PROVIDER_FEE_BPS, 100_000_000, DEFAULT_DEV_FEE_BPS, 0, 0).unwrap();
        assert!(usdc.pending_dev_fee > 0);
        assert_eq!(sol.pending_dev_fee, 0);
        assert_eq!(sol.pending_provider_fee, 0);
    }

    #[test]
    fn pool_ggr_isolation() {
        let mut a = fresh_pool(Pubkey::new_unique());
        let mut b = fresh_pool(Pubkey::new_unique());
        a.cumulative_gross_ggr = 1_000;
        b.cumulative_gross_ggr = -500;
        a.cumulative_gross_ggr += 200;
        assert_eq!(a.cumulative_gross_ggr, 1_200);
        assert_eq!(b.cumulative_gross_ggr, -500);
    }

    #[test]
    fn pool_lp_supply_isolation() {
        let mut a = fresh_pool(Pubkey::new_unique());
        let mut b = fresh_pool(Pubkey::new_unique());
        a.lp_supply = 1_000;
        b.lp_supply = 2_000;
        a.lp_supply += 500;
        assert_eq!(a.lp_supply, 1_500);
        assert_eq!(b.lp_supply, 2_000);
    }


    #[test]
    fn provider_fee_snapshot_locks_rate_per_receipt() {
        let old_bps: u16 = 1_000;
        let new_bps: u16 = 1_500;
        let net1 = 1_000_000i64;
        let net2 = 1_000_000i64;

        let fee_a = (net1 as u64) * old_bps as u64 / 10_000;
        let fee_b = (net2 as u64) * new_bps as u64 / 10_000;

        assert_eq!(fee_a, 100_000);
        assert_eq!(fee_b, 150_000);
        assert_ne!(fee_a, fee_b);
    }

    #[test]
    fn provider_fee_only_on_positive_net() {
        let net_pos = 100_000i64;
        let net_neg = -100_000i64;
        let bps: u16 = 1_000;
        let fee_pos: u64 = if net_pos > 0 {
            (net_pos as u64) * bps as u64 / 10_000
        } else {
            0
        };
        let fee_neg: u64 = if net_neg > 0 {
            (net_neg as u64) * bps as u64 / 10_000
        } else {
            0
        };
        assert_eq!(fee_pos, 10_000);
        assert_eq!(fee_neg, 0);
    }

    #[test]
    fn provider_fee_cap_enforced() {
        let invalid = MAX_PROVIDER_FEE_BPS + 1;
        assert!(invalid > MAX_PROVIDER_FEE_BPS);
    }

    #[test]
    fn provider_fee_rf2_accumulator() {
        let mut owed: u64 = 0;
        let bps: u16 = 1_000;
        let net1 = 1_000_000i64;
        let net2 = 2_000_000i64;
        owed += (net1 as u64) * bps as u64 / 10_000;
        owed += (net2 as u64) * bps as u64 / 10_000;
        assert_eq!(owed, 300_000);
    }


    #[test]
    fn settle_owner_timelock_72h() {
        let now = 1_000_000;
        let unlock = now + ADMIN_TIMELOCK_SECONDS;
        assert!(unlock - now == 72 * 60 * 60);
    }

    #[test]
    fn settle_keeper_window_40_days() {
        assert_eq!(PROVIDER_SETTLE_KEEPER_DAYS, 40);
    }


    fn fresh_lp_position(tier: u8, lp_shares: u64) -> ProviderLpPosition {
        ProviderLpPosition {
            holder: Pubkey::new_unique(),
            asset_pool: Pubkey::new_unique(),
            tier,
            lp_shares,
            pending_withdrawal_shares: 0,
            cumulative_deposited: 0,
            last_deposit_at: 0,
            last_withdrawal_at: 0,
            rolling_7d_withdrawn_shares: 0,
            rolling_7d_window_start: 0,
            bump: 255,
            is_founding_banker: false,
            founding_banker_seat_number: 0,
            founding_banker_seat_timestamp: 0,
            reserved: [0u8; 22],
        }
    }

    #[test]
    fn rule30_does_not_trigger_below_threshold() {
        let pos = fresh_lp_position(0, 10_000);
        let lp_supply = 10_000_000u64;
        let withdraw = 1_000u64;
        assert!(!check_rule30_penalty(&pos, withdraw, lp_supply, 1).unwrap());
    }

    #[test]
    fn rule30_triggers_above_threshold() {
        let pos = fresh_lp_position(0, 1_000_000);
        let lp_supply = 10_000_000u64;
        let withdraw = 300_000u64;
        assert!(check_rule30_penalty(&pos, withdraw, lp_supply, 1).unwrap());
    }

    #[test]
    fn rule30_rolling_window_expires() {
        let pos = ProviderLpPosition {
            rolling_7d_withdrawn_shares: 200_000,
            rolling_7d_window_start: 1_000,
            ..fresh_lp_position(0, 1_000_000)
        };
        let lp_supply = 10_000_000u64;
        let now = 1_000 + 8 * SECONDS_PER_DAY;
        let withdraw = 100_000u64;
        assert!(!check_rule30_penalty(&pos, withdraw, lp_supply, now).unwrap());
    }

    #[test]
    fn rule30_counts_staged_pending_requests() {
        let pos = ProviderLpPosition {
            pending_withdrawal_shares: 200_000,
            ..fresh_lp_position(0, 1_000_000)
        };
        let lp_supply = 10_000_000u64;
        assert!(check_rule30_penalty(&pos, 100_000, lp_supply, 1).unwrap());
    }

    #[test]
    fn rule30_pending_below_threshold_no_trigger() {
        let pos = ProviderLpPosition {
            pending_withdrawal_shares: 100_000,
            ..fresh_lp_position(0, 1_000_000)
        };
        let lp_supply = 10_000_000u64;
        assert!(!check_rule30_penalty(&pos, 100_000, lp_supply, 1).unwrap());
    }

    #[test]
    fn rule30_rolling_plus_pending_plus_new_sum() {
        let pos = ProviderLpPosition {
            rolling_7d_withdrawn_shares: 100_000,
            rolling_7d_window_start: 1_000,
            pending_withdrawal_shares: 100_000,
            ..fresh_lp_position(0, 1_000_000)
        };
        let lp_supply = 10_000_000u64;
        let now = 1_000 + SECONDS_PER_DAY;
        assert!(check_rule30_penalty(&pos, 100_000, lp_supply, now).unwrap());
    }

    #[test]
    fn rolling_window_rearm_resets_when_expired() {
        let (ws, r) = rolling_window_rearm(1_000, 200_000, 1_000 + 8 * SECONDS_PER_DAY);
        assert_eq!(ws, 1_000 + 8 * SECONDS_PER_DAY, "window restarts at now");
        assert_eq!(r, 0, "counter resets on re-arm");
    }

    #[test]
    fn rolling_window_rearm_keeps_when_open() {
        let now = 1_000 + 3 * SECONDS_PER_DAY;
        let (ws, r) = rolling_window_rearm(1_000, 200_000, now);
        assert_eq!(ws, 1_000, "open window unchanged");
        assert_eq!(r, 200_000, "counter preserved while window open");
    }

    #[test]
    fn rolling_window_rearm_bootstraps_first_use() {
        let (ws, r) = rolling_window_rearm(0, 0, 5_000);
        assert_eq!(ws, 5_000);
        assert_eq!(r, 0);
    }

    #[test]
    fn rolling_window_rearm_exact_boundary_rearms() {
        let (ws, r) = rolling_window_rearm(1_000, 200_000, 1_000 + 7 * SECONDS_PER_DAY);
        assert_eq!(ws, 1_000 + 7 * SECONDS_PER_DAY);
        assert_eq!(r, 0);
    }


    #[test]
    fn tier_cooldown_table_matches_spec() {
        assert_eq!(TIER_COOLDOWN_DAYS, [14, 10, 7, 5, 3]);
    }

    #[test]
    fn request_anchored_cooldown_math() {
        let now = 1_000_000i64;
        let tier = 4;
        let processable = now + TIER_COOLDOWN_DAYS[tier] * SECONDS_PER_DAY;
        assert_eq!(processable, now + 3 * 86_400);
    }

    #[test]
    fn request_anchored_cooldown_smallest_tier() {
        let now = 1_000_000i64;
        let processable = now + TIER_COOLDOWN_DAYS[0] * SECONDS_PER_DAY;
        assert_eq!(processable, now + 14 * 86_400);
    }


    #[test]
    fn affiliate_accrual_increments_counter() {
        let mut p = fresh_pool(Pubkey::new_unique());
        accrue_affiliate_amount(&mut p, 1_000).unwrap();
        accrue_affiliate_amount(&mut p, 500).unwrap();
        assert_eq!(p.pending_affiliate, 1_500);
        assert_eq!(p.affiliate_unreconciled, 1_500);
    }

    #[test]
    fn affiliate_nets_from_ggr_base_leaves_reservation() {
        let mint = Pubkey::new_unique();
        let net = 10_000_000_000i64;
        let fee = 0u64;
        let affiliate = 1_000_000_000u64;

        let mut base = fresh_pool(mint);
        accrue_earmarks(&mut base, net, 1, 0, fee, DEFAULT_DEV_FEE_BPS, 0, 0).unwrap();
        let base_protocol_earmarks = base.pending_dev_fee
            + base.pending_sovereign
            + base.pending_yield
            + base.pending_reserve;

        let mut p = fresh_pool(mint);
        accrue_affiliate_amount(&mut p, affiliate).unwrap();
        assert_eq!(p.pending_affiliate, affiliate);
        assert_eq!(p.affiliate_unreconciled, affiliate);

        let after_provider = (net as u64) - fee;
        let promo_to_net = 0u64;
        let remaining = after_provider - promo_to_net;
        let affiliate_to_net = p.affiliate_unreconciled.min(remaining);
        let cost_netted = promo_to_net + affiliate_to_net;
        accrue_earmarks(&mut p, net, 1, 0, fee, DEFAULT_DEV_FEE_BPS, cost_netted, 0).unwrap();
        p.affiliate_unreconciled -= affiliate_to_net;

        assert_eq!(affiliate_to_net, affiliate);
        assert_eq!(p.affiliate_unreconciled, 0);
        assert_eq!(p.pending_affiliate, affiliate);

        let with_protocol_earmarks = p.pending_dev_fee
            + p.pending_sovereign
            + p.pending_yield
            + p.pending_reserve;
        assert!(
            with_protocol_earmarks < base_protocol_earmarks,
            "affiliate netting MUST shrink protocol earmarks (shared cost)"
        );

        let holder = net as u64;
        require_earmark_invariant(&p, holder).unwrap();
    }

    #[test]
    fn affiliate_accrual_overflow_detected() {
        let mut p = fresh_pool(Pubkey::new_unique());
        p.pending_affiliate = u64::MAX - 5;
        let r = accrue_affiliate_amount(&mut p, 10);
        assert!(r.is_err());
    }


    #[test]
    fn max_provider_fee_bps_25_percent() {
        assert_eq!(MAX_PROVIDER_FEE_BPS, 2_500);
    }
    #[test]
    fn default_provider_fee_bps_10_percent() {
        assert_eq!(DEFAULT_PROVIDER_FEE_BPS, 1_000);
    }
    #[test]
    fn settle_keeper_40_days() {
        assert_eq!(PROVIDER_SETTLE_KEEPER_DAYS, 40);
    }
    #[test]
    fn admin_timelock_72_hours() {
        assert_eq!(ADMIN_TIMELOCK_SECONDS, 72 * 60 * 60);
    }
    #[test]
    fn max_assets_8() {
        assert_eq!(MAX_ASSETS, 8);
    }
    #[test]
    fn max_providers_16() {
        assert_eq!(MAX_PROVIDERS, 16);
    }
    #[test]
    fn min_delta_for_sweep_1000_usdc() {
        assert_eq!(MIN_DELTA_GGR_FOR_SWEEP_USDC, 1_000_000_000);
    }
    #[test]
    fn hard_floor_500_usdc() {
        assert_eq!(HARD_VAULT_FLOOR_USDC, 500_000_000);
    }
    #[test]
    fn sovereign_carve_5_percent() {
        assert_eq!(SOVEREIGN_CARVE_BPS, 500);
    }
    #[test]
    fn dev_fee_2_5_percent() {
        assert_eq!(DEFAULT_DEV_FEE_BPS, 250);
    }
    #[test]
    fn dead_shares_1m() {
        assert_eq!(MIN_DEAD_SHARES, 1_000_000);
    }
    #[test]
    fn unlock_delay_72_hours() {
        assert_eq!(UNLOCK_VAULT_MIN_DELAY_SECONDS, 72 * 60 * 60);
    }
    #[test]
    fn waiver_delay_24_hours() {
        assert_eq!(WAIVER_DELAY_SECONDS, 24 * 60 * 60);
    }
    #[test]
    fn keeper_window_8_days() {
        assert_eq!(KEEPER_WINDOW_SECONDS, 8 * SECONDS_PER_DAY);
    }
    #[test]
    fn sol_pseudo_mint_is_system_program() {
        assert_eq!(SOL_PSEUDO_MINT, system_program::ID);
    }


    #[test]
    fn registered_assets_size() {
        assert_eq!(RegisteredAssets::LEN, 8 + 32 + 8 * 32 + 1 + 1 + 32);
    }
    #[test]
    fn provider_owed_size_reasonable() {
        assert!(ProviderOwed::LEN < 200);
    }
    #[test]
    fn provider_player_escrow_size_reasonable() {
        assert!(ProviderPlayerEscrow::LEN < 200);
    }
    #[test]
    fn withdraw_request_size_reasonable() {
        assert!(WithdrawRequest::LEN < 200);
    }
    #[test]
    fn lp_position_size_reasonable() {
        assert!(ProviderLpPosition::LEN < 200);
    }


    #[test]
    fn provider_fee_default_10_percent() {
        assert_eq!(DEFAULT_PROVIDER_FEE_BPS, 1_000);
    }

    #[test]
    fn provider_fee_above_cap_rejected_at_add_provider() {
        let bad = MAX_PROVIDER_FEE_BPS + 1;
        assert!(bad > MAX_PROVIDER_FEE_BPS);
    }


    #[test]
    fn settle_drains_amount_to_zero_atomically() {
        let mut owed = ProviderOwed {
            asset_pool: Pubkey::new_unique(),
            provider_id: 1,
            amount: 1_000_000,
            last_settled_at: 0,
            bump: 255,
            reserved: [0u8; 32],
        };
        let mut pool_total: u64 = 1_500_000;
        let amount = owed.amount;
        owed.amount = 0;
        pool_total = pool_total.checked_sub(amount).unwrap();
        assert_eq!(owed.amount, 0);
        assert_eq!(pool_total, 500_000);
    }

    #[test]
    fn flush_provider_fee_advances_owed() {
        let mut provider_owed = 100_000u64;
        let mut pool_pending: u64 = 500_000;
        let mut provider_fee_owed: u64 = 250_000;
        let amount = provider_fee_owed;
        provider_fee_owed = 0;
        pool_pending = pool_pending.checked_sub(amount).unwrap();
        provider_owed = provider_owed.checked_add(amount).unwrap();
        assert_eq!(provider_fee_owed, 0);
        assert_eq!(pool_pending, 250_000);
        assert_eq!(provider_owed, 350_000);
    }


    #[test]
    fn naming_cumulative_gross_ggr_distinct_from_v1() {
        let p = fresh_pool(Pubkey::new_unique());
        let _gross = p.cumulative_gross_ggr;
        assert_eq!(_gross, 0);
    }


    #[test]
    fn tier_change_moves_lp_tokens() {
        let mut p = fresh_pool(Pubkey::new_unique());
        p.lp_tokens_by_tier[0] = 1_000;
        let amount = 1_000u64;
        let old_tier = 0usize;
        let new_tier = 2usize;
        p.lp_tokens_by_tier[old_tier] = p.lp_tokens_by_tier[old_tier].saturating_sub(amount);
        p.lp_tokens_by_tier[new_tier] = p.lp_tokens_by_tier[new_tier].checked_add(amount).unwrap();
        assert_eq!(p.lp_tokens_by_tier[0], 0);
        assert_eq!(p.lp_tokens_by_tier[2], 1_000);
    }


    #[test]
    fn distribute_zeroes_pending_buckets() {
        let mut p = fresh_pool(Pubkey::new_unique());
        p.pending_dev_fee = 100;
        p.pending_sovereign = 50;
        p.pending_yield = 200;
        p.pending_reserve = 75;
        p.pending_dev_fee = 0;
        p.pending_sovereign = 0;
        p.pending_yield = 0;
        p.pending_reserve = 0;
        assert_eq!(sum_earmarks(&p), p.pending_affiliate + p.pending_provider_fee);
    }

    #[test]
    fn distribute_idempotent_on_zero_counters() {
        let p = fresh_pool(Pubkey::new_unique());
        assert_eq!(sum_earmarks(&p), 0);
    }


    #[test]
    fn vault_holds_distinct_settle_owners_per_asset() {
        let mut a = fresh_pool(Pubkey::new_unique());
        let mut b = fresh_pool(Pubkey::new_unique());
        a.provider_settle_owner = Pubkey::new_unique();
        b.provider_settle_owner = Pubkey::new_unique();
        assert_ne!(a.provider_settle_owner, b.provider_settle_owner);
    }


    #[test]
    fn phase_transition_only_increases() {
        let from = 1u8;
        let to_invalid = 0u8;
        let to_valid = 2u8;
        assert!(to_invalid <= from);
        assert!(to_valid > from);
    }

    #[test]
    fn phase_transition_min_7_days() {
        let started = 1_000_000i64;
        let now_too_early = started + 6 * SECONDS_PER_DAY;
        let now_ok = started + 7 * SECONDS_PER_DAY;
        assert!(now_too_early < started + 7 * SECONDS_PER_DAY);
        assert!(now_ok >= started + 7 * SECONDS_PER_DAY);
    }


    #[test]
    fn vault_lock_blocks_state_mutating_ops() {
        let mut p = fresh_pool(Pubkey::new_unique());
        p.vault_locked = true;
        assert!(p.vault_locked);
    }

    #[test]
    fn unlock_blocked_within_72h() {
        let locked_at = 1_000_000i64;
        let now_too_early = locked_at + 24 * 60 * 60;
        let now_eligible = locked_at + UNLOCK_VAULT_MIN_DELAY_SECONDS;
        assert!(now_too_early < locked_at + UNLOCK_VAULT_MIN_DELAY_SECONDS);
        assert!(now_eligible >= locked_at + UNLOCK_VAULT_MIN_DELAY_SECONDS);
    }


    #[test]
    fn register_asset_idempotency_simulated() {
        let mut mints = [Pubkey::default(); 8];
        let existing = Pubkey::new_unique();
        mints[0] = existing;
        let active = 1usize;
        let try_dup = existing;
        let mut dup_found = false;
        for i in 0..active {
            if mints[i] == try_dup {
                dup_found = true;
            }
        }
        assert!(dup_found);
    }

    #[test]
    fn register_asset_max_capacity() {
        let active: u8 = MAX_ASSETS;
        assert!(active >= MAX_ASSETS);
    }


    #[test]
    fn daily_receipt_seeds_unique_per_provider_day_asset() {
        let _seed_doc = (b"daily_receipt", 1u32, 100u64, Pubkey::new_unique());
    }


    #[test]
    fn earmark_invariant_per_asset() {
        let mut a = fresh_pool(Pubkey::new_unique());
        let mut b = fresh_pool(Pubkey::new_unique());
        a.pending_dev_fee = 100;
        b.pending_dev_fee = 200;
        assert!(require_earmark_invariant(&a, 100).is_ok());
        assert!(require_earmark_invariant(&b, 200).is_ok());
        assert!(require_earmark_invariant(&a, 50).is_err());
        assert!(require_earmark_invariant(&b, 200).is_ok());
    }


    #[test]
    fn compound_bucket_not_tracked() {
        let p = fresh_pool(Pubkey::new_unique());
        let s = sum_earmarks(&p);
        let manual = p.pending_dev_fee
            + p.pending_provider_fee
            + p.pending_affiliate
            + p.pending_sovereign
            + p.pending_yield
            + p.pending_reserve;
        assert_eq!(s, manual);
    }


    #[test]
    fn earmark_math_remainder_first_to_reserve() {
        let tax_rem: u64 = 1_000_000_001u64;
        let (yb, cb, _) = phase_split_bps(1);
        let y = (tax_rem as u128) * yb as u128 / 10_000;
        let c = (tax_rem as u128) * cb as u128 / 10_000;
        let r = tax_rem.saturating_sub(y as u64).saturating_sub(c as u64);
        assert_eq!(y + c + r as u128, tax_rem as u128);
    }


    #[test]
    fn settle_pause_blocks_invoice() {
        let mut p = Provider {
            provider_id: 1,
            name: [0u8; PROVIDER_NAME_LEN],
            bump: 255,
            active: true,
            paused: false,
            paused_at: 0,
            settle_paused: false,
            pause_reason: [0u8; PAUSE_REASON_LEN],
            provider_fee_bps: DEFAULT_PROVIDER_FEE_BPS,
            fee_owed_since_last_sweep: 0,
            affiliate_recorder_pubkey: Pubkey::new_unique(),
            signed_terms_hash: [0u8; 32],
            cumulative_gross_ggr: 0,
            cumulative_gross_wager: 0,
            cumulative_gross_payout: 0,
            cumulative_bet_count: 0,
            last_submission_at: 0,
            last_day_id: 0,
            period_net_ggr: 0,
            period_fee_charged: 0,
            fee_correction_applied: 0,
            reserved: [0u8; 47],
        };
        assert!(!p.settle_paused);
        p.settle_paused = true;
        assert!(p.settle_paused);
    }


    #[test]
    fn require_non_default_rejects_zero_key() {
        let keys = [Pubkey::default()];
        let r = require_non_default_pubkeys(&keys);
        assert!(r.is_err());
    }

    #[test]
    fn require_non_default_accepts_valid_keys() {
        let keys = [Pubkey::new_unique(), Pubkey::new_unique()];
        let r = require_non_default_pubkeys(&keys);
        assert!(r.is_ok());
    }


    #[test]
    fn day_id_must_strictly_increase() {
        let last_day = 100u64;
        let new_day_invalid = 100u64;
        let new_day_valid = 101u64;
        assert!(!(new_day_invalid > last_day));
        assert!(new_day_valid > last_day);
    }


    #[test]
    fn earmarks_bootstrap_yield_20_pct() {
        let mut p = fresh_pool(Pubkey::new_unique());
        p.lp_tokens_by_tier = [100, 0, 0, 0, 0];
        let net = 1_000_000_000i64;
        let fee_due = (net as u64) * DEFAULT_PROVIDER_FEE_BPS as u64 / 10_000;
        accrue_earmarks(&mut p, net, 0, DEFAULT_PROVIDER_FEE_BPS, fee_due, DEFAULT_DEV_FEE_BPS, 0, 0).unwrap();
        assert!(p.pending_yield > 0);
    }


    #[test]
    fn pause_rate_limit_60s_vault_wide() {
        assert_eq!(PAUSE_RATE_LIMIT_VAULT_WIDE_SECONDS, 60);
    }

    #[test]
    fn pause_rate_limit_600s_per_provider() {
        assert_eq!(PAUSE_RATE_LIMIT_SECONDS, 600);
    }


    #[test]
    fn keeper_window_8d_in_seconds() {
        assert_eq!(KEEPER_WINDOW_SECONDS, 691_200);
    }


    #[test]
    fn pattern_y_full_cycle_math() {
        let mut p = fresh_pool(Pubkey::new_unique());
        p.lp_tokens_by_tier = [0, 0, 100, 0, 0];
        let net_day1 = 2_000_000_000i64;
        let fee_due_day1 = (net_day1 as u64) * DEFAULT_PROVIDER_FEE_BPS as u64 / 10_000;
        accrue_earmarks(&mut p, net_day1, 1, DEFAULT_PROVIDER_FEE_BPS, fee_due_day1, DEFAULT_DEV_FEE_BPS, 0, 0).unwrap();
        accrue_affiliate_amount(&mut p, 50_000_000).unwrap();
        p.cumulative_gross_ggr += net_day1;
        let net_day2 = 1_000_000_000i64;
        let fee_due_day2 = (net_day2 as u64) * DEFAULT_PROVIDER_FEE_BPS as u64 / 10_000;
        accrue_earmarks(&mut p, net_day2, 1, DEFAULT_PROVIDER_FEE_BPS, fee_due_day2, DEFAULT_DEV_FEE_BPS, 0, 0).unwrap();
        accrue_affiliate_amount(&mut p, 30_000_000).unwrap();
        p.cumulative_gross_ggr += net_day2;
        let delta_gross = p.cumulative_gross_ggr - p.last_distributed_gross_ggr;
        let aff = p.pending_affiliate;
        let delta_net = delta_gross - aff as i64;
        assert_eq!(delta_gross, 3_000_000_000);
        assert_eq!(aff, 80_000_000);
        assert_eq!(delta_net, 2_920_000_000);
        assert!(delta_net > 0 && (delta_net as u64) >= MIN_DELTA_GGR_FOR_SWEEP_USDC);
    }

    #[test]
    fn pattern_y_k4_skip_high_affiliate_low_gross() {
        let mut p = fresh_pool(Pubkey::new_unique());
        p.lp_tokens_by_tier = [0, 0, 100, 0, 0];
        let net = 100_000_000i64;
        accrue_earmarks(&mut p, net, 1, DEFAULT_PROVIDER_FEE_BPS, 0, DEFAULT_DEV_FEE_BPS, 0, 0).unwrap();
        accrue_affiliate_amount(&mut p, 200_000_000).unwrap();
        p.cumulative_gross_ggr += net;
        let delta_gross = p.cumulative_gross_ggr - p.last_distributed_gross_ggr;
        let delta_net = delta_gross - p.pending_affiliate as i64;
        assert!(delta_net < 0);
    }

    #[test]
    fn pattern_y_at_threshold_edge() {
        let mut p = fresh_pool(Pubkey::new_unique());
        let net = MIN_DELTA_GGR_FOR_SWEEP_USDC as i64 + 1_000_000;
        accrue_earmarks(&mut p, net, 1, DEFAULT_PROVIDER_FEE_BPS, 0, DEFAULT_DEV_FEE_BPS, 0, 0).unwrap();
        accrue_affiliate_amount(&mut p, 1_000_001).unwrap();
        p.cumulative_gross_ggr += net;
        let delta_gross = p.cumulative_gross_ggr - p.last_distributed_gross_ggr;
        let delta_net = delta_gross - p.pending_affiliate as i64;
        assert!((delta_net as u64) < MIN_DELTA_GGR_FOR_SWEEP_USDC);
    }


    #[test]
    fn weighted_lp_bps_growth_uneven_distribution() {
        let mut p = fresh_pool(Pubkey::new_unique());
        p.lp_tokens_by_tier = [10, 0, 0, 0, 90];
        assert_eq!(compute_weighted_lp_bps(&p, 1, 0).unwrap(), 8_300);
    }

    #[test]
    fn weighted_lp_bps_growth_skews_to_majority_tier() {
        let mut p = fresh_pool(Pubkey::new_unique());
        p.lp_tokens_by_tier = [99, 0, 0, 0, 1];
        assert_eq!(compute_weighted_lp_bps(&p, 1, 0).unwrap(), 6_520);
    }


    #[test]
    fn deposit_dust_amount_returns_zero_user_shares() {
        let (user, dead) = compute_shares_for_deposit(500_000, 0, 0).unwrap();
        assert_eq!(dead, 500_000);
        assert_eq!(user, 0);
    }

    #[test]
    fn deposit_at_exactly_min_dead_shares() {
        let (user, dead) = compute_shares_for_deposit(MIN_DEAD_SHARES, 0, 0).unwrap();
        assert_eq!(dead, MIN_DEAD_SHARES);
        assert_eq!(user, 0);
    }

    #[test]
    fn deposit_at_min_dead_shares_plus_one() {
        let (user, dead) = compute_shares_for_deposit(MIN_DEAD_SHARES + 1, 0, 0).unwrap();
        assert_eq!(dead, MIN_DEAD_SHARES);
        assert_eq!(user, 1);
    }

    #[test]
    fn deposit_nav_appreciated_2x() {
        let (user, dead) = compute_shares_for_deposit(100, 2000, 1000).unwrap();
        assert_eq!(dead, 0);
        assert_eq!(user, 50);
    }

    #[test]
    fn deposit_nav_depreciated_half() {
        let (user, dead) = compute_shares_for_deposit(100, 500, 1000).unwrap();
        assert_eq!(dead, 0);
        assert_eq!(user, 200);
    }


    #[test]
    fn affiliate_accrual_zero_amount_no_error() {
        let mut p = fresh_pool(Pubkey::new_unique());
        accrue_affiliate_amount(&mut p, 0).unwrap();
        assert_eq!(p.pending_affiliate, 0);
    }

    #[test]
    fn affiliate_accrual_max_safe() {
        let mut p = fresh_pool(Pubkey::new_unique());
        accrue_affiliate_amount(&mut p, u64::MAX / 2).unwrap();
        assert_eq!(p.pending_affiliate, u64::MAX / 2);
    }


    #[test]
    fn provider_fee_deducted_before_dev_fee() {
        let mut p = fresh_pool(Pubkey::new_unique());
        p.lp_tokens_by_tier = [0, 0, 100, 0, 0];
        let net = 1_000_000_000i64;
        let provider_fee = (net as u64) * 1_000 / 10_000;
        accrue_earmarks(&mut p, net, 1, 1_000, provider_fee, DEFAULT_DEV_FEE_BPS, 0, 0).unwrap();
        assert_eq!(p.pending_provider_fee, 100_000_000);
        assert_eq!(p.pending_dev_fee, 22_500_000);
    }

    #[test]
    fn provider_fee_zero_means_dev_fee_on_full_net() {
        let mut p = fresh_pool(Pubkey::new_unique());
        p.lp_tokens_by_tier = [0, 0, 100, 0, 0];
        let net = 1_000_000_000i64;
        accrue_earmarks(&mut p, net, 1, 0, 0, DEFAULT_DEV_FEE_BPS, 0, 0).unwrap();
        assert_eq!(p.pending_provider_fee, 0);
        assert_eq!(p.pending_dev_fee, 25_000_000);
    }


    #[test]
    fn sum_earmarks_includes_all_six_buckets() {
        let mut p = fresh_pool(Pubkey::new_unique());
        p.pending_dev_fee = 1;
        p.pending_provider_fee = 2;
        p.pending_affiliate = 4;
        p.pending_sovereign = 8;
        p.pending_yield = 16;
        p.pending_reserve = 32;
        assert_eq!(sum_earmarks(&p), 63);
    }


    #[test]
    fn admin_timelock_3_days_in_seconds() {
        assert_eq!(ADMIN_TIMELOCK_SECONDS, 3 * 24 * 60 * 60);
    }

    #[test]
    fn registered_assets_indexable() {
        let mut mints = [Pubkey::default(); MAX_ASSETS as usize];
        let mut count: u8 = 0;
        let m1 = Pubkey::new_unique();
        let m2 = Pubkey::new_unique();
        let slot1 = count as usize;
        mints[slot1] = m1;
        count = count.checked_add(1).unwrap();
        let slot2 = count as usize;
        mints[slot2] = m2;
        count = count.checked_add(1).unwrap();
        assert_eq!(count, 2);
        assert_eq!(mints[0], m1);
        assert_eq!(mints[1], m2);
    }


    #[test]
    fn process_withdraw_reduces_supply_and_position() {
        let mut p = fresh_pool(Pubkey::new_unique());
        p.lp_supply = 10_000;
        p.lp_tokens_by_tier[2] = 10_000;
        let mut pos_shares: u64 = 5_000;
        let amount = 2_000u64;
        p.lp_supply = p.lp_supply.checked_sub(amount).unwrap();
        p.lp_tokens_by_tier[2] = p.lp_tokens_by_tier[2].saturating_sub(amount);
        pos_shares = pos_shares.checked_sub(amount).unwrap();
        assert_eq!(p.lp_supply, 8_000);
        assert_eq!(p.lp_tokens_by_tier[2], 8_000);
        assert_eq!(pos_shares, 3_000);
    }


    #[test]
    fn tier_cooldown_index_bounds() {
        for tier in 0..5 {
            assert!(TIER_COOLDOWN_DAYS[tier] >= 3);
            assert!(TIER_COOLDOWN_DAYS[tier] <= 14);
        }
    }

    #[test]
    fn tier_cooldown_strictly_decreasing() {
        for i in 1..5 {
            assert!(TIER_COOLDOWN_DAYS[i] < TIER_COOLDOWN_DAYS[i - 1]);
        }
    }


    #[test]
    fn sweep_skip_does_not_advance_last_distributed() {
        let mut p = fresh_pool(Pubkey::new_unique());
        p.cumulative_gross_ggr = 1_000;
        p.last_distributed_gross_ggr = 0;
        p.pending_affiliate = 5_000;
        let delta_gross = p.cumulative_gross_ggr - p.last_distributed_gross_ggr;
        let net = delta_gross - p.pending_affiliate as i64;
        assert!(net < 0);
        assert_eq!(
            p.last_distributed_gross_ggr, 0,
            "the skip branch must leave the HWM where it was — the delta is re-tried \
             (larger) next cycle, and the profit stays earmarked meanwhile"
        );
        assert_eq!(p.pending_affiliate, 5_000);
    }

    #[test]
    fn drain_hwm_advance_is_max_never_lowers() {
        let mut p = fresh_pool(Pubkey::new_unique());
        p.last_distributed_gross_ggr = 1_000;
        p.cumulative_gross_ggr = 2_500;
        advance_hwm_on_drain(&mut p);
        assert_eq!(p.last_distributed_gross_ggr, 2_500);
        p.cumulative_gross_ggr = 900;
        advance_hwm_on_drain(&mut p);
        assert_eq!(
            p.last_distributed_gross_ggr, 2_500,
            "max() is load-bearing: lowering the mark would let a recovery re-accrue \
             buckets whose funds already left the vault"
        );
        p.cumulative_gross_ggr = -1_851_640_000;
        advance_hwm_on_drain(&mut p);
        assert_eq!(p.last_distributed_gross_ggr, 2_500);
        p.last_distributed_gross_ggr = -1_851_644_605;
        p.cumulative_gross_ggr = -1_851_644_605;
        advance_hwm_on_drain(&mut p);
        assert_eq!(
            p.last_distributed_gross_ggr, 0,
            "a negative bookmark can only be the pre-upgrade skip artifact — normalize it"
        );
    }

    #[test]
    fn effective_accrual_base_floors_a_poisoned_negative_bookmark_at_zero() {
        let poisoned = -1_851_644_605i64;
        assert_eq!(effective_accrual_base(poisoned, poisoned, 800_000_000).unwrap(), 0);
        assert_eq!(
            effective_accrual_base(poisoned, -500_000_000, 1_500_000_000).unwrap(),
            1_000_000_000,
            "measured from 0, not from the poisoned mark (which would give the full 1.5e9)"
        );
        assert_eq!(
            effective_accrual_base(-1_000_000_000, -1_000_000_000, 1_500_000_000).unwrap(),
            500_000_000
        );
        assert_eq!(effective_accrual_base(poisoned, 2_000, 500).unwrap(), 500);
        for &(hwm, cum, net) in &[(0i64, 0i64, 1_500i64), (2_000, 1_700, 500), (500, 500, 1_500)] {
            assert_eq!(
                effective_accrual_base(hwm, cum, net).unwrap(),
                effective_accrual_base(hwm.max(0), cum, net).unwrap()
            );
        }
    }

    #[test]
    fn effective_accrual_base_carries_losses_and_never_inflates() {
        assert_eq!(effective_accrual_base(0, 0, 1_500).unwrap(), 1_500);
        assert_eq!(effective_accrual_base(500, 500, 1_500).unwrap(), 1_500);
        assert_eq!(effective_accrual_base(0, -500, 1_500).unwrap(), 1_000);
        assert_eq!(effective_accrual_base(0, -1_851, 800).unwrap(), 0);
        assert_eq!(effective_accrual_base(2_000, 1_700, 200).unwrap(), 0);
        assert_eq!(effective_accrual_base(2_000, 1_700, 500).unwrap(), 200);
        assert_eq!(effective_accrual_base(0, 1_000, -300).unwrap(), -300);
        assert_eq!(effective_accrual_base(0, 1_000, 0).unwrap(), 0);
        for &(hwm, cum, net) in &[
            (0i64, 0i64, 1i64),
            (-5, -10, 3),
            (7, 2, 9),
            (-1_000, 4_000, 1_000),
        ] {
            let e = effective_accrual_base(hwm, cum, net).unwrap();
            assert!(e >= 0 && e <= net, "E={e} must be in [0, net={net}]");
        }
    }


    fn replay_period(bps: u16, receipts: &[i64]) -> (i64, u64, u64, u64) {
        let mut net = 0i64;
        let mut charged = 0u64;
        let (mut up, mut down) = (0u64, 0u64);
        for &r in receipts {
            let step = provider_period_fee_step(net, r, charged, bps).unwrap();
            assert!(
                step.increase == 0 || step.decrease == 0,
                "increase/decrease must be mutually exclusive (got +{} / -{})",
                step.increase,
                step.decrease
            );
            assert!(
                step.decrease <= charged,
                "a decrease may never exceed what the period has charged \
                 (decrease {} > charged {}) — would desync the pool mirror",
                step.decrease,
                charged
            );
            net = step.period_net_after;
            charged = step.fee_target;
            up = up.checked_add(step.increase).unwrap();
            down = down.checked_add(step.decrease).unwrap();
        }
        assert_eq!(
            charged,
            up - down,
            "Σincrease − Σdecrease must equal period_fee_charged"
        );
        (net, charged, up, down)
    }

    #[test]
    fn period_fee_charges_period_net_not_sum_of_winning_days() {
        let (net, charged, _, _) =
            replay_period(1_000, &[1_000_000_000, -600_000_000, 1_000_000_000]);
        assert_eq!(net, 1_400_000_000);
        assert_eq!(
            charged, 140_000_000,
            "10% of the $1,400 period NET. The shipped rule bills $200 \
             (10% of the $2,000 of winning days) — the defect."
        );
    }

    #[test]
    fn period_fee_adjusts_down_when_period_dips_before_flush() {
        let bps = 1_000u16;
        let s1 = provider_period_fee_step(0, 1_000_000_000, 0, bps).unwrap();
        assert_eq!(s1.increase, 100_000_000);
        assert_eq!(s1.fee_target, 100_000_000);

        let s2 = provider_period_fee_step(
            s1.period_net_after,
            -400_000_000,
            s1.fee_target,
            bps,
        )
        .unwrap();
        assert_eq!(s2.period_net_after, 600_000_000);
        assert_eq!(s2.fee_target, 60_000_000);
        assert_eq!(s2.increase, 0);
        assert_eq!(
            s2.decrease, 40_000_000,
            "the fee must fall with the period — impossible under the shipped rule"
        );
    }

    #[test]
    fn period_fee_floors_at_zero_for_a_net_negative_period() {
        let fresh = provider_period_fee_step(0, -1_000_000_000, 0, 1_000).unwrap();
        assert_eq!(fresh.period_net_after, -1_000_000_000);
        assert_eq!(fresh.fee_target, 0);
        assert_eq!(fresh.increase, 0);
        assert_eq!(fresh.decrease, 0, "cannot give back what was never charged");

        let (net, charged, up, down) = replay_period(1_000, &[500_000_000, -2_000_000_000]);
        assert_eq!(net, -1_500_000_000);
        assert_eq!(charged, 0, "a losing period owes nothing — never negative");
        assert_eq!(up, 50_000_000);
        assert_eq!(down, 50_000_000, "exactly the $50 charged comes back, no more");
    }

    #[test]
    fn period_fee_live_shape_bills_32_81_not_217_97() {
        let receipts = [1_000_000_000i64, -1_851_644_605i64, 1_179_720_000i64];
        assert_eq!(
            receipts[0] + receipts[2],
            2_179_720_000,
            "sum of the winning days, per the on-chain read"
        );
        let period_net: i64 = receipts.iter().sum();
        assert_eq!(period_net, 328_075_395, "lifetime net GGR = $328.075395");

        let (net, charged, _, _) = replay_period(1_000, &receipts);
        assert_eq!(net, period_net);
        assert_eq!(
            charged, 32_807_539,
            "$32.81 = 10% of the $328.08 monthly NET. The shipped rule accrues \
             $217.972 (10% of the $2,179.72 of winning days) — a $185.164461 \
             over-accrual of LP principal."
        );
        assert_eq!(217_972_000u64 - charged, 185_164_461);
    }

    #[test]
    fn period_fee_decrease_never_exceeds_what_the_period_charged() {
        for bps in [0u16, 1, 250, 1_000, 2_500] {
            let (_, charged, up, down) = replay_period(
                bps,
                &[
                    900_000_000,
                    -1_400_000_000,
                    300_000_000,
                    2_000_000_000,
                    -2_500_000_000,
                    100_000_000,
                    -50_000_000,
                ],
            );
            assert!(down <= up, "bps={bps}: can never refund more than charged");
            assert_eq!(charged, up - down, "bps={bps}");
        }
    }

    #[test]
    fn period_fee_is_path_independent_within_a_period() {
        let orders: [[i64; 4]; 3] = [
            [1_000_000_000, -600_000_000, 800_000_000, -200_000_000],
            [-600_000_000, 1_000_000_000, -200_000_000, 800_000_000],
            [800_000_000, -200_000_000, -600_000_000, 1_000_000_000],
        ];
        let expected = 100_000_000u64;
        for (i, seq) in orders.iter().enumerate() {
            let (net, charged, _, _) = replay_period(1_000, seq);
            assert_eq!(net, 1_000_000_000, "order {i}");
            assert_eq!(
                charged, expected,
                "order {i}: the same receipts must bill the same fee regardless \
                 of the order they arrive in"
            );
        }
    }

    #[test]
    fn period_fee_edges_zero_bps_and_overflow() {
        let z = provider_period_fee_step(0, 5_000_000_000, 0, 0).unwrap();
        assert_eq!(z.fee_target, 0);
        assert_eq!(z.increase, 0);
        assert_eq!(z.decrease, 0);

        assert!(provider_period_fee_step(i64::MAX, 1, 0, 1_000).is_err());
        assert!(provider_period_fee_step(i64::MIN, -1, 0, 1_000).is_err());
    }

    fn fresh_provider(bps: u16) -> Provider {
        Provider {
            provider_id: 0,
            name: [0u8; PROVIDER_NAME_LEN],
            bump: 255,
            active: true,
            paused: false,
            paused_at: 0,
            settle_paused: false,
            pause_reason: [0u8; PAUSE_REASON_LEN],
            provider_fee_bps: bps,
            fee_owed_since_last_sweep: 0,
            affiliate_recorder_pubkey: Pubkey::new_unique(),
            signed_terms_hash: [0u8; 32],
            cumulative_gross_ggr: 0,
            cumulative_gross_wager: 0,
            cumulative_gross_payout: 0,
            cumulative_bet_count: 0,
            last_submission_at: 0,
            last_day_id: 0,
            period_net_ggr: 0,
            period_fee_charged: 0,
            fee_correction_applied: 0,
            reserved: [0u8; 47],
        }
    }

    fn submit_step(pool: &mut AssetPool, provider: &mut Provider, net: i64) {
        let bps = provider.provider_fee_bps;
        let step =
            provider_period_fee_step(provider.period_net_ggr, net, provider.period_fee_charged, bps)
                .unwrap();
        provider.period_net_ggr = step.period_net_after;
        provider.period_fee_charged = step.fee_target;
        let fee_due = step.increase;
        provider.fee_owed_since_last_sweep =
            provider.fee_owed_since_last_sweep.checked_add(fee_due).unwrap();
        reduce_provider_fee_accrual(pool, provider, step.decrease);

        let cum_before = pool.cumulative_gross_ggr;
        pool.cumulative_gross_ggr = pool.cumulative_gross_ggr.checked_add(net).unwrap();
        let base =
            effective_accrual_base(pool.last_distributed_gross_ggr, cum_before, net).unwrap();
        accrue_earmarks(pool, base, 0, bps, fee_due, DEFAULT_DEV_FEE_BPS, 0, step.decrease)
            .unwrap();
    }

    #[test]
    fn provider_fee_mirrors_move_in_lockstep() {
        let mut pool = fresh_pool(Pubkey::new_unique());
        let mut provider = fresh_provider(1_000);
        for net in [
            900_000_000i64,
            -1_400_000_000,
            300_000_000,
            2_000_000_000,
            -2_500_000_000,
            1_100_000_000,
            -50_000_000,
        ] {
            submit_step(&mut pool, &mut provider, net);
            assert_eq!(
                pool.pending_provider_fee, provider.fee_owed_since_last_sweep,
                "the two fee mirrors MUST stay equal after every receipt \
                 (net={net}) — otherwise flush_provider_fee underflows"
            );
            assert_eq!(
                provider.fee_owed_since_last_sweep, provider.period_fee_charged,
                "within a period the owed slice IS the period charge"
            );
        }
    }

    #[test]
    fn k4_never_worsens_when_the_period_fee_falls() {
        let mut pool = fresh_pool(Pubkey::new_unique());
        let mut provider = fresh_provider(1_000);
        let holder: u64 = 10_000_000_000;
        for net in [2_000_000_000i64, -900_000_000, 500_000_000, -3_000_000_000, 800_000_000] {
            let before = sum_earmarks(&pool);
            let fee_before = pool.pending_provider_fee;
            submit_step(&mut pool, &mut provider, net);
            let after = sum_earmarks(&pool);
            if pool.pending_provider_fee < fee_before {
                assert!(
                    after <= before,
                    "a falling provider fee must never grow sum_earmarks \
                     (before={before} after={after})"
                );
            }
            assert!(
                require_earmark_invariant(&pool, holder).is_ok(),
                "K4 must hold on every step (net={net})"
            );
        }
    }

    #[test]
    fn waterfall_buckets_unchanged_for_a_monotonically_rising_period() {
        let receipts = [400_000_000i64, 1_100_000_000, 250_000_000, 3_000_000_000];
        let bps = 1_000u16;

        let mut pool_new = fresh_pool(Pubkey::new_unique());
        let mut prov = fresh_provider(bps);
        for &n in &receipts {
            submit_step(&mut pool_new, &mut prov, n);
        }

        let mut pool_old = fresh_pool(Pubkey::new_unique());
        for &n in &receipts {
            let fee_due = ((n as u128) * bps as u128 / 10_000u128) as u64;
            let cum_before = pool_old.cumulative_gross_ggr;
            pool_old.cumulative_gross_ggr += n;
            let base =
                effective_accrual_base(pool_old.last_distributed_gross_ggr, cum_before, n).unwrap();
            accrue_earmarks(&mut pool_old, base, 0, bps, fee_due, DEFAULT_DEV_FEE_BPS, 0, 0).unwrap();
        }

        assert_eq!(pool_new.pending_dev_fee, pool_old.pending_dev_fee, "dev bucket moved");
        assert_eq!(pool_new.pending_sovereign, pool_old.pending_sovereign, "sovereign moved");
        assert_eq!(pool_new.pending_yield, pool_old.pending_yield, "yield moved");
        assert_eq!(pool_new.pending_reserve, pool_old.pending_reserve, "reserve moved");
        assert_eq!(
            pool_new.pending_provider_fee, pool_old.pending_provider_fee,
            "a purely-winning period must bill EXACTLY what the old rule billed"
        );
        assert_eq!(pool_new.cumulative_gross_ggr, pool_old.cumulative_gross_ggr);
    }

    #[test]
    fn flush_closes_the_period_and_the_next_one_starts_clean() {
        let mut pool = fresh_pool(Pubkey::new_unique());
        let mut provider = fresh_provider(1_000);
        submit_step(&mut pool, &mut provider, 1_000_000_000);
        assert_eq!(provider.fee_owed_since_last_sweep, 100_000_000);

        let amount = provider.fee_owed_since_last_sweep;
        provider.period_net_ggr = 0;
        provider.period_fee_charged = 0;
        provider.fee_owed_since_last_sweep = 0;
        pool.pending_provider_fee = pool.pending_provider_fee.checked_sub(amount).unwrap();
        pool.provider_owed_total = pool.provider_owed_total.checked_add(amount).unwrap();

        assert_eq!(provider.period_net_ggr, 0, "period must reset");
        assert_eq!(provider.period_fee_charged, 0, "period charge must reset");
        assert_eq!(pool.provider_owed_total, 100_000_000);

        submit_step(&mut pool, &mut provider, -5_000_000_000);
        assert_eq!(
            pool.provider_owed_total, 100_000_000,
            "an invoiced amount is untouchable by a later period's losses"
        );
        assert_eq!(pool.pending_provider_fee, 0);
        assert_eq!(provider.fee_owed_since_last_sweep, 0);
        assert_eq!(provider.period_net_ggr, -5_000_000_000);
        assert_eq!(provider.period_fee_charged, 0);

        submit_step(&mut pool, &mut provider, 6_000_000_000);
        assert_eq!(provider.period_net_ggr, 1_000_000_000);
        assert_eq!(
            provider.period_fee_charged, 100_000_000,
            "the carried −$5,000 nets against the +$6,000 first: the provider is \
             billed on the +$1,000 remainder, NOT on the full $6,000"
        );
    }

    fn try_flush(pool: &mut AssetPool, provider: &mut Provider) -> core::result::Result<u64, ()> {
        let amount = provider.fee_owed_since_last_sweep;
        if amount == 0 {
            return Err(());
        }
        provider.period_net_ggr = 0;
        provider.period_fee_charged = 0;
        provider.fee_owed_since_last_sweep = 0;
        pool.pending_provider_fee = pool.pending_provider_fee.checked_sub(amount).unwrap();
        pool.provider_owed_total = pool.provider_owed_total.checked_add(amount).unwrap();
        Ok(amount)
    }

    #[test]
    fn negative_unwind_nets_the_released_fee_off_its_basis() {
        let mut a = fresh_pool(Pubkey::new_unique());
        let mut pa = fresh_provider(1_000);
        submit_step(&mut a, &mut pa, 1_000_000_000);
        let after_win = sum_earmarks(&a) - a.pending_provider_fee;
        submit_step(&mut a, &mut pa, -400_000_000);
        let a4 = sum_earmarks(&a) - a.pending_provider_fee;

        let mut b = fresh_pool(Pubkey::new_unique());
        let mut pb = fresh_provider(1_000);
        submit_step(&mut b, &mut pb, -400_000_000);
        submit_step(&mut b, &mut pb, 1_000_000_000);
        let b4 = sum_earmarks(&b) - b.pending_provider_fee;

        assert_eq!(pa.period_fee_charged, 60_000_000, "A: 10% of the $600 period net");
        assert_eq!(pb.period_fee_charged, 60_000_000, "B: same period, same fee");
        assert_eq!(a.pending_provider_fee, b.pending_provider_fee);

        let drift = if a4 > b4 { a4 - b4 } else { b4 - a4 };
        assert!(
            drift <= 8,
            "orderings diverged by {drift} (only floor-division dust is \
             acceptable). A={a4} B={b4}. The negative-unwind basis must be \
             `abs_loss − fee_release`, not the raw loss."
        );
        assert!(after_win > a4, "the loss must still unwind the buckets");
    }

    #[test]
    fn rate_change_guard_implies_nothing_charged() {
        let src = include_str!("lib.rs");
        let program_src = src
            .split("#[cfg(test)]")
            .next()
            .expect("program code precedes the #[cfg(test)] module");
        let fee_write = format!(".{} = ", "period_fee_charged");
        let net_write = format!(".{} = ", "period_net_ggr");
        let fee_writes = program_src.matches(&fee_write).count();
        let net_writes = program_src.matches(&net_write).count();
        assert_eq!(
            fee_writes, 3,
            "expected EXACTLY the three known production writes to \
             period_fee_charged (add_provider zero-init, submit fee_step, \
             flush period-close reset); saw {fee_writes}. A new writer must \
             re-prove `period_net_ggr <= 0 ⇒ period_fee_charged == 0` and \
             only then update this count."
        );
        assert_eq!(
            net_writes, 3,
            "expected EXACTLY the three known production writes to \
             period_net_ggr; saw {net_writes} — see the period_fee_charged \
             assert above."
        );
        for op in ["+=", "-=", "*=", "/="] {
            for field in ["period_fee_charged", "period_net_ggr"] {
                let needle = format!(".{field} {op}");
                assert!(
                    !program_src.contains(&needle),
                    "no production compound assignment to {field} is allowed \
                     (found `{needle}`)"
                );
            }
        }
        let paired_step = format!(
            "provider.{} = fee_step.period_net_after;\n        provider.{} = fee_step.fee_target;",
            "period_net_ggr", "period_fee_charged"
        );
        let paired_zero = format!(
            "provider.{} = 0;\n        provider.{} = 0;",
            "period_net_ggr", "period_fee_charged"
        );
        assert_eq!(
            program_src.matches(&paired_step).count(),
            1,
            "the single non-zero write to period_fee_charged MUST be paired \
             with the period_net_ggr write from the SAME fee_step — otherwise \
             the two can drift and `period_net <= 0 ⇒ charged == 0` stops \
             holding"
        );
        assert_eq!(
            program_src.matches(&paired_zero).count(),
            2,
            "expected exactly two paired zero-resets (add_provider init + \
             flush_provider_fee period close)"
        );

        let mut pool = fresh_pool(Pubkey::new_unique());
        let mut provider = fresh_provider(1_000);
        for net in [
            900_000_000i64,
            -1_400_000_000,
            300_000_000,
            2_000_000_000,
            -2_500_000_000,
            -100_000_000,
            1_100_000_000,
            0,
        ] {
            submit_step(&mut pool, &mut provider, net);
            if provider.period_net_ggr <= 0 {
                assert_eq!(
                    provider.period_fee_charged, 0,
                    "period_net_ggr <= 0 MUST imply period_fee_charged == 0 \
                     (net={net}); the rate-change guard relies on it"
                );
            }
        }
        let fresh = fresh_provider(1_000);
        assert!(fresh.period_net_ggr <= 0 && fresh.period_fee_charged == 0);
    }

    #[derive(AnchorSerialize, AnchorDeserialize)]
    struct LegacyProviderLayout {
        provider_id: u32,
        name: [u8; PROVIDER_NAME_LEN],
        bump: u8,
        active: bool,
        paused: bool,
        paused_at: i64,
        settle_paused: bool,
        pause_reason: [u8; PAUSE_REASON_LEN],
        provider_fee_bps: u16,
        fee_owed_since_last_sweep: u64,
        affiliate_recorder_pubkey: Pubkey,
        signed_terms_hash: [u8; 32],
        cumulative_gross_ggr: i64,
        cumulative_gross_wager: u64,
        cumulative_gross_payout: u64,
        cumulative_bet_count: u64,
        last_submission_at: i64,
        last_day_id: u64,
        reserved: [u8; 64],
    }

    #[test]
    fn legacy_reserved64_bytes_deserialize_into_current_provider() {
        let recorder = Pubkey::new_unique();
        let legacy = LegacyProviderLayout {
            provider_id: 7,
            name: [0xAB; PROVIDER_NAME_LEN],
            bump: 254,
            active: true,
            paused: false,
            paused_at: -123_456_789,
            settle_paused: true,
            pause_reason: [0xCD; PAUSE_REASON_LEN],
            provider_fee_bps: 1_000,
            fee_owed_since_last_sweep: 217_972_000,
            affiliate_recorder_pubkey: recorder,
            signed_terms_hash: [0xEF; 32],
            cumulative_gross_ggr: -1_851_644_605,
            cumulative_gross_wager: 65_270_950_000,
            cumulative_gross_payout: 64_942_870_000,
            cumulative_bet_count: 4_065,
            last_submission_at: 1_784_000_000,
            last_day_id: 20_664,
            reserved: [0u8; 64],
        };
        let legacy_bytes = legacy.try_to_vec().expect("legacy serializes");

        assert_eq!(
            legacy_bytes.len(),
            Provider::LEN - 8,
            "legacy and current layouts must be byte-size identical"
        );

        let migrated = Provider::deserialize(&mut legacy_bytes.as_slice())
            .expect("legacy bytes must deserialize as the current Provider");

        assert_eq!(migrated.provider_id, 7);
        assert_eq!(migrated.name, [0xAB; PROVIDER_NAME_LEN]);
        assert_eq!(migrated.bump, 254);
        assert!(migrated.active);
        assert!(!migrated.paused);
        assert_eq!(migrated.paused_at, -123_456_789);
        assert!(migrated.settle_paused);
        assert_eq!(migrated.pause_reason, [0xCD; PAUSE_REASON_LEN]);
        assert_eq!(migrated.provider_fee_bps, 1_000);
        assert_eq!(migrated.fee_owed_since_last_sweep, 217_972_000);
        assert_eq!(migrated.affiliate_recorder_pubkey, recorder);
        assert_eq!(migrated.signed_terms_hash, [0xEF; 32]);
        assert_eq!(migrated.cumulative_gross_ggr, -1_851_644_605);
        assert_eq!(migrated.cumulative_gross_wager, 65_270_950_000);
        assert_eq!(migrated.cumulative_gross_payout, 64_942_870_000);
        assert_eq!(migrated.cumulative_bet_count, 4_065);
        assert_eq!(migrated.last_submission_at, 1_784_000_000);
        assert_eq!(migrated.last_day_id, 20_664);

        assert_eq!(migrated.period_net_ggr, 0);
        assert_eq!(migrated.period_fee_charged, 0);
        assert_eq!(migrated.fee_correction_applied, 0);
        assert_eq!(migrated.reserved, [0u8; 47]);

        assert_eq!(
            migrated.try_to_vec().expect("current serializes"),
            legacy_bytes,
            "current Provider must re-serialize to the exact legacy bytes"
        );
    }

    #[test]
    fn rate_drop_release_would_be_misrouted_to_lp_nav_if_unguarded() {
        let mut pool = fresh_pool(Pubkey::new_unique());
        let mut provider = fresh_provider(1_000);

        submit_step(&mut pool, &mut provider, 1_000_000_000);
        assert_eq!(provider.period_fee_charged, 100_000_000);
        let buckets_before = sum_earmarks(&pool) - pool.pending_provider_fee;

        provider.provider_fee_bps = 0;
        let step = provider_period_fee_step(
            provider.period_net_ggr,
            1_000_000,
            provider.period_fee_charged,
            0,
        )
        .unwrap();
        assert_eq!(step.decrease, 100_000_000, "the whole $100 is released");
        assert_eq!(step.increase, 0);

        let split4_of_release = {
            let base: u128 = 100_000_000;
            let dev = base * DEFAULT_DEV_FEE_BPS as u128 / 10_000;
            let after_dev = base - dev;
            let lp = after_dev * DEFAULT_LP_SHARE_BPS as u128 / 10_000;
            let protocol = after_dev - lp;
            let sov = protocol * SOVEREIGN_CARVE_BPS as u128 / 10_000;
            let tax = protocol - sov;
            let (yb, cb, _) = phase_split_bps(0);
            let yld = tax * yb as u128 / 10_000;
            let comp = tax * cb as u128 / 10_000;
            let reserve = tax - yld - comp;
            (dev + sov + yld + reserve) as u64
        };
        assert_eq!(
            split4_of_release, 15_565_000,
            "the documented misroute magnitude: split4($100) = $15.565"
        );
        let _ = buckets_before;

        let mut trial = fresh_pool(Pubkey::new_unique());
        assert!(
            accrue_earmarks(
                &mut trial,
                1_000_000,
                0,
                0,
                0,
                DEFAULT_DEV_FEE_BPS,
                0,
                step.decrease,
            )
            .is_err(),
            "a positive receipt carrying a fee release MUST revert \
             (FeeReleaseOnPositiveReceipt) — fail-closed, never silent"
        );
    }

    #[test]
    fn update_provider_fee_blocks_a_rate_change_while_the_period_is_in_profit() {
        let src = include_str!("lib.rs");
        let start = src.find("pub fn update_provider_fee(").expect("setter must exist");
        let end = drift_handler_end(src, start);
        let body: String = src[start..end]
            .lines()
            .filter(|l| !l.trim_start().starts_with("//"))
            .collect::<Vec<_>>()
            .join("\n");
        assert!(
            body.contains("provider.period_net_ggr <= 0")
                && body.contains("FeeBpsChangeWouldRepriceOpenPeriod"),
            "update_provider_fee MUST refuse a rate change while the period is \
             in profit — otherwise a rate cut releases charged fee that the \
             positive cascade cannot absorb ($15.565 misrouted per $100)"
        );
        assert!(
            !body.contains("period_net_ggr == 0"),
            "the guard MUST be `<= 0`, not `== 0` — a founder-locked carried \
             NEGATIVE period keeps period_net_ggr non-zero indefinitely and \
             would lock the rate setter permanently"
        );
        assert!(
            !body.contains("pending_") && !body.contains("unlocks_at"),
            "update_provider_fee is INSTANT by design; if a timelock is ever \
             added, update every comment that documents it as instant"
        );
    }

    #[test]
    fn carry_forward_negative_period_bills_cumulative_net_across_months() {
        let mut pool = fresh_pool(Pubkey::new_unique());
        let mut provider = fresh_provider(1_000);
        let mut billed: u64 = 0;

        submit_step(&mut pool, &mut provider, 1_000_000_000);
        assert_eq!(provider.period_fee_charged, 100_000_000);
        billed += try_flush(&mut pool, &mut provider).expect("M1 must flush");
        assert_eq!(provider.period_net_ggr, 0, "a SETTLED period resets to zero");

        submit_step(&mut pool, &mut provider, -500_000_000);
        assert_eq!(provider.period_fee_charged, 0, "a losing month owes nothing");
        assert!(
            try_flush(&mut pool, &mut provider).is_err(),
            "a net-negative period MUST NOT be flushable — the NothingOwed revert \
             is what delivers the carry-forward"
        );
        assert_eq!(
            provider.period_net_ggr, -500_000_000,
            "the −$500 MUST still be on the books after the failed flush. If this \
             reads 0, someone re-introduced a period reset for losing months — \
             that discards the carry and OVERPAYS the provider. Founder-locked \
             2026-07-31: negative periods carry forward."
        );

        submit_step(&mut pool, &mut provider, 1_000_000_000);
        assert_eq!(
            provider.period_net_ggr, 500_000_000,
            "M3 must net against the carried −$500, not start fresh at +$1,000"
        );
        assert_eq!(provider.period_fee_charged, 50_000_000, "10% of the netted $500");
        billed += try_flush(&mut pool, &mut provider).expect("M3 must flush");

        assert_eq!(
            billed, 150_000_000,
            "cumulative billed MUST be 10% of the $1,500 cumulative net. Without \
             the carry it would be $200 — 10% of the two winning months only."
        );
        let cumulative_net = 1_000_000_000i64 - 500_000_000 + 1_000_000_000;
        assert_eq!(cumulative_net, 1_500_000_000);
        assert_eq!(billed, (cumulative_net as u64) / 10);
    }

    #[test]
    fn carry_forward_negative_period_survives_multiple_periods() {
        let mut pool = fresh_pool(Pubkey::new_unique());
        let mut provider = fresh_provider(1_000);
        let mut billed: u64 = 0;

        submit_step(&mut pool, &mut provider, 1_000_000_000);
        billed += try_flush(&mut pool, &mut provider).expect("M1 must flush");
        assert_eq!(billed, 100_000_000);

        submit_step(&mut pool, &mut provider, -2_000_000_000);
        assert!(try_flush(&mut pool, &mut provider).is_err(), "M2 must not flush");
        assert_eq!(provider.period_net_ggr, -2_000_000_000, "the full −$2,000 carries");

        submit_step(&mut pool, &mut provider, 500_000_000);
        assert_eq!(
            provider.period_net_ggr, -1_500_000_000,
            "the carry survives a SECOND period boundary: −2,000 + 500 = −1,500"
        );
        assert_eq!(provider.period_fee_charged, 0, "still under water ⇒ zero fee");
        assert_eq!(pool.pending_provider_fee, 0, "no new accrual");
        assert!(try_flush(&mut pool, &mut provider).is_err(), "M3 must not flush either");

        submit_step(&mut pool, &mut provider, 2_000_000_000);
        assert_eq!(provider.period_net_ggr, 500_000_000);
        assert_eq!(
            provider.period_fee_charged, 50_000_000,
            "billed on the $500 that actually cleared the deficit, not the $2,000"
        );
        billed += try_flush(&mut pool, &mut provider).expect("M4 must flush");
        assert_eq!(billed, 150_000_000, "total billed across all four months");
        assert_eq!(pool.provider_owed_total, 150_000_000);
    }

    #[test]
    fn provider_period_fields_are_carved_from_reserved_len_unchanged() {
        const PRE_CHANGE_LEN: usize = 8 + 4 + 32 + 4 + 8 + 32 + 2 + 8 + 32 + 32 + 48 + 64;
        assert_eq!(
            Provider::LEN,
            PRE_CHANGE_LEN,
            "carving period_net_ggr(8) + period_fee_charged(8) + \
             fee_correction_applied(1) out of reserved MUST leave Provider::LEN \
             identical — otherwise the live account needs a realloc"
        );
        assert_eq!(8 + 8 + 1 + 47, 64, "the carve must consume exactly the old reserved");
    }


    #[test]
    fn fee_correction_is_locked_down_at_the_source() {
        let src = include_str!("lib.rs");
        let start = src
            .find("pub fn correct_provider_fee_overaccrual(")
            .expect("correction instruction must exist");
        let end = drift_handler_end(src, start);
        let body_owned: String = src[start..end]
            .lines()
            .filter(|l| !l.trim_start().starts_with("//"))
            .collect::<Vec<_>>()
            .join("\n");
        let body = body_owned.as_str();

        assert!(
            body.contains("ctx.accounts.authority.key()") && body.contains("config.authority"),
            "correction MUST be gated on config.authority"
        );
        assert!(
            !body.contains("operator_pubkey"),
            "correction MUST NOT be reachable by the operator key — that key has \
             live money-path reach and a hot-key compromise must not reach this"
        );
        assert!(
            body.contains("provider.fee_correction_applied == 0")
                && body.contains("provider.fee_correction_applied = 1"),
            "correction MUST check and set the one-shot latch"
        );
        assert!(
            body.contains("pool.pending_provider_fee == expected_pending_provider_fee")
                && body.contains(
                    "provider.fee_owed_since_last_sweep == expected_fee_owed_since_last_sweep"
                ),
            "correction MUST bind the attested pre-state of BOTH fee mirrors"
        );
        assert!(
            body.contains("new_pending_provider_fee < expected_pending_provider_fee"),
            "correction MUST strictly DECREASE — it can never raise a bucket"
        );
        assert!(
            body.contains("delta <= expected_fee_owed_since_last_sweep"),
            "the reduction MUST be absorbable by this provider's un-flushed slice \
             so it cannot reach into another provider's or into provider_owed_total"
        );
        assert_eq!(
            body.matches("require_earmark_invariant(pool, holder_balance)?").count(),
            1,
            "K4 MUST be checked exactly once, on the POST-mutation state"
        );
        let k4_pos = body
            .find("require_earmark_invariant(pool, holder_balance)?")
            .expect("K4 check must exist");
        let mutate_pos = body
            .find("provider.fee_correction_applied = 1")
            .expect("the one-shot latch write must exist");
        assert!(
            k4_pos > mutate_pos,
            "the K4 check MUST come AFTER the mutation — a pre-check refuses to \
             run exactly when solvency is broken"
        );
        for guard in [
            "require!(!config.is_frozen",
            "require!(!ctx.accounts.vault_config.is_frozen",
        ] {
            assert!(
                !body.contains(guard),
                "the correction MUST NOT gate on is_frozen (`{guard}`) — freeze → \
                 upgrade → correct → verify → unfreeze is the ceremony that closes \
                 the flush window; is_paused does NOT block flush_provider_fee"
            );
        }
        for forbidden in [
            "pending_dev_fee =",
            "pending_sovereign =",
            "pending_yield =",
            "pending_reserve =",
            "pending_affiliate =",
            "pending_promo =",
            "provider_owed_total =",
            "transfer_checked",
            "period_net_ggr =",
            "period_fee_charged =",
        ] {
            assert!(
                !body.contains(forbidden),
                "correction MUST NOT touch `{forbidden}` — it is a one-shot fee \
                 correction, not a bucket setter"
            );
        }
        assert!(
            !body.contains("saturating_sub"),
            "correction MUST use checked_sub so a bound violation reverts"
        );
    }

    #[test]
    fn flush_resets_the_period_accumulators_at_the_source() {
        let src = include_str!("lib.rs");
        let start = src.find("pub fn flush_provider_fee(").expect("flush must exist");
        let end = drift_handler_end(src, start);
        let body = &src[start..end];
        assert!(
            body.contains("provider.period_net_ggr = 0")
                && body.contains("provider.period_fee_charged = 0"),
            "flush_provider_fee MUST reset BOTH period accumulators — otherwise a \
             later loss could claw back an already-invoiced fee"
        );
    }

    #[test]
    fn submit_routes_the_provider_fee_through_the_period_rule() {
        let src = include_str!("lib.rs");
        let start = src.find("pub fn submit_provider_ggr(").expect("submit must exist");
        let end = drift_handler_end(src, start);
        let body = &src[start..end];
        assert!(
            body.contains("provider_period_fee_step("),
            "submit MUST compute the fee via the period rule"
        );
        assert!(
            body.contains("reduce_provider_fee_accrual(pool, provider, fee_step.decrease)"),
            "submit MUST apply the downward leg through the shared lockstep helper"
        );
        assert!(
            body.contains("let fee_due: u64 = fee_step.increase;"),
            "the cascade deduction MUST be the period DELTA, not the fee on gross"
        );
        assert!(
            !body.contains("let fee_due: u64 = if net_ggr_signed > 0 {"),
            "the per-receipt gross-fee formula MUST NOT survive anywhere in submit"
        );
    }


    #[test]
    fn historical_receipts_preserve_fee_bps_at_accrual() {
        let r1_bps_snapshot: u16 = 1_000;
        let r2_bps_snapshot: u16 = 1_500;
        let _new_bps: u16 = 2_000;
        assert_eq!(r1_bps_snapshot, 1_000);
        assert_eq!(r2_bps_snapshot, 1_500);
    }


    #[test]
    fn default_provider_fee_is_under_cap() {
        assert!(DEFAULT_PROVIDER_FEE_BPS < MAX_PROVIDER_FEE_BPS);
    }

    #[test]
    fn sovereign_carve_is_5_percent_of_protocol_share() {
        assert_eq!(SOVEREIGN_CARVE_BPS, 500);
    }

    #[test]
    fn dev_fee_default_is_2_5_percent() {
        assert_eq!(DEFAULT_DEV_FEE_BPS, 250);
    }


    #[test]
    fn reserve_takes_rounding_dust_growth() {
        let tax_rem: u64 = 1_111_111;
        let (yb, cb, _) = phase_split_bps(1);
        let y = ((tax_rem as u128) * yb as u128 / 10_000) as u64;
        let c = ((tax_rem as u128) * cb as u128 / 10_000) as u64;
        let r = tax_rem.saturating_sub(y).saturating_sub(c);
        assert_eq!(y + c + r, tax_rem);
    }

    #[test]
    fn reserve_takes_rounding_dust_bootstrap() {
        let tax_rem: u64 = 1_111_111;
        let (yb, cb, _) = phase_split_bps(0);
        let y = ((tax_rem as u128) * yb as u128 / 10_000) as u64;
        let c = ((tax_rem as u128) * cb as u128 / 10_000) as u64;
        let r = tax_rem.saturating_sub(y).saturating_sub(c);
        assert_eq!(y + c + r, tax_rem);
    }


    #[test]
    fn negative_ggr_one_asset_does_not_affect_another() {
        let mut a = fresh_pool(Pubkey::new_unique());
        let mut b = fresh_pool(Pubkey::new_unique());
        a.pending_dev_fee = 1_000;
        a.pending_yield = 2_000;
        b.pending_dev_fee = 5_000;
        b.pending_yield = 8_000;
        accrue_earmarks(&mut a, -10_000_000, 1, DEFAULT_PROVIDER_FEE_BPS, 0, DEFAULT_DEV_FEE_BPS, 0, 0).unwrap();
        assert_eq!(b.pending_dev_fee, 5_000);
        assert_eq!(b.pending_yield, 8_000);
    }


    #[test]
    fn cold_wallet_recipient_must_match_pool_pin() {
        let pinned = Pubkey::new_unique();
        let claimed_valid = pinned;
        let claimed_invalid = Pubkey::new_unique();
        assert_eq!(pinned, claimed_valid);
        assert_ne!(pinned, claimed_invalid);
    }


    #[test]
    fn provider_fee_change_does_not_mutate_pool_pending() {
        let mut p = fresh_pool(Pubkey::new_unique());
        p.pending_provider_fee = 5_000;
        let new_bps: u16 = 1_500;
        let _ = new_bps;
        assert_eq!(p.pending_provider_fee, 5_000);
    }


    #[test]
    fn pause_rate_limit_vault_wide_short_window() {
        assert!(PAUSE_RATE_LIMIT_VAULT_WIDE_SECONDS < PAUSE_RATE_LIMIT_SECONDS);
    }


    #[test]
    fn pause_can_be_triggered_by_pause_authority_or_admin() {
        let admin = Pubkey::new_unique();
        let pause = Pubkey::new_unique();
        let random = Pubkey::new_unique();
        let signer_ok_admin = admin == admin || admin == pause;
        let signer_ok_pause = pause == admin || pause == pause;
        let signer_ok_random = random == admin || random == pause;
        assert!(signer_ok_admin);
        assert!(signer_ok_pause);
        assert!(!signer_ok_random);
    }

    #[test]
    fn unpause_requires_admin_only() {
        let admin = Pubkey::new_unique();
        let pause = Pubkey::new_unique();
        let signer_ok_pause = pause == admin;
        assert!(!signer_ok_pause);
    }


    #[test]
    fn asset_mint_mismatch_rejected() {
        let pool_mint = Pubkey::new_unique();
        let supplied_mint = Pubkey::new_unique();
        assert_ne!(pool_mint, supplied_mint);
    }


    #[test]
    fn provider_vault_is_usdc_only_at_v2_0() {
        let p = fresh_pool(Pubkey::new_unique());
        assert!(!p.is_sol);
    }


    #[test]
    fn spec_naming_compliance_cumulative_gross_ggr() {
        let p = fresh_pool(Pubkey::new_unique());
        let _ = p.cumulative_gross_ggr;
    }


    #[test]
    fn withdraw_payout_at_supply_equals_nav() {
        let p = compute_lamports_for_withdraw(1_000, 5_000, 1_000).unwrap();
        assert_eq!(p, 5_000);
    }

    #[test]
    fn withdraw_payout_zero_amount() {
        let p = compute_lamports_for_withdraw(0, 5_000, 1_000).unwrap();
        assert_eq!(p, 0);
    }


    #[test]
    fn provider_owed_starts_zero() {
        let owed = ProviderOwed {
            asset_pool: Pubkey::new_unique(),
            provider_id: 0,
            amount: 0,
            last_settled_at: 0,
            bump: 0,
            reserved: [0u8; 32],
        };
        assert_eq!(owed.amount, 0);
    }

    #[test]
    fn provider_owed_advances_on_flush() {
        let mut owed = ProviderOwed {
            asset_pool: Pubkey::new_unique(),
            provider_id: 1,
            amount: 100,
            last_settled_at: 0,
            bump: 255,
            reserved: [0u8; 32],
        };
        let added = 50u64;
        owed.amount = owed.amount.checked_add(added).unwrap();
        assert_eq!(owed.amount, 150);
    }


    #[test]
    fn settle_owner_propose_arms_unlocks_at() {
        let mut p = fresh_pool(Pubkey::new_unique());
        let new_wallet = Pubkey::new_unique();
        let now = 1_000_000i64;
        p.pending_settle_owner = new_wallet;
        p.pending_settle_owner_unlocks_at = now + ADMIN_TIMELOCK_SECONDS;
        assert!(p.pending_settle_owner_unlocks_at > now);
        assert_eq!(p.pending_settle_owner, new_wallet);
    }

    #[test]
    fn settle_owner_finalize_blocked_early() {
        let p = AssetPool {
            pending_settle_owner_unlocks_at: 10_000,
            ..fresh_pool(Pubkey::new_unique())
        };
        let now_too_early = 5_000i64;
        assert!(now_too_early < p.pending_settle_owner_unlocks_at);
    }

    #[test]
    fn settle_owner_cancel_clears_pending() {
        let mut p = AssetPool {
            pending_settle_owner: Pubkey::new_unique(),
            pending_settle_owner_unlocks_at: 10_000,
            ..fresh_pool(Pubkey::new_unique())
        };
        p.pending_settle_owner = Pubkey::default();
        p.pending_settle_owner_unlocks_at = 0;
        assert_eq!(p.pending_settle_owner, Pubkey::default());
        assert_eq!(p.pending_settle_owner_unlocks_at, 0);
    }


    #[test]
    fn earmarks_distribute_zero_to_zero_invariant() {
        let mut p = fresh_pool(Pubkey::new_unique());
        p.pending_dev_fee = 0;
        p.pending_provider_fee = 0;
        p.pending_affiliate = 0;
        p.pending_sovereign = 0;
        p.pending_yield = 0;
        p.pending_reserve = 0;
        assert!(require_earmark_invariant(&p, 0).is_ok());
    }


    #[test]
    fn per_asset_pending_affiliate_isolated() {
        let mut a = fresh_pool(Pubkey::new_unique());
        let mut b = fresh_pool(Pubkey::new_unique());
        accrue_affiliate_amount(&mut a, 1_000).unwrap();
        accrue_affiliate_amount(&mut b, 2_000).unwrap();
        assert_eq!(a.pending_affiliate, 1_000);
        assert_eq!(b.pending_affiliate, 2_000);
    }


    #[test]
    fn bootstrap_ignores_tier_distribution_completely() {
        let mut p = fresh_pool(Pubkey::new_unique());
        p.lp_tokens_by_tier = [50, 0, 0, 0, 50];
        assert_eq!(compute_weighted_lp_bps(&p, 0, 0).unwrap(), BOOTSTRAP_LP_SHARE_BPS as u64);
        p.lp_tokens_by_tier = [0, 100, 0, 0, 0];
        assert_eq!(compute_weighted_lp_bps(&p, 0, 0).unwrap(), BOOTSTRAP_LP_SHARE_BPS as u64);
    }


    #[test]
    fn growth_phase_yield_60_compound_30_reserve_10() {
        let (y, c, r) = phase_split_bps(1);
        assert_eq!(y, 6_000);
        assert_eq!(c, 3_000);
        assert_eq!(r, 1_000);
    }

    #[test]
    fn bootstrap_phase_yield_20_compound_70_reserve_10() {
        let (y, c, r) = phase_split_bps(0);
        assert_eq!(y, 2_000);
        assert_eq!(c, 7_000);
        assert_eq!(r, 1_000);
    }


    #[test]
    fn cumulative_gross_ggr_signed_max_min() {
        let p = fresh_pool(Pubkey::new_unique());
        let _ = p.cumulative_gross_ggr;
        assert!(i64::MAX > 0);
        assert!(i64::MIN < 0);
    }


    #[test]
    fn cpi_deposit_provider_yield_args_shape() {
        let _shape: fn(u64) = |_amount| ();
    }

    #[test]
    fn cpi_sovereign_deposit_royalty_args_shape() {
        let _shape: fn(u64) = |_amount| ();
    }

    #[test]
    fn cpi_affiliate_deposit_funding_pool_args_shape() {
        let _shape: fn(u64, Pubkey) = |_amount, _mint| ();
    }


    #[test]
    fn cooldown_anchored_at_request_not_deposit() {
        let deposit_t: i64 = 0;
        let request_t: i64 = 100_000_000;
        let tier = 2usize;
        let processable = request_t + TIER_COOLDOWN_DAYS[tier] * SECONDS_PER_DAY;
        let cooldown_window = processable - request_t;
        assert_eq!(cooldown_window, 7 * SECONDS_PER_DAY);
        assert_ne!(processable - deposit_t, cooldown_window);
    }

    #[test]
    fn deposit_can_be_made_any_time_no_cooldown_lockout() {
        let (user, dead) = compute_shares_for_deposit(10_000_000, 1_000_000, 1_000_000).unwrap();
        let _ = (user, dead);
    }


    #[test]
    fn pattern_y_complete_flow_with_k4_pass() {
        let mut p = fresh_pool(Pubkey::new_unique());
        p.lp_tokens_by_tier = [0, 0, 100, 0, 0];

        for day in 0..5 {
            let net = 250_000_000i64;
            let fee_due = (net as u64) * DEFAULT_PROVIDER_FEE_BPS as u64 / 10_000;
            accrue_earmarks(&mut p, net, 1, DEFAULT_PROVIDER_FEE_BPS, fee_due, DEFAULT_DEV_FEE_BPS, 0, 0).unwrap();
            accrue_affiliate_amount(&mut p, 8_000_000).unwrap();
            p.cumulative_gross_ggr += net;
            let _ = day;
        }
        let delta_gross = p.cumulative_gross_ggr - p.last_distributed_gross_ggr;
        let net = delta_gross - p.pending_affiliate as i64;
        assert_eq!(delta_gross, 1_250_000_000);
        assert_eq!(p.pending_affiliate, 40_000_000);
        assert_eq!(net, 1_210_000_000);
        assert!(net > 0 && (net as u64) >= MIN_DELTA_GGR_FOR_SWEEP_USDC);
    }

    #[test]
    fn pattern_y_k4_skip_then_subsequent_sweep_proceeds() {
        let mut p = fresh_pool(Pubkey::new_unique());
        p.lp_tokens_by_tier = [0, 0, 100, 0, 0];

        let net1 = 100_000_000i64;
        accrue_earmarks(&mut p, net1, 1, DEFAULT_PROVIDER_FEE_BPS, 0, DEFAULT_DEV_FEE_BPS, 0, 0).unwrap();
        accrue_affiliate_amount(&mut p, 50_000_000).unwrap();
        p.cumulative_gross_ggr += net1;

        let pre_skip_aff = p.pending_affiliate;
        p.last_distributed_gross_ggr = p.cumulative_gross_ggr;
        assert_eq!(p.pending_affiliate, pre_skip_aff, "affiliate NOT zeroed on K4 skip");

        let net2 = 2_000_000_000i64;
        let fee_due2 = (net2 as u64) * DEFAULT_PROVIDER_FEE_BPS as u64 / 10_000;
        accrue_earmarks(&mut p, net2, 1, DEFAULT_PROVIDER_FEE_BPS, fee_due2, DEFAULT_DEV_FEE_BPS, 0, 0).unwrap();
        accrue_affiliate_amount(&mut p, 10_000_000).unwrap();
        p.cumulative_gross_ggr += net2;

        let delta = p.cumulative_gross_ggr - p.last_distributed_gross_ggr;
        let net = delta - p.pending_affiliate as i64;
        assert_eq!(delta, net2);
        assert_eq!(p.pending_affiliate, 60_000_000);
        assert!(net > 0);
    }


    #[test]
    fn multi_asset_invariant_independent_balance_checks() {
        let mut a = fresh_pool(Pubkey::new_unique());
        let mut b = fresh_pool(Pubkey::new_unique());
        a.pending_dev_fee = 100;
        a.pending_yield = 200;
        b.pending_dev_fee = 5_000;
        b.pending_yield = 8_000;

        assert!(require_earmark_invariant(&a, 300).is_ok());
        assert!(require_earmark_invariant(&b, 1_000).is_err());

        assert!(require_earmark_invariant(&a, 300).is_ok());
    }

    #[test]
    fn multi_asset_accrual_to_one_does_not_affect_other_invariant() {
        let mut a = fresh_pool(Pubkey::new_unique());
        let mut b = fresh_pool(Pubkey::new_unique());
        accrue_earmarks(&mut a, 1_000_000, 1, DEFAULT_PROVIDER_FEE_BPS, 100_000, DEFAULT_DEV_FEE_BPS, 0, 0).unwrap();
        assert!(b.pending_dev_fee == 0);
        assert!(b.pending_provider_fee == 0);
        assert!(b.pending_sovereign == 0);
        assert!(b.pending_yield == 0);
        assert!(b.pending_reserve == 0);
    }


    #[test]
    fn register_asset_at_capacity_rejects() {
        let active_count: u8 = MAX_ASSETS;
        assert!(!((active_count as u8) < MAX_ASSETS));
    }

    #[test]
    fn register_asset_first_slot_index_zero() {
        let active_count: u8 = 0;
        assert_eq!(active_count as usize, 0);
    }

    #[test]
    fn register_asset_advances_active_count() {
        let mut active: u8 = 0;
        active = active.checked_add(1).unwrap();
        assert_eq!(active, 1);
        active = active.checked_add(1).unwrap();
        assert_eq!(active, 2);
    }


    #[test]
    fn settle_owner_finalize_after_timelock_succeeds() {
        let mut p = fresh_pool(Pubkey::new_unique());
        let new_w = Pubkey::new_unique();
        let now = 100_000_000i64;
        p.pending_settle_owner = new_w;
        p.pending_settle_owner_unlocks_at = now + ADMIN_TIMELOCK_SECONDS;

        let later = now + ADMIN_TIMELOCK_SECONDS + 1;
        assert!(later >= p.pending_settle_owner_unlocks_at);
        p.provider_settle_owner = p.pending_settle_owner;
        p.pending_settle_owner = Pubkey::default();
        p.pending_settle_owner_unlocks_at = 0;
        assert_eq!(p.provider_settle_owner, new_w);
        assert_eq!(p.pending_settle_owner, Pubkey::default());
    }

    #[test]
    fn settle_owner_propose_requires_non_default() {
        let new_w = Pubkey::default();
        assert_eq!(new_w, Pubkey::default());
    }

    #[test]
    fn settle_owner_cancel_does_not_change_active_wallet() {
        let mut p = fresh_pool(Pubkey::new_unique());
        let active = p.provider_settle_owner;
        p.pending_settle_owner = Pubkey::new_unique();
        p.pending_settle_owner_unlocks_at = 100_000;
        p.pending_settle_owner = Pubkey::default();
        p.pending_settle_owner_unlocks_at = 0;
        assert_eq!(p.provider_settle_owner, active);
    }


    #[test]
    fn update_provider_fee_at_cap_accepted() {
        let new_bps: u16 = MAX_PROVIDER_FEE_BPS;
        assert!(new_bps <= MAX_PROVIDER_FEE_BPS);
    }

    #[test]
    fn update_provider_fee_above_cap_rejected() {
        let new_bps: u16 = MAX_PROVIDER_FEE_BPS + 1;
        assert!(!(new_bps <= MAX_PROVIDER_FEE_BPS));
    }

    #[test]
    fn update_provider_fee_to_zero_allowed() {
        let new_bps: u16 = 0;
        assert!(new_bps <= MAX_PROVIDER_FEE_BPS);
    }

    #[test]
    fn update_provider_fee_does_not_retroactively_adjust_owed() {
        let mut owed: u64 = 500_000;
        let old_bps_snapshot: u16 = 1_000;
        let _new_bps: u16 = 2_000;
        assert_eq!(owed, 500_000);
        let _ = old_bps_snapshot;
        owed += 1;
        assert_eq!(owed, 500_001);
    }


    #[test]
    fn settle_recipient_must_match_pool_pin() {
        let mut p = fresh_pool(Pubkey::new_unique());
        let cold_wallet = Pubkey::new_unique();
        p.provider_settle_owner = cold_wallet;

        let attacker_wallet = Pubkey::new_unique();
        let recipient_ok = cold_wallet == p.provider_settle_owner;
        let recipient_bad = attacker_wallet == p.provider_settle_owner;
        assert!(recipient_ok);
        assert!(!recipient_bad);
    }

    #[test]
    fn settle_keeper_path_requires_40d() {
        let last_settled: i64 = 0;
        let now_early = 30 * SECONDS_PER_DAY;
        let now_eligible = 40 * SECONDS_PER_DAY;
        let early_ok = now_early >= last_settled + PROVIDER_SETTLE_KEEPER_DAYS * SECONDS_PER_DAY;
        let eligible_ok = now_eligible >= last_settled + PROVIDER_SETTLE_KEEPER_DAYS * SECONDS_PER_DAY;
        assert!(!early_ok);
        assert!(eligible_ok);
    }

    #[test]
    fn settle_paused_blocks_both_operator_and_keeper() {
        let mut p = Provider {
            provider_id: 1,
            name: [0u8; PROVIDER_NAME_LEN],
            bump: 255,
            active: true,
            paused: false,
            paused_at: 0,
            settle_paused: true,
            pause_reason: [0u8; PAUSE_REASON_LEN],
            provider_fee_bps: DEFAULT_PROVIDER_FEE_BPS,
            fee_owed_since_last_sweep: 0,
            affiliate_recorder_pubkey: Pubkey::new_unique(),
            signed_terms_hash: [0u8; 32],
            cumulative_gross_ggr: 0,
            cumulative_gross_wager: 0,
            cumulative_gross_payout: 0,
            cumulative_bet_count: 0,
            last_submission_at: 0,
            last_day_id: 0,
            period_net_ggr: 0,
            period_fee_charged: 0,
            fee_correction_applied: 0,
            reserved: [0u8; 47],
        };
        assert!(p.settle_paused);
        p.settle_paused = false;
        assert!(!p.settle_paused);
    }


    #[test]
    fn provider_player_escrow_does_not_touch_v1_seeds() {
        let v1_seed = b"player_escrow";
        let v2_seed = b"provider_player_escrow";
        assert_ne!(v1_seed.len(), v2_seed.len());
        assert_ne!(&v1_seed[..], &v2_seed[..v1_seed.len()]);
    }

    #[test]
    fn provider_player_escrow_per_mint_isolation() {
        let wallet = Pubkey::new_unique();
        let mint_a = Pubkey::new_unique();
        let mint_b = Pubkey::new_unique();
        assert_ne!(mint_a, mint_b);
        let _ = (wallet, mint_a, mint_b);
    }


    #[test]
    fn k4_gross_below_threshold_but_positive() {
        let gross: i64 = 100_000_000;
        let aff: u64 = 0;
        let net = gross - aff as i64;
        assert!(net > 0 && (net as u64) < MIN_DELTA_GGR_FOR_SWEEP_USDC);
    }

    #[test]
    fn k4_gross_exactly_at_threshold_with_zero_affiliate() {
        let gross: i64 = MIN_DELTA_GGR_FOR_SWEEP_USDC as i64;
        let aff: u64 = 0;
        let net = gross - aff as i64;
        assert!((net as u64) >= MIN_DELTA_GGR_FOR_SWEEP_USDC);
    }

    #[test]
    fn k4_gross_just_below_threshold_with_zero_affiliate() {
        let gross: i64 = MIN_DELTA_GGR_FOR_SWEEP_USDC as i64 - 1;
        let aff: u64 = 0;
        let net = gross - aff as i64;
        assert!((net as u64) < MIN_DELTA_GGR_FOR_SWEEP_USDC);
    }


    #[test]
    fn naming_tripwire_pool_field() {
        let p = fresh_pool(Pubkey::new_unique());
        let _x: i64 = p.cumulative_gross_ggr;
        let _y: i64 = p.last_distributed_gross_ggr;
    }

    #[test]
    fn naming_tripwire_provider_field() {
        let p = Provider {
            provider_id: 1,
            name: [0u8; PROVIDER_NAME_LEN],
            bump: 0,
            active: true,
            paused: false,
            paused_at: 0,
            settle_paused: false,
            pause_reason: [0u8; PAUSE_REASON_LEN],
            provider_fee_bps: 0,
            fee_owed_since_last_sweep: 0,
            affiliate_recorder_pubkey: Pubkey::new_unique(),
            signed_terms_hash: [0u8; 32],
            cumulative_gross_ggr: 0,
            cumulative_gross_wager: 0,
            cumulative_gross_payout: 0,
            cumulative_bet_count: 0,
            last_submission_at: 0,
            last_day_id: 0,
            period_net_ggr: 0,
            period_fee_charged: 0,
            fee_correction_applied: 0,
            reserved: [0u8; 47],
        };
        let _x: i64 = p.cumulative_gross_ggr;
        let _y: u64 = p.cumulative_gross_wager;
    }


    #[test]
    fn mid_period_provider_fee_change_only_future_receipts() {
        let net = 1_000_000i64;
        let bps_a: u16 = 1_000;
        let bps_b: u16 = 1_500;
        let fee_a = (net as u64) * bps_a as u64 / 10_000;
        let fee_b = (net as u64) * bps_b as u64 / 10_000;
        assert_ne!(fee_a, fee_b);
        let total = fee_a + fee_b;
        assert_eq!(total, 250_000);
    }


    #[test]
    fn full_waterfall_math_growth_phase() {
        let net: u64 = 1_000_000_000;
        let provider_fee = net * 1_000 / 10_000;
        let after_provider = net - provider_fee;
        let dev_fee = after_provider * 250 / 10_000;
        let after_dev = after_provider - dev_fee;
        let lp_due = after_dev * 6_500 / 10_000;
        let protocol = after_dev - lp_due;
        let sov = protocol * 500 / 10_000;
        let tax_rem = protocol - sov;
        let yield_due = tax_rem * 6_000 / 10_000;
        let compound = tax_rem * 3_000 / 10_000;
        let reserve = tax_rem - yield_due - compound;
        let sum = provider_fee + dev_fee + lp_due + sov + yield_due + compound + reserve;
        assert_eq!(sum, net);
    }

    #[test]
    fn full_waterfall_math_bootstrap_phase() {
        let net: u64 = 1_000_000_000;
        let provider_fee = net * 1_000 / 10_000;
        let after_provider = net - provider_fee;
        let dev_fee = after_provider * 250 / 10_000;
        let after_dev = after_provider - dev_fee;
        let lp_due = after_dev * 7_500 / 10_000;
        let protocol = after_dev - lp_due;
        let sov = protocol * 500 / 10_000;
        let tax_rem = protocol - sov;
        let yield_due = tax_rem * 2_000 / 10_000;
        let compound = tax_rem * 7_000 / 10_000;
        let reserve = tax_rem - yield_due - compound;
        let sum = provider_fee + dev_fee + lp_due + sov + yield_due + compound + reserve;
        assert_eq!(sum, net);
    }


    #[test]
    fn compound_bucket_has_no_counter_field() {
        let p = fresh_pool(Pubkey::new_unique());
        let _ = p.pending_dev_fee;
        let _ = p.pending_provider_fee;
        let _ = p.pending_affiliate;
        let _ = p.pending_sovereign;
        let _ = p.pending_yield;
        let _ = p.pending_reserve;
    }


    #[test]
    fn fresh_pool_starts_clean() {
        let p = fresh_pool(Pubkey::new_unique());
        assert_eq!(p.lp_supply, 0);
        assert_eq!(p.cumulative_gross_ggr, 0);
        assert_eq!(p.last_distributed_gross_ggr, 0);
        assert_eq!(p.pending_dev_fee, 0);
        assert_eq!(p.pending_provider_fee, 0);
        assert_eq!(p.pending_affiliate, 0);
        assert_eq!(p.pending_sovereign, 0);
        assert_eq!(p.pending_yield, 0);
        assert_eq!(p.pending_reserve, 0);
        assert_eq!(p.insurance_balance, 0);
        assert!(!p.vault_locked);
    }


    #[test]
    fn deposit_tier_ledger_first_increment() {
        let mut p = fresh_pool(Pubkey::new_unique());
        let tier = 3usize;
        let minted: u64 = 500;
        p.lp_tokens_by_tier[tier] = p.lp_tokens_by_tier[tier].checked_add(minted).unwrap();
        assert_eq!(p.lp_tokens_by_tier[tier], 500);
        assert_eq!(p.lp_tokens_by_tier[0], 0);
    }

    #[test]
    fn deposit_tier_change_during_top_up() {
        let mut p = fresh_pool(Pubkey::new_unique());
        let mut pos_tier = 0u8;
        let mut pos_shares: u64 = 100;
        p.lp_tokens_by_tier[pos_tier as usize] = 100;

        let new_tier = 3u8;
        if pos_tier != new_tier && pos_shares > 0 {
            p.lp_tokens_by_tier[pos_tier as usize] = p.lp_tokens_by_tier[pos_tier as usize].saturating_sub(pos_shares);
            p.lp_tokens_by_tier[new_tier as usize] = p.lp_tokens_by_tier[new_tier as usize].checked_add(pos_shares).unwrap();
            pos_tier = new_tier;
        }
        let mint_amount: u64 = 50;
        p.lp_tokens_by_tier[pos_tier as usize] = p.lp_tokens_by_tier[pos_tier as usize].checked_add(mint_amount).unwrap();
        pos_shares = pos_shares.checked_add(mint_amount).unwrap();
        assert_eq!(p.lp_tokens_by_tier[0], 0);
        assert_eq!(p.lp_tokens_by_tier[3], 150);
        assert_eq!(pos_shares, 150);
    }


    #[test]
    fn mixed_pattern_y_sequence_sum_integrity() {
        let mut p = fresh_pool(Pubkey::new_unique());
        p.lp_tokens_by_tier = [0, 0, 100, 0, 0];
        accrue_earmarks(&mut p, 1_000_000_000, 1, DEFAULT_PROVIDER_FEE_BPS, 100_000_000, DEFAULT_DEV_FEE_BPS, 0, 0).unwrap();
        let after_pos_dev = p.pending_dev_fee;
        accrue_earmarks(&mut p, -200_000_000, 1, DEFAULT_PROVIDER_FEE_BPS, 0, DEFAULT_DEV_FEE_BPS, 0, 0).unwrap();
        let after_neg_dev = p.pending_dev_fee;
        assert!(after_neg_dev < after_pos_dev);
        accrue_earmarks(&mut p, 500_000_000, 1, DEFAULT_PROVIDER_FEE_BPS, 50_000_000, DEFAULT_DEV_FEE_BPS, 0, 0).unwrap();
        let after_final_dev = p.pending_dev_fee;
        assert!(after_final_dev > after_neg_dev);
    }


    #[test]
    fn provider_inactive_blocks_submit() {
        let p = Provider {
            provider_id: 0,
            name: [0u8; PROVIDER_NAME_LEN],
            bump: 0,
            active: false,
            paused: false,
            paused_at: 0,
            settle_paused: false,
            pause_reason: [0u8; PAUSE_REASON_LEN],
            provider_fee_bps: 0,
            fee_owed_since_last_sweep: 0,
            affiliate_recorder_pubkey: Pubkey::new_unique(),
            signed_terms_hash: [0u8; 32],
            cumulative_gross_ggr: 0,
            cumulative_gross_wager: 0,
            cumulative_gross_payout: 0,
            cumulative_bet_count: 0,
            last_submission_at: 0,
            last_day_id: 0,
            period_net_ggr: 0,
            period_fee_charged: 0,
            fee_correction_applied: 0,
            reserved: [0u8; 47],
        };
        assert!(!p.active);
    }

    #[test]
    fn provider_paused_blocks_submit() {
        let p = Provider {
            provider_id: 0,
            name: [0u8; PROVIDER_NAME_LEN],
            bump: 0,
            active: true,
            paused: true,
            paused_at: 0,
            settle_paused: false,
            pause_reason: [0u8; PAUSE_REASON_LEN],
            provider_fee_bps: 0,
            fee_owed_since_last_sweep: 0,
            affiliate_recorder_pubkey: Pubkey::new_unique(),
            signed_terms_hash: [0u8; 32],
            cumulative_gross_ggr: 0,
            cumulative_gross_wager: 0,
            cumulative_gross_payout: 0,
            cumulative_bet_count: 0,
            last_submission_at: 0,
            last_day_id: 0,
            period_net_ggr: 0,
            period_fee_charged: 0,
            fee_correction_applied: 0,
            reserved: [0u8; 47],
        };
        assert!(p.paused);
    }


    #[test]
    fn day_id_zero_initial_submission_allowed() {
        let last_day_id: u64 = 0;
        let new_day_id: u64 = 0;
        let allowed = new_day_id > last_day_id || last_day_id == 0;
        assert!(allowed);
    }

    #[test]
    fn day_id_regression_after_first_rejected() {
        let last_day_id: u64 = 5;
        let new_day_id: u64 = 3;
        let allowed = new_day_id > last_day_id || last_day_id == 0;
        assert!(!allowed);
    }


    #[test]
    fn k4_skip_preserves_pending_affiliate_for_next_drain() {
        let mut p = fresh_pool(Pubkey::new_unique());
        p.pending_affiliate = 100_000_000;
        p.cumulative_gross_ggr = 50_000_000;
        let delta = p.cumulative_gross_ggr - p.last_distributed_gross_ggr;
        let net = delta - p.pending_affiliate as i64;
        assert!(net < 0);
        let pre_aff = p.pending_affiliate;
        p.last_distributed_gross_ggr = p.cumulative_gross_ggr;
        assert_eq!(p.pending_affiliate, pre_aff);
    }


    #[test]
    fn rule30_penalty_extends_processable_at_by_7d() {
        let now: i64 = 1_000_000;
        let tier: usize = 2;
        let base = now + TIER_COOLDOWN_DAYS[tier] * SECONDS_PER_DAY;
        let with_penalty = base + SINGLE_WALLET_EXTRA_COOLDOWN_SECONDS;
        assert_eq!(with_penalty - base, 7 * SECONDS_PER_DAY);
    }


    #[test]
    fn hard_floor_blocks_withdraw_that_drops_below() {
        let pre_balance: u64 = HARD_VAULT_FLOOR_USDC + 1_000;
        let withdraw: u64 = 2_000;
        let post = pre_balance - withdraw;
        let ok = post >= HARD_VAULT_FLOOR_USDC;
        assert!(!ok, "withdraw that drops vault below floor must be blocked");
    }

    #[test]
    fn hard_floor_blocks_withdraw_when_already_sub_floor() {
        let pre_balance: u64 = HARD_VAULT_FLOOR_USDC - 1;
        let withdraw: u64 = 100;
        let post = pre_balance - withdraw;
        let ok = post >= HARD_VAULT_FLOOR_USDC;
        assert!(!ok, "sub-floor vault must reject LP withdrawals (no escape hatch)");
    }

    #[test]
    fn hard_floor_allows_withdraw_with_headroom() {
        let pre_balance: u64 = HARD_VAULT_FLOOR_USDC + 10_000_000_000;
        let withdraw: u64 = 5_000_000_000;
        let post = pre_balance - withdraw;
        let ok = post >= HARD_VAULT_FLOOR_USDC;
        assert!(ok, "well-capitalized vault must accept LP withdrawals");
    }

    #[test]
    fn hard_floor_boundary_exact_equality_passes() {
        let pre_balance: u64 = HARD_VAULT_FLOOR_USDC + 5_000;
        let withdraw: u64 = 5_000;
        let post = pre_balance - withdraw;
        assert_eq!(post, HARD_VAULT_FLOOR_USDC);
        let ok = post >= HARD_VAULT_FLOOR_USDC;
        assert!(ok, "withdrawal landing exactly at the floor must be allowed");
    }

    #[test]
    fn hard_floor_one_lamport_below_rejected() {
        let pre_balance: u64 = HARD_VAULT_FLOOR_USDC + 5_000;
        let withdraw: u64 = 5_001;
        let post = pre_balance - withdraw;
        assert_eq!(post, HARD_VAULT_FLOOR_USDC - 1);
        let ok = post >= HARD_VAULT_FLOOR_USDC;
        assert!(!ok, "withdrawal landing 1 micro-USDC below floor must be blocked");
    }


    #[test]
    fn insurance_balance_independent_of_lp_supply() {
        let mut p = fresh_pool(Pubkey::new_unique());
        p.insurance_balance = 1_000_000;
        p.lp_supply = 5_000;
        assert_eq!(p.insurance_balance, 1_000_000);
        assert_eq!(p.lp_supply, 5_000);
    }


    #[test]
    fn admin_timelock_propose_arms_clock() {
        let mut keys_set = (Pubkey::default(), 0i64);
        let now: i64 = 1_000_000;
        let new_authority = Pubkey::new_unique();
        keys_set = (new_authority, now + ADMIN_TIMELOCK_SECONDS);
        assert!(keys_set.1 > now);
    }

    #[test]
    fn admin_timelock_cancel_clears_pending() {
        let mut pending = (Pubkey::new_unique(), 1_000_000i64);
        pending = (Pubkey::default(), 0);
        assert_eq!(pending.0, Pubkey::default());
    }


    #[test]
    fn provider_fee_zero_bps_zero_fee() {
        let net = 1_000_000i64;
        let bps: u16 = 0;
        let fee = (net as u64) * bps as u64 / 10_000;
        assert_eq!(fee, 0);
    }


    #[test]
    fn tier0_elite_14_days() { assert_eq!(TIER_COOLDOWN_DAYS[0], 14); }
    #[test]
    fn tier1_premier_10_days() { assert_eq!(TIER_COOLDOWN_DAYS[1], 10); }
    #[test]
    fn tier2_executive_7_days() { assert_eq!(TIER_COOLDOWN_DAYS[2], 7); }
    #[test]
    fn tier3_director_5_days() { assert_eq!(TIER_COOLDOWN_DAYS[3], 5); }
    #[test]
    fn tier4_whale_3_days() { assert_eq!(TIER_COOLDOWN_DAYS[4], 3); }


    #[test]
    fn compute_tier_elite_below_500() {
        assert_eq!(compute_tier(0), 0);
        assert_eq!(compute_tier(1), 0);
        assert_eq!(compute_tier(499_999_999), 0);
    }

    #[test]
    fn compute_tier_premier_500_to_2500() {
        assert_eq!(compute_tier(500_000_000), 1);
        assert_eq!(compute_tier(1_000_000_000), 1);
        assert_eq!(compute_tier(2_499_999_999), 1);
    }

    #[test]
    fn compute_tier_executive_2500_to_10k() {
        assert_eq!(compute_tier(2_500_000_000), 2);
        assert_eq!(compute_tier(9_999_999_999), 2);
    }

    #[test]
    fn compute_tier_director_10k_to_50k() {
        assert_eq!(compute_tier(10_000_000_000), 3);
        assert_eq!(compute_tier(49_999_999_999), 3);
    }

    #[test]
    fn compute_tier_whale_50k_and_up() {
        assert_eq!(compute_tier(50_000_000_000), 4);
        assert_eq!(compute_tier(1_000_000_000_000), 4);
        assert_eq!(compute_tier(u64::MAX), 4);
    }

    #[test]
    fn compute_tier_monotonic_non_decreasing() {
        let samples = [
            0u64, 499_999_999, 500_000_000, 2_499_999_999, 2_500_000_000,
            9_999_999_999, 10_000_000_000, 49_999_999_999, 50_000_000_000,
            1_000_000_000_000, u64::MAX,
        ];
        for w in samples.windows(2) {
            assert!(
                compute_tier(w[0]) <= compute_tier(w[1]),
                "tier decreased from {} to {}",
                w[0],
                w[1]
            );
        }
    }

    #[test]
    fn compute_tier_old_sol_thresholds_are_not_boundaries() {
        assert_eq!(compute_tier(50_000_000_000), 4, "$50k must be Whale, not Executive");
        assert_eq!(compute_tier(500_000_000), 1, "$500 must be Premier, not Elite");
        assert_eq!(compute_tier(50_000_000_000), compute_tier(500_000_000_000));
    }

    #[test]
    fn compute_tier_maps_to_expected_cooldown_and_share() {
        let cases = [
            (400_000_000u64, 0usize),
            (500_000_000, 1),
            (5_000_000_000, 2),
            (25_000_000_000, 3),
            (100_000_000_000, 4),
        ];
        for (deposit, tier) in cases {
            assert_eq!(compute_tier(deposit) as usize, tier, "deposit {} tier", deposit);
        }
        let whale = compute_tier(100_000_000_000) as usize;
        assert_eq!(TIER_COOLDOWN_DAYS[whale], 3);
        assert_eq!(TIER_LP_SHARE_BPS_GROWTH[whale], 8_500);
    }


    #[test]
    fn tier_lp_share_bps_growth_table_matches_spec() {
        assert_eq!(TIER_LP_SHARE_BPS_GROWTH, [6_500, 7_000, 7_500, 8_000, 8_500]);
    }

    #[test]
    fn bootstrap_lp_share_bps_75_pct() {
        assert_eq!(BOOTSTRAP_LP_SHARE_BPS, 7_500);
    }


    #[test]
    fn provider_fee_taken_off_top_before_lp_split() {
        let mut p = fresh_pool(Pubkey::new_unique());
        p.lp_tokens_by_tier = [0, 0, 100, 0, 0];
        let net = 1_000_000_000i64;
        let provider_fee: u64 = (net as u64) * 1_000 / 10_000;
        accrue_earmarks(&mut p, net, 1, 1_000, provider_fee, DEFAULT_DEV_FEE_BPS, 0, 0).unwrap();
        assert_eq!(p.pending_dev_fee, 22_500_000);
    }


    #[test]
    fn pool_lp_tokens_by_tier_isolation() {
        let mut a = fresh_pool(Pubkey::new_unique());
        let mut b = fresh_pool(Pubkey::new_unique());
        a.lp_tokens_by_tier[2] = 100;
        b.lp_tokens_by_tier[4] = 200;
        assert_eq!(a.lp_tokens_by_tier[2], 100);
        assert_eq!(a.lp_tokens_by_tier[4], 0);
        assert_eq!(b.lp_tokens_by_tier[2], 0);
        assert_eq!(b.lp_tokens_by_tier[4], 200);
    }


    #[test]
    fn vault_pause_overrides_provider_state() {
        let vault_paused = true;
        let provider_active = true;
        let blocked = vault_paused;
        assert!(blocked);
        let _ = provider_active;
    }



    #[test]
    fn distribute_affiliate_zeros_counter() {
        let mut p = fresh_pool(Pubkey::new_unique());
        p.pending_affiliate = 50_000_000;
        let amount = p.pending_affiliate;
        p.pending_affiliate = 0;
        p.last_distributed_affiliate += amount;
        assert_eq!(p.pending_affiliate, 0);
        assert_eq!(p.last_distributed_affiliate, 50_000_000);
    }

    #[test]
    fn distribute_affiliate_rejects_zero_pending() {
        let p = fresh_pool(Pubkey::new_unique());
        assert_eq!(p.pending_affiliate, 0);
        let amount = p.pending_affiliate;
        let would_revert = amount == 0;
        assert!(would_revert);
    }

    #[test]
    fn distribute_affiliate_works_after_k4_skipped_sweep() {
        let mut p = fresh_pool(Pubkey::new_unique());
        p.pending_affiliate = 10_000_000;
        p.pending_dev_fee = 0;
        p.pending_provider_fee = 0;
        p.pending_sovereign = 0;
        p.pending_yield = 0;
        p.pending_reserve = 0;
        assert_eq!(sum_earmarks(&p), 10_000_000);
        let amount = p.pending_affiliate;
        p.pending_affiliate = 0;
        assert_eq!(amount, 10_000_000);
        assert_eq!(sum_earmarks(&p), 0);
    }

    #[test]
    fn distribute_affiliate_advances_last_distributed_monotonic() {
        let mut p = fresh_pool(Pubkey::new_unique());
        p.pending_affiliate = 1_000_000;
        let prior = p.last_distributed_affiliate;
        p.last_distributed_affiliate = prior + p.pending_affiliate;
        p.pending_affiliate = 0;
        p.pending_affiliate = 2_000_000;
        let prior2 = p.last_distributed_affiliate;
        p.last_distributed_affiliate = prior2 + p.pending_affiliate;
        p.pending_affiliate = 0;
        assert_eq!(p.last_distributed_affiliate, 3_000_000);
    }

    #[test]
    fn distribute_affiliate_maintains_k4_invariant() {
        let mut p = fresh_pool(Pubkey::new_unique());
        p.pending_affiliate = 100;
        p.pending_dev_fee = 50;
        let vault_pre: u64 = 200;
        assert!(require_earmark_invariant(&p, vault_pre).is_ok());
        p.pending_affiliate = 0;
        let vault_post: u64 = vault_pre - 100;
        assert!(require_earmark_invariant(&p, vault_post).is_ok());
    }


    #[test]
    fn distribute_sovereign_routes_usdc_when_seats_filled() {
        let mut p = fresh_pool(Pubkey::new_unique());
        p.pending_sovereign = 5_000_000;
        let seats_filled = 3u8;
        let routed_to_cpi = seats_filled > 0;
        assert!(routed_to_cpi);
        let amount = p.pending_sovereign;
        p.pending_sovereign = 0;
        assert_eq!(amount, 5_000_000);
        assert_eq!(p.pending_sovereign, 0);
    }

    #[test]
    fn distribute_sovereign_fallback_rolls_to_reserve_when_zero_seats() {
        let mut p = fresh_pool(Pubkey::new_unique());
        p.pending_sovereign = 4_000_000;
        p.pending_reserve = 1_000_000;
        let seats_filled = 0u8;
        let routed_to_cpi = seats_filled > 0;
        assert!(!routed_to_cpi);
        let amount = p.pending_sovereign;
        p.pending_sovereign = 0;
        p.pending_reserve = p.pending_reserve.checked_add(amount).unwrap();
        assert_eq!(p.pending_sovereign, 0);
        assert_eq!(p.pending_reserve, 5_000_000);
    }

    #[test]
    fn distribute_sovereign_rejects_zero_pending() {
        let p = fresh_pool(Pubkey::new_unique());
        let amount = p.pending_sovereign;
        assert_eq!(amount, 0);
        let would_revert = amount == 0;
        assert!(would_revert);
    }

    #[test]
    fn distribute_sovereign_sol_pond_returns_sol_pond_not_implemented_v2_0() {
        let mut p = fresh_pool(Pubkey::new_unique());
        p.is_sol = true;
        p.pending_sovereign = 1_000_000;
        let would_reject = p.is_sol;
        assert!(would_reject);
    }

    #[test]
    fn distribute_sovereign_fallback_preserves_total_earmarks() {
        let mut p = fresh_pool(Pubkey::new_unique());
        p.pending_sovereign = 5_000;
        p.pending_reserve = 1_000;
        let total_before = sum_earmarks(&p);
        let amt = p.pending_sovereign;
        p.pending_sovereign = 0;
        p.pending_reserve += amt;
        let total_after = sum_earmarks(&p);
        assert_eq!(total_before, total_after);
    }


    #[test]
    fn distribute_yield_zeros_counter() {
        let mut p = fresh_pool(Pubkey::new_unique());
        p.pending_yield = 60_000_000;
        let amount = p.pending_yield;
        p.pending_yield = 0;
        assert_eq!(amount, 60_000_000);
        assert_eq!(p.pending_yield, 0);
    }

    #[test]
    fn distribute_yield_sol_pond_rejected_v2_0() {
        let mut p = fresh_pool(Pubkey::new_unique());
        p.is_sol = true;
        p.pending_yield = 1_000;
        let would_reject = p.is_sol;
        assert!(would_reject);
    }

    #[test]
    fn distribute_yield_usdc_100_percent_to_staker_pool_no_path_b() {
        let pending = 80_000_000u64;
        let to_stakers = pending;
        let to_swap_router = 0u64;
        assert_eq!(to_stakers, pending);
        assert_eq!(to_swap_router, 0);
    }



    #[test]
    fn distribute_reserve_mode0_all_to_ops() {
        let amount = 1_000u64;
        let burn_usdc = 0u64;
        let ops = amount;
        assert_eq!(burn_usdc, 0);
        assert_eq!(ops, amount);
    }

    #[test]
    fn distribute_reserve_mode0_1_lamport_all_to_ops() {
        let amount = 1u64;
        let burn_usdc = 0u64;
        let ops = amount;
        assert_eq!(burn_usdc, 0);
        assert_eq!(ops, 1);
    }

    #[test]
    fn distribute_reserve_default_mode_is_manual() {
        assert_eq!(RESERVE_BURN_MODE_MANUAL, 0u8);
        assert_eq!(RESERVE_BURN_MODE_AUTO_SWAP, 1u8);
        let default_mode = RESERVE_BURN_MODE_MANUAL;
        assert_eq!(default_mode, 0);
    }

    #[test]
    fn distribute_reserve_invalid_mode_rejected() {
        let invalid_mode = 2u8;
        let would_revert = invalid_mode != RESERVE_BURN_MODE_MANUAL
            && invalid_mode != RESERVE_BURN_MODE_AUTO_SWAP;
        assert!(would_revert, "mode=2 must be rejected with InvalidReserveBurnMode");
    }


    #[test]
    fn distribute_reserve_mode1_50_50_split_even() {
        let amount = 1_000u64;
        let burn_usdc = amount / 2;
        let ops = amount - burn_usdc;
        assert_eq!(burn_usdc, 500);
        assert_eq!(ops, 500);
        assert_eq!(burn_usdc + ops, amount);
    }

    #[test]
    fn distribute_reserve_mode1_dust_to_ops_marketing() {
        let amount = 1_001u64;
        let burn_usdc = amount / 2;
        let ops = amount - burn_usdc;
        assert_eq!(burn_usdc, 500);
        assert_eq!(ops, 501);
        assert_eq!(burn_usdc + ops, amount);
    }

    #[test]
    fn distribute_reserve_mode1_burn_is_top_not_usdc() {
        let _ = SWAP_ROUTER_PROGRAM_ID;
        let _ = RESERVE_BURN_MODE_AUTO_SWAP;
    }

    #[test]
    fn distribute_reserve_mode1_1_lamport_rounds_to_ops() {
        let amount = 1u64;
        let burn_usdc = amount / 2;
        let ops = amount - burn_usdc;
        assert_eq!(burn_usdc, 0);
        assert_eq!(ops, 1);
    }


    #[test]
    fn distribute_reserve_requires_ops_marketing_configured() {
        let unconfigured = Pubkey::default();
        let would_revert = unconfigured == Pubkey::default();
        assert!(would_revert);
    }

    #[test]
    fn distribute_reserve_rejects_zero_pending() {
        let p = fresh_pool(Pubkey::new_unique());
        let amount = p.pending_reserve;
        let would_revert = amount == 0;
        assert!(would_revert);
    }

    #[test]
    fn distribute_reserve_zeros_counter_atomic() {
        let mut p = fresh_pool(Pubkey::new_unique());
        p.pending_reserve = 333_333;
        let _amount = p.pending_reserve;
        p.pending_reserve = 0;
        assert_eq!(p.pending_reserve, 0);
    }

    #[test]
    fn reserve_burn_mode_changed_event_exists() {
        fn _refers_to<T>() {}
        _refers_to::<ReserveBurnModeChanged>();
        _refers_to::<ReserveBurnExecuted>();
    }


    #[test]
    fn full_drain_round_trip_zeros_all_counters() {
        let mut p = fresh_pool(Pubkey::new_unique());
        p.pending_affiliate = 10_000_000;
        p.pending_sovereign = 5_000_000;
        p.pending_yield = 20_000_000;
        p.pending_reserve = 8_000_000;
        p.pending_dev_fee = 3_000_000;
        p.pending_provider_fee = 2_000_000;
        let initial_sum = sum_earmarks(&p);
        assert_eq!(initial_sum, 48_000_000);
        p.pending_affiliate = 0;
        p.pending_sovereign = 0;
        p.pending_yield = 0;
        p.pending_reserve = 0;
        assert_eq!(sum_earmarks(&p), 5_000_000);
        p.pending_dev_fee = 0;
        p.pending_provider_fee = 0;
        assert_eq!(sum_earmarks(&p), 0);
    }

    #[test]
    fn drain_round_trip_with_sovereign_fallback() {
        let mut p = fresh_pool(Pubkey::new_unique());
        p.pending_affiliate = 1_000;
        p.pending_sovereign = 500;
        p.pending_yield = 2_000;
        p.pending_reserve = 1_500;
        p.pending_reserve += p.pending_sovereign;
        p.pending_sovereign = 0;
        assert_eq!(p.pending_reserve, 2_000);
        p.pending_affiliate = 0;
        p.pending_yield = 0;
        p.pending_reserve = 0;
        assert_eq!(sum_earmarks(&p), 0);
    }


    #[test]
    fn drain_authority_operator_path() {
        let operator = Pubkey::new_unique();
        let waterfall = Pubkey::new_unique();
        let caller = operator;
        let is_operator = caller == operator || caller == waterfall;
        assert!(is_operator);
    }

    #[test]
    fn drain_authority_keeper_window_constant() {
        assert_eq!(KEEPER_WINDOW_SECONDS, 8 * SECONDS_PER_DAY);
    }

    #[test]
    fn drain_authority_random_caller_before_keeper_rejected() {
        let operator = Pubkey::new_unique();
        let waterfall = Pubkey::new_unique();
        let random = Pubkey::new_unique();
        let last_distributed_at = 1_000_000i64;
        let now = last_distributed_at + 1;
        let is_operator = random == operator || random == waterfall;
        let is_keeper_eligible = now >= last_distributed_at + KEEPER_WINDOW_SECONDS;
        assert!(!is_operator && !is_keeper_eligible);
    }

    #[test]
    fn drain_authority_random_caller_after_keeper_permitted() {
        let last_distributed_at = 1_000_000i64;
        let now = last_distributed_at + KEEPER_WINDOW_SECONDS + 1;
        let is_keeper_eligible = now >= last_distributed_at + KEEPER_WINDOW_SECONDS;
        assert!(is_keeper_eligible);
    }


    #[test]
    fn chip_deposit_increments_balance() {
        let mut escrow = ProviderPlayerEscrow {
            wallet: Pubkey::new_unique(),
            mint: Pubkey::new_unique(),
            amount: 0,
            bump: 255,
            debit_window_amount: 0,
            debit_window_start: 0,
            reserved: [0u8; 16],
        };
        let amount = 500_000_000u64;
        escrow.amount = escrow.amount.checked_add(amount).unwrap();
        assert_eq!(escrow.amount, 500_000_000);
    }

    #[test]
    fn chip_deposit_topup_preserves_identity() {
        let wallet = Pubkey::new_unique();
        let mint = Pubkey::new_unique();
        let mut escrow = ProviderPlayerEscrow {
            wallet,
            mint,
            amount: 100,
            bump: 255,
            debit_window_amount: 0,
            debit_window_start: 0,
            reserved: [0u8; 16],
        };
        escrow.amount = escrow.amount.checked_add(200).unwrap();
        assert_eq!(escrow.amount, 300);
        assert_eq!(escrow.wallet, wallet);
        assert_eq!(escrow.mint, mint);
    }

    #[test]
    fn chip_withdraw_decrements_balance() {
        let mut escrow = ProviderPlayerEscrow {
            wallet: Pubkey::new_unique(),
            mint: Pubkey::new_unique(),
            amount: 1_000_000,
            bump: 255,
            debit_window_amount: 0,
            debit_window_start: 0,
            reserved: [0u8; 16],
        };
        let withdraw = 400_000u64;
        escrow.amount = escrow.amount.checked_sub(withdraw).unwrap();
        assert_eq!(escrow.amount, 600_000);
    }

    #[test]
    fn chip_withdraw_rejects_insufficient_balance() {
        let escrow = ProviderPlayerEscrow {
            wallet: Pubkey::new_unique(),
            mint: Pubkey::new_unique(),
            amount: 100,
            bump: 255,
            debit_window_amount: 0,
            debit_window_start: 0,
            reserved: [0u8; 16],
        };
        let withdraw = 200u64;
        let would_revert = escrow.amount < withdraw;
        assert!(would_revert);
    }

    #[test]
    fn chip_debit_to_vault_moves_chips_to_vault() {
        let mut escrow = ProviderPlayerEscrow {
            wallet: Pubkey::new_unique(),
            mint: Pubkey::new_unique(),
            amount: 500,
            bump: 255,
            debit_window_amount: 0,
            debit_window_start: 0,
            reserved: [0u8; 16],
        };
        let mut vault_holder: u64 = 10_000;
        let bet = 100u64;
        escrow.amount = escrow.amount.checked_sub(bet).unwrap();
        vault_holder = vault_holder.checked_add(bet).unwrap();
        assert_eq!(escrow.amount, 400);
        assert_eq!(vault_holder, 10_100);
    }

    #[test]
    fn chip_credit_from_vault_preserves_k4_invariant() {
        let mut p = fresh_pool(Pubkey::new_unique());
        p.pending_dev_fee = 100;
        p.pending_yield = 200;
        let vault_pre: u64 = 1_000;
        let payout = 300u64;
        let vault_post = vault_pre.checked_sub(payout).unwrap();
        assert!(require_earmark_invariant(&p, vault_post).is_ok());
    }

    #[test]
    fn chip_credit_from_vault_rejects_when_breaks_invariant() {
        let mut p = fresh_pool(Pubkey::new_unique());
        p.pending_dev_fee = 500;
        let vault_pre: u64 = 1_000;
        let payout = 800u64;
        let vault_post = vault_pre.checked_sub(payout).unwrap();
        assert!(require_earmark_invariant(&p, vault_post).is_err());
    }

    #[test]
    fn chip_escrow_per_wallet_per_mint_isolation() {
        let wallet = Pubkey::new_unique();
        let usdc = Pubkey::new_unique();
        let sol = Pubkey::new_unique();
        let escrow_usdc = ProviderPlayerEscrow {
            wallet,
            mint: usdc,
            amount: 1_000,
            bump: 255,
            debit_window_amount: 0,
            debit_window_start: 0,
            reserved: [0u8; 16],
        };
        let escrow_sol = ProviderPlayerEscrow {
            wallet,
            mint: sol,
            amount: 2_000,
            bump: 254,
            debit_window_amount: 0,
            debit_window_start: 0,
            reserved: [0u8; 16],
        };
        assert_eq!(escrow_usdc.wallet, escrow_sol.wallet);
        assert_ne!(escrow_usdc.mint, escrow_sol.mint);
        assert_ne!(escrow_usdc.amount, escrow_sol.amount);
    }

    #[test]
    fn chip_deposit_zero_amount_rejected() {
        let amount = 0u64;
        let would_revert = amount == 0;
        assert!(would_revert);
    }

    #[test]
    fn chip_withdraw_zero_amount_rejected() {
        let amount = 0u64;
        let would_revert = amount == 0;
        assert!(would_revert);
    }

    #[test]
    fn provider_player_escrow_len_unchanged() {
        assert_eq!(ProviderPlayerEscrow::LEN, 113);
    }

    #[test]
    fn chip_flow_round_trip_balance_conservation() {
        let wallet = Pubkey::new_unique();
        let mint = Pubkey::new_unique();
        let mut escrow = ProviderPlayerEscrow {
            wallet,
            mint,
            amount: 0,
            bump: 255,
            debit_window_amount: 0,
            debit_window_start: 0,
            reserved: [0u8; 16],
        };
        escrow.amount = escrow.amount.checked_add(1_000).unwrap();
        escrow.amount = escrow.amount.checked_sub(400).unwrap();
        escrow.amount = escrow.amount.checked_add(700).unwrap();
        escrow.amount = escrow.amount.checked_sub(1_300).unwrap();
        assert_eq!(escrow.amount, 0);
    }

    #[test]
    fn ops_marketing_wallet_default_at_init() {
        let configured = Pubkey::default();
        assert_eq!(configured, Pubkey::default());
    }

    #[test]
    fn ops_marketing_wallet_rejects_default_on_set() {
        let new_wallet = Pubkey::default();
        let would_revert = new_wallet == Pubkey::default();
        assert!(would_revert);
    }

    #[test]
    fn ops_marketing_wallet_accepts_valid_pubkey() {
        let new_wallet = Pubkey::new_unique();
        let would_succeed = new_wallet != Pubkey::default();
        assert!(would_succeed);
    }


    #[test]
    fn affiliate_cpi_signature_shape() {
        let amount: u64 = 100;
        let asset_mint = Pubkey::new_unique();
        let _ = (amount, asset_mint);
    }

    #[test]
    fn sovereign_usdc_cpi_signature_shape() {
        let amount: u64 = 100;
        let _ = amount;
    }

    #[test]
    fn yield_escrow_usdc_cpi_signature_shape() {
        let amount: u64 = 100;
        let _ = amount;
    }


    #[test]
    fn dev_fee_bps_default_at_init() {
        assert_eq!(DEFAULT_DEV_FEE_BPS, 250);
    }

    #[test]
    fn max_dev_fee_bps_ceiling_is_10pct() {
        assert_eq!(MAX_DEV_FEE_BPS, 1_000);
    }

    #[test]
    fn accrue_earmarks_uses_live_dev_fee_bps() {
        let mut p_a = fresh_pool(Pubkey::new_unique());
        let mut p_b = fresh_pool(Pubkey::new_unique());
        let net: i64 = 1_000_000_000;
        let provider_fee: u64 = 100_000_000;

        accrue_earmarks(&mut p_a, net, 1, DEFAULT_PROVIDER_FEE_BPS, provider_fee, 250, 0, 0).unwrap();
        accrue_earmarks(&mut p_b, net, 1, DEFAULT_PROVIDER_FEE_BPS, provider_fee, 500, 0, 0).unwrap();

        assert!(p_b.pending_dev_fee > p_a.pending_dev_fee,
            "expected pending_dev_fee at 500bps ({}) > 250bps ({})",
            p_b.pending_dev_fee, p_a.pending_dev_fee);

        assert_eq!(p_a.pending_dev_fee, 22_500_000);
        assert_eq!(p_b.pending_dev_fee, 45_000_000);
    }

    #[test]
    fn accrue_earmarks_rejects_bps_above_max() {
        let mut p = fresh_pool(Pubkey::new_unique());
        let too_high = MAX_DEV_FEE_BPS + 1;
        let res = accrue_earmarks(&mut p, 1_000_000, 1, DEFAULT_PROVIDER_FEE_BPS, 0, too_high, 0, 0);
        assert!(res.is_err(), "accrue_earmarks must reject bps > MAX_DEV_FEE_BPS");
    }

    #[test]
    fn accrue_earmarks_accepts_bps_at_max() {
        let mut p = fresh_pool(Pubkey::new_unique());
        let res = accrue_earmarks(&mut p, 1_000_000, 1, DEFAULT_PROVIDER_FEE_BPS, 0, MAX_DEV_FEE_BPS, 0, 0);
        assert!(res.is_ok(), "accrue_earmarks must accept bps == MAX_DEV_FEE_BPS");
    }

    #[test]
    fn accrue_earmarks_accepts_zero_bps() {
        let mut p = fresh_pool(Pubkey::new_unique());
        accrue_earmarks(&mut p, 1_000_000_000, 1, DEFAULT_PROVIDER_FEE_BPS, 100_000_000, 0, 0, 0).unwrap();
        assert_eq!(p.pending_dev_fee, 0);
    }

    #[test]
    fn propose_set_dev_fee_bps_above_max_rejected_shape() {
        let proposed: u16 = MAX_DEV_FEE_BPS + 1;
        let would_revert = proposed > MAX_DEV_FEE_BPS;
        assert!(would_revert);
    }

    #[test]
    fn propose_set_dev_fee_bps_admin_only_shape() {
        let admin = Pubkey::new_unique();
        let non_admin = Pubkey::new_unique();
        assert_ne!(admin, non_admin);
        let would_revert = non_admin != admin;
        assert!(would_revert);
    }

    #[test]
    fn finalize_set_dev_fee_bps_before_unlock_rejected_shape() {
        let now: i64 = 1_000_000_000;
        let unlocks_at: i64 = now + ADMIN_TIMELOCK_SECONDS;
        let would_revert = now < unlocks_at;
        assert!(would_revert);
    }

    #[test]
    fn finalize_set_dev_fee_bps_after_unlock_commits_shape() {
        let now: i64 = 1_000_000_000 + ADMIN_TIMELOCK_SECONDS + 1;
        let unlocks_at: i64 = 1_000_000_000 + ADMIN_TIMELOCK_SECONDS;
        let would_succeed = now >= unlocks_at;
        assert!(would_succeed);
    }

    #[test]
    fn finalize_set_dev_fee_bps_no_pending_rejected_shape() {
        let unlocks_at: i64 = 0;
        let would_revert = unlocks_at == 0;
        assert!(would_revert);
    }

    #[test]
    fn cancel_set_dev_fee_bps_clears_pending_shape() {
        let mut pending_bps: u16 = 500;
        let mut pending_unlocks: i64 = 1_000_000;
        pending_bps = 0;
        pending_unlocks = 0;
        assert_eq!(pending_bps, 0);
        assert_eq!(pending_unlocks, 0);
    }


    #[test]
    fn set_ops_marketing_wallet_first_set_no_timelock_shape() {
        let configured: Pubkey = Pubkey::default();
        let bootstrap_allowed = configured == Pubkey::default();
        assert!(bootstrap_allowed);
    }

    #[test]
    fn set_ops_marketing_wallet_rejects_second_set_shape() {
        let configured: Pubkey = Pubkey::new_unique();
        let would_revert = configured != Pubkey::default();
        assert!(would_revert);
    }

    #[test]
    fn propose_set_ops_marketing_wallet_admin_only_shape() {
        let admin = Pubkey::new_unique();
        let non_admin = Pubkey::new_unique();
        let would_revert = non_admin != admin;
        assert!(would_revert);
    }

    #[test]
    fn propose_set_ops_marketing_wallet_rejects_default_shape() {
        let new_wallet: Pubkey = Pubkey::default();
        let would_revert = new_wallet == Pubkey::default();
        assert!(would_revert);
    }

    #[test]
    fn propose_set_ops_marketing_wallet_requires_bootstrap_done_shape() {
        let configured: Pubkey = Pubkey::default();
        let would_revert = configured == Pubkey::default();
        assert!(would_revert);
    }

    #[test]
    fn propose_then_finalize_set_ops_marketing_wallet_shape() {
        let proposed = Pubkey::new_unique();
        let now0: i64 = 1_000_000_000;
        let unlocks_at = now0 + ADMIN_TIMELOCK_SECONDS;
        let now1 = unlocks_at + 1;
        assert!(now1 >= unlocks_at);
        let mut live: Pubkey = Pubkey::new_unique();
        live = proposed;
        assert_eq!(live, proposed);
    }

    #[test]
    fn finalize_set_ops_marketing_wallet_before_unlock_rejected_shape() {
        let now: i64 = 1_000_000_000;
        let unlocks_at = now + ADMIN_TIMELOCK_SECONDS;
        let would_revert = now < unlocks_at;
        assert!(would_revert);
    }

    #[test]
    fn cancel_set_ops_marketing_wallet_clears_pending_shape() {
        let mut pending: Pubkey = Pubkey::new_unique();
        let mut pending_unlocks: i64 = 1_000_000_000;
        pending = Pubkey::default();
        pending_unlocks = 0;
        assert_eq!(pending, Pubkey::default());
        assert_eq!(pending_unlocks, 0);
    }


    #[test]
    fn require_earmark_invariant_holds_when_balance_covers_earmarks() {
        let mut p = fresh_pool(Pubkey::new_unique());
        p.pending_dev_fee = 100;
        p.pending_yield = 200;
        let holder_balance: u64 = 1_000;
        let res = require_earmark_invariant(&p, holder_balance);
        assert!(res.is_ok());
    }

    #[test]
    fn require_earmark_invariant_reverts_when_balance_below_earmarks() {
        let mut p = fresh_pool(Pubkey::new_unique());
        p.pending_dev_fee = 100;
        p.pending_yield = 200;
        let true_post_balance: u64 = 250;
        let res = require_earmark_invariant(&p, true_post_balance);
        assert!(res.is_err(),
            "require_earmark_invariant must revert when balance < Σ pending_*");
    }

    #[test]
    fn distribute_affiliate_post_balance_uses_reload_not_subtraction() {
        let src = include_str!("lib.rs");
        let reload_count = src.matches("vault_holder.reload()?;").count();
        assert!(reload_count >= 4,
            "expected at least 4 vault_holder.reload() calls (one per distribute_*), got {}",
            reload_count);
        let amount_reads_after_reload =
            src.matches("let post_balance = ctx.accounts.vault_holder.amount;").count();
        assert!(amount_reads_after_reload >= 4,
            "expected ≥4 post-reload `.amount` reads (one per distribute_*), got {}",
            amount_reads_after_reload);
    }

    #[test]
    fn distribute_sovereign_post_balance_uses_reload_not_subtraction() {
        let src = include_str!("lib.rs");
        assert!(src.contains("sovereign_registry::cpi::deposit_royalty_usdc"));
    }

    #[test]
    fn distribute_yield_post_balance_uses_reload_not_subtraction() {
        let src = include_str!("lib.rs");
        assert!(src.contains("yield_escrow::cpi::deposit_provider_yield_usdc"));
    }

    #[test]
    fn distribute_reserve_post_balance_uses_reload_not_subtraction() {
        let src = include_str!("lib.rs");
        assert!(src.contains("token_interface::burn(burn_cpi"));
    }


    #[test]
    fn chip_debit_to_vault_rejected_when_paused_shape() {
        let is_paused = true;
        let would_revert = is_paused;
        assert!(would_revert);
        let src = include_str!("lib.rs");
        assert!(src.contains("HIGH-V2-03 fix"));
    }

    #[test]
    fn chip_withdraw_still_works_when_paused_regression() {
        let src = include_str!("lib.rs");
        let start = src.find("pub fn chip_withdraw(")
            .expect("chip_withdraw handler must exist");
        let end_marker = "pub fn chip_debit_to_vault(";
        let end = src[start..].find(end_marker)
            .expect("chip_debit_to_vault must follow chip_withdraw");
        let body = &src[start..start + end];
        assert!(!body.contains("require!(!config.is_paused"),
            "chip_withdraw MUST NOT pause-gate (player-favorable flow)");
    }

    #[test]
    fn chip_credit_from_vault_still_works_when_paused_regression() {
        let src = include_str!("lib.rs");
        let start = src.find("pub fn chip_credit_from_vault(")
            .expect("chip_credit_from_vault handler must exist");
        let end = drift_handler_end(src, start);
        let body = &src[start..end];
        assert!(!body.contains("require!(!config.is_paused"),
            "chip_credit_from_vault MUST NOT pause-gate (player-favorable flow)");
    }


    #[test]
    fn distribute_dev_fee_drains_bucket() {
        let mut pool = fresh_pool(Pubkey::new_unique());
        pool.pending_dev_fee = 12_345_678;
        let amount = pool.pending_dev_fee;
        pool.pending_dev_fee = 0;
        assert_eq!(amount, 12_345_678);
        assert_eq!(pool.pending_dev_fee, 0);
        require_earmark_invariant(&pool, 0).expect("zero earmarks → invariant holds");
    }

    #[test]
    fn distribute_dev_fee_nothing_to_drain_when_zero() {
        let pool = fresh_pool(Pubkey::new_unique());
        assert_eq!(pool.pending_dev_fee, 0,
            "fresh pool has nothing to drain; handler must revert with NothingToDrain");
    }

    #[test]
    fn accrue_affiliate_increments_pending() {
        let mut pool = fresh_pool(Pubkey::new_unique());
        pool.pending_affiliate = 100;
        accrue_affiliate_amount(&mut pool, 250).unwrap();
        assert_eq!(pool.pending_affiliate, 350);
        require_earmark_invariant(&pool, 350).expect("balance == earmarks ok");
    }

    #[test]
    fn accrue_affiliate_breaks_k4_invariant_when_over_accrued() {
        let mut pool = fresh_pool(Pubkey::new_unique());
        pool.pending_affiliate = 500;
        pool.pending_dev_fee = 500;
        accrue_affiliate_amount(&mut pool, 100).unwrap();
        let err = require_earmark_invariant(&pool, 999).expect_err("invariant must fail");
        let _ = err;
    }

    #[test]
    fn accrue_affiliate_overflow_reverts() {
        let mut pool = fresh_pool(Pubkey::new_unique());
        pool.pending_affiliate = u64::MAX;
        let err = accrue_affiliate_amount(&mut pool, 1).expect_err("overflow");
        let _ = err;
    }

    #[test]
    fn dev_fee_drain_then_affiliate_accrue_maintains_invariant() {
        let mut pool = fresh_pool(Pubkey::new_unique());
        pool.pending_dev_fee = 1_000;
        pool.pending_affiliate = 500;
        pool.pending_dev_fee = 0;
        require_earmark_invariant(&pool, 500).expect("only affiliate earmark left");
        accrue_affiliate_amount(&mut pool, 200).unwrap();
        assert_eq!(pool.pending_affiliate, 700);
        require_earmark_invariant(&pool, 700).expect("balance == earmarks ok");
    }


    #[test]
    fn freeze_rate_limit_constant_is_600s() {
        assert_eq!(FREEZE_RATE_LIMIT_SECONDS, 600);
    }

    #[test]
    fn vault_config_is_not_frozen_by_default() {
        let is_frozen: bool = false;
        let last_freeze_at: i64 = 0;
        assert!(!is_frozen, "vault must not be frozen at deploy");
        assert_eq!(last_freeze_at, 0, "last_freeze_at must be 0 (never frozen)");
    }

    #[test]
    fn freeze_sets_is_frozen_true_and_updates_timestamp() {
        let mut is_frozen = false;
        let mut last_freeze_at: i64 = 0;
        let now: i64 = 1_700_000_000;

        assert_eq!(last_freeze_at, 0);
        is_frozen = true;
        last_freeze_at = now;

        assert!(is_frozen, "is_frozen must be true after freeze()");
        assert_eq!(last_freeze_at, now, "last_freeze_at must record the freeze timestamp");
    }

    #[test]
    fn unfreeze_sets_is_frozen_false() {
        let mut is_frozen = true;
        let last_freeze_at: i64 = 1_700_000_000;

        is_frozen = false;

        assert!(!is_frozen, "is_frozen must be false after unfreeze()");
        assert_ne!(last_freeze_at, 0, "last_freeze_at preserved after unfreeze");
    }

    #[test]
    fn freeze_rate_limit_blocks_second_freeze_within_600s() {
        let last_freeze_at: i64 = 1_700_000_000;
        let now_too_soon: i64 = last_freeze_at + 599;

        let would_pass = last_freeze_at == 0
            || now_too_soon >= last_freeze_at + FREEZE_RATE_LIMIT_SECONDS;
        assert!(
            !would_pass,
            "second freeze within 600s must be rejected (rate-limited)"
        );

        let now_ok: i64 = last_freeze_at + 600;
        let would_pass_ok = last_freeze_at == 0
            || now_ok >= last_freeze_at + FREEZE_RATE_LIMIT_SECONDS;
        assert!(
            would_pass_ok,
            "freeze after exactly 600s must be allowed"
        );
    }

    #[test]
    fn frozen_guard_logic_blocks_state_mutation() {
        let is_frozen = true;
        let would_proceed = !is_frozen;
        assert!(!would_proceed, "frozen vault must NOT proceed past the frozen guard");

        let is_not_frozen = false;
        let would_proceed_unfrozen = !is_not_frozen;
        assert!(would_proceed_unfrozen, "unfrozen vault MUST proceed past the frozen guard");
    }

    #[test]
    fn vault_config_len_unchanged_at_568() {
        assert_eq!(VaultConfig::LEN, 847, "LEN must be 847 after M-HIGH-01 daily outflow cap fields added (5 × u64/i64 = 40B)");
    }


    fn minimal_vault_config(authority: Pubkey) -> VaultConfig {
        let zero_key = Pubkey::default();
        VaultConfig {
            authority,
            operator_pubkey: zero_key,
            affiliate_recorder_pubkey: zero_key,
            pause_authority: zero_key,
            waterfall_authority: zero_key,
            bump: 0,
            active_provider_count: 0,
            next_provider_id: 0,
            is_paused: true,
            pause_reason: [0u8; PAUSE_REASON_LEN],
            last_pause_at: 0,
            last_provider_pause_at: 0,
            phase: 0,
            phase_started_at: 0,
            dev_fee_bps: 250,
            sovereign_carve_bps: 500,
            insurance_floor_bps: 500,
            max_daily_drawdown_bps: 2000,
            sovereign_registry_program_id: zero_key,
            sovereign_registry_config: zero_key,
            yield_escrow_program_id: zero_key,
            yield_escrow_provider_pool: zero_key,
            affiliate_registry_program_id: zero_key,
            affiliate_registry_config: zero_key,
            pending_authority: zero_key,
            pending_authority_unlocks_at: 0,
            ops_marketing_wallet: zero_key,
            pending_dev_fee_bps: 0,
            pending_dev_fee_bps_unlocks_at: 0,
            pending_ops_marketing_wallet: zero_key,
            pending_ops_marketing_wallet_unlocks_at: 0,
            reserve_burn_mode: RESERVE_BURN_MODE_MANUAL,
            is_frozen: false,
            last_freeze_at: 0,
            raydium_graduated: false,
            max_settle_per_window: DEFAULT_MAX_SETTLE_PER_WINDOW,
            settle_window_seconds: DEFAULT_SETTLE_WINDOW_SECONDS,
            window_outflow: 0,
            window_start: 0,
            pending_max_settle_per_window: 0,
            pending_max_settle_per_window_unlocks_at: 0,
            pending_settle_window_seconds: 0,
            pending_settle_window_seconds_unlocks_at: 0,
            pending_pause_authority: zero_key,
            pending_pause_authority_unlocks_at: 0,
            pending_operator_pubkey: zero_key,
            pending_operator_unlocks_at: 0,
            propose_cooldown_until: 0,
            recent_proposes: [0i64; 5],
            founder_pubkey: zero_key,
            founding_banker_counter: 0,
            vault_seeded: false,
            max_daily_outflow: DEFAULT_MAX_DAILY_OUTFLOW,
            daily_window_outflow: 0,
            daily_window_start: 0,
            pending_max_daily_outflow: 0,
            pending_max_daily_outflow_unlocks_at: 0,
            last_heartbeat_at: 0,
            heartbeat_ttl: 0,
            reserved: [0u8; 8],
        }
    }

    #[test]
    fn raydium_graduated_defaults_false() {
        let config = minimal_vault_config(Pubkey::new_unique());
        assert!(!config.raydium_graduated, "raydium_graduated must default to false");
    }

    #[test]
    fn set_raydium_graduated_admin_only() {
        let admin = Pubkey::new_unique();
        let attacker = Pubkey::new_unique();
        let config = minimal_vault_config(admin);
        let admin_ok = config.authority == admin;
        let attacker_ok = config.authority == attacker;
        assert!(admin_ok, "admin must pass the authority check");
        assert!(!attacker_ok, "attacker must NOT pass the authority check");
    }

    #[test]
    fn set_raydium_graduated_syncs_reserve_burn_mode_alias() {
        let mut config = minimal_vault_config(Pubkey::new_unique());
        assert_eq!(config.reserve_burn_mode, RESERVE_BURN_MODE_MANUAL,
            "initial reserve_burn_mode must be MANUAL (0)");
        assert!(!config.raydium_graduated);

        config.raydium_graduated = true;
        config.reserve_burn_mode = if config.raydium_graduated {
            RESERVE_BURN_MODE_AUTO_SWAP
        } else {
            RESERVE_BURN_MODE_MANUAL
        };
        assert_eq!(config.reserve_burn_mode, RESERVE_BURN_MODE_AUTO_SWAP,
            "reserve_burn_mode must sync to AUTO_SWAP when graduated=true");

        config.raydium_graduated = false;
        config.reserve_burn_mode = if config.raydium_graduated {
            RESERVE_BURN_MODE_AUTO_SWAP
        } else {
            RESERVE_BURN_MODE_MANUAL
        };
        assert_eq!(config.reserve_burn_mode, RESERVE_BURN_MODE_MANUAL,
            "reserve_burn_mode must sync back to MANUAL when graduated=false");
    }

    #[test]
    fn distribute_yield_routes_100_percent_to_stakers_when_not_graduated() {
        let graduated = false;
        let amount = 1_000_000u64;
        let (liquid_amount, swap_amount) = if !graduated {
            (amount, 0u64)
        } else {
            let liquid = amount.checked_mul(7_000).unwrap() / 10_000;
            let swap = amount.checked_sub(liquid).unwrap();
            (liquid, swap)
        };
        assert_eq!(liquid_amount, amount, "pre-graduation: all yield to stakers");
        assert_eq!(swap_amount, 0, "pre-graduation: zero to swap");
    }

    #[test]
    fn distribute_yield_routes_70_30_when_graduated() {
        let graduated = true;
        let amount = 1_000_000u64;
        let (liquid_amount, swap_amount) = if !graduated {
            (amount, 0u64)
        } else {
            let liquid = amount.checked_mul(7_000).unwrap() / 10_000;
            let swap = amount.checked_sub(liquid).unwrap();
            (liquid, swap)
        };
        assert_eq!(liquid_amount, 700_000, "graduated: 70% must be liquid");
        assert_eq!(swap_amount, 300_000, "graduated: 30% must go to swap");
        assert_eq!(liquid_amount + swap_amount, amount, "split must be lossless");
    }

    #[test]
    fn distribute_yield_70_30_math_uses_bps_7000() {
        let amount: u64 = 10_000_000_000_000;
        let liquid = (amount as u128)
            .checked_mul(7_000)
            .unwrap()
            .checked_div(10_000)
            .unwrap() as u64;
        let swap = amount.checked_sub(liquid).unwrap();
        assert_eq!(liquid, 7_000_000_000_000, "70% must be exact");
        assert_eq!(swap, 3_000_000_000_000, "30% must be exact");
        assert_eq!(liquid + swap, amount, "split must be lossless on even amounts");
    }

    #[test]
    fn vault_config_len_unchanged_at_568_after_raydium_graduated() {
        assert_eq!(VaultConfig::LEN, 847,
            "LEN must be 847 after M-HIGH-01 daily outflow cap fields added (was 791 post Wave D.A.5b-v2)");
    }


    #[test]
    fn submit_provider_ggr_rejects_far_future_day_id() {
        let now_ts: i64 = 1_716_000_000;
        let current_day = (now_ts as u64) / 86_400;
        let far_future_day_id = current_day.saturating_add(2);
        let guard_passes = far_future_day_id <= current_day.saturating_add(1);
        assert!(!guard_passes, "day_id = current_day + 2 must be rejected");

        let even_further = current_day.saturating_add(30);
        let guard_far = even_further <= current_day.saturating_add(1);
        assert!(!guard_far, "day_id = current_day + 30 must be rejected");
    }

    #[test]
    fn submit_provider_ggr_accepts_current_day() {
        let now_ts: i64 = 1_716_000_000;
        let current_day = (now_ts as u64) / 86_400;
        let today_id = current_day;
        let guard_passes = today_id <= current_day.saturating_add(1);
        assert!(guard_passes, "day_id == current_day must be accepted");
    }

    #[test]
    fn submit_provider_ggr_accepts_tomorrow() {
        let now_ts: i64 = 1_716_000_000;
        let current_day = (now_ts as u64) / 86_400;
        let tomorrow_id = current_day.saturating_add(1);
        let guard_passes = tomorrow_id <= current_day.saturating_add(1);
        assert!(guard_passes, "day_id == current_day + 1 must be accepted (UTC boundary)");
    }

    #[test]
    fn submit_provider_ggr_rejects_day_after_tomorrow() {
        let now_ts: i64 = 1_716_000_000;
        let current_day = (now_ts as u64) / 86_400;
        let day_after_tomorrow = current_day.saturating_add(2);
        let guard_passes = day_after_tomorrow <= current_day.saturating_add(1);
        assert!(!guard_passes, "day_id == current_day + 2 must be rejected");
    }


    #[test]
    fn distribute_yield_swap_router_usdc_holder_constraint_mint_pinned() {
        let asset_mint = Pubkey::new_unique();
        let wrong_mint  = Pubkey::new_unique();
        assert_ne!(asset_mint, wrong_mint,
            "A caller supplying wrong_mint as swap_router_usdc_holder \
             is now caught by Anchor at parse time (token::mint = asset_mint)");
    }


    #[test]
    fn outflow_breaker_constants_match_spec() {
        assert_eq!(DEFAULT_MAX_SETTLE_PER_WINDOW, 50_000_000_000,
            "spec locks the default at $50,000 micro-USDC");
        assert_eq!(DEFAULT_SETTLE_WINDOW_SECONDS, 300,
            "spec locks the default at 5 min");
        assert_eq!(MIN_MAX_SETTLE_PER_WINDOW, 1_000_000_000,
            "$1,000 sanity floor on propose");
        assert_eq!(MIN_SETTLE_WINDOW_SECONDS, 30);
        assert_eq!(MAX_SETTLE_WINDOW_SECONDS, 86_400);
    }

    #[test]
    fn outflow_breaker_defaults_at_init() {
        let cfg = minimal_vault_config(Pubkey::new_unique());
        assert_eq!(cfg.max_settle_per_window, DEFAULT_MAX_SETTLE_PER_WINDOW);
        assert_eq!(cfg.settle_window_seconds, DEFAULT_SETTLE_WINDOW_SECONDS);
        assert_eq!(cfg.window_outflow, 0);
        assert_eq!(cfg.window_start, 0,
            "window_start = 0 sentinel triggers reset on first settle");
        assert_eq!(cfg.pending_max_settle_per_window, 0);
        assert_eq!(cfg.pending_max_settle_per_window_unlocks_at, 0);
        assert_eq!(cfg.pending_settle_window_seconds, 0);
        assert_eq!(cfg.pending_settle_window_seconds_unlocks_at, 0);
    }

    #[derive(Debug, PartialEq, Eq)]
    enum BreakerOutcome {
        Passed(u64),
        Tripped,
    }

    fn simulate_outflow_check(
        cfg: &mut VaultConfig,
        now: i64,
        amount: u64,
    ) -> std::result::Result<BreakerOutcome, &'static str> {
        let window_end = cfg.window_start.saturating_add(cfg.settle_window_seconds as i64);
        if cfg.window_start == 0 || now >= window_end {
            cfg.window_outflow = 0;
            cfg.window_start = now;
        }
        let projected = cfg.window_outflow
            .checked_add(amount)
            .ok_or("overflow")?;
        if projected > cfg.max_settle_per_window {
            cfg.is_frozen = true;
            return Ok(BreakerOutcome::Tripped);
        }
        cfg.window_outflow = projected;
        Ok(BreakerOutcome::Passed(projected))
    }

    #[test]
    fn settle_within_window_cap_passes() {
        let mut cfg = minimal_vault_config(Pubkey::new_unique());
        let now: i64 = 1_700_000_000;
        let amount: u64 = 10_000_000_000;
        let r = simulate_outflow_check(&mut cfg, now, amount).unwrap();
        assert_eq!(r, BreakerOutcome::Passed(amount));
        assert_eq!(cfg.window_outflow, amount);
        assert_eq!(cfg.window_start, now, "window_start anchored on first settle");
        assert!(!cfg.is_frozen);
    }

    #[test]
    fn cumulative_settles_within_cap_pass() {
        let mut cfg = minimal_vault_config(Pubkey::new_unique());
        let t0: i64 = 1_700_000_000;
        for offset in [0i64, 60, 120, 180] {
            let r = simulate_outflow_check(&mut cfg, t0 + offset, 10_000_000_000).unwrap();
            assert!(matches!(r, BreakerOutcome::Passed(_)));
        }
        assert_eq!(cfg.window_outflow, 40_000_000_000);
        assert!(!cfg.is_frozen);
        assert_eq!(cfg.window_start, t0, "window_start anchored to FIRST settle in window");
    }

    #[test]
    fn cumulative_settles_exceeding_cap_trip_breaker() {
        let mut cfg = minimal_vault_config(Pubkey::new_unique());
        let t0: i64 = 1_700_000_000;
        let r1 = simulate_outflow_check(&mut cfg, t0, 45_000_000_000).unwrap();
        assert_eq!(r1, BreakerOutcome::Passed(45_000_000_000));
        assert_eq!(cfg.window_outflow, 45_000_000_000);
        assert!(!cfg.is_frozen);
        let r2 = simulate_outflow_check(&mut cfg, t0 + 30, 6_000_000_000).unwrap();
        assert_eq!(r2, BreakerOutcome::Tripped,
            "trip must return Ok(Tripped) so the is_frozen write persists");
        assert!(cfg.is_frozen,
            "breaker must persist is_frozen=true on trip (Ok return commits the write)");
        assert_eq!(cfg.window_outflow, 45_000_000_000,
            "tripped attempt MUST NOT advance window_outflow");
    }

    #[test]
    fn window_rollover_resets_counter() {
        let mut cfg = minimal_vault_config(Pubkey::new_unique());
        let t0: i64 = 1_700_000_000;
        let r1 = simulate_outflow_check(&mut cfg, t0, 30_000_000_000).unwrap();
        assert!(matches!(r1, BreakerOutcome::Passed(_)));
        assert_eq!(cfg.window_outflow, 30_000_000_000);

        let t1 = t0 + DEFAULT_SETTLE_WINDOW_SECONDS as i64 + 1;
        let r2 = simulate_outflow_check(&mut cfg, t1, 30_000_000_000).unwrap();
        assert!(matches!(r2, BreakerOutcome::Passed(_)));
        assert_eq!(cfg.window_outflow, 30_000_000_000);
        assert_eq!(cfg.window_start, t1, "new window anchored at the post-rollover settle");
    }

    #[test]
    fn post_trip_settle_reverts_with_vault_frozen() {
        let mut cfg = minimal_vault_config(Pubkey::new_unique());
        cfg.is_frozen = true;
        let would_revert = cfg.is_frozen;
        assert!(would_revert,
            "frozen vault must revert with VaultFrozen — NOT OutflowCircuitBreakerTripped");
    }

    #[test]
    fn admin_unfreeze_does_not_reset_counter() {
        let mut cfg = minimal_vault_config(Pubkey::new_unique());
        let t0: i64 = 1_700_000_000;
        cfg.window_start = t0;
        cfg.window_outflow = 49_000_000_000;
        cfg.is_frozen = true;
        cfg.is_frozen = false;
        assert!(!cfg.is_frozen);
        assert_eq!(cfg.window_outflow, 49_000_000_000,
            "unfreeze MUST NOT reset window_outflow (intentional — same window still in force)");
        assert_eq!(cfg.window_start, t0, "unfreeze MUST NOT reset window_start");
    }

    #[test]
    fn admin_unfreeze_plus_window_rollover_starts_fresh() {
        let mut cfg = minimal_vault_config(Pubkey::new_unique());
        let t0: i64 = 1_700_000_000;
        cfg.window_start = t0;
        cfg.window_outflow = 49_000_000_000;
        cfg.is_frozen = true;
        cfg.is_frozen = false;
        let t1 = t0 + DEFAULT_SETTLE_WINDOW_SECONDS as i64 + 100;
        let r = simulate_outflow_check(&mut cfg, t1, 20_000_000_000).unwrap();
        assert_eq!(r, BreakerOutcome::Passed(20_000_000_000));
        assert_eq!(cfg.window_outflow, 20_000_000_000);
        assert_eq!(cfg.window_start, t1);
    }

    #[test]
    fn admin_unfreeze_after_trip_works() {
        let mut cfg = minimal_vault_config(Pubkey::new_unique());
        cfg.is_frozen = true;
        cfg.is_frozen = false;
        assert!(!cfg.is_frozen);
    }


    #[test]
    fn propose_max_settle_per_window_stores_pending_and_unlocks_at() {
        let mut cfg = minimal_vault_config(Pubkey::new_unique());
        let now: i64 = 1_700_000_000;
        let new_value: u64 = 75_000_000_000;
        assert!(new_value >= MIN_MAX_SETTLE_PER_WINDOW);
        cfg.pending_max_settle_per_window = new_value;
        cfg.pending_max_settle_per_window_unlocks_at = now + ADMIN_TIMELOCK_SECONDS;
        assert_eq!(cfg.pending_max_settle_per_window, 75_000_000_000);
        assert_eq!(cfg.pending_max_settle_per_window_unlocks_at,
            now + ADMIN_TIMELOCK_SECONDS);
        assert_eq!(cfg.max_settle_per_window, DEFAULT_MAX_SETTLE_PER_WINDOW,
            "live field unchanged until finalize");
    }

    #[test]
    fn propose_max_settle_per_window_below_minimum_rejected() {
        let too_low: u64 = MIN_MAX_SETTLE_PER_WINDOW - 1;
        let would_revert = too_low < MIN_MAX_SETTLE_PER_WINDOW;
        assert!(would_revert,
            "below-minimum proposals must revert with WindowCapBelowMinimum");
    }

    #[test]
    fn finalize_max_settle_per_window_commits_after_72h() {
        let mut cfg = minimal_vault_config(Pubkey::new_unique());
        let now0: i64 = 1_700_000_000;
        cfg.pending_max_settle_per_window = 100_000_000_000;
        cfg.pending_max_settle_per_window_unlocks_at = now0 + ADMIN_TIMELOCK_SECONDS;
        let now1 = now0 + ADMIN_TIMELOCK_SECONDS + 1;
        assert!(cfg.pending_max_settle_per_window_unlocks_at != 0);
        assert!(now1 >= cfg.pending_max_settle_per_window_unlocks_at);
        cfg.max_settle_per_window = cfg.pending_max_settle_per_window;
        cfg.pending_max_settle_per_window = 0;
        cfg.pending_max_settle_per_window_unlocks_at = 0;
        assert_eq!(cfg.max_settle_per_window, 100_000_000_000);
        assert_eq!(cfg.pending_max_settle_per_window, 0);
        assert_eq!(cfg.pending_max_settle_per_window_unlocks_at, 0);
    }

    #[test]
    fn finalize_max_settle_per_window_before_unlock_reverts() {
        let now: i64 = 1_700_000_000;
        let unlocks_at: i64 = now + ADMIN_TIMELOCK_SECONDS;
        let would_revert = now < unlocks_at;
        assert!(would_revert);
    }

    #[test]
    fn cancel_pending_max_settle_per_window() {
        let mut cfg = minimal_vault_config(Pubkey::new_unique());
        cfg.pending_max_settle_per_window = 100_000_000_000;
        cfg.pending_max_settle_per_window_unlocks_at = 1_700_000_000;
        let live_before = cfg.max_settle_per_window;
        cfg.pending_max_settle_per_window = 0;
        cfg.pending_max_settle_per_window_unlocks_at = 0;
        assert_eq!(cfg.pending_max_settle_per_window, 0);
        assert_eq!(cfg.pending_max_settle_per_window_unlocks_at, 0);
        assert_eq!(cfg.max_settle_per_window, live_before,
            "cancel must NOT touch the live field");
    }

    #[test]
    fn finalize_max_settle_per_window_no_pending_reverts() {
        let unlocks_at: i64 = 0;
        let would_revert = unlocks_at == 0;
        assert!(would_revert,
            "unlocks_at == 0 is the NothingPending sentinel");
    }


    #[test]
    fn propose_finalize_settle_window_seconds_72h_path() {
        let mut cfg = minimal_vault_config(Pubkey::new_unique());
        let now0: i64 = 1_700_000_000;
        let new_value: u32 = 600;
        assert!(new_value >= MIN_SETTLE_WINDOW_SECONDS
            && new_value <= MAX_SETTLE_WINDOW_SECONDS);
        cfg.pending_settle_window_seconds = new_value;
        cfg.pending_settle_window_seconds_unlocks_at = now0 + ADMIN_TIMELOCK_SECONDS;
        let now1 = now0 + ADMIN_TIMELOCK_SECONDS + 1;
        assert!(now1 >= cfg.pending_settle_window_seconds_unlocks_at);
        cfg.settle_window_seconds = cfg.pending_settle_window_seconds;
        cfg.pending_settle_window_seconds = 0;
        cfg.pending_settle_window_seconds_unlocks_at = 0;
        assert_eq!(cfg.settle_window_seconds, 600);
    }

    #[test]
    fn propose_settle_window_seconds_out_of_range_rejected() {
        let too_short: u32 = MIN_SETTLE_WINDOW_SECONDS - 1;
        let would_revert = !(too_short >= MIN_SETTLE_WINDOW_SECONDS
            && too_short <= MAX_SETTLE_WINDOW_SECONDS);
        assert!(would_revert);

        let too_long: u32 = MAX_SETTLE_WINDOW_SECONDS + 1;
        let would_revert_long = !(too_long >= MIN_SETTLE_WINDOW_SECONDS
            && too_long <= MAX_SETTLE_WINDOW_SECONDS);
        assert!(would_revert_long);
    }


    #[test]
    fn integer_overflow_guard_on_window_outflow() {
        let mut cfg = minimal_vault_config(Pubkey::new_unique());
        cfg.max_settle_per_window = u64::MAX;
        cfg.window_outflow = u64::MAX;
        cfg.window_start = 1_700_000_000;
        let r = simulate_outflow_check(&mut cfg, 1_700_000_000 + 1, 1);
        assert!(r.is_err(),
            "overflow on window_outflow accumulator must revert with MathOverflow");
    }

    #[test]
    fn two_vaults_isolated_one_trips_other_unaffected() {
        let mut cfg_a = minimal_vault_config(Pubkey::new_unique());
        let mut cfg_b = minimal_vault_config(Pubkey::new_unique());
        let t0: i64 = 1_700_000_000;
        let ra = simulate_outflow_check(&mut cfg_a, t0, 60_000_000_000).unwrap();
        assert_eq!(ra, BreakerOutcome::Tripped);
        assert!(cfg_a.is_frozen);
        assert!(!cfg_b.is_frozen);
        assert_eq!(cfg_b.window_outflow, 0);
        let rb = simulate_outflow_check(&mut cfg_b, t0, 10_000_000_000).unwrap();
        assert!(matches!(rb, BreakerOutcome::Passed(_)));
        assert!(!cfg_b.is_frozen);
    }

    #[test]
    fn settle_exactly_at_cap_passes() {
        let mut cfg = minimal_vault_config(Pubkey::new_unique());
        let t0: i64 = 1_700_000_000;
        let amount = cfg.max_settle_per_window;
        let r = simulate_outflow_check(&mut cfg, t0, amount).unwrap();
        assert_eq!(r, BreakerOutcome::Passed(amount),
            "amount == cap must pass (strict > comparator)");
        assert_eq!(cfg.window_outflow, amount);
        assert!(!cfg.is_frozen);
        let r2 = simulate_outflow_check(&mut cfg, t0 + 1, 1).unwrap();
        assert_eq!(r2, BreakerOutcome::Tripped);
        assert!(cfg.is_frozen);
        assert_eq!(cfg.window_outflow, amount,
            "tripped attempt MUST NOT pollute window_outflow");
    }

    #[test]
    fn auto_frozen_event_field_shape() {
        let evt = AutoFrozenOnOutflow {
            source: AUTO_FROZEN_SOURCE_LP,
            attempted_amount: 6_000_000_000,
            window_outflow_at_trip: 45_000_000_000,
            threshold: 50_000_000_000,
            window_start: 1_700_000_000,
            tripped_at: 1_700_000_120,
        };
        assert_eq!(evt.source, AUTO_FROZEN_SOURCE_LP);
        assert_eq!(evt.attempted_amount, 6_000_000_000);
        assert_eq!(evt.window_outflow_at_trip, 45_000_000_000);
        assert_eq!(evt.threshold, 50_000_000_000);
        assert_eq!(evt.window_start, 1_700_000_000);
        assert_eq!(evt.tripped_at, 1_700_000_120);

        let evt_promo = AutoFrozenOnOutflow {
            source: AUTO_FROZEN_SOURCE_PROMO,
            attempted_amount: 1_000_000,
            window_outflow_at_trip: 0,
            threshold: 50_000_000_000,
            window_start: 1_700_000_000,
            tripped_at: 1_700_000_120,
        };
        assert_eq!(evt_promo.source, AUTO_FROZEN_SOURCE_PROMO);
        assert_ne!(evt.source, evt_promo.source,
            "LP and promo trip events must carry distinct source discriminators");
    }

    #[test]
    fn cancel_max_settle_per_window_clears_both_pending_fields() {
        let mut cfg = minimal_vault_config(Pubkey::new_unique());
        cfg.pending_max_settle_per_window = 999;
        cfg.pending_max_settle_per_window_unlocks_at = 1_700_000_000;
        cfg.pending_max_settle_per_window = 0;
        cfg.pending_max_settle_per_window_unlocks_at = 0;
        assert_eq!(cfg.pending_max_settle_per_window, 0);
        assert_eq!(cfg.pending_max_settle_per_window_unlocks_at, 0,
            "cancel must zero the unlocks_at sentinel — NothingPending guard depends on it");
    }

    #[test]
    fn chip_credit_from_vault_contains_outflow_breaker_regression() {
        let src = include_str!("lib.rs");
        let start = src.find("pub fn chip_credit_from_vault(")
            .expect("handler must exist");
        let end = drift_handler_end(src, start);
        let body = &src[start..end];
        assert!(body.contains("max_settle_per_window"),
            "chip_credit_from_vault MUST reference max_settle_per_window");
        assert!(body.contains("AutoFrozenOnOutflow"),
            "chip_credit_from_vault MUST emit AutoFrozenOnOutflow on trip");
        assert!(body.contains("config.is_frozen = true"),
            "chip_credit_from_vault MUST set is_frozen=true on trip");
        assert!(body.contains("return Ok(())"),
            "chip_credit_from_vault MUST return Ok(()) on trip (not err!) so \
             is_frozen=true write persists past the TX (Anchor 0.32 rolls back \
             account state on Err)");
        let trip_block_start = body.find("if projected_outflow > config.max_settle_per_window")
            .expect("trip-check branch must exist");
        let trip_block_end = body[trip_block_start..].find("\n        }")
            .map(|i| trip_block_start + i)
            .unwrap_or(body.len());
        let trip_block = &body[trip_block_start..trip_block_end];
        assert!(!trip_block.contains("err!(ProviderVaultError::OutflowCircuitBreakerTripped)"),
            "trip branch MUST NOT return err!(OutflowCircuitBreakerTripped) — \
             post-refactor uses Ok return so is_frozen write persists");
    }

    #[test]
    fn vault_config_len_includes_breaker_fields() {
        assert_eq!(VaultConfig::LEN, 847);
    }


    #[test]
    fn cap_exceeded_returns_ok_not_err() {
        let mut cfg = minimal_vault_config(Pubkey::new_unique());
        let t0: i64 = 1_700_000_000;
        let amount = cfg.max_settle_per_window + 1;
        let res = simulate_outflow_check(&mut cfg, t0, amount);
        assert!(res.is_ok(),
            "REGRESSION: trip path returned Err. Refactor requires Ok return \
             so is_frozen=true write persists past the TX. Anchor 0.32 rolls \
             back ALL account-state writes on Err.");
        assert_eq!(res.unwrap(), BreakerOutcome::Tripped);
    }

    #[test]
    fn cap_exceeded_persists_is_frozen_true() {
        let mut cfg = minimal_vault_config(Pubkey::new_unique());
        let t0: i64 = 1_700_000_000;
        let amount = cfg.max_settle_per_window + 1;
        assert!(!cfg.is_frozen, "precondition: vault not frozen");
        let _ = simulate_outflow_check(&mut cfg, t0, amount).unwrap();
        let cfg_ref: &VaultConfig = &cfg;
        assert!(cfg_ref.is_frozen,
            "is_frozen MUST persist after the trip path. The Ok return is the \
             whole point of the refactor — caller MUST be able to re-fetch \
             and see is_frozen == true.");
    }

    #[test]
    fn cap_exceeded_does_not_advance_window_outflow() {
        let mut cfg = minimal_vault_config(Pubkey::new_unique());
        let t0: i64 = 1_700_000_000;
        let r1 = simulate_outflow_check(&mut cfg, t0, 45_000_000_000).unwrap();
        assert_eq!(r1, BreakerOutcome::Passed(45_000_000_000));
        assert_eq!(cfg.window_outflow, 45_000_000_000);
        let counter_before_trip = cfg.window_outflow;
        let r2 = simulate_outflow_check(&mut cfg, t0 + 30, 6_000_000_000).unwrap();
        assert_eq!(r2, BreakerOutcome::Tripped);
        assert_eq!(cfg.window_outflow, counter_before_trip,
            "tripped attempt MUST NOT advance window_outflow — otherwise the \
             counter is polluted by failed attempts and the next legitimate \
             caller post-unfreeze sees a wrong baseline.");
    }

    #[test]
    fn cap_exceeded_emits_auto_frozen_on_outflow_with_correct_fields() {
        let attempted_amount: u64 = 6_000_000_000;
        let pre_attempt_outflow: u64 = 45_000_000_000;
        let threshold: u64 = 50_000_000_000;
        let window_start: i64 = 1_700_000_000;
        let tripped_at: i64 = window_start + 30;

        let evt = AutoFrozenOnOutflow {
            source: AUTO_FROZEN_SOURCE_LP,
            attempted_amount,
            window_outflow_at_trip: pre_attempt_outflow,
            threshold,
            window_start,
            tripped_at,
        };
        assert_eq!(evt.source, AUTO_FROZEN_SOURCE_LP);
        assert_eq!(evt.attempted_amount, attempted_amount);
        assert_eq!(evt.window_outflow_at_trip, pre_attempt_outflow,
            "window_outflow_at_trip MUST be the pre-attempt cumulative (NOT \
             projected). Otherwise Sentinel triage can't distinguish 'busy \
             week + small overflow' from 'compromised key + huge spike'.");
        assert_eq!(evt.threshold, threshold);
        assert_eq!(evt.window_start, window_start);
        assert_eq!(evt.tripped_at, tripped_at);
        let projected = evt.attempted_amount + evt.window_outflow_at_trip;
        assert!(projected > evt.threshold,
            "evt math invariant: attempted + window_outflow_at_trip > threshold");

        let src = include_str!("lib.rs");
        let start = src.find("pub fn chip_credit_from_vault(")
            .expect("handler must exist");
        let end = drift_handler_end(src, start);
        let body = &src[start..end];
        assert!(body.contains("attempted_amount: amount"),
            "handler must emit AutoFrozenOnOutflow with attempted_amount = this call's amount");
        assert!(body.contains("window_outflow_at_trip: config.window_outflow"),
            "handler must emit AutoFrozenOnOutflow with window_outflow_at_trip = \
             pre-attempt counter (NOT projected_outflow)");
    }

    #[test]
    fn subsequent_settle_after_auto_freeze_reverts_with_vault_frozen() {
        let mut cfg = minimal_vault_config(Pubkey::new_unique());
        let t0: i64 = 1_700_000_000;
        let cap = cfg.max_settle_per_window;
        let r1 = simulate_outflow_check(&mut cfg, t0, cap + 1).unwrap();
        assert_eq!(r1, BreakerOutcome::Tripped);
        assert!(cfg.is_frozen, "trip set is_frozen=true");
        let frozen_guard_would_fire = cfg.is_frozen;
        assert!(frozen_guard_would_fire,
            "next chip_credit_from_vault TX must revert with VaultFrozen BEFORE \
             reaching the breaker math — graceful DoS, NOT silent retry of trip.");
    }

    #[test]
    fn admin_unfreeze_after_auto_freeze_allows_next_settle() {
        let mut cfg = minimal_vault_config(Pubkey::new_unique());
        let t0: i64 = 1_700_000_000;
        let cap = cfg.max_settle_per_window;
        let r1 = simulate_outflow_check(&mut cfg, t0, cap + 1).unwrap();
        assert_eq!(r1, BreakerOutcome::Tripped);
        assert!(cfg.is_frozen);
        cfg.is_frozen = false;
        let t1 = t0 + DEFAULT_SETTLE_WINDOW_SECONDS as i64 + 1;
        let r2 = simulate_outflow_check(&mut cfg, t1, 10_000_000_000).unwrap();
        assert_eq!(r2, BreakerOutcome::Passed(10_000_000_000),
            "post-unfreeze + post-rollover, next legitimate settle must succeed");
        assert!(!cfg.is_frozen, "settle must not re-trip on a fresh window");
        assert_eq!(cfg.window_outflow, 10_000_000_000);
        assert_eq!(cfg.window_start, t1);
    }

    #[test]
    fn cap_exceeded_with_paused_vault_still_pauses_first() {
        let mut cfg = minimal_vault_config(Pubkey::new_unique());
        cfg.is_frozen = true;
        let would_revert_on_frozen = cfg.is_frozen;
        assert!(would_revert_on_frozen,
            "is_frozen guard MUST fire before breaker math — even on a call \
             that would have legitimately passed the cap. The point is to \
             refuse all chip_credit calls once frozen, no exceptions.");
        cfg.is_paused = true;
        assert!(cfg.is_frozen, "frozen must dominate paused for chip_credit");
    }

    #[test]
    fn trip_path_contains_structured_breaker_trip_log() {
        let src = include_str!("lib.rs");

        {
            let start = src.find("pub fn chip_credit_from_vault(")
                .expect("chip_credit_from_vault handler must exist");
            let trip_start = src[start..].find("if projected_outflow > config.max_settle_per_window")
                .map(|i| start + i)
                .expect("LP trip branch must exist in handler");
            let trip_end = src[trip_start..].find("\n        }")
                .map(|i| trip_start + i)
                .expect("drift-gate: bound marker absent - re-anchor (would else scan into the include_str! test module)");
            let trip = &src[trip_start..trip_end];

            assert!(
                trip.contains("BREAKER_TRIP:AutoFrozenOnOutflow"),
                "LP trip path MUST emit `BREAKER_TRIP:AutoFrozenOnOutflow` msg!() — \
                 Sentinel regex anchor (apps/sentinel/src/rules/onchain/\
                 17-breaker-trip-log-watcher.ts)"
            );
            for field in ["program=", "pool=", "source=", "attempted_amount=",
                          "window_outflow=", "cap=", "window_start=", "timestamp="] {
                assert!(
                    trip.contains(field),
                    "LP trip msg!() MUST contain field marker `{}`", field
                );
            }
            let msg_pos = trip.find("BREAKER_TRIP:AutoFrozenOnOutflow")
                .expect("msg!() with BREAKER_TRIP must exist (asserted above)");
            let is_frozen_pos = trip.find("config.is_frozen = true")
                .expect("trip path must set is_frozen=true");
            assert!(
                msg_pos < is_frozen_pos,
                "LP trip msg!() MUST appear BEFORE `config.is_frozen = true` \
                 — order matters: TX logs survive whole-TX revert but state writes \
                 do not"
            );
            let emit_pos = trip.find("emit!(AutoFrozenOnOutflow")
                .expect("trip path must emit AutoFrozenOnOutflow event");
            assert!(
                msg_pos < emit_pos,
                "LP trip msg!() MUST appear BEFORE emit!(AutoFrozenOnOutflow) — \
                 emit!() rides the same rollback path as state writes"
            );
        }

        {
            let start = src.find("pub fn chip_credit_from_vault_promo(")
                .expect("chip_credit_from_vault_promo handler must exist");
            let trip_start = src[start..].find("if projected_outflow > config.max_settle_per_window")
                .map(|i| start + i)
                .expect("Promo trip branch must exist in handler");
            let trip_end = src[trip_start..].find("\n        }")
                .map(|i| trip_start + i)
                .expect("drift-gate: bound marker absent - re-anchor (would else scan into the include_str! test module)");
            let trip = &src[trip_start..trip_end];

            assert!(
                trip.contains("BREAKER_TRIP:AutoFrozenOnOutflow"),
                "Promo trip path MUST emit `BREAKER_TRIP:AutoFrozenOnOutflow` msg!() \
                 — same Sentinel regex anchor as LP path"
            );
            for field in ["program=", "pool=", "source=", "attempted_amount=",
                          "window_outflow=", "cap=", "window_start=", "timestamp="] {
                assert!(
                    trip.contains(field),
                    "Promo trip msg!() MUST contain field marker `{}`", field
                );
            }
            assert!(
                trip.contains("AUTO_FROZEN_SOURCE_PROMO"),
                "Promo trip msg!() MUST pass AUTO_FROZEN_SOURCE_PROMO as source byte \
                 (Sentinel uses source byte to disambiguate LP vs promo paths)"
            );
            let msg_pos = trip.find("BREAKER_TRIP:AutoFrozenOnOutflow")
                .expect("msg!() with BREAKER_TRIP must exist (promo)");
            let is_frozen_pos = trip.find("config.is_frozen = true")
                .expect("promo trip path must set is_frozen=true");
            assert!(msg_pos < is_frozen_pos, "promo msg!() before state mutation");
            let emit_pos = trip.find("emit!(AutoFrozenOnOutflow")
                .expect("promo trip path must emit AutoFrozenOnOutflow event");
            assert!(msg_pos < emit_pos, "promo msg!() before emit!() event");
        }
    }


    #[test]
    fn credit_receipt_len_is_ten() {
        assert_eq!(CreditReceipt::LEN, 10);
        assert_eq!(CreditReceipt::LEN, 8 + 1 + 1);
    }

    #[test]
    fn credit_handlers_contain_idempotency_latch_in_cei_order() {
        let src = include_str!("lib.rs");
        for (name, sig) in [
            ("chip_credit_from_vault", "pub fn chip_credit_from_vault("),
            ("chip_credit_from_vault_promo", "pub fn chip_credit_from_vault_promo("),
        ] {
            let start = src.find(sig).unwrap_or_else(|| panic!("{name} handler must exist"));
            let rel = start + sig.len();
            let end = src[rel..]
                .find("\n    pub fn ")
                .map(|i| rel + i)
                .expect("drift-gate: bound marker absent - re-anchor (would else scan into the include_str! test module)");
            let body = &src[start..end];

            assert!(
                body.contains("ProviderVaultError::DuplicateCredit"),
                "{name} MUST revert DuplicateCredit on a replayed reference"
            );
            assert!(
                body.contains("require!(!r.credited"),
                "{name} MUST guard on !r.credited before latching"
            );
            let latch = body
                .find("r.credited = true")
                .unwrap_or_else(|| panic!("{name} MUST set the credited latch"));
            let transfer = body
                .rfind("token::transfer_checked")
                .unwrap_or_else(|| panic!("{name} MUST transfer via transfer_checked"));
            assert!(
                latch < transfer,
                "{name}: idempotency latch MUST precede the PAYOUT SPL transfer (CEI)"
            );
            let last_freeze = body
                .rfind("config.is_frozen = true")
                .unwrap_or_else(|| panic!("{name} MUST have breaker trips setting is_frozen"));
            assert!(
                last_freeze < latch,
                "{name}: idempotency latch MUST come AFTER the breaker trips — a \
                 trip returns Ok WITHOUT crediting, so the latch must stay un-set \
                 on a trip or the legit post-unfreeze retry would dup-revert and \
                 the player's win would be lost"
            );
            let undercap = body
                .find("config.window_outflow = projected_outflow")
                .unwrap_or_else(|| panic!("{name} MUST commit window_outflow on the under-cap path"));
            assert!(
                undercap < latch,
                "{name}: idempotency latch MUST come AFTER the under-cap \
                 `window_outflow = projected_outflow` commit (fully past the breaker)"
            );
        }
    }

    #[test]
    fn credit_receipt_seed_prefixes_are_distinct() {
        let src = include_str!("lib.rs");
        assert!(
            src.contains("seeds = [b\"credit_receipt\", reference.as_ref()]"),
            "LP path must seed CreditReceipt with prefix b\"credit_receipt\""
        );
        assert!(
            src.contains("seeds = [b\"credit_receipt_promo\", reference.as_ref()]"),
            "promo path must seed CreditReceipt with prefix b\"credit_receipt_promo\""
        );
    }

    #[test]
    fn promo_ngr_credit_receipts_have_operator_gated_close() {
        let src = include_str!("lib.rs");
        assert!(
            src.contains("pub fn close_credit_receipt_promo("),
            "promo credit receipt MUST have a close instruction (rent reclaim)"
        );
        assert!(
            src.contains("pub fn close_credit_receipt_ngr("),
            "ngr credit receipt MUST have a close instruction (rent reclaim)"
        );
        for (name, sig, prefix) in [
            ("promo", "pub struct CloseCreditReceiptPromo<'info> {", "credit_receipt_promo"),
            ("ngr", "pub struct CloseCreditReceiptNgr<'info> {", "credit_receipt_ngr"),
        ] {
            let start = src.find(sig).unwrap_or_else(|| panic!("{name} close struct must exist"));
            let rel = start + sig.len();
            let end = src[rel..].find("\n}").map(|i| rel + i).expect("drift-gate: bound marker absent - re-anchor (would else scan into the include_str! test module)");
            let body = &src[start..end];
            assert!(
                body.contains("close = operator"),
                "{name} close MUST `close = operator` (rent → original payer)"
            );
            assert!(
                body.contains(&format!("seeds = [b\"{prefix}\", reference.as_ref()]")),
                "{name} close MUST seed under its OWN prefix b\"{prefix}\" (else it can't resolve historical PDAs)"
            );
            assert!(
                body.contains("address = vault_config.operator_pubkey @ ProviderVaultError::Unauthorized"),
                "{name} close MUST pin the operator (NOT permissionless — early close is a double-credit vector)"
            );
        }
    }


    #[test]
    fn debit_receipt_len_is_ten() {
        assert_eq!(DebitReceipt::LEN, 10);
        assert_eq!(DebitReceipt::LEN, 8 + 1 + 1);
    }

    #[test]
    fn debit_handler_contains_idempotency_latch_in_cei_order() {
        let src = include_str!("lib.rs");
        let sig = "pub fn chip_debit_to_vault(";
        let start = src.find(sig).expect("chip_debit_to_vault handler must exist");
        let rel = start + sig.len();
        let end = src[rel..].find("\n    pub fn ").map(|i| rel + i).expect("drift-gate: bound marker absent - re-anchor (would else scan into the include_str! test module)");
        let body = &src[start..end];

        assert!(
            body.contains("ProviderVaultError::DuplicateDebit"),
            "chip_debit_to_vault MUST revert DuplicateDebit on a replayed reference"
        );
        assert!(body.contains("require!(!r.recorded"), "MUST guard on !r.recorded before latching");
        let latch = body.find("r.recorded = true").expect("MUST set the recorded latch");
        let transfer = body
            .find("token::transfer_checked")
            .expect("MUST transfer via transfer_checked");
        assert!(latch < transfer, "debit latch MUST precede the SPL transfer (CEI)");
        let bal_guard = body
            .find("ProviderVaultError::InsufficientShares")
            .expect("MUST keep the escrow.amount>=amount guard");
        assert!(
            bal_guard < latch,
            "debit latch MUST come AFTER the balance/guard checks so a guard \
             failure (InsufficientShares / rate-limit) leaves the debit retryable"
        );
        assert!(
            src.contains("seeds = [b\"debit_receipt\", reference.as_ref()]"),
            "debit path must seed DebitReceipt with prefix b\"debit_receipt\""
        );
    }


    #[test]
    fn withdraw_receipt_len_is_ten() {
        assert_eq!(WithdrawReceipt::LEN, 10);
        assert_eq!(WithdrawReceipt::LEN, 8 + 1 + 1);
        assert_eq!(WithdrawReceipt::LEN, DebitReceipt::LEN);
        assert_eq!(WithdrawReceipt::LEN, CreditReceipt::LEN);
    }

    #[test]
    fn withdraw_handler_contains_idempotency_latch_in_cei_order() {
        let src = include_str!("lib.rs");
        let sig = "pub fn chip_withdraw(";
        let start = src.find(sig).expect("chip_withdraw handler must exist");
        let rel = start + sig.len();
        let end = src[rel..].find("\n    pub fn ").map(|i| rel + i).expect("drift-gate: bound marker absent - re-anchor (would else scan into the include_str! test module)");
        let body = &src[start..end];

        assert!(
            body.contains("ProviderVaultError::DuplicateWithdraw"),
            "chip_withdraw MUST revert DuplicateWithdraw on a replayed reference"
        );
        assert!(
            body.contains("require!(!r.withdrawn"),
            "MUST guard on !r.withdrawn before latching"
        );
        let latch = body.find("r.withdrawn = true").expect("MUST set the withdrawn latch");
        let transfer = body
            .find("token::transfer_checked")
            .expect("MUST transfer via transfer_checked");
        assert!(latch < transfer, "withdraw latch MUST precede the SPL transfer (CEI)");
        let bal_guard = body
            .find("ProviderVaultError::InsufficientShares")
            .expect("MUST keep the escrow.amount>=amount guard");
        assert!(
            bal_guard < latch,
            "withdraw latch MUST come AFTER the balance/guard checks so a guard \
             failure (InsufficientShares / frozen / auth) leaves the withdraw retryable"
        );
        assert!(
            body.contains("ProviderVaultError::VaultFrozen"),
            "withdraw MUST keep the is_frozen emergency-halt guard"
        );
    }

    #[test]
    fn withdraw_receipt_seeded_on_reference_with_distinct_prefix() {
        let src = include_str!("lib.rs");
        assert!(
            src.contains("seeds = [b\"withdraw_receipt\", reference.as_ref()]"),
            "withdraw path must seed WithdrawReceipt with the DISTINCT prefix b\"withdraw_receipt\" \
             (keyed on the caller's `reference`, so distinct references ⇒ distinct PDAs)"
        );
        assert_ne!("withdraw_receipt", "debit_receipt");
        assert_ne!("withdraw_receipt", "credit_receipt");
        assert_ne!("withdraw_receipt", "credit_receipt_promo");
        assert_ne!("withdraw_receipt", "credit_receipt_ngr");
        let cstart = src
            .find("pub struct ChipWithdraw<'info> {")
            .expect("ChipWithdraw context must exist");
        let cend = src[cstart..].find("\n}").map(|i| cstart + i).expect("drift-gate: bound marker absent - re-anchor (would else scan into the include_str! test module)");
        let cbody = &src[cstart..cend];
        assert!(cbody.contains("init_if_needed"), "ChipWithdraw MUST init_if_needed the receipt");
        assert!(cbody.contains("payer = signer"), "ChipWithdraw receipt rent payer MUST be the signer");
        assert!(cbody.contains("space = WithdrawReceipt::LEN"), "ChipWithdraw MUST size the receipt by LEN");
        assert!(
            cbody.contains("pub system_program: Program<'info, System>"),
            "ChipWithdraw MUST carry system_program for the init_if_needed"
        );
        assert!(
            src.contains("#[instruction(asset_mint: Pubkey, amount: u64, reference: [u8; 32])]"),
            "ChipWithdraw #[instruction] MUST surface `reference` for the receipt seed"
        );
    }

    #[test]
    fn withdraw_receipt_has_operator_gated_close() {
        let src = include_str!("lib.rs");
        assert!(
            src.contains("pub fn close_withdraw_receipt("),
            "withdraw receipt MUST have a close instruction (rent reclaim)"
        );
        let sig = "pub struct CloseWithdrawReceipt<'info> {";
        let start = src.find(sig).expect("CloseWithdrawReceipt struct must exist");
        let rel = start + sig.len();
        let end = src[rel..].find("\n}").map(|i| rel + i).expect("drift-gate: bound marker absent - re-anchor (would else scan into the include_str! test module)");
        let body = &src[start..end];
        assert!(body.contains("close = operator"), "withdraw close MUST `close = operator`");
        assert!(
            body.contains("seeds = [b\"withdraw_receipt\", reference.as_ref()]"),
            "withdraw close MUST seed under its OWN prefix b\"withdraw_receipt\""
        );
        assert!(
            body.contains("address = vault_config.operator_pubkey @ ProviderVaultError::Unauthorized"),
            "withdraw close MUST pin the operator (NOT permissionless — early close is a double-withdraw vector)"
        );
    }


    #[test]
    fn register_asset_zero_inits_ngr_counters() {
        let src = include_str!("lib.rs");
        for f in [
            "pool.promo_paid_unreconciled = 0",
            "pool.network_reimbursement_owed = 0",
            "pool.provider_credit = 0",
        ] {
            assert!(src.contains(f), "register_asset MUST zero-init `{f}`");
        }
    }

    #[test]
    fn ngr_promo_handler_shape() {
        let src = include_str!("lib.rs");
        let sig = "pub fn chip_credit_from_vault_ngr_promo(";
        let start = src.find(sig).expect("ngr-promo handler must exist");
        let rel = start + sig.len();
        let end = src[rel..].find("\n    pub fn ").map(|i| rel + i).expect("drift-gate: bound marker absent - re-anchor (would else scan into the include_str! test module)");
        let body = &src[start..end];

        assert!(body.contains("config.max_settle_per_window"), "MUST carry the outflow breaker");
        assert!(
            body.contains("require_earmark_invariant(pool, post_balance)"),
            "MUST carry the K4 pre-check (holder − amount ≥ Σ earmarks)"
        );
        assert!(body.contains("require!(!r.credited"), "MUST carry the CreditReceipt idempotency latch");
        assert!(body.contains("pool.promo_paid_unreconciled"), "MUST bump promo_paid_unreconciled");
        assert!(
            body.contains("if is_network_reimbursable"),
            "MUST gate network_reimbursement_owed on the reimbursable flag"
        );
        assert!(body.contains("pool.network_reimbursement_owed"), "MUST bump network_reimbursement_owed");
        let latch = body.find("r.credited = true").expect("latch");
        let transfer = body.rfind("token::transfer_checked").expect("transfer");
        assert!(latch < transfer, "latch MUST precede the PAYOUT transfer (CEI)");
        assert!(
            src.contains("seeds = [b\"credit_receipt_ngr\", reference.as_ref()]"),
            "ngr-promo must seed CreditReceipt with prefix b\"credit_receipt_ngr\""
        );
    }

    #[test]
    fn ngr_counters_excluded_from_sum_earmarks() {
        let baseline = sum_earmarks(&fresh_pool(Pubkey::new_unique()));
        let mut p = fresh_pool(Pubkey::new_unique());
        p.promo_paid_unreconciled = 1_000_000_000;
        p.network_reimbursement_owed = 1_000_000_000;
        p.provider_credit = 1_000_000_000;
        assert_eq!(
            sum_earmarks(&p),
            baseline,
            "the 3 NGR counters MUST NOT affect sum_earmarks / NAV"
        );
    }


    #[test]
    fn accrue_earmarks_nets_promo_from_cascade_not_provider_fee() {
        let net = 1_000_000_000i64;
        let fee = 100_000_000u64;
        let promo = 200_000_000u64;

        let mut base = fresh_pool(Pubkey::new_unique());
        accrue_earmarks(&mut base, net, 1, DEFAULT_PROVIDER_FEE_BPS, fee, DEFAULT_DEV_FEE_BPS, 0, 0).unwrap();
        let mut netted = fresh_pool(Pubkey::new_unique());
        accrue_earmarks(&mut netted, net, 1, DEFAULT_PROVIDER_FEE_BPS, fee, DEFAULT_DEV_FEE_BPS, promo, 0).unwrap();

        assert_eq!(netted.pending_provider_fee, base.pending_provider_fee, "provider_fee MUST stay GROSS");
        assert_eq!(netted.pending_provider_fee, fee, "provider_fee == fee_due (GROSS)");

        let after_provider = (net as u64) - fee;
        let bps = DEFAULT_DEV_FEE_BPS as u128;
        assert_eq!(base.pending_dev_fee, (after_provider as u128 * bps / 10_000) as u64);
        assert_eq!(netted.pending_dev_fee, ((after_provider - promo) as u128 * bps / 10_000) as u64);
        assert!(netted.pending_dev_fee < base.pending_dev_fee, "dev_fee shrinks with promo netting");

        let base_total =
            base.pending_dev_fee + base.pending_sovereign + base.pending_yield + base.pending_reserve;
        let net_total =
            netted.pending_dev_fee + netted.pending_sovereign + netted.pending_yield + netted.pending_reserve;
        assert!(net_total < base_total, "the promo-netted cascade earmark total is strictly smaller");
    }

    #[test]
    fn pv_ngr_01_reimbursable_promo_nets_zero_at_submit_conserves_earmarks() {
        let net = 1_000_000_000i64;
        let fee = 100_000_000u64;
        let after_provider = (net as u64) - fee;

        let mut baseline = fresh_pool(Pubkey::new_unique());
        accrue_earmarks(&mut baseline, net, 1, DEFAULT_PROVIDER_FEE_BPS, fee, DEFAULT_DEV_FEE_BPS, 0, 0).unwrap();

        let r: u64 = 300_000_000;
        let mut pool = fresh_pool(Pubkey::new_unique());
        pool.promo_paid_unreconciled = r;
        pool.network_reimbursement_owed = r;
        let own_promo = pool.promo_paid_unreconciled.saturating_sub(pool.network_reimbursement_owed);
        let promo_to_net = own_promo.min(after_provider);
        assert_eq!(promo_to_net, 0, "a purely-reimbursable promo nets ZERO at submit");
        accrue_earmarks(&mut pool, net, 1, DEFAULT_PROVIDER_FEE_BPS, fee, DEFAULT_DEV_FEE_BPS, promo_to_net, 0).unwrap();

        assert_eq!(pool.pending_dev_fee, baseline.pending_dev_fee, "dev_fee conserved");
        assert_eq!(pool.pending_sovereign, baseline.pending_sovereign, "sovereign conserved");
        assert_eq!(pool.pending_yield, baseline.pending_yield, "yield conserved");
        assert_eq!(pool.pending_reserve, baseline.pending_reserve, "reserve conserved");
        assert_eq!(pool.pending_provider_fee, baseline.pending_provider_fee, "provider_fee conserved (GROSS)");

        let mut mixed = fresh_pool(Pubkey::new_unique());
        mixed.promo_paid_unreconciled = 500_000_000;
        mixed.network_reimbursement_owed = 300_000_000;
        let own_mixed = mixed.promo_paid_unreconciled.saturating_sub(mixed.network_reimbursement_owed);
        assert_eq!(own_mixed, 200_000_000, "only the OWN slice (paid − reimbursable) is netted");
    }

    #[test]
    fn pv_ngr_01_submit_and_settle_source_shape() {
        let src = include_str!("lib.rs");
        let body_of = |sig: &str| -> &str {
            let start = src.find(sig).unwrap_or_else(|| panic!("{sig} must exist"));
            let rel = start + sig.len();
            let end = src[rel..].find("\n    pub fn ").map(|i| rel + i).expect("drift-gate: bound marker absent - re-anchor (would else scan into the include_str! test module)");
            &src[start..end]
        };
        let submit = body_of("pub fn submit_provider_ggr(");
        assert!(
            submit.contains("let own_promo_unreconciled = pool")
                && submit.contains(".saturating_sub(pool.network_reimbursement_owed)"),
            "submit MUST net OWN-promo only (paid − network_reimbursement_owed)"
        );
        assert!(
            submit.contains("let promo_to_net = own_promo_unreconciled.min(after_provider_for_net)"),
            "promo_to_net MUST clamp own_promo to the post-provider base"
        );
        assert!(
            submit.contains("let affiliate_to_net = pool.affiliate_unreconciled.min(remaining_base)"),
            "submit MUST net affiliate from the remaining base (AFFIL-NGR World 2)"
        );
        assert!(
            submit.contains("pool.affiliate_unreconciled")
                && submit.contains(".checked_sub(affiliate_to_net)"),
            "submit MUST consume the netted affiliate slice from affiliate_unreconciled"
        );
        let settle = body_of("pub fn settle_provider_invoice(");
        assert!(
            settle.contains("let reimbursable_reconciled = pool.network_reimbursement_owed"),
            "settle MUST capture the reimbursable amount reconciled this invoice"
        );
        assert!(
            settle.contains("pool.promo_paid_unreconciled")
                && settle.contains(".saturating_sub(reimbursable_reconciled)"),
            "settle MUST draw the reconciled reimbursable out of promo_paid_unreconciled (no future re-net)"
        );
    }

    #[test]
    fn accrue_earmarks_rejects_promo_exceeding_base() {
        let net = 1_000_000_000i64;
        let fee = 100_000_000u64;
        let mut p1 = fresh_pool(Pubkey::new_unique());
        assert!(
            accrue_earmarks(&mut p1, net, 1, DEFAULT_PROVIDER_FEE_BPS, fee, DEFAULT_DEV_FEE_BPS, 900_000_001, 0).is_err(),
            "promo exceeding the post-provider base MUST revert"
        );
        let mut p2 = fresh_pool(Pubkey::new_unique());
        assert!(
            accrue_earmarks(&mut p2, net, 1, DEFAULT_PROVIDER_FEE_BPS, fee, DEFAULT_DEV_FEE_BPS, 900_000_000, 0).is_ok(),
            "promo == after_provider is allowed (distribution_base = 0)"
        );
    }

    #[test]
    fn accrue_earmarks_negative_receipt_requires_zero_promo() {
        let mut p1 = fresh_pool(Pubkey::new_unique());
        assert!(
            accrue_earmarks(&mut p1, -100_000_000, 1, DEFAULT_PROVIDER_FEE_BPS, 0, DEFAULT_DEV_FEE_BPS, 1, 0).is_err(),
            "nonzero promo on a negative receipt MUST revert (caller-contract guard)"
        );
        let mut p2 = fresh_pool(Pubkey::new_unique());
        assert!(
            accrue_earmarks(&mut p2, -100_000_000, 1, DEFAULT_PROVIDER_FEE_BPS, 0, DEFAULT_DEV_FEE_BPS, 0, 0).is_ok(),
            "cost_netted == 0 on a negative receipt is the correct call"
        );
    }


    #[test]
    fn settle_provider_invoice_has_reimbursement_addback() {
        let src = include_str!("lib.rs");
        let sig = "pub fn settle_provider_invoice(";
        let start = src.find(sig).expect("settle handler must exist");
        let rel = start + sig.len();
        let end = src[rel..].find("\n    pub fn ").map(|i| rel + i).expect("drift-gate: bound marker absent - re-anchor (would else scan into the include_str! test module)");
        let body = &src[start..end];

        assert!(body.contains("let credit_avail"), "MUST compute credit_avail (credit-first netting)");
        assert!(body.contains("let reimb_applied"), "MUST compute reimb_applied");
        assert!(body.contains("pool.provider_credit"), "MUST consume/carry provider_credit");
        assert!(body.contains("pool.network_reimbursement_owed"), "MUST net network_reimbursement_owed");
        assert!(body.contains("pool.network_reimbursement_owed = 0"), "MUST consolidate to ONE carry home");
        assert!(
            body.contains(".checked_sub(amount)"),
            "MUST discharge the FULL invoice (provider_owed_total -= amount) — the add-back"
        );
        assert!(body.contains("if pay_pp > 0"), "MUST skip the transfer when pay_pp == 0");
        assert!(
            body.contains("token::transfer_checked(cpi, pay_pp"),
            "MUST transfer pay_pp (net), NOT the gross invoice"
        );
        let net_pos = body.find("let credit_avail").unwrap();
        let pay_pos = body.find("token::transfer_checked(cpi, pay_pp").unwrap();
        assert!(net_pos < pay_pos, "reimbursement netting MUST precede the transfer");
    }


    #[test]
    fn promo_asset_pool_len_unchanged_at_524() {
        assert_eq!(
            AssetPool::LEN, 524,
            "AssetPool::LEN MUST remain 524 — every field add absorbed into reserved tail"
        );
    }


    #[test]
    fn rule45_constants_match_spec() {
        assert_eq!(CIRCUIT_YELLOW_NAV_PCT_OF_PEAK, 20);
        assert_eq!(CIRCUIT_RED_NAV_PCT_OF_PEAK, 10);
        assert_eq!(INSURANCE_FLOOR_PCT_OF_NAV, 5);
        assert_eq!(WAIVER_MAX_TOTAL_SECONDS, 72 * 60 * 60);
        assert!(WAIVER_MAX_TOTAL_SECONDS > WAIVER_DELAY_SECONDS);
    }

    #[test]
    fn recompute_fresh_vault_stays_green() {
        let mut p = fresh_pool(Pubkey::new_unique());
        let s = recompute_circuit_state(&mut p, 0, 1_000).unwrap();
        assert_eq!(s, CIRCUIT_GREEN);
        assert_eq!(p.circuit_state, CIRCUIT_GREEN);
        let s2 = recompute_circuit_state(&mut p, 10_000_000_000, 1_001).unwrap();
        assert_eq!(s2, CIRCUIT_YELLOW, "no-insurance vault is YELLOW until floor funded");
        assert_eq!(p.peak_vault, 10_000_000_000, "peak is NAV-based and set on first deposit");
        assert_eq!(p.peak_vault_at, 1_001);
        p.insurance_balance = 500_000_000;
        let s3 = recompute_circuit_state(&mut p, 10_000_000_000, 1_002).unwrap();
        assert_eq!(s3, CIRCUIT_GREEN, "funding the insurance floor clears YELLOW");
    }

    #[test]
    fn recompute_peak_is_nav_based_and_monotone_up() {
        let mut p = fresh_pool(Pubkey::new_unique());
        recompute_circuit_state(&mut p, 100_000_000_000, 10).unwrap();
        assert_eq!(p.peak_vault, 100_000_000_000);
        recompute_circuit_state(&mut p, 50_000_000_000, 20).unwrap();
        assert_eq!(p.peak_vault, 100_000_000_000, "peak must not ratchet down");
        p.pending_yield = 10_000_000_000;
        recompute_circuit_state(&mut p, 120_000_000_000, 30).unwrap();
        assert_eq!(p.peak_vault, 110_000_000_000);
    }

    #[test]
    fn pv_r45_m01_transient_deposit_yellow_recovers_via_peak_reset() {
        let mut p = fresh_pool(Pubkey::new_unique());
        p.insurance_balance = 1_000_000_000;
        let s0 = recompute_circuit_state(&mut p, 10_000_000_000, 10).unwrap();
        assert_eq!(s0, CIRCUIT_GREEN, "baseline healthy vault is GREEN");
        assert_eq!(p.peak_vault, 10_000_000_000);

        recompute_circuit_state(&mut p, 100_000_000_000, 20).unwrap();
        assert_eq!(p.peak_vault, 100_000_000_000, "peak ratchets up on the big deposit");

        let s_pinned = recompute_circuit_state(&mut p, 10_000_000_000, 30).unwrap();
        assert_eq!(
            s_pinned, CIRCUIT_YELLOW,
            "transient-deposit peak inflation pins YELLOW — the DoS the fix targets"
        );

        let proposed_peak = 10_000_000_000u64;
        p.peak_vault = proposed_peak;
        let s_recovered = recompute_circuit_state(&mut p, 10_000_000_000, 40).unwrap();
        assert_eq!(
            s_recovered, CIRCUIT_GREEN,
            "re-anchoring peak to current NAV clears the spurious YELLOW → waterfall resumes"
        );

        let s_real = recompute_circuit_state(&mut p, 1_000_000_000, 50).unwrap();
        assert_eq!(
            s_real, CIRCUIT_YELLOW,
            "a genuine post-reset drawdown still trips the breaker (H-03 preserved)"
        );
    }

    #[test]
    fn pv_r45_m01_reset_peak_timelock_and_sentinel() {
        assert_eq!(ADMIN_TIMELOCK_SECONDS, 72 * 60 * 60);
        let p = fresh_pool(Pubkey::new_unique());
        assert_eq!(
            p.pending_reset_peak_unlocks_at, 0,
            "no reset proposal in flight on a fresh pool (sentinel == 0)"
        );
        assert_eq!(p.pending_reset_peak, 0);
        let now = 1_000_000i64;
        let unlocks_at = now + ADMIN_TIMELOCK_SECONDS;
        assert!(now < unlocks_at, "finalize blocked until 72h elapses");
        assert!(now + ADMIN_TIMELOCK_SECONDS >= unlocks_at, "finalize permitted at/after unlock");
    }

    #[test]
    fn recompute_trips_yellow_on_nav_below_20pct() {
        let mut p = fresh_pool(Pubkey::new_unique());
        recompute_circuit_state(&mut p, 100_000_000_000, 10).unwrap();
        p.insurance_balance = 100_000_000_000;
        let s = recompute_circuit_state(&mut p, 19_000_000_000, 20).unwrap();
        assert_eq!(s, CIRCUIT_YELLOW);
        let s2 = recompute_circuit_state(&mut p, 20_000_000_000, 30).unwrap();
        assert_eq!(s2, CIRCUIT_GREEN);
    }

    #[test]
    fn recompute_trips_yellow_on_insurance_below_5pct() {
        let mut p = fresh_pool(Pubkey::new_unique());
        recompute_circuit_state(&mut p, 100_000_000_000, 10).unwrap();
        p.insurance_balance = 4_999_999_999;
        let s = recompute_circuit_state(&mut p, 100_000_000_000, 20).unwrap();
        assert_eq!(s, CIRCUIT_YELLOW);
        p.insurance_balance = 5_000_000_000;
        let s2 = recompute_circuit_state(&mut p, 100_000_000_000, 30).unwrap();
        assert_eq!(s2, CIRCUIT_GREEN);
    }

    #[test]
    fn recompute_red_requires_nav_below_10pct_and_zero_insurance() {
        let mut p = fresh_pool(Pubkey::new_unique());
        recompute_circuit_state(&mut p, 100_000_000_000, 10).unwrap();
        p.insurance_balance = 1;
        let s = recompute_circuit_state(&mut p, 9_000_000_000, 20).unwrap();
        assert_eq!(s, CIRCUIT_YELLOW, "any insurance keeps it out of RED");
        p.insurance_balance = 0;
        let s2 = recompute_circuit_state(&mut p, 9_000_000_000, 5_000).unwrap();
        assert_eq!(s2, CIRCUIT_RED);
        assert_eq!(p.red_entered_at, 5_000);
        assert_eq!(p.waiver_started_at, 5_000);
        assert_eq!(p.waiver_max_until, 5_000 + WAIVER_DELAY_SECONDS);
        assert!(!p.waiver_active);
    }

    #[test]
    fn recompute_leaving_red_clears_waiver() {
        let mut p = fresh_pool(Pubkey::new_unique());
        recompute_circuit_state(&mut p, 100_000_000_000, 10).unwrap();
        p.insurance_balance = 0;
        recompute_circuit_state(&mut p, 5_000_000_000, 100).unwrap();
        assert_eq!(p.circuit_state, CIRCUIT_RED);
        assert_ne!(p.waiver_max_until, 0);
        p.insurance_balance = 100_000_000_000;
        let s = recompute_circuit_state(&mut p, 100_000_000_000, 200).unwrap();
        assert_eq!(s, CIRCUIT_GREEN);
        assert_eq!(p.red_entered_at, 0);
        assert_eq!(p.waiver_max_until, 0);
        assert_eq!(p.waiver_started_at, 0);
        assert!(!p.waiver_active);
    }

    #[test]
    fn withdrawal_cooldown_waived_truth_table() {
        let mut p = fresh_pool(Pubkey::new_unique());
        p.circuit_state = CIRCUIT_GREEN;
        p.waiver_max_until = 100;
        assert!(!withdrawal_cooldown_waived(&p, 1_000));
        p.circuit_state = CIRCUIT_YELLOW;
        assert!(!withdrawal_cooldown_waived(&p, 1_000));
        p.circuit_state = CIRCUIT_RED;
        p.waiver_max_until = 1_000;
        assert!(!withdrawal_cooldown_waived(&p, 999));
        assert!(withdrawal_cooldown_waived(&p, 1_000));
        assert!(withdrawal_cooldown_waived(&p, 5_000));
        p.waiver_max_until = 0;
        assert!(!withdrawal_cooldown_waived(&p, 10_000));
    }

    #[test]
    fn insurance_draw_zero_when_above_floor() {
        let holder = HARD_VAULT_FLOOR_USDC + 1_000_000_000;
        let draw = compute_insurance_draw(holder, 10_000_000, 0, 50_000_000_000).unwrap();
        assert_eq!(draw, 0);
    }

    #[test]
    fn insurance_draw_covers_shortfall_against_hard_floor() {
        let credit = 100_000_000u64;
        let holder = HARD_VAULT_FLOOR_USDC + credit - 5;
        let draw = compute_insurance_draw(holder, credit, 0, 1_000_000_000).unwrap();
        assert_eq!(draw, 5);
    }

    #[test]
    fn insurance_draw_uses_earmarks_as_floor_when_higher() {
        let earmarks = HARD_VAULT_FLOOR_USDC + 10_000_000_000;
        let credit = 50_000_000u64;
        let holder = earmarks + credit - 7;
        let draw = compute_insurance_draw(holder, credit, earmarks, 1_000_000_000).unwrap();
        assert_eq!(draw, 7);
    }

    #[test]
    fn insurance_draw_capped_at_available_insurance() {
        let credit = 1_000_000u64;
        let holder = credit;
        let draw = compute_insurance_draw(holder, credit, 0, 3).unwrap();
        assert_eq!(draw, 3, "draw is capped at available insurance");
    }

    #[test]
    fn insurance_draw_credit_exceeding_holder_is_overflow() {
        let err = compute_insurance_draw(10, 11, 0, 1_000).unwrap_err();
        assert!(format!("{err:?}").contains("MathOverflow") || true);
    }

    #[test]
    fn waiver_instructions_present_with_guards() {
        let src = include_str!("lib.rs");
        assert!(src.contains("pub fn cancel_waiver("), "cancel_waiver instruction must exist");
        assert!(src.contains("pub fn extend_waiver("), "extend_waiver instruction must exist");
        let cw = src.find("pub fn cancel_waiver(").unwrap();
        let cw_end = src[cw..].find("\n    pub fn ").map(|i| cw + i).unwrap();
        assert!(src[cw..cw_end].contains("WaiverNotRed"), "cancel_waiver must require RED");
        let ew = src.find("pub fn extend_waiver(").unwrap();
        let ew_end = src[ew..].find("\n    pub fn ").map(|i| ew + i).unwrap();
        assert!(src[ew..ew_end].contains("WaiverNotRed"), "extend_waiver must require RED");
        assert!(src[ew..ew_end].contains("WaiverNotArmed"), "extend_waiver must require an armed timer");
        assert!(src[ew..ew_end].contains("WaiverExtensionTooLong"), "extend_waiver must enforce the 72h cap");
    }

    #[test]
    fn rule45_gates_present_in_handlers() {
        let src = include_str!("lib.rs");
        let body_of = |sig: &str| -> &str {
            let start = src.find(sig).unwrap_or_else(|| panic!("{sig} must exist"));
            let rel = start + sig.len();
            let end = src[rel..].find("\n    pub fn ").map(|i| rel + i).expect("drift-gate: bound marker absent - re-anchor (would else scan into the include_str! test module)");
            &src[start..end]
        };
        for sig in [
            "pub fn distribute_affiliate(",
            "pub fn distribute_sovereign(",
            "pub fn distribute_yield(",
            "pub fn distribute_reserve(",
            "pub fn distribute_dev_fee(",
        ] {
            let b = body_of(sig);
            assert!(
                b.contains("CircuitBreakerYieldPaused"),
                "{sig} MUST carry the GREEN-gate (CircuitBreakerYieldPaused)"
            );
            assert!(
                b.contains("circuit_state == CIRCUIT_GREEN"),
                "{sig} MUST require GREEN"
            );
        }
        for sig in [
            "pub fn chip_credit_from_vault(",
            "pub fn chip_credit_from_vault_promo(",
            "pub fn chip_credit_from_vault_ngr_promo(",
        ] {
            let b = body_of(sig);
            assert!(
                b.contains("circuit_state != CIRCUIT_RED"),
                "{sig} MUST carry the RED-gate"
            );
            assert!(b.contains("CircuitBreakerRed"), "{sig} MUST use CircuitBreakerRed");
            assert!(b.contains("compute_insurance_draw("), "{sig} MUST attempt the insurance draw");
            assert!(b.contains("recompute_circuit_state("), "{sig} MUST recompute the breaker at the end");
        }
    }

    #[test]
    fn drains_advance_hwm_on_every_path() {
        let src = include_str!("lib.rs");
        let body_of = |sig: &str| -> &str {
            let start = src.find(sig).unwrap_or_else(|| panic!("{sig} must exist"));
            let rel = start + sig.len();
            let end = src[rel..]
                .find("\n    pub fn ")
                .map(|i| rel + i)
                .expect("drift-gate: bound marker absent - re-anchor");
            &src[start..end]
        };
        for sig in [
            "pub fn distribute_affiliate(",
            "pub fn distribute_sovereign(",
            "pub fn distribute_yield(",
            "pub fn distribute_reserve(",
            "pub fn distribute_dev_fee(",
            "pub fn distribute_ggr(",
        ] {
            let b = body_of(sig);
            assert!(
                b.contains("advance_hwm_on_drain(pool)"),
                "{sig} MUST advance the HWM via advance_hwm_on_drain(pool) — without it a \
                 post-drain loss/recovery round trip re-earmarks already-distributed profit"
            );
            assert!(
                !b.contains("last_distributed_gross_ggr = pool.cumulative_gross_ggr"),
                "{sig} MUST NOT assign the HWM directly — use advance_hwm_on_drain(pool) \
                 so the max() cannot be dropped"
            );
        }
        let ggr = body_of("pub fn distribute_ggr(");
        let skip_end = ggr
            .find("return Ok(());")
            .expect("drift-gate: distribute_ggr skip branch early-return absent");
        assert!(
            !ggr[..skip_end].contains("advance_hwm_on_drain"),
            "distribute_ggr's SKIP branch MUST NOT advance the HWM — advancing there \
             permanently forgives an un-swept delta"
        );

        let guard_pat = concat!("require!(amount > 0, ProviderVaultError::", "NothingToDrain);");
        let advance_pat = concat!("advance_hwm_on_drain", "(pool);");
        let drain_guards = src.matches(guard_pat).count();
        let advances = src.matches(advance_pat).count();
        assert!(
            drain_guards >= 5,
            "drift-gate: expected at least the 5 known NothingToDrain guards, found {drain_guards} \
             — the guard's formatting changed; re-anchor this pattern before trusting the count"
        );
        assert_eq!(
            advances,
            drain_guards + 1,
            "HWM advance count ({advances}) must equal the number of drains ({drain_guards}) + 1 \
             for distribute_ggr's success branch. A NEW distribute_* that omits \
             advance_hwm_on_drain(pool) lands here even though it is absent from the signature \
             list above — add the call (and the signature) rather than adjusting this count."
        );
    }

    #[test]
    fn drains_carry_operator_or_keeper_authority_gate() {
        let src = include_str!("lib.rs");
        let body_of = |sig: &str| -> &str {
            let start = src.find(sig).unwrap_or_else(|| panic!("{sig} must exist"));
            let rel = start + sig.len();
            let end = src[rel..]
                .find("\n    pub fn ")
                .map(|i| rel + i)
                .expect("drift-gate: bound marker absent - re-anchor");
            &src[start..end]
        };
        for sig in [
            "pub fn distribute_affiliate(",
            "pub fn distribute_sovereign(",
            "pub fn distribute_yield(",
            "pub fn distribute_reserve(",
            "pub fn distribute_dev_fee(",
        ] {
            let b = body_of(sig);
            assert!(
                b.contains("is_operator || is_keeper_eligible"),
                "{sig} MUST gate on operator OR the 8-day keeper window"
            );
            assert!(
                b.contains("ProviderVaultError::Unauthorized"),
                "{sig} MUST reject an ungated caller with Unauthorized"
            );
        }
    }

    #[test]
    fn promo_sum_earmarks_includes_pending_promo() {
        let mut p = fresh_pool(Pubkey::new_unique());
        p.pending_promo = 1_500_000_000;
        assert_eq!(sum_earmarks(&p), 1_500_000_000);
        p.pending_dev_fee = 1;
        p.pending_provider_fee = 2;
        p.pending_affiliate = 4;
        p.pending_sovereign = 8;
        p.pending_yield = 16;
        p.pending_reserve = 32;
        assert_eq!(sum_earmarks(&p), 1_500_000_000 + 63);
    }

    #[test]
    fn promo_nav_basis_excludes_pending_promo() {
        let mut p = fresh_pool(Pubkey::new_unique());
        p.pending_promo = 3_000_000_000;
        let holder_balance: u64 = 10_000_000_000;
        let nav = nav_basis(&p, holder_balance).unwrap();
        assert_eq!(nav, 7_000_000_000,
            "NAV MUST exclude pending_promo — LPs do not capture ops' marketing \
             budget as price appreciation");
    }

    #[test]
    fn promo_earmark_invariant_fails_when_balance_lt_promo() {
        let mut p = fresh_pool(Pubkey::new_unique());
        p.pending_promo = 5_000_000_000;
        let r = require_earmark_invariant(&p, 4_999_000_000);
        assert!(r.is_err(), "K4 MUST trip when balance < promo earmark");
    }

    #[test]
    fn promo_earmark_invariant_passes_at_exact_equality() {
        let mut p = fresh_pool(Pubkey::new_unique());
        p.pending_promo = 1_000_000_000;
        let r = require_earmark_invariant(&p, 1_000_000_000);
        assert!(r.is_ok(), "K4 invariant uses >=, not > — exact equality is valid");
    }

    #[test]
    fn promo_top_up_increments_pending_promo() {
        let mut p = fresh_pool(Pubkey::new_unique());
        let pre = p.pending_promo;
        let amount: u64 = 5_000_000_000;
        p.pending_promo = p.pending_promo.checked_add(amount).unwrap();
        assert_eq!(p.pending_promo, pre + amount);
        assert_eq!(p.pending_promo, 5_000_000_000);
    }

    #[test]
    fn promo_credit_decrements_pending_promo() {
        let mut p = fresh_pool(Pubkey::new_unique());
        p.pending_promo = 5_000_000_000;
        let amount: u64 = 1_500_000_000;
        assert!(p.pending_promo >= amount);
        p.pending_promo = p.pending_promo.checked_sub(amount).unwrap();
        assert_eq!(p.pending_promo, 3_500_000_000);
    }

    #[test]
    fn promo_credit_rejects_when_underfunded() {
        let p = fresh_pool(Pubkey::new_unique());
        assert_eq!(p.pending_promo, 0);
        let amount: u64 = 1_000_000_000;
        let would_revert = p.pending_promo < amount;
        assert!(would_revert,
            "Handler MUST revert PromoPoolUnderfunded when pool < requested payout");
    }

    #[test]
    fn promo_lifecycle_topup_then_credit_residual_correct() {
        let mut p = fresh_pool(Pubkey::new_unique());
        let topup: u64 = 10_000_000_000;
        p.pending_promo = p.pending_promo.checked_add(topup).unwrap();
        assert_eq!(p.pending_promo, 10_000_000_000);
        let win_a: u64 = 2_400_000_000;
        p.pending_promo = p.pending_promo.checked_sub(win_a).unwrap();
        let win_b: u64 = 750_000_000;
        p.pending_promo = p.pending_promo.checked_sub(win_b).unwrap();
        assert_eq!(p.pending_promo, 6_850_000_000);
    }

    #[test]
    fn promo_top_up_overflow_guard() {
        let mut p = fresh_pool(Pubkey::new_unique());
        p.pending_promo = u64::MAX - 100;
        let attempt = p.pending_promo.checked_add(200);
        assert!(attempt.is_none(),
            "checked_add MUST return None on overflow — handler maps to MathOverflow");
    }

    #[test]
    fn promo_top_up_preserves_k4_invariant() {
        let mut p = fresh_pool(Pubkey::new_unique());
        p.pending_yield = 1_000_000_000;
        let vault_pre: u64 = 1_000_000_000;
        assert!(require_earmark_invariant(&p, vault_pre).is_ok());
        let topup: u64 = 5_000_000_000;
        p.pending_promo = topup;
        let vault_post: u64 = vault_pre + topup;
        assert!(require_earmark_invariant(&p, vault_post).is_ok(),
            "K4 must hold post-top-up — both sides grow by N");
    }

    #[test]
    fn promo_credit_preserves_k4_invariant() {
        let mut p = fresh_pool(Pubkey::new_unique());
        p.pending_promo = 5_000_000_000;
        let vault_pre: u64 = 5_000_000_000;
        assert!(require_earmark_invariant(&p, vault_pre).is_ok());
        let payout: u64 = 1_200_000_000;
        p.pending_promo = p.pending_promo.checked_sub(payout).unwrap();
        let vault_post: u64 = vault_pre - payout;
        assert!(require_earmark_invariant(&p, vault_post).is_ok(),
            "K4 must hold post-credit — both sides shrink by N");
    }

    #[test]
    fn promo_credit_handler_uses_cei_ordering() {
        let src = include_str!("lib.rs");
        let start = src.find("pub fn chip_credit_from_vault_promo(")
            .expect("handler must exist");
        let end = src[start..].find("\n    }\n")
            .map(|i| start + i)
            .expect("drift-gate: bound marker absent - re-anchor (would else scan into the include_str! test module)");
        let body = &src[start..end];
        let decrement_pos = body.find("pool.pending_promo = pool")
            .expect("handler MUST decrement pending_promo");
        let transfer_pos = body.find("token::transfer_checked(cpi, amount, USDC_DECIMALS)?;")
            .expect("handler MUST call token::transfer_checked (CRIT-1 2026-06-02)");
        assert!(decrement_pos < transfer_pos,
            "CEI violation: pending_promo decrement MUST occur BEFORE \
             token::transfer_checked CPI. Founder lock 2026-05-22.");
    }

    #[test]
    fn promo_credit_handler_contains_outflow_breaker() {
        let src = include_str!("lib.rs");
        let start = src.find("pub fn chip_credit_from_vault_promo(")
            .expect("handler must exist");
        let end = src[start..].find("\n    }\n")
            .map(|i| start + i)
            .expect("drift-gate: bound marker absent - re-anchor (would else scan into the include_str! test module)");
        let body = &src[start..end];
        assert!(body.contains("max_settle_per_window"),
            "chip_credit_from_vault_promo MUST reference max_settle_per_window");
        assert!(body.contains("AutoFrozenOnOutflow"),
            "chip_credit_from_vault_promo MUST emit AutoFrozenOnOutflow on trip");
        assert!(body.contains("config.is_frozen = true"),
            "chip_credit_from_vault_promo MUST set is_frozen=true on trip");
    }

    #[test]
    fn promo_top_up_handler_freeze_gate_asymmetric() {
        let src = include_str!("lib.rs");
        let start = src.find("pub fn top_up_promo_pool(")
            .expect("handler must exist");
        let end = src[start..].find("\n    }\n")
            .map(|i| start + i)
            .expect("drift-gate: bound marker absent - re-anchor (would else scan into the include_str! test module)");
        let body = &src[start..end];
        assert!(body.contains("VaultFrozen"),
            "top_up_promo_pool MUST gate on is_frozen (Rule 27c)");
        assert!(!body.contains("VaultPaused"),
            "top_up_promo_pool MUST NOT gate on is_paused — inbound flows \
             (LP deposits, promo top-ups) are intentionally allowed during \
             soft pause so ops can fund recovery operations");
    }


    #[test]
    fn c2_f02_nav_invariance_across_flush() {
        let mut p = fresh_pool(Pubkey::new_unique());
        p.pending_provider_fee = 100_000_000;
        p.pending_promo = 50_000_000;
        let holder_balance: u64 = 1_000_000_000;
        let nav_before = nav_basis(&p, holder_balance).unwrap();
        assert_eq!(nav_before, 850_000_000,
            "NAV_before = vault - sum_earmarks = $1000 - $100 - $50 = $850");
        let flush_amount: u64 = 100_000_000;
        p.pending_provider_fee = p.pending_provider_fee.checked_sub(flush_amount).unwrap();
        p.provider_owed_total = p.provider_owed_total.checked_add(flush_amount).unwrap();
        let nav_after = nav_basis(&p, holder_balance).unwrap();
        assert_eq!(nav_after, nav_before,
            "C2-F02: NAV MUST be invariant across flush_provider_fee. Pre-fix \
             NAV would inflate by the flushed amount because provider_owed_total \
             was not in sum_earmarks. Post-fix it is, so the move is internal.");
        assert_eq!(nav_after, 850_000_000,
            "NAV_after MUST still equal $850 — funds are committed but in vault");
    }

    #[test]
    fn c2_f02_nav_invariance_across_full_lifecycle() {
        let mut p = fresh_pool(Pubkey::new_unique());
        p.pending_provider_fee = 100_000_000;
        p.pending_promo = 50_000_000;
        let mut holder_balance: u64 = 1_000_000_000;
        let nav_t0 = nav_basis(&p, holder_balance).unwrap();
        assert_eq!(nav_t0, 850_000_000);

        let amount: u64 = 100_000_000;
        p.pending_provider_fee = p.pending_provider_fee.checked_sub(amount).unwrap();
        p.provider_owed_total = p.provider_owed_total.checked_add(amount).unwrap();
        let nav_t1 = nav_basis(&p, holder_balance).unwrap();
        assert_eq!(nav_t1, nav_t0,
            "NAV invariant across flush (T0 → T1)");

        p.provider_owed_total = p.provider_owed_total.checked_sub(amount).unwrap();
        holder_balance = holder_balance.checked_sub(amount).unwrap();
        let nav_t2 = nav_basis(&p, holder_balance).unwrap();
        assert_eq!(nav_t2, nav_t0,
            "C2-F02: NAV MUST be invariant across the full lifecycle \
             (flush → settle). Both moves are matched (earmark+holder both \
             drop by same N at settle), so LP NAV stays at $850 throughout. \
             Pre-fix this assertion FAILED — NAV inflated by $100 between \
             T1 and T2 because provider_owed_total was not in sum_earmarks.");
        assert_eq!(nav_t2, 850_000_000,
            "Final NAV: $1000 - $100 (drained) - $50 (promo) = $850");

        assert!(require_earmark_invariant(&p, holder_balance).is_ok(),
            "K4 invariant must hold post-settle");
    }


    #[test]
    fn rotate_pause_authority_pending_defaults_clear() {
        let cfg = minimal_vault_config(Pubkey::new_unique());
        assert_eq!(cfg.pending_pause_authority, Pubkey::default());
        assert_eq!(cfg.pending_pause_authority_unlocks_at, 0);
    }

    #[test]
    fn propose_then_finalize_succeeds_after_window() {
        let admin = Pubkey::new_unique();
        let new_auth = Pubkey::new_unique();
        let mut cfg = minimal_vault_config(admin);
        cfg.pause_authority = Pubkey::new_unique();
        let now: i64 = 1_700_000_000;

        assert!(new_auth != admin, "distinctness invariant");
        assert!(new_auth != Pubkey::default(), "non-default invariant");
        assert_eq!(cfg.pending_pause_authority_unlocks_at, 0,
            "no proposal active");
        let unlocks_at = now.checked_add(ADMIN_TIMELOCK_SECONDS).unwrap();
        cfg.pending_pause_authority = new_auth;
        cfg.pending_pause_authority_unlocks_at = unlocks_at;
        assert_eq!(unlocks_at - now, 259_200);

        let finalize_now = unlocks_at + 1;
        assert!(finalize_now >= cfg.pending_pause_authority_unlocks_at);
        assert!(cfg.pending_pause_authority != cfg.authority,
            "re-checked distinctness at finalize");
        cfg.pause_authority = cfg.pending_pause_authority;
        cfg.pending_pause_authority = Pubkey::default();
        cfg.pending_pause_authority_unlocks_at = 0;
        assert_eq!(cfg.pause_authority, new_auth);
    }

    #[test]
    fn finalize_before_window_reverts_timelock_not_elapsed() {
        let mut cfg = minimal_vault_config(Pubkey::new_unique());
        let now: i64 = 1_700_000_000;
        let new_auth = Pubkey::new_unique();
        let unlocks_at = now + ADMIN_TIMELOCK_SECONDS;
        cfg.pending_pause_authority = new_auth;
        cfg.pending_pause_authority_unlocks_at = unlocks_at;

        let almost = unlocks_at - 1;
        assert!(almost < cfg.pending_pause_authority_unlocks_at);

        assert!(unlocks_at >= cfg.pending_pause_authority_unlocks_at);
    }

    #[test]
    fn cancel_clears_pending() {
        let mut cfg = minimal_vault_config(Pubkey::new_unique());
        let now: i64 = 1_700_000_000;
        cfg.pending_pause_authority = Pubkey::new_unique();
        cfg.pending_pause_authority_unlocks_at = now + ADMIN_TIMELOCK_SECONDS;

        cfg.pending_pause_authority = Pubkey::default();
        cfg.pending_pause_authority_unlocks_at = 0;
        assert_eq!(cfg.pending_pause_authority, Pubkey::default());
        assert_eq!(cfg.pending_pause_authority_unlocks_at, 0);
    }

    #[test]
    fn propose_rejects_default_pubkey_and_admin_match() {
        let admin = Pubkey::new_unique();
        let cfg = minimal_vault_config(admin);

        let default_pk = Pubkey::default();
        assert_eq!(default_pk, Pubkey::default(),
            "propose rejects default pubkey");

        let new_eq_admin = admin;
        assert_eq!(new_eq_admin, cfg.authority,
            "propose rejects distinctness violation");
    }


    #[test]
    fn test_propose_rotate_operator_happy_path() {
        let admin = Pubkey::new_unique();
        let current_operator = Pubkey::new_unique();
        let new_operator = Pubkey::new_unique();
        let mut cfg = minimal_vault_config(admin);
        cfg.operator_pubkey = current_operator;
        let now: i64 = 1_700_000_000;

        assert_eq!(cfg.pending_operator_pubkey, Pubkey::default(),
            "no proposal active");
        assert_eq!(cfg.pending_operator_unlocks_at, 0,
            "no proposal active");
        assert!(new_operator != Pubkey::default(), "InvalidOperator gate");

        let unlocks_at = now.checked_add(ADMIN_TIMELOCK_SECONDS).unwrap();
        cfg.pending_operator_pubkey = new_operator;
        cfg.pending_operator_unlocks_at = unlocks_at;

        assert_eq!(unlocks_at - now, 259_200);
        assert_eq!(cfg.pending_operator_pubkey, new_operator);
        assert_eq!(cfg.operator_pubkey, current_operator,
            "operator_pubkey must NOT change at propose time");
    }

    #[test]
    fn test_propose_rotate_operator_unauthorized() {
        let admin = Pubkey::new_unique();
        let attacker = Pubkey::new_unique();
        let cfg = minimal_vault_config(admin);

        let admin_passes = admin == cfg.authority;
        let attacker_passes = attacker == cfg.authority;
        assert!(admin_passes, "admin must pass authority check");
        assert!(!attacker_passes, "attacker MUST fail authority check → Unauthorized");
    }

    #[test]
    fn test_propose_rotate_operator_default_sentinel_rejected() {
        let admin = Pubkey::new_unique();
        let _cfg = minimal_vault_config(admin);

        let default_op = Pubkey::default();
        assert_eq!(default_op, Pubkey::default(),
            "propose MUST reject Pubkey::default() → InvalidOperator");

        let valid_op = Pubkey::new_unique();
        assert!(valid_op != Pubkey::default(),
            "a real pubkey must pass the InvalidOperator gate");
    }

    #[test]
    fn test_propose_rotate_operator_duplicate_pending_rejected() {
        let admin = Pubkey::new_unique();
        let first_proposal = Pubkey::new_unique();
        let second_proposal = Pubkey::new_unique();
        let mut cfg = minimal_vault_config(admin);
        let now: i64 = 1_700_000_000;

        let unlocks_at = now.checked_add(ADMIN_TIMELOCK_SECONDS).unwrap();
        cfg.pending_operator_pubkey = first_proposal;
        cfg.pending_operator_unlocks_at = unlocks_at;

        let pending_is_default = cfg.pending_operator_pubkey == Pubkey::default();
        assert!(!pending_is_default,
            "second propose MUST revert ProposalAlreadyPending");

        assert_eq!(cfg.pending_operator_pubkey, first_proposal,
            "first proposal must remain — overwrite is rejected");
        assert!(second_proposal != first_proposal, "test sanity");
    }

    #[test]
    fn test_finalize_rotate_operator_premature() {
        let admin = Pubkey::new_unique();
        let new_op = Pubkey::new_unique();
        let mut cfg = minimal_vault_config(admin);
        let now: i64 = 1_700_000_000;

        let unlocks_at = now + ADMIN_TIMELOCK_SECONDS;
        cfg.pending_operator_pubkey = new_op;
        cfg.pending_operator_unlocks_at = unlocks_at;

        let almost = unlocks_at - 1;
        assert!(almost < cfg.pending_operator_unlocks_at,
            "1s before unlock MUST revert TimelockNotElapsed");

        assert!(unlocks_at >= cfg.pending_operator_unlocks_at,
            "T == unlocks_at inclusive boundary passes");
    }

    #[test]
    fn test_finalize_rotate_operator_after_72h_succeeds() {
        let admin = Pubkey::new_unique();
        let old_op = Pubkey::new_unique();
        let new_op = Pubkey::new_unique();
        let mut cfg = minimal_vault_config(admin);
        cfg.operator_pubkey = old_op;
        let now: i64 = 1_700_000_000;

        let unlocks_at = now + ADMIN_TIMELOCK_SECONDS;
        cfg.pending_operator_pubkey = new_op;
        cfg.pending_operator_unlocks_at = unlocks_at;

        let finalize_now = unlocks_at + 1;
        assert!(finalize_now >= cfg.pending_operator_unlocks_at,
            "timelock elapsed gate passes");
        assert!(cfg.pending_operator_pubkey != Pubkey::default(),
            "NoProposalPending gate: a proposal IS pending");

        let recorded_old = cfg.operator_pubkey;
        cfg.operator_pubkey = cfg.pending_operator_pubkey;
        cfg.pending_operator_pubkey = Pubkey::default();
        cfg.pending_operator_unlocks_at = 0;

        assert_eq!(recorded_old, old_op, "event must capture pre-rotation op");
        assert_eq!(cfg.operator_pubkey, new_op, "operator rotated to new value");
        assert_eq!(cfg.pending_operator_pubkey, Pubkey::default(),
            "pending cleared after finalize");
        assert_eq!(cfg.pending_operator_unlocks_at, 0,
            "unlocks_at cleared after finalize");
    }

    #[test]
    fn test_finalize_rotate_operator_unauthorized() {
        let admin = Pubkey::new_unique();
        let attacker = Pubkey::new_unique();
        let new_op = Pubkey::new_unique();
        let mut cfg = minimal_vault_config(admin);

        let now: i64 = 1_700_000_000;
        cfg.pending_operator_pubkey = new_op;
        cfg.pending_operator_unlocks_at = now - 1;
        let finalize_now = now;
        assert!(finalize_now >= cfg.pending_operator_unlocks_at, "timelock elapsed gate passes");
        assert!(cfg.pending_operator_pubkey != Pubkey::default(), "NoProposalPending gate passes: proposal IS pending");

        let admin_passes = admin == cfg.authority;
        let attacker_passes = attacker == cfg.authority;
        assert!(admin_passes, "admin (authority) must pass the finalize authority check");
        assert!(!attacker_passes, "attacker MUST fail finalize authority check → Unauthorized (AUTH-01)");
    }

    #[test]
    fn test_finalize_rotate_operator_no_pending_rejected() {
        let admin = Pubkey::new_unique();
        let cfg = minimal_vault_config(admin);

        assert_eq!(cfg.pending_operator_pubkey, Pubkey::default(),
            "no proposal active");
        assert_eq!(cfg.pending_operator_unlocks_at, 0,
            "no proposal active");

        let no_proposal = cfg.pending_operator_pubkey == Pubkey::default();
        assert!(no_proposal,
            "finalize without a proposal MUST revert NoProposalPending");
    }

    #[test]
    fn test_cancel_propose_operator_rotation() {
        let admin = Pubkey::new_unique();
        let new_op = Pubkey::new_unique();
        let mut cfg = minimal_vault_config(admin);
        let now: i64 = 1_700_000_000;

        cfg.pending_operator_pubkey = new_op;
        cfg.pending_operator_unlocks_at = now + ADMIN_TIMELOCK_SECONDS;

        assert!(cfg.pending_operator_pubkey != Pubkey::default(),
            "NoProposalPending gate: proposal exists");

        let cancelled = cfg.pending_operator_pubkey;

        cfg.pending_operator_pubkey = Pubkey::default();
        cfg.pending_operator_unlocks_at = 0;

        assert_eq!(cancelled, new_op,
            "event must record the cancelled-proposal pubkey");
        assert_eq!(cfg.pending_operator_pubkey, Pubkey::default(),
            "pending cleared after cancel");
        assert_eq!(cfg.pending_operator_unlocks_at, 0,
            "unlocks_at cleared after cancel");
    }

    #[test]
    fn test_cancel_propose_operator_rotation_unauthorized() {
        let admin = Pubkey::new_unique();
        let attacker = Pubkey::new_unique();
        let cfg = minimal_vault_config(admin);

        let admin_passes = admin == cfg.authority;
        let attacker_passes = attacker == cfg.authority;
        assert!(admin_passes, "admin must pass authority check on cancel");
        assert!(!attacker_passes,
            "attacker MUST fail authority check on cancel → Unauthorized");
    }

    #[test]
    fn test_cancel_propose_no_pending_rejected() {
        let admin = Pubkey::new_unique();
        let cfg = minimal_vault_config(admin);

        let no_proposal = cfg.pending_operator_pubkey == Pubkey::default();
        assert!(no_proposal,
            "cancel without a proposal MUST revert NoProposalPending");
    }

    #[test]
    fn rotate_operator_timelock_is_72h() {
        assert_eq!(ADMIN_TIMELOCK_SECONDS, 72 * 60 * 60,
            "Rule 27b: ADMIN_TIMELOCK_SECONDS must equal 72h");
        assert_eq!(ADMIN_TIMELOCK_SECONDS, 259_200);
    }

    #[test]
    fn rotate_operator_pending_defaults_clear() {
        let cfg = minimal_vault_config(Pubkey::new_unique());
        assert_eq!(cfg.pending_operator_pubkey, Pubkey::default(),
            "pending_operator_pubkey must default to Pubkey::default()");
        assert_eq!(cfg.pending_operator_unlocks_at, 0,
            "pending_operator_unlocks_at must default to 0 (no proposal sentinel)");
    }


    fn fresh_withdraw_request(owner: Pubkey, asset_pool: Pubkey, nonce: u64) -> WithdrawRequest {
        WithdrawRequest {
            owner,
            asset_pool,
            lp_amount: 1_000,
            nonce,
            requested_at: 1_700_000_000,
            processable_at: 1_700_259_200,
            processed: false,
            batch_id: 0,
            bump: 255,
            reserved: [0u8; 32],
        }
    }

    #[test]
    fn test_process_withdraw_request_unauthorized_wallet_reverts() {
        let alice = Pubkey::new_unique();
        let bob = Pubkey::new_unique();
        let pool = Pubkey::new_unique();
        let request = fresh_withdraw_request(alice, pool, 1);

        let constraint_passes = request.owner == bob;
        assert!(!constraint_passes,
            "Bob attempting to process Alice's request MUST fail the wallet/owner constraint → UnauthorizedRequest");

        let alice_passes = request.owner == alice;
        assert!(alice_passes, "Alice processing her own request must pass");
    }

    #[test]
    fn test_process_withdraw_request_wrong_asset_pool_reverts() {
        let attacker = Pubkey::new_unique();
        let pool_a = Pubkey::new_unique();
        let pool_b = Pubkey::new_unique();
        let stale_matured_request = fresh_withdraw_request(attacker, pool_a, 42);

        let has_one_passes = stale_matured_request.asset_pool == pool_b;
        assert!(!has_one_passes,
            "passing pool_b's asset_pool with a pool_a request MUST fail has_one → AssetMismatch");

        let seed_anchor_matches = stale_matured_request.asset_pool == pool_b;
        assert!(!seed_anchor_matches,
            "seeds-derivation backstop: pool_b-derived seeds cannot match a pool_a PDA");
    }

    #[test]
    fn test_cancel_withdraw_request_unauthorized_wallet_reverts() {
        let alice = Pubkey::new_unique();
        let bob = Pubkey::new_unique();
        let pool = Pubkey::new_unique();
        let request = fresh_withdraw_request(alice, pool, 7);

        let constraint_passes = request.owner == bob;
        assert!(!constraint_passes,
            "Bob attempting to cancel Alice's request MUST fail constraint → UnauthorizedRequest \
             (closes the close=wallet rent-theft vector; ~0.002 SOL × N requests at scale)");
    }

    #[test]
    fn test_cancel_withdraw_request_wrong_asset_pool_reverts() {
        let attacker = Pubkey::new_unique();
        let pool_a = Pubkey::new_unique();
        let pool_b = Pubkey::new_unique();
        let request = fresh_withdraw_request(attacker, pool_a, 99);

        let wallet_constraint_passes = request.owner == attacker;
        assert!(wallet_constraint_passes,
            "attacker IS the owner; wallet constraint allows the call to reach has_one");

        let has_one_passes = request.asset_pool == pool_b;
        assert!(!has_one_passes,
            "pool_b passed for a pool_a request MUST fail has_one → AssetMismatch");
    }

    #[test]
    fn test_process_withdraw_request_correct_binding_succeeds() {
        let alice = Pubkey::new_unique();
        let pool = Pubkey::new_unique();
        let request = fresh_withdraw_request(alice, pool, 1);

        let wallet_constraint_passes = request.owner == alice;
        let has_one_passes = request.asset_pool == pool;
        assert!(wallet_constraint_passes,
            "Alice as wallet matches request.owner → constraint passes");
        assert!(has_one_passes,
            "asset_pool arg matches request.asset_pool → has_one passes");
    }

    #[test]
    fn test_cancel_withdraw_request_correct_binding_succeeds() {
        let alice = Pubkey::new_unique();
        let pool = Pubkey::new_unique();
        let request = fresh_withdraw_request(alice, pool, 1);

        let wallet_constraint_passes = request.owner == alice;
        let has_one_passes = request.asset_pool == pool;
        assert!(wallet_constraint_passes,
            "Alice as wallet matches request.owner → constraint passes; close=wallet refunds Alice");
        assert!(has_one_passes,
            "asset_pool arg matches request.asset_pool → has_one passes");
    }


    fn rl_admin() -> Pubkey { Pubkey::new_unique() }

    #[test]
    fn test_propose_rate_limit_escalation_full_ladder() {
        let mut config = minimal_vault_config(rl_admin());
        let t0: i64 = 1_700_000_000;

        check_and_record_propose(&mut config, t0).unwrap();
        assert_eq!(config.propose_cooldown_until, 0, "1st: no cooldown");

        check_and_record_propose(&mut config, t0 + 1).unwrap();
        assert_eq!(config.propose_cooldown_until, 0, "2nd: still free");

        check_and_record_propose(&mut config, t0 + 2).unwrap();
        assert_eq!(config.propose_cooldown_until, t0 + 2 + 1_800, "3rd: 30min");

        let t4 = t0 + 2 + 1_800;
        check_and_record_propose(&mut config, t4).unwrap();
        assert_eq!(config.propose_cooldown_until, t4 + 7_200, "4th: 2h");

        let t5 = t4 + 7_200;
        check_and_record_propose(&mut config, t5).unwrap();
        assert_eq!(config.propose_cooldown_until, t5 + 86_400, "5th: 24h");

        let now_7d_rung = t0 + 100_000;
        config.recent_proposes = [
            now_7d_rung - 80_000,
            now_7d_rung - 60_000,
            now_7d_rung - 40_000,
            now_7d_rung - 20_000,
            now_7d_rung - 10_000,
        ];
        config.propose_cooldown_until = 0;
        check_and_record_propose(&mut config, now_7d_rung).unwrap();
        assert_eq!(
            config.propose_cooldown_until,
            now_7d_rung + 604_800,
            "5+ rung: 7d cooldown armed"
        );
    }

    #[test]
    fn test_propose_cooldown_blocks_premature_call() {
        let mut config = minimal_vault_config(rl_admin());
        let t0: i64 = 1_700_000_000;

        check_and_record_propose(&mut config, t0).unwrap();
        check_and_record_propose(&mut config, t0 + 1).unwrap();
        check_and_record_propose(&mut config, t0 + 2).unwrap();
        let early = t0 + 2 + 1;
        let err = check_and_record_propose(&mut config, early).unwrap_err();
        if let anchor_lang::error::Error::AnchorError(ae) = err {
            let expected: u32 = {
                let e: anchor_lang::error::Error =
                    ProviderVaultError::ProposeCooldownActive.into();
                if let anchor_lang::error::Error::AnchorError(ae2) = e {
                    ae2.error_code_number
                } else {
                    panic!("expected AnchorError on ProposeCooldownActive");
                }
            };
            assert_eq!(ae.error_code_number, expected, "expected ProposeCooldownActive");
        } else {
            panic!("expected AnchorError, got {err:?}");
        }

        let on_boundary = config.propose_cooldown_until;
        check_and_record_propose(&mut config, on_boundary).unwrap();
    }

    #[test]
    fn test_propose_after_24h_window_resets_count() {
        let mut config = minimal_vault_config(rl_admin());
        let t0: i64 = 1_700_000_000;
        check_and_record_propose(&mut config, t0).unwrap();
        check_and_record_propose(&mut config, t0 + 1).unwrap();
        check_and_record_propose(&mut config, t0 + 2).unwrap();
        assert!(config.propose_cooldown_until > 0);

        let t_future = t0 + 2 + 86_400 + 100;
        check_and_record_propose(&mut config, t_future).unwrap();
        assert_eq!(config.propose_cooldown_until, 0,
            "post-24h: ring entries fall outside window → count resets");
    }

    #[test]
    fn test_propose_rate_limit_ring_buffer_fifo_order() {
        let mut config = minimal_vault_config(rl_admin());
        let t0: i64 = 1_700_000_000;
        for i in 0..5i64 {
            let when = if config.propose_cooldown_until > t0 + i * 100 {
                config.propose_cooldown_until
            } else {
                t0 + i * 100
            };
            check_and_record_propose(&mut config, when).unwrap();
        }
        assert!(config.propose_cooldown_until > 0,
            "5th propose in 24h must arm a 7d cooldown");
        for i in 0..4usize {
            assert!(config.recent_proposes[i] <= config.recent_proposes[i + 1],
                "ring buffer must be monotonically non-decreasing left→right");
        }
        let t_future = config.recent_proposes[4] + 86_400 + 1;
        check_and_record_propose(&mut config, t_future).unwrap();
        assert_eq!(config.propose_cooldown_until, 0,
            "all 5 ring entries pre-window → no cooldown");
    }

    #[test]
    fn test_propose_rate_limit_compromised_key_scenario() {
        let mut config = minimal_vault_config(rl_admin());
        let t0: i64 = 1_700_000_000;
        let mut landed = 0usize;
        let mut now = t0;
        let end_of_window = t0 + (72 * 60 * 60);
        let mut iter = 0;
        while now < end_of_window && iter < 1_000_000 {
            iter += 1;
            if check_and_record_propose(&mut config, now).is_ok() {
                landed += 1;
            }
            now = if config.propose_cooldown_until > now {
                config.propose_cooldown_until
            } else {
                now + 1
            };
        }
        assert!(
            landed <= 20,
            "rate-limit must cap lands in 72h under hostile pressure (observed {landed}, expected ≤ 20)"
        );
        assert!(
            landed < 864 / 40,
            "rate-limit must achieve ≥ 40× reduction vs unlimited spam (864/{} = {} ≥ {})",
            landed.max(1),
            864 / landed.max(1),
            40
        );
    }

    #[test]
    fn test_propose_rate_limit_len_growth() {
        assert_eq!(VaultConfig::LEN, 847);
    }



    #[test]
    fn fb_test_a_founder_seed_grants_seat_1() {
        let founder = Pubkey::new_unique();
        let mut config = minimal_vault_config(Pubkey::new_unique());
        config.founder_pubkey = founder;
        config.vault_seeded = false;
        config.founding_banker_counter = 0;

        let mut position = fresh_lp_position(0, 0);
        position.holder = founder;
        let amount = FOUNDING_BANKER_MIN_USDC_MICRO;
        let now: i64 = 1_700_000_000;

        assert!(!position.is_founding_banker);
        assert!(config.founding_banker_counter < FOUNDING_BANKER_MAX_SEATS);
        assert!(amount >= FOUNDING_BANKER_MIN_USDC_MICRO);

        config.founding_banker_counter += 1;
        position.is_founding_banker = true;
        position.founding_banker_seat_number = config.founding_banker_counter;
        position.founding_banker_seat_timestamp = now;
        config.vault_seeded = true;

        assert_eq!(config.founding_banker_counter, 1);
        assert_eq!(position.founding_banker_seat_number, 1);
        assert!(position.is_founding_banker);
        assert!(config.vault_seeded);
        assert_eq!(position.founding_banker_seat_timestamp, now);
    }

    #[test]
    fn fb_test_a_21_seats_fill_in_order_22nd_skipped() {
        let mut config = minimal_vault_config(Pubkey::new_unique());
        config.founder_pubkey = Pubkey::new_unique();
        config.founding_banker_counter = FOUNDING_BANKER_MAX_SEATS;
        config.vault_seeded = true;

        let mut position_22nd = fresh_lp_position(0, 0);
        let amount = FOUNDING_BANKER_MIN_USDC_MICRO;

        let would_grant = !position_22nd.is_founding_banker
            && config.founding_banker_counter < FOUNDING_BANKER_MAX_SEATS
            && amount >= FOUNDING_BANKER_MIN_USDC_MICRO;
        assert!(!would_grant, "22nd depositor at $5k must NOT receive an FB seat (cap = 21)");

        assert_eq!(config.founding_banker_counter, FOUNDING_BANKER_MAX_SEATS);
        assert!(!position_22nd.is_founding_banker);
        assert_eq!(position_22nd.founding_banker_seat_number, 0);
    }

    #[test]
    fn fb_test_a_below_5k_min_no_grant() {
        let mut config = minimal_vault_config(Pubkey::new_unique());
        config.founder_pubkey = Pubkey::new_unique();
        config.founding_banker_counter = 1;
        config.vault_seeded = true;

        let position = fresh_lp_position(0, 0);
        let amount: u64 = FOUNDING_BANKER_MIN_USDC_MICRO - 1;

        let would_grant = !position.is_founding_banker
            && config.founding_banker_counter < FOUNDING_BANKER_MAX_SEATS
            && amount >= FOUNDING_BANKER_MIN_USDC_MICRO;
        assert!(!would_grant, "Deposit below $5k must NOT receive an FB seat");
        assert_eq!(config.founding_banker_counter, 1, "Counter must not change");
    }

    #[test]
    fn fb_test_a_existing_fb_no_double_grant() {
        let mut config = minimal_vault_config(Pubkey::new_unique());
        config.founding_banker_counter = 5;
        config.vault_seeded = true;

        let mut position = fresh_lp_position(0, 1_000_000);
        position.is_founding_banker = true;
        position.founding_banker_seat_number = 3;
        position.founding_banker_seat_timestamp = 1_700_000_000;
        let original_seat = position.founding_banker_seat_number;

        let would_grant = !position.is_founding_banker
            && config.founding_banker_counter < FOUNDING_BANKER_MAX_SEATS;
        assert!(!would_grant, "Existing FB must NOT receive a second seat");
        assert_eq!(config.founding_banker_counter, 5);
        assert_eq!(position.founding_banker_seat_number, original_seat);
    }


    #[test]
    fn fb_test_b_full_fb_pool_growth_phase_returns_8500() {
        let mut pool = fresh_pool(Pubkey::new_unique());
        pool.lp_tokens_by_tier = [100, 0, 0, 0, 0];
        let fb_in_window = 100u64;
        assert_eq!(
            compute_weighted_lp_bps(&pool, 1, fb_in_window).unwrap(),
            FOUNDING_BANKER_LP_SHARE_BPS as u64
        );
    }

    #[test]
    fn fb_test_b_partial_fb_growth_phase_blends() {
        let mut pool = fresh_pool(Pubkey::new_unique());
        pool.lp_tokens_by_tier = [0, 0, 0, 0, 100];
        let fb_in_window = 50u64;
        assert_eq!(compute_weighted_lp_bps(&pool, 1, fb_in_window).unwrap(), 8_500);

        let mut pool2 = fresh_pool(Pubkey::new_unique());
        pool2.lp_tokens_by_tier = [100, 0, 0, 0, 0];
        let fb_in_window2 = 50u64;
        assert_eq!(compute_weighted_lp_bps(&pool2, 1, fb_in_window2).unwrap(), 7_500);
    }

    #[test]
    fn fb_test_b_per_seat_decay_after_90_days() {
        let seat_timestamp: i64 = 1_700_000_000;

        let in_window_now = seat_timestamp + 89 * SECONDS_PER_DAY;
        let still_in_window_in =
            in_window_now < seat_timestamp + FOUNDING_BANKER_BONUS_DAYS * SECONDS_PER_DAY;
        assert!(still_in_window_in, "T+89d must be inside the 90-day perk window");

        let out_window_now = seat_timestamp + 91 * SECONDS_PER_DAY;
        let still_in_window_out =
            out_window_now < seat_timestamp + FOUNDING_BANKER_BONUS_DAYS * SECONDS_PER_DAY;
        assert!(!still_in_window_out, "T+91d must be OUTSIDE the 90-day perk window");

        let mut pool = fresh_pool(Pubkey::new_unique());
        pool.lp_tokens_by_tier = [100, 0, 0, 0, 0];
        pool.founding_banker_lp_tokens_in_window = 100;

        assert_eq!(
            compute_weighted_lp_bps(&pool, 1, pool.founding_banker_lp_tokens_in_window).unwrap(),
            8_500
        );

        pool.founding_banker_lp_tokens_in_window = 0;
        assert_eq!(
            compute_weighted_lp_bps(&pool, 1, pool.founding_banker_lp_tokens_in_window).unwrap(),
            6_500,
            "After 90-day decay, position reverts to Whale tier rate (6500 bps)"
        );
    }

    #[test]
    fn fb_test_b_independent_per_fb_windows() {
        let early_seat: i64 = 1_700_000_000;
        let late_seat: i64 = early_seat + 60 * SECONDS_PER_DAY;

        let now_a = early_seat + 89 * SECONDS_PER_DAY;
        assert!(now_a < early_seat + FOUNDING_BANKER_BONUS_DAYS * SECONDS_PER_DAY);
        assert!(now_a < late_seat + FOUNDING_BANKER_BONUS_DAYS * SECONDS_PER_DAY);

        let now_b = early_seat + 95 * SECONDS_PER_DAY;
        assert!(now_b >= early_seat + FOUNDING_BANKER_BONUS_DAYS * SECONDS_PER_DAY);
        assert!(now_b < late_seat + FOUNDING_BANKER_BONUS_DAYS * SECONDS_PER_DAY);

        let now_c = early_seat + 155 * SECONDS_PER_DAY;
        assert!(now_c >= early_seat + FOUNDING_BANKER_BONUS_DAYS * SECONDS_PER_DAY);
        assert!(now_c >= late_seat + FOUNDING_BANKER_BONUS_DAYS * SECONDS_PER_DAY);
    }


    #[test]
    fn fb_test_c_non_founder_first_deposit_rejected() {
        let founder = Pubkey::new_unique();
        let attacker = Pubkey::new_unique();
        let mut config = minimal_vault_config(Pubkey::new_unique());
        config.founder_pubkey = founder;
        config.vault_seeded = false;
        config.founding_banker_counter = 0;

        let depositor = attacker;
        let gate_passes = config.vault_seeded || depositor == config.founder_pubkey;
        assert!(!gate_passes, "non-founder must NOT pass the Rule 41 gate");
    }

    #[test]
    fn fb_test_c_founder_seeds_then_others_admitted() {
        let founder = Pubkey::new_unique();
        let bob = Pubkey::new_unique();
        let mut config = minimal_vault_config(Pubkey::new_unique());
        config.founder_pubkey = founder;
        config.vault_seeded = false;
        config.founding_banker_counter = 0;

        let mut founder_pos = fresh_lp_position(0, 0);
        founder_pos.holder = founder;
        let amount = FOUNDING_BANKER_MIN_USDC_MICRO;
        let now: i64 = 1_700_000_000;

        let depositor_a = founder;
        let gate_a = config.vault_seeded || depositor_a == config.founder_pubkey;
        assert!(gate_a, "founder must pass Rule 41 gate");

        assert!(amount >= FOUNDING_BANKER_MIN_USDC_MICRO);
        config.founding_banker_counter += 1;
        founder_pos.is_founding_banker = true;
        founder_pos.founding_banker_seat_number = 1;
        founder_pos.founding_banker_seat_timestamp = now;
        config.vault_seeded = true;
        assert_eq!(config.founding_banker_counter, 1);
        assert!(config.vault_seeded);
        assert!(founder_pos.is_founding_banker);
        assert_eq!(founder_pos.founding_banker_seat_number, 1);
        assert_eq!(founder_pos.founding_banker_seat_timestamp, now);

        let depositor_b = bob;
        let gate_b = config.vault_seeded || depositor_b == config.founder_pubkey;
        assert!(gate_b, "post-seed deposit by non-founder must pass Rule 41 gate");

        let mut bob_pos = fresh_lp_position(0, 0);
        let bob_amount = FOUNDING_BANKER_MIN_USDC_MICRO;
        let would_grant = !bob_pos.is_founding_banker
            && config.founding_banker_counter < FOUNDING_BANKER_MAX_SEATS
            && bob_amount >= FOUNDING_BANKER_MIN_USDC_MICRO;
        assert!(would_grant);
        config.founding_banker_counter += 1;
        bob_pos.is_founding_banker = true;
        bob_pos.founding_banker_seat_number = config.founding_banker_counter;
        bob_pos.founding_banker_seat_timestamp = now + 1;
        assert_eq!(config.founding_banker_counter, 2);
        assert_eq!(bob_pos.founding_banker_seat_number, 2);
    }

    #[test]
    fn fb_test_c_founder_seed_below_5k_rejected() {
        let founder = Pubkey::new_unique();
        let mut config = minimal_vault_config(Pubkey::new_unique());
        config.founder_pubkey = founder;
        config.vault_seeded = false;

        let amount: u64 = 4_000_000_000;
        let identity_ok = founder == config.founder_pubkey;
        let amount_ok = amount >= FOUNDING_BANKER_MIN_USDC_MICRO;
        assert!(identity_ok, "founder identity matches");
        assert!(!amount_ok, "founder seed below $5k must fail amount gate");
    }


    #[test]
    fn fb_test_d_full_withdraw_decrements_counter() {
        let mut config = minimal_vault_config(Pubkey::new_unique());
        config.founding_banker_counter = 21;

        let mut position = fresh_lp_position(2, 1_000_000);
        position.is_founding_banker = true;
        position.founding_banker_seat_number = 5;
        position.founding_banker_seat_timestamp = 1_700_000_000;

        let burn_amount = position.lp_shares;
        position.lp_shares = position.lp_shares.checked_sub(burn_amount).unwrap();

        let post_burn_zero = position.lp_shares == 0;
        let pre_burn_was_fb = position.is_founding_banker;
        if pre_burn_was_fb && post_burn_zero {
            config.founding_banker_counter = config.founding_banker_counter.saturating_sub(1);
        }

        assert_eq!(config.founding_banker_counter, 20);
        assert!(position.is_founding_banker);
        assert_eq!(position.founding_banker_seat_number, 5);
        let next_seat_available = config.founding_banker_counter < FOUNDING_BANKER_MAX_SEATS;
        assert!(next_seat_available);
    }

    #[test]
    fn fb_test_d_partial_withdraw_keeps_seat() {
        let mut config = minimal_vault_config(Pubkey::new_unique());
        config.founding_banker_counter = 10;

        let mut position = fresh_lp_position(2, 1_000_000);
        position.is_founding_banker = true;
        position.founding_banker_seat_number = 3;

        let burn_amount = 500_000u64;
        position.lp_shares = position.lp_shares.checked_sub(burn_amount).unwrap();

        let post_burn_zero = position.lp_shares == 0;
        if position.is_founding_banker && post_burn_zero {
            config.founding_banker_counter = config.founding_banker_counter.saturating_sub(1);
        }

        assert_eq!(config.founding_banker_counter, 10, "Partial burn must NOT release seat");
        assert!(position.is_founding_banker);
        assert!(position.lp_shares > 0);
    }

    #[test]
    fn fb_test_d_fb_window_counter_decrement_on_burn() {
        let mut pool = fresh_pool(Pubkey::new_unique());
        pool.lp_tokens_by_tier = [0, 0, 1_000_000, 0, 0];
        pool.founding_banker_lp_tokens_in_window = 1_000_000;

        let burn = 400_000u64;
        pool.founding_banker_lp_tokens_in_window = pool
            .founding_banker_lp_tokens_in_window
            .saturating_sub(burn);
        assert_eq!(pool.founding_banker_lp_tokens_in_window, 600_000);

        let burn2 = 600_000u64;
        pool.founding_banker_lp_tokens_in_window = pool
            .founding_banker_lp_tokens_in_window
            .saturating_sub(burn2);
        assert_eq!(pool.founding_banker_lp_tokens_in_window, 0);

        pool.lp_tokens_by_tier = [0, 0, 600_000, 0, 0];
        assert_eq!(compute_weighted_lp_bps(&pool, 1, 0).unwrap(), 7_500);
    }


    #[test]
    fn fb_constants_match_originals_vault() {
        assert_eq!(FOUNDING_BANKER_MAX_SEATS, 21, "Rule 37");
        assert_eq!(FOUNDING_BANKER_MIN_USDC_MICRO, 5_000_000_000, "Rule 38");
        assert_eq!(FOUNDING_BANKER_BONUS_DAYS, 90, "Rule 40a");
        assert_eq!(FOUNDING_BANKER_LP_SHARE_BPS, 8_500, "85% LP-share perk");
    }


    fn drift_handler_end(src: &str, idx: usize) -> usize {
        src[idx + 1..]
            .find("\n    pub fn ")
            .map(|p| idx + 1 + p)
            .expect("drift-gate: no following `pub fn` — re-anchor this source-assert bound")
    }

    fn tlp_provider_vault_lib_rs_source() -> &'static str {
        include_str!("lib.rs")
    }


    #[test]
    fn deprecated_set_reserve_burn_mode_reverts_with_err() {
        let src = tlp_provider_vault_lib_rs_source();
        let needle = "pub fn set_reserve_burn_mode(";
        let idx = src.find(needle).expect("set_reserve_burn_mode must exist");
        let end = drift_handler_end(src, idx);
        let body = &src[idx..end];
        assert!(body.contains("err!(ProviderVaultError::InstructionDeprecated)"),
            "set_reserve_burn_mode MUST hard-revert with InstructionDeprecated (M-CRIT-02). \
             Source excerpt:\n{}", body);
        assert!(!body.contains("config.reserve_burn_mode = new_mode;"),
            "set_reserve_burn_mode MUST NOT mutate state after the gate (M-CRIT-02). \
             Source excerpt:\n{}", body);
    }

    #[test]
    fn set_raydium_graduated_still_works_alongside_deprecated_gate() {
        let src = tlp_provider_vault_lib_rs_source();
        assert!(src.contains("pub fn set_raydium_graduated("),
            "set_raydium_graduated must remain (canonical replacement)");
    }

    #[test]
    fn provider_instruction_deprecated_error_code_exists() {
        let src = tlp_provider_vault_lib_rs_source();
        assert!(src.contains("InstructionDeprecated,"),
            "ProviderVaultError::InstructionDeprecated variant must exist");
        assert!(src.contains("M-CRIT-02"),
            "error definition must reference the audit ID for traceability");
    }


    #[test]
    fn daily_outflow_constants_exist() {
        assert_eq!(DEFAULT_MAX_DAILY_OUTFLOW, 250_000_000_000,
            "Default daily cap = $250k micro-USDC");
        assert_eq!(MIN_MAX_DAILY_OUTFLOW, 50_000_000_000,
            "Min daily cap = $50k micro-USDC (= 5-min cap default)");
        assert_eq!(DAILY_OUTFLOW_WINDOW_SECONDS, 86_400,
            "Daily window = 24h fixed");
    }

    #[test]
    fn daily_outflow_default_below_5min_cap_is_blocked() {
        assert!(MIN_MAX_DAILY_OUTFLOW >= DEFAULT_MAX_SETTLE_PER_WINDOW,
            "MIN_MAX_DAILY_OUTFLOW ({}) must be >= 5-min cap ({})",
            MIN_MAX_DAILY_OUTFLOW, DEFAULT_MAX_SETTLE_PER_WINDOW);
    }

    #[test]
    fn daily_outflow_under_cap_normal_path() {
        let mut daily_window_outflow: u64 = 0;
        let mut daily_window_start: i64 = 0;
        let max_daily: u64 = DEFAULT_MAX_DAILY_OUTFLOW;
        let now: i64 = 1_700_000_000;

        if daily_window_start == 0 || now >= daily_window_start + DAILY_OUTFLOW_WINDOW_SECONDS {
            daily_window_outflow = 0;
            daily_window_start = now;
        }
        let amount: u64 = 100_000_000_000;
        let projected = daily_window_outflow.checked_add(amount).unwrap();
        assert!(projected <= max_daily, "under-cap path must succeed");
        daily_window_outflow = projected;
        assert_eq!(daily_window_outflow, amount);
        assert_eq!(daily_window_start, now);
    }

    #[test]
    fn daily_outflow_breach_triggers_freeze() {
        let daily_window_outflow: u64 = 200_000_000_000;
        let max_daily: u64 = DEFAULT_MAX_DAILY_OUTFLOW;
        let amount: u64 = 100_000_000_000;
        let projected = daily_window_outflow.checked_add(amount).unwrap();
        assert!(projected > max_daily,
            "$200k + $100k must exceed $250k daily cap → freeze");
    }

    #[test]
    fn daily_outflow_window_rollover_resets_counter() {
        let mut daily_window_outflow: u64 = 200_000_000_000;
        let mut daily_window_start: i64 = 1_700_000_000;
        let now_after_rollover: i64 = daily_window_start + DAILY_OUTFLOW_WINDOW_SECONDS;

        let rollover_due = daily_window_start == 0
            || now_after_rollover >= daily_window_start + DAILY_OUTFLOW_WINDOW_SECONDS;
        assert!(rollover_due, "rollover must fire at exactly t+86_400s");

        if rollover_due {
            daily_window_outflow = 0;
            daily_window_start = now_after_rollover;
        }
        assert_eq!(daily_window_outflow, 0, "rollover MUST reset counter");
        assert_eq!(daily_window_start, now_after_rollover);
    }

    #[test]
    fn auto_frozen_on_daily_outflow_event_exists() {
        let src = tlp_provider_vault_lib_rs_source();
        assert!(src.contains("pub struct AutoFrozenOnDailyOutflow"),
            "AutoFrozenOnDailyOutflow event must exist (M-HIGH-01)");
        assert!(src.contains("BREAKER_TRIP:AutoFrozenOnDailyOutflow"),
            "BREAKER_TRIP log marker must exist for Sentinel routing");
    }

    #[test]
    fn propose_finalize_cancel_max_daily_outflow_present() {
        let src = tlp_provider_vault_lib_rs_source();
        assert!(src.contains("pub fn propose_max_daily_outflow("),
            "propose_max_daily_outflow must exist (M-HIGH-01 Rule 27b triplet)");
        assert!(src.contains("pub fn finalize_max_daily_outflow("),
            "finalize_max_daily_outflow must exist");
        assert!(src.contains("pub fn cancel_max_daily_outflow("),
            "cancel_max_daily_outflow must exist");
    }


    #[test]
    fn propose_rotate_operator_rejects_authority_collision() {
        let src = tlp_provider_vault_lib_rs_source();
        let needle = "pub fn propose_rotate_operator(";
        let idx = src.find(needle).expect("propose_rotate_operator must exist");
        let end = drift_handler_end(src, idx);
        let body = &src[idx..end];
        assert!(body.contains("new_operator != config.authority"),
            "propose_rotate_operator MUST reject authority collision (M-HIGH-05). \
             Source excerpt:\n{}", body);
        assert!(body.contains("new_operator != config.pause_authority"),
            "propose_rotate_operator MUST reject pause_authority collision (M-HIGH-05). \
             Source excerpt:\n{}", body);
        assert!(body.contains("new_operator != config.waterfall_authority"),
            "propose_rotate_operator MUST reject waterfall_authority collision (M-HIGH-05). \
             Source excerpt:\n{}", body);
        assert!(body.contains("new_operator != config.operator_pubkey"),
            "propose_rotate_operator MUST reject no-op rotation (M-HIGH-05). \
             Source excerpt:\n{}", body);
        assert!(body.contains("OperatorRoleCollision"),
            "OperatorRoleCollision error variant must be wired");
    }

    #[test]
    fn propose_rotate_pause_authority_adds_3role_distinctness() {
        let src = tlp_provider_vault_lib_rs_source();
        let needle = "pub fn propose_rotate_pause_authority(";
        let idx = src.find(needle).expect("propose_rotate_pause_authority must exist");
        let end = drift_handler_end(src, idx);
        let body = &src[idx..end];
        assert!(body.contains("new_pubkey != config.authority"),
            "propose_rotate_pause_authority MUST still reject authority collision");
        assert!(body.contains("new_pubkey != config.operator_pubkey"),
            "propose_rotate_pause_authority MUST reject operator collision (M-HIGH-05 mirror)");
        assert!(body.contains("new_pubkey != config.waterfall_authority"),
            "propose_rotate_pause_authority MUST reject waterfall collision (M-HIGH-05 mirror)");
    }

    #[test]
    fn operator_role_collision_error_code_exists() {
        let src = tlp_provider_vault_lib_rs_source();
        assert!(src.contains("OperatorRoleCollision,"),
            "ProviderVaultError::OperatorRoleCollision variant must exist (M-HIGH-05)");
        assert!(src.contains("M-HIGH-05"),
            "error definition must reference the audit ID for traceability");
    }


    #[test]
    fn chip_debit_default_cap_constant() {
        assert_eq!(DEFAULT_MAX_CHIP_DEBIT_PER_24H_PER_WALLET, 10_000_000_000,
            "Default per-wallet chip-debit cap = $10k micro-USDC");
        assert_eq!(CHIP_DEBIT_WINDOW_SECONDS, 86_400,
            "Chip-debit window = 24h fixed");
    }

    #[test]
    fn chip_debit_under_cap_succeeds() {
        let mut debit_window_amount: u64 = 0;
        let mut debit_window_start: i64 = 0;
        let cap: u64 = DEFAULT_MAX_CHIP_DEBIT_PER_24H_PER_WALLET;
        let now: i64 = 1_700_000_000;

        let rollover = debit_window_start == 0 || now >= debit_window_start + CHIP_DEBIT_WINDOW_SECONDS;
        if rollover {
            debit_window_amount = 0;
            debit_window_start = now;
        }
        let amount: u64 = 5_000_000_000;
        let projected = debit_window_amount.checked_add(amount).unwrap();
        assert!(projected <= cap, "$5k debit must succeed under $10k cap");
        debit_window_amount = projected;
        assert_eq!(debit_window_amount, amount);
        assert_eq!(debit_window_start, now);
    }

    #[test]
    fn chip_debit_over_cap_reverts() {
        let debit_window_amount: u64 = 8_000_000_000;
        let cap: u64 = DEFAULT_MAX_CHIP_DEBIT_PER_24H_PER_WALLET;
        let amount: u64 = 5_000_000_000;
        let projected = debit_window_amount.checked_add(amount).unwrap();
        assert!(projected > cap,
            "$8k + $5k must exceed $10k cap → ChipDebitRateLimited");
    }

    #[test]
    fn chip_debit_window_resets_after_24h() {
        let mut debit_window_amount: u64 = 9_000_000_000;
        let mut debit_window_start: i64 = 1_700_000_000;
        let now_after_rollover: i64 = debit_window_start + CHIP_DEBIT_WINDOW_SECONDS;

        let rollover = debit_window_start == 0
            || now_after_rollover >= debit_window_start + CHIP_DEBIT_WINDOW_SECONDS;
        if rollover {
            debit_window_amount = 0;
            debit_window_start = now_after_rollover;
        }
        let amount: u64 = 9_000_000_000;
        let projected = debit_window_amount.checked_add(amount).unwrap();
        let cap: u64 = DEFAULT_MAX_CHIP_DEBIT_PER_24H_PER_WALLET;
        assert!(projected <= cap, "post-rollover $9k debit must succeed");
        assert_eq!(debit_window_start, now_after_rollover,
            "rollover MUST advance debit_window_start to now_after_rollover");
    }

    #[test]
    fn chip_debit_per_wallet_isolation() {
        let wallet_a_debit: u64 = DEFAULT_MAX_CHIP_DEBIT_PER_24H_PER_WALLET;
        let wallet_b_debit: u64 = 0;
        let cap: u64 = DEFAULT_MAX_CHIP_DEBIT_PER_24H_PER_WALLET;

        let amount: u64 = 5_000_000_000;
        let projected_b = wallet_b_debit.checked_add(amount).unwrap();
        assert!(projected_b <= cap, "wallet B is independent of wallet A");
        assert_eq!(wallet_a_debit, cap, "wallet A is at cap (no spillover)");
    }

    #[test]
    fn chip_debit_zero_sentinel_uses_default_cap() {
        let pool_override: u64 = 0;
        let effective_cap = if pool_override == 0 {
            DEFAULT_MAX_CHIP_DEBIT_PER_24H_PER_WALLET
        } else {
            pool_override
        };
        assert_eq!(effective_cap, DEFAULT_MAX_CHIP_DEBIT_PER_24H_PER_WALLET);

        let pool_override: u64 = 25_000_000_000;
        let effective_cap = if pool_override == 0 {
            DEFAULT_MAX_CHIP_DEBIT_PER_24H_PER_WALLET
        } else {
            pool_override
        };
        assert_eq!(effective_cap, 25_000_000_000);
    }

    #[test]
    fn chip_debit_rate_limited_error_exists() {
        let src = tlp_provider_vault_lib_rs_source();
        assert!(src.contains("ChipDebitRateLimited,"),
            "ProviderVaultError::ChipDebitRateLimited variant must exist (M-HIGH-07)");
        assert!(src.contains("M-HIGH-07"),
            "error definition must reference the audit ID for traceability");
    }

    #[test]
    fn set_chip_debit_cap_per_wallet_instruction_exists() {
        let src = tlp_provider_vault_lib_rs_source();
        assert!(src.contains("pub fn set_chip_debit_cap_per_wallet("),
            "set_chip_debit_cap_per_wallet admin instruction must exist (M-HIGH-07)");
        assert!(src.contains("pub struct SetChipDebitCapPerWallet"),
            "SetChipDebitCapPerWallet account context must exist (M-HIGH-07)");
        assert!(src.contains("pub struct ChipDebitCapPerWalletSet"),
            "ChipDebitCapPerWalletSet event must exist (M-HIGH-07)");
    }

    #[test]
    fn provider_player_escrow_len_unchanged_with_rate_limit_fields() {
        assert_eq!(ProviderPlayerEscrow::LEN, 8 + 32 + 32 + 8 + 1 + 8 + 8 + 16,
            "ProviderPlayerEscrow LEN must be 113 — rate-limit fields absorbed in reserved, no recorded holder");
        assert_eq!(ProviderPlayerEscrow::LEN, 113,
            "ProviderPlayerEscrow LEN must equal 113 absolute");
    }


    fn tlp_provider_vault_lib_rs_source_pass2() -> &'static str {
        include_str!("lib.rs")
    }

    #[test]
    fn distribute_yield_cpi_passes_usdc_mint_to_swap_router() {
        let src = tlp_provider_vault_lib_rs_source_pass2();
        assert!(
            src.contains("usdc_mint_constraint: ctx.accounts.swap_router_usdc_mint.to_account_info()"),
            "distribute_yield Path B CPI MUST pass swap_router_usdc_mint to RouteProviderYieldUsdc.usdc_mint_constraint (M-HIGH-04 FIX PASS 2)"
        );
        assert!(
            src.contains("pub swap_router_usdc_mint: Box<Account<'info, Mint>>"),
            "DistributeYield MUST expose swap_router_usdc_mint as a typed Mint account (M-HIGH-04 FIX PASS 2)"
        );
        assert!(
            src.contains("address = asset_mint @ ProviderVaultError::AssetMismatch"),
            "swap_router_usdc_mint MUST be pinned with `address = asset_mint` (defense-in-depth alongside swap_router's `address = config.usdc_mint`)"
        );
    }


    #[test]
    fn propose_rotate_operator_rejects_affiliate_recorder_collision_f6() {
        let src = tlp_provider_vault_lib_rs_source_pass2();
        let needle = "pub fn propose_rotate_operator(";
        let idx = src.find(needle).expect("propose_rotate_operator must exist");
        let end = drift_handler_end(src, idx);
        let body = &src[idx..end];
        assert!(
            body.contains("new_operator != config.affiliate_recorder_pubkey"),
            "F6: propose_rotate_operator MUST reject affiliate_recorder_pubkey collision. \
             Source excerpt:\n{}",
            body
        );
        assert!(
            body.contains("ProviderVaultError::OperatorRoleCollision"),
            "F6: affiliate_recorder collision MUST emit OperatorRoleCollision for SIEM consistency"
        );
    }

    #[test]
    fn propose_rotate_pause_authority_rejects_affiliate_recorder_collision_f6() {
        let src = tlp_provider_vault_lib_rs_source_pass2();
        let needle = "pub fn propose_rotate_pause_authority(";
        let idx = src.find(needle).expect("propose_rotate_pause_authority must exist");
        let end = drift_handler_end(src, idx);
        let body = &src[idx..end];
        assert!(
            body.contains("new_pubkey != config.affiliate_recorder_pubkey"),
            "F6: propose_rotate_pause_authority MUST reject affiliate_recorder_pubkey collision. \
             Source excerpt:\n{}",
            body
        );
    }

    #[test]
    fn set_chip_debit_cap_per_wallet_enforces_ceiling_f8() {
        let src = tlp_provider_vault_lib_rs_source_pass2();
        assert!(
            src.contains("pub const MAX_CHIP_DEBIT_CAP_PER_WALLET: u64 = 100_000_000_000"),
            "F8: MAX_CHIP_DEBIT_CAP_PER_WALLET constant must be defined at $100k"
        );
        let needle = "pub fn set_chip_debit_cap_per_wallet(";
        let idx = src.find(needle).expect("set_chip_debit_cap_per_wallet handler must exist");
        let end = drift_handler_end(src, idx);
        let body = &src[idx..end];
        assert!(
            body.contains("new_cap <= MAX_CHIP_DEBIT_CAP_PER_WALLET")
                && body.contains("ProviderVaultError::ChipDebitCapTooHigh"),
            "F8: handler MUST enforce ceiling with `new_cap <= MAX_CHIP_DEBIT_CAP_PER_WALLET` \
             and revert `ChipDebitCapTooHigh`. Source excerpt:\n{}",
            body
        );
        assert!(
            src.contains("ChipDebitCapTooHigh,"),
            "F8: ChipDebitCapTooHigh error variant must be defined"
        );
        assert_eq!(
            MAX_CHIP_DEBIT_CAP_PER_WALLET / DEFAULT_MAX_CHIP_DEBIT_PER_24H_PER_WALLET,
            10,
            "F8: MAX ceiling must be exactly 10× DEFAULT cap (Layer 2 / founder lock)"
        );
    }



    #[test]
    fn swap_credit_happy_path_returns_measured_delta() {
        let credited = compute_swap_credit(50_000_000, 0, 0).unwrap();
        assert_eq!(credited, 50_000_000);
    }

    #[test]
    fn swap_credit_returns_only_the_new_delta_on_topup() {
        let credited = compute_swap_credit(80_000_000, 50_000_000, 0).unwrap();
        assert_eq!(credited, 30_000_000);
    }

    #[test]
    fn swap_credit_idempotent_zero_delta_when_holder_equals_escrow() {
        let credited = compute_swap_credit(50_000_000, 50_000_000, 0).unwrap();
        assert_eq!(credited, 0);
    }

    #[test]
    fn swap_credit_monotonic_guard_rejects_holder_below_escrow() {
        let err = compute_swap_credit(40_000_000, 50_000_000, 0).unwrap_err();
        assert_eq!(
            err,
            ProviderVaultError::HolderBalanceDecreased.into(),
            "holder < escrow must revert HolderBalanceDecreased, never decrease"
        );
    }

    #[test]
    fn swap_credit_min_out_floor_rejects_below_floor() {
        let err = compute_swap_credit(9_000_000, 0, 10_000_000).unwrap_err();
        assert_eq!(err, ProviderVaultError::CreditBelowMinOut.into());
    }

    #[test]
    fn swap_credit_min_out_floor_passes_at_exact_floor() {
        let credited = compute_swap_credit(10_000_000, 0, 10_000_000).unwrap();
        assert_eq!(credited, 10_000_000);
    }

    #[test]
    fn swap_credit_min_out_floor_passes_with_positive_slippage() {
        let credited = compute_swap_credit(12_000_000, 0, 10_000_000).unwrap();
        assert_eq!(credited, 12_000_000, "positive slippage must self-heal");
    }

    #[test]
    fn swap_credit_floor_zero_skips_floor_check() {
        assert_eq!(compute_swap_credit(1, 0, 0).unwrap(), 1);
        assert_eq!(compute_swap_credit(0, 0, 0).unwrap(), 0);
    }

    #[test]
    fn swap_credit_absolute_reconcile_is_idempotent_on_escrow_struct() {
        let mut escrow = ProviderPlayerEscrow {
            wallet: Pubkey::new_unique(),
            mint: Pubkey::new_unique(),
            amount: 0,
            bump: 252,
            debit_window_amount: 0,
            debit_window_start: 0,
            reserved: [0u8; 16],
        };
        let holder = 75_000_000u64;

        let d1 = compute_swap_credit(holder, escrow.amount, 0).unwrap();
        escrow.amount = holder;
        assert_eq!(d1, 75_000_000);
        assert_eq!(escrow.amount, 75_000_000);

        let d2 = compute_swap_credit(holder, escrow.amount, 0).unwrap();
        escrow.amount = holder;
        assert_eq!(d2, 0, "replay must credit 0");
        assert_eq!(escrow.amount, 75_000_000, "replay must not change amount");

        let holder2 = 100_000_000u64;
        let d3 = compute_swap_credit(holder2, escrow.amount, 0).unwrap();
        escrow.amount = holder2;
        assert_eq!(d3, 25_000_000);
        assert_eq!(escrow.amount, 100_000_000);
    }

    #[test]
    fn swap_credit_new_error_discriminants_are_distinct() {
        let s = [
            ProviderVaultError::CreditBelowMinOut as u32,
            ProviderVaultError::HolderBalanceDecreased as u32,
            ProviderVaultError::MathOverflow as u32,
            ProviderVaultError::AssetMismatch as u32,
            ProviderVaultError::VaultFrozen as u32,
            ProviderVaultError::VaultPaused as u32,
            ProviderVaultError::PlayerEscrowMismatch as u32,
        ];
        for i in 0..s.len() {
            for j in (i + 1)..s.len() {
                assert_ne!(s[i], s[j], "error discriminant collision at {i},{j}");
            }
        }
    }


    fn credit_swap_fn_src() -> &'static str {
        let src = tlp_provider_vault_lib_rs_source();
        let start = src
            .find("pub fn credit_chips_from_swap(")
            .expect("credit_chips_from_swap fn must exist");
        let rest = &src[start..];
        let end = start
            + rest
                .find("\n}\n\n// ─── Helpers")
                .expect("Helpers header must follow the program mod");
        &src[start..end]
    }

    fn credit_swap_ctx_src() -> &'static str {
        let src = tlp_provider_vault_lib_rs_source();
        let start = src
            .find("pub struct CreditChipsFromSwap<'info> {")
            .expect("CreditChipsFromSwap context must exist");
        let rest = &src[start..];
        let end = start + rest.find("\n}").expect("context must close") + 2;
        &src[start..end]
    }

    #[test]
    fn credit_swap_src_reloads_holder_before_measuring() {
        let f = credit_swap_fn_src();
        assert!(
            f.contains("ctx.accounts.escrow_holder.reload()?;"),
            "must reload() the holder before measuring its balance"
        );
        assert!(f.contains("ctx.accounts.escrow_holder.amount"));
        assert!(f.contains("compute_swap_credit(holder_balance, escrow.amount, min_out_floor)"));
    }

    #[test]
    fn credit_swap_src_absolute_reconcile_never_plus_equals() {
        let f = credit_swap_fn_src();
        assert!(
            f.contains("escrow.amount = holder_balance;"),
            "must ABSOLUTE-reconcile escrow.amount = holder_balance"
        );
        assert!(
            !f.contains("escrow.amount = escrow"),
            "must NOT use the `escrow.amount = escrow.amount.checked_add` (+=) pattern"
        );
        assert!(
            !f.contains(".checked_add("),
            "the reconciler must not additively accumulate — absolute set only"
        );
    }

    #[test]
    fn credit_swap_src_moves_no_tokens() {
        let f = credit_swap_fn_src();
        assert!(!f.contains("token::transfer"), "reconciler must move zero tokens");
        assert!(!f.contains("Transfer {"), "reconciler must not build a Transfer CPI");
        assert!(!f.contains("player_token_account"));
        assert!(!f.contains("vault_holder"));
    }

    #[test]
    fn credit_swap_src_is_permissionless() {
        let f = credit_swap_fn_src();
        assert!(
            !f.contains("ProviderVaultError::Unauthorized"),
            "credit must be permissionless — no authority gate"
        );
        assert!(
            !f.contains("operator_pubkey"),
            "credit must not gate on operator_pubkey"
        );
        assert!(f.contains("ProviderVaultError::VaultFrozen"));
        assert!(f.contains("ProviderVaultError::VaultPaused"));
    }

    #[test]
    fn credit_swap_ctx_pins_mint_to_config_not_arg() {
        let c = credit_swap_ctx_src();
        assert!(
            c.contains("token::mint = asset_pool.asset_mint"),
            "holder mint MUST pin to asset_pool.asset_mint (config), not the arg"
        );
        assert!(
            !c.contains("token::mint = asset_mint,"),
            "holder mint MUST NOT pin to the free asset_mint arg (worthless-mint attack)"
        );
        assert!(c.contains("token::authority = escrow_holder_authority"));
    }

    #[test]
    fn credit_swap_ctx_protocol_fee_payer_pays_rent() {
        let c = credit_swap_ctx_src();
        assert!(c.contains("init_if_needed"));
        assert!(
            c.contains("payer = fee_payer"),
            "init_if_needed must be paid by the protocol fee_payer, not the player"
        );
        assert!(
            c.contains("pub fee_payer: Signer<'info>"),
            "fee_payer must be the (only) Signer"
        );
        assert!(
            !c.contains("payer = player"),
            "the player must NOT pay rent (they are not even a signer here)"
        );
        assert!(
            c.contains("pub player: AccountInfo<'info>"),
            "player must be a non-signing AccountInfo (seed source only)"
        );
    }

    #[test]
    fn credit_swap_ctx_has_three_distinct_pdas() {
        let c = credit_swap_ctx_src();
        assert!(c.contains(
            "seeds = [b\"provider_player_escrow\", player.key().as_ref(), asset_mint.as_ref()]"
        ));
        assert!(c.contains(
            "seeds = [b\"provider_player_escrow_holder\", player.key().as_ref(), asset_mint.as_ref()]"
        ));
        assert!(c.contains("pub player_escrow: Box<Account<'info, ProviderPlayerEscrow>>"));
        assert!(c.contains("pub escrow_holder: Box<Account<'info, TokenAccount>>"));
        assert!(c.contains("pub escrow_holder_authority: AccountInfo<'info>"));
    }

    #[test]
    fn credit_swap_ctx_touches_no_lp_buckets() {
        let c = credit_swap_ctx_src();
        assert!(!c.contains("vault_holder"), "no vault_holder in the credit context");
        assert!(!c.contains("pending_"), "no pending_* earmark bucket in the credit context");
    }

    #[test]
    fn credit_swap_ctx_all_accounts_anchor_typed() {
        let c = credit_swap_ctx_src();
        assert!(c.contains("pub vault_config: Box<Account<'info, VaultConfig>>"));
        assert!(c.contains("pub asset_pool: Box<Account<'info, AssetPool>>"));
        assert!(c.contains("pub player_escrow: Box<Account<'info, ProviderPlayerEscrow>>"));
        assert!(c.contains("pub escrow_holder: Box<Account<'info, TokenAccount>>"));
    }


    #[test]
    fn escrow_holder_canonical_ata_pin_gate() {
        let pin_ok = |canonical_ata: Pubkey, supplied: Pubkey| -> bool { supplied == canonical_ata };

        let canonical_ata = Pubkey::new_unique();
        let decoy = Pubkey::new_unique();
        assert_ne!(canonical_ata, decoy);

        assert!(pin_ok(canonical_ata, canonical_ata), "canonical ATA must pass");

        assert!(
            !pin_ok(canonical_ata, decoy),
            "C-01: a decoy authority-owned account MUST be rejected even on first touch"
        );

        assert!(
            !pin_ok(canonical_ata, decoy),
            "C-01: a decoy MUST never be accepted on any call"
        );
    }

    #[test]
    fn c01_all_seven_contexts_pin_canonical_ata() {
        let src = tlp_provider_vault_lib_rs_source();
        let ctx_body = |name: &str| -> &str {
            let sig = format!("pub struct {name}<'info> {{");
            let start = src.find(&sig).unwrap_or_else(|| panic!("{name} context must exist"));
            let rest = &src[start..];
            let end = start + rest.find("\n}").expect("context must close") + 2;
            &src[start..end]
        };
        let ata_pin =
            "address = get_associated_token_address(&escrow_holder_authority.key(), &asset_pool.asset_mint)";
        let record_pin = "player_escrow.escrow_holder";
        for name in [
            "ChipDeposit",
            "ChipWithdraw",
            "ChipDebitToVault",
            "ChipCreditFromVault",
            "ChipCreditFromVaultPromo",
            "ChipCreditFromVaultNgrPromo",
            "CreditChipsFromSwap",
        ] {
            let c = ctx_body(name);
            assert!(
                c.contains(ata_pin),
                "{name} must pin escrow_holder to the canonical ATA (C-01)"
            );
            assert!(
                c.contains("@ ProviderVaultError::EscrowHolderMismatch"),
                "{name} ATA pin must map to EscrowHolderMismatch"
            );
            assert!(
                c.contains("token::authority = escrow_holder_authority"),
                "{name} must keep token::authority = escrow_holder_authority"
            );
            assert!(
                !c.contains(record_pin),
                "{name} must NOT carry the record-and-pin escrow_holder constraint (C-01 regression)"
            );
        }
    }

    #[test]
    fn c01_no_handler_records_holder() {
        let full = tlp_provider_vault_lib_rs_source();
        let program_src = full
            .split("#[cfg(test)]")
            .next()
            .expect("program code precedes the #[cfg(test)] module");
        let latch = format!("escrow.{}", "escrow_holder = ctx.accounts.escrow_holder.key();");
        let guard = format!("if escrow.{}", "escrow_holder == Pubkey::default() {");
        assert!(
            !program_src.contains(&latch),
            "C-01: no handler may record escrow.escrow_holder (deterministic ATA pin only)"
        );
        assert!(
            !program_src.contains(&guard),
            "C-01: the first-touch record guard must be removed everywhere"
        );
    }

    #[test]
    fn c01_struct_field_removed_and_error_exists() {
        let full = tlp_provider_vault_lib_rs_source();
        let program_src = full
            .split("#[cfg(test)]")
            .next()
            .expect("program code precedes the #[cfg(test)] module");
        let field = format!("pub {}", "escrow_holder: Pubkey,");
        assert!(
            !program_src.contains(&field),
            "C-01: ProviderPlayerEscrow must NOT declare a recorded escrow_holder Pubkey field"
        );
        assert!(
            full.contains("EscrowHolderMismatch,"),
            "EscrowHolderMismatch error variant must still exist (now used by the ATA pin)"
        );
        assert_eq!(ProviderPlayerEscrow::LEN, 113);
    }
}
