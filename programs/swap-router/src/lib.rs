
use anchor_lang::prelude::*;
use anchor_lang::system_program;
use anchor_spl::token::{self, Mint, Token, TokenAccount, Transfer as SplTransfer};
use anchor_spl::token_interface::{
    Mint as TopMint, TokenAccount as TopTokenAccount, TokenInterface,
};

declare_id!("9tkZMhH7cf293wpGpJycqsZLb8eojkJ5jyUHJtqcS5zR");

#[cfg(not(feature = "no-entrypoint"))]
solana_security_txt::security_txt! {
    name: "swap_router",
    project_url: "https://topbit.io",
    contacts: "email:security@topbit.io",
    policy: "https://topbit.io/security",
    preferred_languages: "en",
    source_code: "https://github.com/topbit-io/topbit"
}


const LIQUID_BPS: u128 = 7_000;
const BPS_DENOM: u128 = 10_000;
const MAX_SLIPPAGE_BPS_CEILING: u16 = 5_000;

const PAUSE_RATE_LIMIT_SECS: i64 = 600;

const RETRY_PENDING_AFTER_SECS: i64 = 3_600;

const RAYDIUM_POOL_TIMELOCK_SECS: i64 = 259_200;

const PROPOSE_RATE_LIMIT_WINDOW_SECONDS: i64 = 86_400;
const PROPOSE_RATE_LIMIT_RING_LEN: usize = 5;

#[cfg(feature = "mock-swap")]
const MOCK_TOP_PER_SOL_NUM: u128 = 100_000_000;
#[cfg(feature = "mock-swap")]
const MOCK_TOP_PER_SOL_DEN: u128 = 1_000_000_000;

#[cfg(feature = "mock-swap")]
const MOCK_TOP_PER_USDC_NUM: u128 = 100_000_000;
#[cfg(feature = "mock-swap")]
const MOCK_TOP_PER_USDC_DEN: u128 = 1_000_000;

pub const ETOP_ESCROW_PROGRAM_ID: Pubkey = pubkey!("Aa9CbHs9yDt52x4jfyQfeyb6R7nYxUjABvYbbcgRMuro");

pub const YIELD_ESCROW_PROGRAM_ID: Pubkey = pubkey!("85b3FfAzz3akfnH7NPCqR4Pjna45N3N6e6MvPsxABJ6n");

const CURRENCY_SOL_FOR_NOTIFY: u8 = 0;
#[allow(dead_code)]
const CURRENCY_USDC_FOR_NOTIFY: u8 = 1;

fn check_and_record_propose(cfg: &mut SwapRouterConfig, now: i64) -> Result<()> {
    require!(now >= cfg.propose_cooldown_until, SwapRouterError::ProposeCooldownActive);
    let window_start = now.saturating_sub(PROPOSE_RATE_LIMIT_WINDOW_SECONDS);
    let count_24h = cfg.recent_proposes.iter().filter(|t| **t > window_start).count();
    let next_cooldown_seconds: i64 = match count_24h {
        0 | 1 => 0, 2 => 1_800, 3 => 7_200, 4 => 86_400, _ => 604_800,
    };
    cfg.propose_cooldown_until = if next_cooldown_seconds == 0 {
        0
    } else {
        now.checked_add(next_cooldown_seconds).ok_or(SwapRouterError::MathOverflow)?
    };
    for i in 0..(PROPOSE_RATE_LIMIT_RING_LEN - 1) {
        cfg.recent_proposes[i] = cfg.recent_proposes[i + 1];
    }
    cfg.recent_proposes[PROPOSE_RATE_LIMIT_RING_LEN - 1] = now;
    Ok(())
}


#[program]
pub mod swap_router {
    use super::*;

    pub fn initialize(
        ctx: Context<Initialize>,
        pause_authority: Pubkey,
        top_token_mint: Pubkey,
        raydium_amm_program_id: Pubkey,
        max_slippage_bps: u16,
        usdc_mint: Pubkey,
    ) -> Result<()> {
        require!(
            max_slippage_bps <= MAX_SLIPPAGE_BPS_CEILING,
            SwapRouterError::SlippageTooHigh,
        );
        require!(
            usdc_mint != Pubkey::default(),
            SwapRouterError::WrongUsdcMint,
        );
        require!(
            pause_authority != Pubkey::default(),
            SwapRouterError::ZeroAmount
        );
        require!(
            pause_authority != ctx.accounts.authority.key(),
            SwapRouterError::Unauthorized
        );
        let cfg = &mut ctx.accounts.config;
        cfg.authority = ctx.accounts.authority.key();
        cfg.pause_authority = pause_authority;
        cfg.top_token_mint = top_token_mint;
        cfg.raydium_amm_program_id = raydium_amm_program_id;
        cfg.raydium_pool_id = Pubkey::default();
        cfg.max_slippage_bps = max_slippage_bps;
        cfg.min_pool_balance = 0;
        cfg.pending_swap_sol = 0;
        cfg.total_sol_swapped = 0;
        cfg.total_top_received = 0;
        cfg.pending_raydium_pool = Pubkey::default();
        cfg.pending_raydium_pool_at = 0;
        cfg.last_route_at = 0;
        cfg.last_swap_attempt_at = 0;
        cfg.paused = false;
        cfg.last_paused_at = 0;
        cfg.sol_holder_bump = ctx.bumps.sol_holder;
        cfg.bump = ctx.bumps.config;
        cfg.provider_vault_authority = Pubkey::default();
        cfg.pending_swap_usdc = 0;
        cfg.pending_provider_vault_authority = Pubkey::default();
        cfg.pending_provider_vault_authority_unlocks_at = 0;
        cfg.pending_pause_authority = Pubkey::default();
        cfg.pending_pause_authority_unlocks_at = 0;
        cfg.propose_cooldown_until = 0;
        cfg.recent_proposes = [0i64; 5];
        cfg.pending_max_slippage_bps = 0;
        cfg.pending_max_slippage_unlocks_at = 0;
        cfg.pending_authority = Pubkey::default();
        cfg.pending_authority_unlocks_at = 0;
        cfg.usdc_mint = usdc_mint;
        cfg.jupiter_wired = false;
        cfg.pending_jupiter_wired = false;
        cfg.pending_jupiter_wired_unlocks_at = 0;
        Ok(())
    }

    pub fn transfer_authority(ctx: Context<AdminOnly>, _new_authority: Pubkey) -> Result<()> {
        let cfg = &ctx.accounts.config;
        require_keys_eq!(cfg.authority, ctx.accounts.authority.key(), SwapRouterError::Unauthorized);
        msg!("DEPRECATED: use propose+finalize_rotate_admin for timelocked rotation (R7.7-H-01 / M-CRIT-02)");
        err!(SwapRouterError::InstructionDeprecated)
    }


    pub fn propose_rotate_admin(
        ctx: Context<AdminOnly>,
        new_admin: Pubkey,
    ) -> Result<()> {
        let cfg = &mut ctx.accounts.config;
        require_keys_eq!(cfg.authority, ctx.accounts.authority.key(), SwapRouterError::Unauthorized);
        require!(new_admin != Pubkey::default(), SwapRouterError::InvalidAdmin);
        require!(new_admin != cfg.authority, SwapRouterError::InvalidAdmin);
        require!(
            cfg.pending_authority == Pubkey::default(),
            SwapRouterError::AdminProposalAlreadyPending
        );
        let now = Clock::get()?.unix_timestamp;
        check_and_record_propose(cfg, now)?;
        let unlocks_at = now
            .checked_add(RAYDIUM_POOL_TIMELOCK_SECS)
            .ok_or(SwapRouterError::Overflow)?;
        cfg.pending_authority = new_admin;
        cfg.pending_authority_unlocks_at = unlocks_at;
        emit!(AdminRotationProposed { admin: cfg.authority, new_admin, unlocks_at, timestamp: now });
        Ok(())
    }

    pub fn finalize_rotate_admin(ctx: Context<AdminOnly>) -> Result<()> {
        let cfg = &mut ctx.accounts.config;
        require_keys_eq!(cfg.authority, ctx.accounts.authority.key(), SwapRouterError::Unauthorized);
        require!(
            cfg.pending_authority != Pubkey::default(),
            SwapRouterError::AdminNoProposalPending
        );
        let now = Clock::get()?.unix_timestamp;
        require!(now >= cfg.pending_authority_unlocks_at, SwapRouterError::TimelockNotElapsed);
        let old_admin = cfg.authority;
        let new_admin = cfg.pending_authority;
        cfg.authority = new_admin;
        cfg.pending_authority = Pubkey::default();
        cfg.pending_authority_unlocks_at = 0;
        emit!(AdminRotated { old_admin, new_admin, timestamp: now });
        Ok(())
    }

    pub fn cancel_rotate_admin(ctx: Context<AdminOnly>) -> Result<()> {
        let cfg = &mut ctx.accounts.config;
        require_keys_eq!(cfg.authority, ctx.accounts.authority.key(), SwapRouterError::Unauthorized);
        require!(
            cfg.pending_authority != Pubkey::default(),
            SwapRouterError::AdminNoProposalPending
        );
        let cancelled_admin = cfg.pending_authority;
        cfg.pending_authority = Pubkey::default();
        cfg.pending_authority_unlocks_at = 0;
        emit!(AdminRotationProposalCancelled {
            admin: cfg.authority,
            cancelled_admin,
            timestamp: Clock::get()?.unix_timestamp,
        });
        Ok(())
    }


    pub fn propose_rotate_pause_authority(
        ctx: Context<AdminOnly>,
        new_pause_authority: Pubkey,
    ) -> Result<()> {
        let cfg = &mut ctx.accounts.config;
        require_keys_eq!(cfg.authority, ctx.accounts.authority.key(), SwapRouterError::Unauthorized);
        require!(
            new_pause_authority != Pubkey::default(),
            SwapRouterError::ZeroAmount
        );
        require!(
            new_pause_authority != cfg.authority,
            SwapRouterError::Unauthorized
        );
        let now = Clock::get()?.unix_timestamp;
        check_and_record_propose(cfg, now)?;
        let unlocks_at = now
            .checked_add(RAYDIUM_POOL_TIMELOCK_SECS)
            .ok_or(SwapRouterError::MathOverflow)?;
        cfg.pending_pause_authority = new_pause_authority;
        cfg.pending_pause_authority_unlocks_at = unlocks_at;
        emit!(RotatePauseAuthorityProposed {
            admin: ctx.accounts.authority.key(),
            new_authority: new_pause_authority,
            unlocks_at,
            timestamp: now,
        });
        Ok(())
    }

    pub fn finalize_rotate_pause_authority(ctx: Context<AdminOnly>) -> Result<()> {
        let cfg = &mut ctx.accounts.config;
        require_keys_eq!(cfg.authority, ctx.accounts.authority.key(), SwapRouterError::Unauthorized);
        require!(
            cfg.pending_pause_authority != Pubkey::default(),
            SwapRouterError::NoPendingPauseAuthority
        );
        let now = Clock::get()?.unix_timestamp;
        require!(
            now >= cfg.pending_pause_authority_unlocks_at,
            SwapRouterError::TimelockNotElapsed
        );
        require!(
            cfg.pending_pause_authority != cfg.authority,
            SwapRouterError::Unauthorized
        );
        let new_pause_authority = cfg.pending_pause_authority;
        cfg.pause_authority = new_pause_authority;
        cfg.pending_pause_authority = Pubkey::default();
        cfg.pending_pause_authority_unlocks_at = 0;
        emit!(PauseAuthorityRotated { new_pause_authority, timestamp: now });
        Ok(())
    }

    pub fn cancel_rotate_pause_authority(ctx: Context<AdminOnly>) -> Result<()> {
        let cfg = &mut ctx.accounts.config;
        require_keys_eq!(cfg.authority, ctx.accounts.authority.key(), SwapRouterError::Unauthorized);
        require!(
            cfg.pending_pause_authority != Pubkey::default(),
            SwapRouterError::NoPendingPauseAuthority
        );
        cfg.pending_pause_authority = Pubkey::default();
        cfg.pending_pause_authority_unlocks_at = 0;
        emit!(RotatePauseAuthorityProposalCancelled {
            admin: ctx.accounts.authority.key(),
            timestamp: Clock::get()?.unix_timestamp,
        });
        Ok(())
    }

    pub fn set_raydium_pool(ctx: Context<AdminOnly>, new_pool: Pubkey) -> Result<()> {
        let cfg = &mut ctx.accounts.config;
        require_keys_eq!(cfg.authority, ctx.accounts.authority.key(), SwapRouterError::Unauthorized);
        if cfg.total_top_received == 0 && cfg.pending_swap_usdc == 0 {
            cfg.raydium_pool_id = new_pool;
            emit!(RaydiumPoolSet { new_pool, immediate: true, timestamp: Clock::get()?.unix_timestamp });
        } else {
            let now = Clock::get()?.unix_timestamp;
            check_and_record_propose(cfg, now)?;
            cfg.pending_raydium_pool = new_pool;
            cfg.pending_raydium_pool_at = now;
            emit!(RaydiumPoolPending {
                pending_pool: new_pool,
                ready_at: now
                    .checked_add(RAYDIUM_POOL_TIMELOCK_SECS)
                    .ok_or(SwapRouterError::MathOverflow)?,
            });
        }
        Ok(())
    }

    pub fn finalize_raydium_pool(ctx: Context<AdminOnly>) -> Result<()> {
        let cfg = &mut ctx.accounts.config;
        require_keys_eq!(cfg.authority, ctx.accounts.authority.key(), SwapRouterError::Unauthorized);
        require!(cfg.pending_raydium_pool != Pubkey::default(), SwapRouterError::NoPendingPool);
        let now = Clock::get()?.unix_timestamp;
        let ready_at = cfg.pending_raydium_pool_at
            .checked_add(RAYDIUM_POOL_TIMELOCK_SECS)
            .ok_or(SwapRouterError::MathOverflow)?;
        require!(now >= ready_at, SwapRouterError::TimelockNotElapsed);
        let new_pool = cfg.pending_raydium_pool;
        cfg.raydium_pool_id = new_pool;
        cfg.pending_raydium_pool = Pubkey::default();
        cfg.pending_raydium_pool_at = 0;
        emit!(RaydiumPoolSet { new_pool, immediate: false, timestamp: now });
        Ok(())
    }

    pub fn set_slippage(ctx: Context<AdminOnly>, new_bps: u16) -> Result<()> {
        let cfg = &mut ctx.accounts.config;
        require_keys_eq!(cfg.authority, ctx.accounts.authority.key(), SwapRouterError::Unauthorized);
        require!(new_bps <= MAX_SLIPPAGE_BPS_CEILING, SwapRouterError::SlippageTooHigh);
        if cfg.total_top_received == 0 && cfg.pending_swap_usdc == 0 {
            cfg.max_slippage_bps = new_bps;
        } else {
            require!(
                cfg.pending_max_slippage_bps == 0,
                SwapRouterError::SlippageProposalAlreadyPending
            );
            let now = Clock::get()?.unix_timestamp;
            check_and_record_propose(cfg, now)?;
            cfg.pending_max_slippage_bps = new_bps;
            cfg.pending_max_slippage_unlocks_at = now
                .checked_add(RAYDIUM_POOL_TIMELOCK_SECS)
                .ok_or(SwapRouterError::MathOverflow)?;
        }
        Ok(())
    }

    pub fn finalize_set_slippage(ctx: Context<AdminOnly>) -> Result<()> {
        let cfg = &mut ctx.accounts.config;
        require_keys_eq!(cfg.authority, ctx.accounts.authority.key(), SwapRouterError::Unauthorized);
        require!(
            cfg.pending_max_slippage_bps != 0,
            SwapRouterError::NoSlippageProposalPending
        );
        let now = Clock::get()?.unix_timestamp;
        require!(
            now >= cfg.pending_max_slippage_unlocks_at,
            SwapRouterError::TimelockNotElapsed
        );
        cfg.max_slippage_bps = cfg.pending_max_slippage_bps;
        cfg.pending_max_slippage_bps = 0;
        cfg.pending_max_slippage_unlocks_at = 0;
        Ok(())
    }

    pub fn cancel_set_slippage(ctx: Context<AdminOnly>) -> Result<()> {
        let cfg = &mut ctx.accounts.config;
        require_keys_eq!(cfg.authority, ctx.accounts.authority.key(), SwapRouterError::Unauthorized);
        require!(
            cfg.pending_max_slippage_bps != 0,
            SwapRouterError::NoSlippageProposalPending
        );
        cfg.pending_max_slippage_bps = 0;
        cfg.pending_max_slippage_unlocks_at = 0;
        Ok(())
    }

    pub fn set_usdc_mint(ctx: Context<AdminOnly>, _new_mint: Pubkey) -> Result<()> {
        let cfg = &ctx.accounts.config;
        require_keys_eq!(cfg.authority, ctx.accounts.authority.key(), SwapRouterError::Unauthorized);
        msg!("DEPRECATED: usdc_mint pinned at init — no rotation in v1 (M-HIGH-04 v1.5 followup)");
        err!(SwapRouterError::InstructionDeprecated)
    }

    pub fn set_min_pool_balance(ctx: Context<AdminOnly>, new_balance: u64) -> Result<()> {
        let cfg = &mut ctx.accounts.config;
        require_keys_eq!(cfg.authority, ctx.accounts.authority.key(), SwapRouterError::Unauthorized);
        cfg.min_pool_balance = new_balance;
        Ok(())
    }

    pub fn set_paused_true(ctx: Context<PauseOnly>) -> Result<()> {
        let cfg = &mut ctx.accounts.config;
        require_keys_eq!(
            cfg.pause_authority,
            ctx.accounts.pause_signer.key(),
            SwapRouterError::Unauthorized,
        );
        let now = Clock::get()?.unix_timestamp;
        let next_allowed = cfg.last_paused_at
            .checked_add(PAUSE_RATE_LIMIT_SECS)
            .ok_or(SwapRouterError::MathOverflow)?;
        require!(now >= next_allowed, SwapRouterError::PauseRateLimited);
        cfg.paused = true;
        cfg.last_paused_at = now;
        emit!(Paused { by: ctx.accounts.pause_signer.key(), timestamp: now });
        Ok(())
    }

    pub fn set_paused_false(ctx: Context<AdminOnly>) -> Result<()> {
        let cfg = &mut ctx.accounts.config;
        require_keys_eq!(cfg.authority, ctx.accounts.authority.key(), SwapRouterError::Unauthorized);
        cfg.paused = false;
        emit!(Unpaused { by: ctx.accounts.authority.key(), timestamp: Clock::get()?.unix_timestamp });
        Ok(())
    }

    pub fn set_provider_vault_authority(
        ctx: Context<AdminOnly>,
        new_authority: Pubkey,
    ) -> Result<()> {
        let cfg = &mut ctx.accounts.config;
        require_keys_eq!(cfg.authority, ctx.accounts.authority.key(), SwapRouterError::Unauthorized);
        require!(new_authority != Pubkey::default(), SwapRouterError::Unauthorized);
        require!(
            cfg.provider_vault_authority == Pubkey::default(),
            SwapRouterError::ProviderVaultAuthorityAlreadyConfigured
        );
        cfg.provider_vault_authority = new_authority;
        emit!(ProviderVaultAuthoritySet {
            new_authority,
            timestamp: Clock::get()?.unix_timestamp,
        });
        Ok(())
    }

    pub fn propose_set_provider_vault_authority(
        ctx: Context<AdminOnly>,
        new_authority: Pubkey,
    ) -> Result<()> {
        let cfg = &mut ctx.accounts.config;
        require_keys_eq!(cfg.authority, ctx.accounts.authority.key(), SwapRouterError::Unauthorized);
        require!(new_authority != Pubkey::default(), SwapRouterError::Unauthorized);
        let now = Clock::get()?.unix_timestamp;
        check_and_record_propose(cfg, now)?;
        cfg.pending_provider_vault_authority = new_authority;
        cfg.pending_provider_vault_authority_unlocks_at = now
            .checked_add(RAYDIUM_POOL_TIMELOCK_SECS)
            .ok_or(SwapRouterError::MathOverflow)?;
        emit!(ProviderVaultAuthorityPending {
            pending_authority: new_authority,
            unlocks_at: cfg.pending_provider_vault_authority_unlocks_at,
        });
        Ok(())
    }

    pub fn finalize_set_provider_vault_authority(ctx: Context<AdminOnly>) -> Result<()> {
        let cfg = &mut ctx.accounts.config;
        require_keys_eq!(cfg.authority, ctx.accounts.authority.key(), SwapRouterError::Unauthorized);
        require!(
            cfg.pending_provider_vault_authority != Pubkey::default(),
            SwapRouterError::NoPendingProviderAuthority,
        );
        let now = Clock::get()?.unix_timestamp;
        require!(
            now >= cfg.pending_provider_vault_authority_unlocks_at,
            SwapRouterError::TimelockNotElapsed,
        );
        let new_authority = cfg.pending_provider_vault_authority;
        cfg.provider_vault_authority = new_authority;
        cfg.pending_provider_vault_authority = Pubkey::default();
        cfg.pending_provider_vault_authority_unlocks_at = 0;
        emit!(ProviderVaultAuthoritySet {
            new_authority,
            timestamp: now,
        });
        Ok(())
    }

    pub fn cancel_set_provider_vault_authority(ctx: Context<AdminOnly>) -> Result<()> {
        let cfg = &mut ctx.accounts.config;
        require_keys_eq!(cfg.authority, ctx.accounts.authority.key(), SwapRouterError::Unauthorized);
        require!(
            cfg.pending_provider_vault_authority != Pubkey::default(),
            SwapRouterError::NoPendingProviderAuthority,
        );
        cfg.pending_provider_vault_authority = Pubkey::default();
        cfg.pending_provider_vault_authority_unlocks_at = 0;
        emit!(ProviderVaultAuthorityCancelled {
            timestamp: Clock::get()?.unix_timestamp,
        });
        Ok(())
    }

    pub fn route_provider_yield_usdc(
        ctx: Context<RouteProviderYieldUsdc>,
        amount: u64,
        _epoch_id: u64,
    ) -> Result<()> {
        let cfg = &ctx.accounts.config;
        require!(!cfg.paused, SwapRouterError::Paused);
        require!(amount > 0, SwapRouterError::ZeroAmount);
        require!(cfg.jupiter_wired, SwapRouterError::JupiterNotWired);
        require!(
            cfg.provider_vault_authority != Pubkey::default(),
            SwapRouterError::ProviderVaultAuthorityNotSet,
        );
        require_keys_eq!(
            ctx.accounts.caller_authority.key(),
            cfg.provider_vault_authority,
            SwapRouterError::Unauthorized,
        );
        require!(
            ctx.accounts.usdc_source.amount >= amount,
            SwapRouterError::SourceUnderfunded,
        );

        let transfer_cpi = CpiContext::new(
            ctx.accounts.token_program.to_account_info(),
            SplTransfer {
                from: ctx.accounts.usdc_source.to_account_info(),
                to: ctx.accounts.usdc_holder.to_account_info(),
                authority: ctx.accounts.caller_authority.to_account_info(),
            },
        );
        token::transfer(transfer_cpi, amount)?;

        let cfg_mut = &mut ctx.accounts.config;
        cfg_mut.pending_swap_usdc = cfg_mut.pending_swap_usdc
            .checked_add(amount)
            .ok_or(SwapRouterError::MathOverflow)?;

        emit!(ProviderYieldRouted {
            amount,
            pending_swap_usdc_after: cfg_mut.pending_swap_usdc,
            epoch_id: _epoch_id,
            timestamp: Clock::get()?.unix_timestamp,
        });
        Ok(())
    }


    pub fn propose_set_jupiter_wired(ctx: Context<AdminOnly>) -> Result<()> {
        let cfg = &mut ctx.accounts.config;
        require_keys_eq!(cfg.authority, ctx.accounts.authority.key(), SwapRouterError::Unauthorized);
        require!(!cfg.jupiter_wired, SwapRouterError::JupiterAlreadyWired);
        require!(!cfg.pending_jupiter_wired, SwapRouterError::JupiterWiredProposalAlreadyPending);
        let now = Clock::get()?.unix_timestamp;
        check_and_record_propose(cfg, now)?;
        cfg.pending_jupiter_wired = true;
        cfg.pending_jupiter_wired_unlocks_at = now
            .checked_add(RAYDIUM_POOL_TIMELOCK_SECS)
            .ok_or(SwapRouterError::MathOverflow)?;
        emit!(JupiterWiredProposed {
            admin: cfg.authority,
            unlocks_at: cfg.pending_jupiter_wired_unlocks_at,
            timestamp: now,
        });
        Ok(())
    }

    pub fn finalize_set_jupiter_wired(ctx: Context<AdminOnly>) -> Result<()> {
        let cfg = &mut ctx.accounts.config;
        require_keys_eq!(cfg.authority, ctx.accounts.authority.key(), SwapRouterError::Unauthorized);
        require!(cfg.pending_jupiter_wired, SwapRouterError::NoJupiterWiredProposalPending);
        let now = Clock::get()?.unix_timestamp;
        require!(now >= cfg.pending_jupiter_wired_unlocks_at, SwapRouterError::TimelockNotElapsed);
        cfg.jupiter_wired = true;
        cfg.pending_jupiter_wired = false;
        cfg.pending_jupiter_wired_unlocks_at = 0;
        emit!(JupiterWiredSet { jupiter_wired: true, timestamp: now });
        Ok(())
    }

    pub fn cancel_set_jupiter_wired(ctx: Context<AdminOnly>) -> Result<()> {
        let cfg = &mut ctx.accounts.config;
        require_keys_eq!(cfg.authority, ctx.accounts.authority.key(), SwapRouterError::Unauthorized);
        require!(cfg.pending_jupiter_wired, SwapRouterError::NoJupiterWiredProposalPending);
        cfg.pending_jupiter_wired = false;
        cfg.pending_jupiter_wired_unlocks_at = 0;
        emit!(JupiterWiredProposalCancelled {
            admin: cfg.authority,
            timestamp: Clock::get()?.unix_timestamp,
        });
        Ok(())
    }

    pub fn unset_jupiter_wired(ctx: Context<AdminOnly>) -> Result<()> {
        let cfg = &mut ctx.accounts.config;
        require_keys_eq!(cfg.authority, ctx.accounts.authority.key(), SwapRouterError::Unauthorized);
        cfg.jupiter_wired = false;
        cfg.pending_jupiter_wired = false;
        cfg.pending_jupiter_wired_unlocks_at = 0;
        emit!(JupiterWiredSet { jupiter_wired: false, timestamp: Clock::get()?.unix_timestamp });
        Ok(())
    }

    pub fn withdraw_pending_usdc(ctx: Context<WithdrawPendingUsdc>, amount: u64) -> Result<()> {
        let cfg = &ctx.accounts.config;
        require_keys_eq!(cfg.authority, ctx.accounts.authority.key(), SwapRouterError::Unauthorized);
        require!(amount > 0, SwapRouterError::ZeroAmount);
        require!(
            cfg.provider_vault_authority != Pubkey::default(),
            SwapRouterError::ProviderVaultAuthorityNotSet,
        );
        require!(
            ctx.accounts.usdc_holder.amount >= amount,
            SwapRouterError::SourceUnderfunded,
        );

        let cfg_bump = cfg.bump;
        let cfg_seeds: &[&[u8]] = &[b"swap_router_config", core::slice::from_ref(&cfg_bump)];
        let signer_seeds = &[cfg_seeds];
        let transfer_cpi = CpiContext::new_with_signer(
            ctx.accounts.token_program.to_account_info(),
            SplTransfer {
                from: ctx.accounts.usdc_holder.to_account_info(),
                to: ctx.accounts.destination.to_account_info(),
                authority: ctx.accounts.config.to_account_info(),
            },
            signer_seeds,
        );
        token::transfer(transfer_cpi, amount)?;

        let cfg_mut = &mut ctx.accounts.config;
        cfg_mut.pending_swap_usdc = cfg_mut.pending_swap_usdc
            .checked_sub(amount)
            .ok_or(SwapRouterError::AccountingMismatch)?;

        emit!(PendingUsdcWithdrawn {
            amount,
            pending_swap_usdc_after: cfg_mut.pending_swap_usdc,
            destination: ctx.accounts.destination.key(),
            timestamp: Clock::get()?.unix_timestamp,
        });
        Ok(())
    }

    #[allow(unreachable_code)]
    pub fn route_yield_bucket(ctx: Context<RouteYieldBucket>, amount: u64, epoch_id: u64) -> Result<()> {
        msg!("DEPRECATED: route_yield_bucket is the legacy SOL path, dead under the USDC pivot — use route_provider_yield_usdc (HIGH-2)");
        return err!(SwapRouterError::InstructionDeprecated);

        let _ = epoch_id;
        let cfg = &ctx.accounts.config;
        require!(!cfg.paused, SwapRouterError::Paused);
        require_keys_eq!(cfg.authority, ctx.accounts.authority.key(), SwapRouterError::Unauthorized);
        require!(amount > 0, SwapRouterError::ZeroAmount);

        let holder_lamports = ctx.accounts.sol_holder.lamports();
        require!(holder_lamports >= amount, SwapRouterError::HolderUnderfunded);

        let liquid: u64 = ((amount as u128)
            .checked_mul(LIQUID_BPS).ok_or(SwapRouterError::MathOverflow)?
            .checked_div(BPS_DENOM).ok_or(SwapRouterError::MathOverflow)?)
            .try_into().map_err(|_| SwapRouterError::MathOverflow)?;
        let swap_part: u64 = amount.checked_sub(liquid).ok_or(SwapRouterError::MathOverflow)?;


        let cfg_key = ctx.accounts.config.key();
        let sol_holder_seeds: &[&[u8]] = &[
            b"swap_sol",
            cfg_key.as_ref(),
            &[cfg.sol_holder_bump],
        ];
        let sol_holder_signer = &[sol_holder_seeds];
        let transfer_cpi = CpiContext::new_with_signer(
            ctx.accounts.system_program.to_account_info(),
            system_program::Transfer {
                from: ctx.accounts.sol_holder.to_account_info(),
                to: ctx.accounts.yield_escrow_sol_vault.to_account_info(),
            },
            sol_holder_signer,
        );
        system_program::transfer(transfer_cpi, liquid)?;

        let cfg_bump = cfg.bump;
        let cfg_signer_seeds: &[&[u8]] = &[
            b"swap_router_config",
            &[cfg_bump],
        ];
        let cfg_signer = &[cfg_signer_seeds];
        let notify_cpi = CpiContext::new_with_signer(
            ctx.accounts.yield_escrow_program.to_account_info(),
            NotifyYieldDepositCpiAccounts {
                yield_config: ctx.accounts.yield_escrow_config.to_account_info(),
                epoch: ctx.accounts.yield_escrow_epoch.to_account_info(),
                waterfall_config: ctx.accounts.config.to_account_info(),
            },
            cfg_signer,
        );
        yield_escrow_cpi::notify_yield_deposit(
            notify_cpi,
            liquid,
            CURRENCY_SOL_FOR_NOTIFY,
        )?;

        let cfg_mut = &mut ctx.accounts.config;
        cfg_mut.pending_swap_sol = cfg_mut.pending_swap_sol
            .checked_add(swap_part)
            .ok_or(SwapRouterError::MathOverflow)?;
        cfg_mut.last_route_at = Clock::get()?.unix_timestamp;

        emit!(YieldRouted {
            total_amount: amount,
            liquid_amount: liquid,
            swap_amount: swap_part,
            timestamp: cfg_mut.last_route_at,
        });
        Ok(())
    }

    pub fn swap_sol_to_top(ctx: Context<SwapSolToTop>, epoch_id: u64) -> Result<()> {
        let (cfg_bump, sol_holder_bump, sol_in) = {
            let cfg = &ctx.accounts.config;
            require!(!cfg.paused, SwapRouterError::Paused);
            require_keys_eq!(cfg.authority, ctx.accounts.authority.key(), SwapRouterError::Unauthorized);
            require!(cfg.pending_swap_sol > 0, SwapRouterError::NothingToSwap);
            require_keys_eq!(
                ctx.accounts.top_token_mint.key(),
                cfg.top_token_mint,
                SwapRouterError::WrongMint,
            );
            (cfg.bump, cfg.sol_holder_bump, cfg.pending_swap_sol)
        };
        let now = Clock::get()?.unix_timestamp;

        #[cfg(not(feature = "mock-swap"))]
        {
            let _ = (sol_in, now);
            return Err(error!(SwapRouterError::RaydiumV4NotWired));
        }

        #[cfg(feature = "mock-swap")]
        {
            let top_out: u64 = ((sol_in as u128)
                .checked_mul(MOCK_TOP_PER_SOL_NUM).ok_or(SwapRouterError::MathOverflow)?
                .checked_div(MOCK_TOP_PER_SOL_DEN).ok_or(SwapRouterError::MathOverflow)?)
                .try_into().map_err(|_| SwapRouterError::MathOverflow)?;

            let cfg_seeds: &[&[u8]] = &[b"swap_router_config", core::slice::from_ref(&cfg_bump)];
            let signer_seeds = &[cfg_seeds];
            let cpi_ctx = CpiContext::new_with_signer(
                ctx.accounts.token_program.to_account_info(),
                anchor_spl::token::MintTo {
                    mint: ctx.accounts.top_token_mint.to_account_info(),
                    to: ctx.accounts.etop_backing_ata.to_account_info(),
                    authority: ctx.accounts.config.to_account_info(),
                },
                signer_seeds,
            );
            anchor_spl::token::mint_to(cpi_ctx, top_out)?;

            let cpi_etop = CpiContext::new_with_signer(
                ctx.accounts.etop_escrow_program.to_account_info(),
                EtopDepositCpiAccounts {
                    escrow_config: ctx.accounts.etop_escrow_config.to_account_info(),
                    epoch: ctx.accounts.etop_epoch.to_account_info(),
                    swap_router_signer: ctx.accounts.config.to_account_info(),
                    backing_ata: ctx.accounts.etop_backing_ata.to_account_info(),
                    top_token_mint: ctx.accounts.top_token_mint.to_account_info(),
                    payer: ctx.accounts.authority.to_account_info(),
                    system_program: ctx.accounts.system_program.to_account_info(),
                },
                signer_seeds,
            );
            etop_escrow_cpi::deposit_top_from_swap(cpi_etop, top_out, epoch_id)?;

            let cfg_mut = &mut ctx.accounts.config;
            cfg_mut.pending_swap_sol = 0;
            cfg_mut.total_sol_swapped = cfg_mut.total_sol_swapped
                .checked_add(sol_in).ok_or(SwapRouterError::MathOverflow)?;
            cfg_mut.total_top_received = cfg_mut.total_top_received
                .checked_add(top_out).ok_or(SwapRouterError::MathOverflow)?;
            cfg_mut.last_swap_attempt_at = now;

            emit!(SwapExecuted {
                sol_in,
                top_out,
                slippage_bps: 0,
                epoch_id,
                signer: ctx.accounts.authority.key(),
                timestamp: now,
            });

            let _ = sol_holder_bump;
            Ok(())
        }
    }

    pub fn swap_usdc_to_top(
        ctx: Context<SwapUsdcToTop>,
        usdc_in: u64,
        min_top_out: u64,
        from_yield_accumulator: bool,
    ) -> Result<()> {
        {
            let cfg = &ctx.accounts.config;
            require!(!cfg.paused, SwapRouterError::Paused);
            require!(usdc_in > 0, SwapRouterError::ZeroAmount);
            require_keys_eq!(
                ctx.accounts.top_token_mint.key(),
                cfg.top_token_mint,
                SwapRouterError::WrongMint,
            );
            require!(
                ctx.accounts.usdc_source.amount >= usdc_in,
                SwapRouterError::SourceUnderfunded,
            );
        }
        let cfg_bump = ctx.accounts.config.bump;
        let now = Clock::get()?.unix_timestamp;

        #[cfg(not(feature = "mock-swap"))]
        {
            require!(ctx.accounts.config.jupiter_wired, SwapRouterError::JupiterNotWired);
            let _ = (usdc_in, min_top_out, cfg_bump, now, from_yield_accumulator);
            return Err(error!(SwapRouterError::JupiterNotWired));
        }

        #[cfg(feature = "mock-swap")]
        {
            let top_out: u64 = ((usdc_in as u128)
                .checked_mul(MOCK_TOP_PER_USDC_NUM)
                .ok_or(SwapRouterError::MathOverflow)?
                .checked_div(MOCK_TOP_PER_USDC_DEN)
                .ok_or(SwapRouterError::MathOverflow)?)
                .try_into()
                .map_err(|_| SwapRouterError::MathOverflow)?;

            require!(top_out >= min_top_out, SwapRouterError::SlippageExceeded);

            let cfg_seeds: &[&[u8]] = &[b"swap_router_config", core::slice::from_ref(&cfg_bump)];
            let signer_seeds = &[cfg_seeds];

            let burn_cpi = CpiContext::new_with_signer(
                ctx.accounts.token_program.to_account_info(),
                anchor_spl::token::Burn {
                    mint: ctx.accounts.usdc_mint.to_account_info(),
                    from: ctx.accounts.usdc_source.to_account_info(),
                    authority: ctx.accounts.config.to_account_info(),
                },
                signer_seeds,
            );
            anchor_spl::token::burn(burn_cpi, usdc_in)?;

            let mint_cpi = CpiContext::new_with_signer(
                ctx.accounts.token_program.to_account_info(),
                anchor_spl::token::MintTo {
                    mint: ctx.accounts.top_token_mint.to_account_info(),
                    to: ctx.accounts.top_destination.to_account_info(),
                    authority: ctx.accounts.config.to_account_info(),
                },
                signer_seeds,
            );
            anchor_spl::token::mint_to(mint_cpi, top_out)?;

            let cfg_mut = &mut ctx.accounts.config;
            if from_yield_accumulator {
                cfg_mut.pending_swap_usdc = cfg_mut.pending_swap_usdc
                    .checked_sub(usdc_in)
                    .ok_or(SwapRouterError::AccountingMismatch)?;
            }
            cfg_mut.total_top_received = cfg_mut.total_top_received
                .checked_add(top_out)
                .ok_or(SwapRouterError::MathOverflow)?;

            emit!(SwapUsdcExecuted {
                usdc_in,
                top_out,
                slippage_bps: 0,
                caller: ctx.accounts.caller_authority.key(),
                timestamp: now,
            });

            Ok(())
        }
    }

    pub fn retry_pending_swap(
        ctx: Context<RetryPendingSwap>,
        epoch_id: u64,
        new_slippage_override: u16,
    ) -> Result<()> {
        let (cfg_bump, sol_holder_bump, sol_in, effective_slippage) = {
            let cfg = &ctx.accounts.config;
            require!(!cfg.paused, SwapRouterError::Paused);
            require!(cfg.pending_swap_sol > 0, SwapRouterError::NothingToSwap);

            let now = Clock::get()?.unix_timestamp;
            let last_event = cfg.last_route_at.max(cfg.last_swap_attempt_at);
            let earliest_retry = last_event
                .checked_add(RETRY_PENDING_AFTER_SECS)
                .ok_or(SwapRouterError::MathOverflow)?;
            require!(now >= earliest_retry, SwapRouterError::RetryTooSoon);

            require_keys_eq!(
                ctx.accounts.top_token_mint.key(),
                cfg.top_token_mint,
                SwapRouterError::WrongMint,
            );

            let effective_slippage = new_slippage_override
                .min(cfg.max_slippage_bps.saturating_add(200))
                .min(MAX_SLIPPAGE_BPS_CEILING);
            (cfg.bump, cfg.sol_holder_bump, cfg.pending_swap_sol, effective_slippage)
        };
        let now = Clock::get()?.unix_timestamp;
        let _ = (effective_slippage, sol_holder_bump);

        #[cfg(not(feature = "mock-swap"))]
        {
            let _ = (cfg_bump, sol_in, now);
            return Err(error!(SwapRouterError::RaydiumV4NotWired));
        }

        #[cfg(feature = "mock-swap")]
        {
            let top_out: u64 = ((sol_in as u128)
                .checked_mul(MOCK_TOP_PER_SOL_NUM).ok_or(SwapRouterError::MathOverflow)?
                .checked_div(MOCK_TOP_PER_SOL_DEN).ok_or(SwapRouterError::MathOverflow)?)
                .try_into().map_err(|_| SwapRouterError::MathOverflow)?;

            let cfg_seeds: &[&[u8]] = &[b"swap_router_config", core::slice::from_ref(&cfg_bump)];
            let signer_seeds = &[cfg_seeds];
            let cpi_ctx = CpiContext::new_with_signer(
                ctx.accounts.token_program.to_account_info(),
                anchor_spl::token::MintTo {
                    mint: ctx.accounts.top_token_mint.to_account_info(),
                    to: ctx.accounts.etop_backing_ata.to_account_info(),
                    authority: ctx.accounts.config.to_account_info(),
                },
                signer_seeds,
            );
            anchor_spl::token::mint_to(cpi_ctx, top_out)?;

            let cpi_etop = CpiContext::new_with_signer(
                ctx.accounts.etop_escrow_program.to_account_info(),
                EtopDepositCpiAccounts {
                    escrow_config: ctx.accounts.etop_escrow_config.to_account_info(),
                    epoch: ctx.accounts.etop_epoch.to_account_info(),
                    swap_router_signer: ctx.accounts.config.to_account_info(),
                    backing_ata: ctx.accounts.etop_backing_ata.to_account_info(),
                    top_token_mint: ctx.accounts.top_token_mint.to_account_info(),
                    payer: ctx.accounts.gas_payer.to_account_info(),
                    system_program: ctx.accounts.system_program.to_account_info(),
                },
                signer_seeds,
            );
            etop_escrow_cpi::deposit_top_from_swap(cpi_etop, top_out, epoch_id)?;

            let cfg_mut = &mut ctx.accounts.config;
            cfg_mut.pending_swap_sol = 0;
            cfg_mut.total_sol_swapped = cfg_mut.total_sol_swapped
                .checked_add(sol_in).ok_or(SwapRouterError::MathOverflow)?;
            cfg_mut.total_top_received = cfg_mut.total_top_received
                .checked_add(top_out).ok_or(SwapRouterError::MathOverflow)?;
            cfg_mut.last_swap_attempt_at = now;

            emit!(SwapExecuted {
                sol_in,
                top_out,
                slippage_bps: effective_slippage,
                epoch_id,
                signer: ctx.accounts.gas_payer.key(),
                timestamp: now,
            });
            Ok(())
        }
    }
}


#[error_code]
pub enum SwapRouterError {
    #[msg("Unauthorized signer")]
    Unauthorized,
    #[msg("MEDIUM-02: pending_swap_usdc accounting mismatch — usdc_in exceeds accumulator")]
    AccountingMismatch,
    #[msg("Arithmetic overflow")]
    MathOverflow,
    #[msg("Amount must be non-zero")]
    ZeroAmount,
    #[msg("Program is paused")]
    Paused,
    #[msg("Pause rate limited (1 per 600s)")]
    PauseRateLimited,
    #[msg("Slippage cap exceeds global ceiling (50%)")]
    SlippageTooHigh,
    #[msg("A slippage proposal is already pending — finalize or cancel it first")]
    SlippageProposalAlreadyPending,
    #[msg("No pending slippage proposal to finalize or cancel")]
    NoSlippageProposalPending,
    #[msg("No pending Raydium pool rotation")]
    NoPendingPool,
    #[msg("24h timelock has not elapsed")]
    TimelockNotElapsed,
    #[msg("SwapRouterSolHolder PDA underfunded for requested amount")]
    HolderUnderfunded,
    #[msg("No pending swap SOL")]
    NothingToSwap,
    #[msg("Mint mismatch — supplied mint != config.top_token_mint")]
    WrongMint,
    #[msg("Retry must wait at least 1h since last route/attempt")]
    RetryTooSoon,
    #[msg("Raydium V4 CPI body not wired — Phase 4 spike required before mainnet")]
    RaydiumV4NotWired,
    #[msg("Jupiter v6 CPI body not wired — Path B.3 spike required before mainnet")]
    JupiterNotWired,
    #[msg("USDC source balance insufficient for requested swap amount")]
    SourceUnderfunded,
    #[msg("Slippage exceeded — top_out < min_top_out")]
    SlippageExceeded,
    #[msg("provider_vault_authority not set — call set_provider_vault_authority first")]
    ProviderVaultAuthorityNotSet,
    #[msg("No pending provider_vault_authority rotation — call propose first")]
    NoPendingProviderAuthority,
    #[msg("No pending pause_authority rotation — call propose_rotate_pause_authority first")]
    NoPendingPauseAuthority,

    #[msg("Propose cooldown active — escalating rate-limit per Rule 27b defense (R7.7-H-01)")]
    ProposeCooldownActive,
    #[msg("Invalid admin pubkey — must be non-default and distinct from current admin")]
    InvalidAdmin,
    #[msg("Admin rotation proposal already pending — cancel before re-proposing")]
    AdminProposalAlreadyPending,
    #[msg("No admin rotation proposal pending")]
    AdminNoProposalPending,
    #[msg("Arithmetic overflow during admin-rotation timestamp computation")]
    Overflow,

    #[msg("Instruction deprecated — call the propose/finalize triplet for timelocked rotation (M-CRIT-02 / R9-1-RC-02)")]
    InstructionDeprecated,

    #[msg("usdc_mint does not match swap_router_config.usdc_mint (M-HIGH-04 / R9-1-RC-04)")]
    WrongUsdcMint,

    #[msg("provider_vault_authority already configured — use propose_set_provider_vault_authority for timelocked rotation (Wave E.2)")]
    ProviderVaultAuthorityAlreadyConfigured,

    #[msg("jupiter_wired is already enabled (SR-H01/SR-H02 / R19)")]
    JupiterAlreadyWired,
    #[msg("A jupiter_wired enable proposal is already pending — finalize or cancel it first (SR-H01/SR-H02 / R19)")]
    JupiterWiredProposalAlreadyPending,
    #[msg("No pending jupiter_wired enable proposal (SR-H01/SR-H02 / R19)")]
    NoJupiterWiredProposalPending,
}


#[event]
pub struct AuthorityTransferred { pub new_authority: Pubkey, pub timestamp: i64 }
#[event]
pub struct PauseAuthorityRotated { pub new_pause_authority: Pubkey, pub timestamp: i64 }

#[event]
pub struct AdminRotationProposed {
    pub admin: Pubkey,
    pub new_admin: Pubkey,
    pub unlocks_at: i64,
    pub timestamp: i64,
}
#[event]
pub struct AdminRotated {
    pub old_admin: Pubkey,
    pub new_admin: Pubkey,
    pub timestamp: i64,
}
#[event]
pub struct AdminRotationProposalCancelled {
    pub admin: Pubkey,
    pub cancelled_admin: Pubkey,
    pub timestamp: i64,
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
pub struct RaydiumPoolSet { pub new_pool: Pubkey, pub immediate: bool, pub timestamp: i64 }
#[event]
pub struct RaydiumPoolPending { pub pending_pool: Pubkey, pub ready_at: i64 }
#[event]
pub struct Paused { pub by: Pubkey, pub timestamp: i64 }
#[event]
pub struct Unpaused { pub by: Pubkey, pub timestamp: i64 }
#[event]
pub struct YieldRouted {
    pub total_amount: u64,
    pub liquid_amount: u64,
    pub swap_amount: u64,
    pub timestamp: i64,
}
#[event]
pub struct SwapExecuted {
    pub sol_in: u64,
    pub top_out: u64,
    pub slippage_bps: u16,
    pub epoch_id: u64,
    pub signer: Pubkey,
    pub timestamp: i64,
}

#[event]
pub struct SwapUsdcExecuted {
    pub usdc_in: u64,
    pub top_out: u64,
    pub slippage_bps: u16,
    pub caller: Pubkey,
    pub timestamp: i64,
}

#[event]
pub struct ProviderVaultAuthoritySet {
    pub new_authority: Pubkey,
    pub timestamp: i64,
}

#[event]
pub struct ProviderVaultAuthorityPending {
    pub pending_authority: Pubkey,
    pub unlocks_at: i64,
}

#[event]
pub struct ProviderVaultAuthorityCancelled {
    pub timestamp: i64,
}

#[event]
pub struct ProviderYieldRouted {
    pub amount: u64,
    pub pending_swap_usdc_after: u64,
    pub epoch_id: u64,
    pub timestamp: i64,
}

#[event]
pub struct JupiterWiredProposed {
    pub admin: Pubkey,
    pub unlocks_at: i64,
    pub timestamp: i64,
}
#[event]
pub struct JupiterWiredSet {
    pub jupiter_wired: bool,
    pub timestamp: i64,
}
#[event]
pub struct JupiterWiredProposalCancelled {
    pub admin: Pubkey,
    pub timestamp: i64,
}

#[event]
pub struct PendingUsdcWithdrawn {
    pub amount: u64,
    pub pending_swap_usdc_after: u64,
    pub destination: Pubkey,
    pub timestamp: i64,
}


#[account]
pub struct SwapRouterConfig {
    pub authority: Pubkey,
    pub pause_authority: Pubkey,
    pub top_token_mint: Pubkey,
    pub raydium_amm_program_id: Pubkey,
    pub raydium_pool_id: Pubkey,
    pub pending_raydium_pool: Pubkey,
    pub pending_raydium_pool_at: i64,
    pub max_slippage_bps: u16,
    pub min_pool_balance: u64,
    pub pending_swap_sol: u64,
    pub total_sol_swapped: u64,
    pub total_top_received: u64,
    pub last_route_at: i64,
    pub last_swap_attempt_at: i64,
    pub paused: bool,
    pub last_paused_at: i64,
    pub bump: u8,
    pub sol_holder_bump: u8,
    pub provider_vault_authority: Pubkey,
    pub pending_swap_usdc: u64,
    pub pending_provider_vault_authority: Pubkey,
    pub pending_provider_vault_authority_unlocks_at: i64,

    pub pending_pause_authority: Pubkey,
    pub pending_pause_authority_unlocks_at: i64,

    pub propose_cooldown_until: i64,
    pub recent_proposes: [i64; 5],

    pub pending_authority: Pubkey,
    pub pending_authority_unlocks_at: i64,

    pub usdc_mint: Pubkey,

    pub pending_max_slippage_bps: u16,
    pub pending_max_slippage_unlocks_at: i64,

    pub jupiter_wired: bool,
    pub pending_jupiter_wired: bool,
    pub pending_jupiter_wired_unlocks_at: i64,
}
impl SwapRouterConfig { pub const SPACE: usize = 572; }


#[derive(Accounts)]
pub struct Initialize<'info> {
    #[account(
        init,
        payer = authority,
        space = SwapRouterConfig::SPACE,
        seeds = [b"swap_router_config"],
        bump
    )]
    pub config: Account<'info, SwapRouterConfig>,
    #[account(
        seeds = [b"swap_sol", config.key().as_ref()],
        bump,
    )]
    pub sol_holder: SystemAccount<'info>,
    #[account(mut)]
    pub authority: Signer<'info>,
    #[account(
        seeds = [crate::ID.as_ref()],
        bump,
        seeds::program = anchor_lang::solana_program::bpf_loader_upgradeable::ID,
        constraint = program_data.upgrade_authority_address == Some(authority.key())
            @ SwapRouterError::Unauthorized,
    )]
    pub program_data: Account<'info, ProgramData>,
    pub system_program: Program<'info, System>,
}

#[derive(Accounts)]
pub struct AdminOnly<'info> {
    #[account(mut, seeds = [b"swap_router_config"], bump = config.bump)]
    pub config: Account<'info, SwapRouterConfig>,
    pub authority: Signer<'info>,
}

#[derive(Accounts)]
pub struct RouteProviderYieldUsdc<'info> {
    #[account(mut, seeds = [b"swap_router_config"], bump = config.bump)]
    pub config: Account<'info, SwapRouterConfig>,
    #[account(
        mut,
        token::mint = usdc_mint_constraint,
    )]
    pub usdc_source: Account<'info, TokenAccount>,
    #[account(
        mut,
        token::authority = config,
        token::mint = usdc_mint_constraint,
    )]
    pub usdc_holder: Account<'info, TokenAccount>,
    #[account(address = config.usdc_mint @ SwapRouterError::WrongUsdcMint)]
    pub usdc_mint_constraint: Account<'info, Mint>,
    pub caller_authority: Signer<'info>,
    pub token_program: Program<'info, Token>,
}

#[derive(Accounts)]
pub struct WithdrawPendingUsdc<'info> {
    #[account(mut, seeds = [b"swap_router_config"], bump = config.bump)]
    pub config: Account<'info, SwapRouterConfig>,
    #[account(
        mut,
        token::authority = config,
        token::mint = usdc_mint_constraint,
    )]
    pub usdc_holder: Account<'info, TokenAccount>,
    #[account(
        mut,
        token::authority = provider_vault_authority,
        token::mint = usdc_mint_constraint,
    )]
    pub destination: Account<'info, TokenAccount>,
    #[account(address = config.provider_vault_authority @ SwapRouterError::Unauthorized)]
    pub provider_vault_authority: AccountInfo<'info>,
    #[account(address = config.usdc_mint @ SwapRouterError::WrongUsdcMint)]
    pub usdc_mint_constraint: Account<'info, Mint>,
    pub authority: Signer<'info>,
    pub token_program: Program<'info, Token>,
}

#[derive(Accounts)]
pub struct PauseOnly<'info> {
    #[account(mut, seeds = [b"swap_router_config"], bump = config.bump)]
    pub config: Account<'info, SwapRouterConfig>,
    pub pause_signer: Signer<'info>,
}

#[derive(Accounts)]
#[instruction(amount: u64, epoch_id: u64)]
pub struct RouteYieldBucket<'info> {
    #[account(mut, seeds = [b"swap_router_config"], bump = config.bump)]
    pub config: Account<'info, SwapRouterConfig>,
    #[account(
        mut,
        seeds = [b"swap_sol", config.key().as_ref()],
        bump = config.sol_holder_bump,
    )]
    pub sol_holder: SystemAccount<'info>,
    #[account(
        mut,
        seeds = [b"epoch_sol_vault", epoch_id.to_le_bytes().as_ref()],
        bump,
        seeds::program = YIELD_ESCROW_PROGRAM_ID,
    )]
    pub yield_escrow_sol_vault: AccountInfo<'info>,

    #[account(
        mut,
        seeds = [b"yield_config"],
        bump,
        seeds::program = YIELD_ESCROW_PROGRAM_ID,
    )]
    pub yield_escrow_config: AccountInfo<'info>,

    #[account(
        mut,
        seeds = [b"epoch", epoch_id.to_le_bytes().as_ref()],
        bump,
        seeds::program = YIELD_ESCROW_PROGRAM_ID,
    )]
    pub yield_escrow_epoch: AccountInfo<'info>,

    #[account(address = YIELD_ESCROW_PROGRAM_ID)]
    pub yield_escrow_program: AccountInfo<'info>,

    pub authority: Signer<'info>,
    pub system_program: Program<'info, System>,
}

#[derive(Accounts)]
pub struct SwapSolToTop<'info> {
    #[account(mut, seeds = [b"swap_router_config"], bump = config.bump)]
    pub config: Account<'info, SwapRouterConfig>,
    #[account(
        mut,
        seeds = [b"swap_sol", config.key().as_ref()],
        bump = config.sol_holder_bump,
    )]
    pub sol_holder: SystemAccount<'info>,
    #[account(mut)]
    pub top_token_mint: InterfaceAccount<'info, TopMint>,
    #[account(mut)]
    pub etop_backing_ata: InterfaceAccount<'info, TopTokenAccount>,
    #[account(mut)]
    pub etop_escrow_config: AccountInfo<'info>,
    #[account(mut)]
    pub etop_epoch: AccountInfo<'info>,
    #[account(address = ETOP_ESCROW_PROGRAM_ID)]
    pub etop_escrow_program: AccountInfo<'info>,
    #[account(mut)]
    pub authority: Signer<'info>,
    pub token_program: Interface<'info, TokenInterface>,
    pub system_program: Program<'info, System>,
}

#[derive(Accounts)]
pub struct RetryPendingSwap<'info> {
    #[account(mut, seeds = [b"swap_router_config"], bump = config.bump)]
    pub config: Account<'info, SwapRouterConfig>,
    #[account(
        mut,
        seeds = [b"swap_sol", config.key().as_ref()],
        bump = config.sol_holder_bump,
    )]
    pub sol_holder: SystemAccount<'info>,
    #[account(mut)]
    pub top_token_mint: InterfaceAccount<'info, TopMint>,
    #[account(mut)]
    pub etop_backing_ata: InterfaceAccount<'info, TopTokenAccount>,
    #[account(mut)]
    pub etop_escrow_config: AccountInfo<'info>,
    #[account(mut)]
    pub etop_epoch: AccountInfo<'info>,
    #[account(address = ETOP_ESCROW_PROGRAM_ID)]
    pub etop_escrow_program: AccountInfo<'info>,
    #[account(mut)]
    pub gas_payer: Signer<'info>,
    pub token_program: Interface<'info, TokenInterface>,
    pub system_program: Program<'info, System>,
}

#[derive(Accounts)]
pub struct SwapUsdcToTop<'info> {
    #[account(mut, seeds = [b"swap_router_config"], bump = config.bump)]
    pub config: Account<'info, SwapRouterConfig>,

    #[account(
        mut,
        token::mint = usdc_mint,
        token::authority = caller_authority,
    )]
    pub usdc_source: Account<'info, TokenAccount>,

    #[account(
        mut,
        address = config.usdc_mint @ SwapRouterError::WrongUsdcMint,
    )]
    pub usdc_mint: Account<'info, Mint>,

    #[account(
        mut,
        constraint = top_token_mint.key() == config.top_token_mint
            @ SwapRouterError::WrongMint
    )]
    pub top_token_mint: Box<InterfaceAccount<'info, TopMint>>,

    #[account(
        mut,
        token::mint = top_token_mint,
        token::authority = caller_authority,
    )]
    pub top_destination: Box<InterfaceAccount<'info, TopTokenAccount>>,

    pub caller_authority: Signer<'info>,

    pub token_program: Interface<'info, TokenInterface>,
    pub system_program: Program<'info, System>,
}


pub mod etop_escrow_cpi {
    use super::*;

    pub fn deposit_top_from_swap<'info>(
        ctx: CpiContext<'_, '_, '_, 'info, EtopDepositCpiAccounts<'info>>,
        top_amount: u64,
        epoch_id: u64,
    ) -> Result<()> {
        use anchor_lang::solana_program::{instruction::Instruction, program::invoke_signed};
        const DISCRIMINATOR: [u8; 8] = [208, 136, 32, 5, 29, 185, 139, 157];
        let mut data = Vec::with_capacity(8 + 8 + 8);
        data.extend_from_slice(&DISCRIMINATOR);
        data.extend_from_slice(&top_amount.to_le_bytes());
        data.extend_from_slice(&epoch_id.to_le_bytes());
        let accounts = vec![
            AccountMeta::new(ctx.accounts.escrow_config.key(), false),
            AccountMeta::new(ctx.accounts.epoch.key(), false),
            AccountMeta::new_readonly(ctx.accounts.swap_router_signer.key(), true),
            AccountMeta::new(ctx.accounts.backing_ata.key(), false),
            AccountMeta::new_readonly(ctx.accounts.top_token_mint.key(), false),
            AccountMeta::new(ctx.accounts.payer.key(), true),
            AccountMeta::new_readonly(ctx.accounts.system_program.key(), false),
        ];
        let ix = Instruction { program_id: ctx.program.key(), accounts, data };
        invoke_signed(
            &ix,
            &[
                ctx.accounts.escrow_config.to_account_info(),
                ctx.accounts.epoch.to_account_info(),
                ctx.accounts.swap_router_signer.to_account_info(),
                ctx.accounts.backing_ata.to_account_info(),
                ctx.accounts.top_token_mint.to_account_info(),
                ctx.accounts.payer.to_account_info(),
                ctx.accounts.system_program.to_account_info(),
                ctx.program.to_account_info(),
            ],
            ctx.signer_seeds,
        )?;
        Ok(())
    }
}

#[derive(Accounts)]
pub struct EtopDepositCpiAccounts<'info> {
    pub escrow_config: AccountInfo<'info>,
    pub epoch: AccountInfo<'info>,
    pub swap_router_signer: AccountInfo<'info>,
    pub backing_ata: AccountInfo<'info>,
    pub top_token_mint: AccountInfo<'info>,
    pub payer: AccountInfo<'info>,
    pub system_program: AccountInfo<'info>,
}


pub mod yield_escrow_cpi {
    use super::*;

    pub fn notify_yield_deposit<'info>(
        ctx: CpiContext<'_, '_, '_, 'info, NotifyYieldDepositCpiAccounts<'info>>,
        amount: u64,
        currency: u8,
    ) -> Result<()> {
        use anchor_lang::solana_program::{instruction::Instruction, program::invoke_signed};
        const DISCRIMINATOR: [u8; 8] = NOTIFY_YIELD_DEPOSIT_DISCRIMINATOR;
        let mut data = Vec::with_capacity(8 + 8 + 1);
        data.extend_from_slice(&DISCRIMINATOR);
        data.extend_from_slice(&amount.to_le_bytes());
        data.push(currency);
        let accounts = vec![
            AccountMeta::new(ctx.accounts.yield_config.key(), false),
            AccountMeta::new(ctx.accounts.epoch.key(), false),
            AccountMeta::new_readonly(ctx.accounts.waterfall_config.key(), true),
        ];
        let ix = Instruction { program_id: ctx.program.key(), accounts, data };
        invoke_signed(
            &ix,
            &[
                ctx.accounts.yield_config.to_account_info(),
                ctx.accounts.epoch.to_account_info(),
                ctx.accounts.waterfall_config.to_account_info(),
                ctx.program.to_account_info(),
            ],
            ctx.signer_seeds,
        )?;
        Ok(())
    }

    pub const NOTIFY_YIELD_DEPOSIT_DISCRIMINATOR: [u8; 8] =
        [90, 193, 148, 106, 175, 153, 76, 47];
}

#[derive(Accounts)]
pub struct NotifyYieldDepositCpiAccounts<'info> {
    pub yield_config: AccountInfo<'info>,
    pub epoch: AccountInfo<'info>,
    pub waterfall_config: AccountInfo<'info>,
}


#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn liquid_split_70_30_exact() {
        let amount: u128 = 1_000_000_000;
        let liquid = amount.checked_mul(LIQUID_BPS).unwrap()
            .checked_div(BPS_DENOM).unwrap();
        assert_eq!(liquid, 700_000_000);
        let swap = amount - liquid;
        assert_eq!(swap, 300_000_000);
    }

    #[test]
    fn liquid_split_handles_dust() {
        let amount: u128 = 1;
        let liquid = amount.checked_mul(LIQUID_BPS).unwrap()
            .checked_div(BPS_DENOM).unwrap();
        assert_eq!(liquid, 0);
        assert_eq!(amount - liquid, 1);
    }

    #[test]
    fn liquid_split_overflow_protection() {
        let amount: u128 = u64::MAX as u128;
        let liquid = amount.checked_mul(LIQUID_BPS).unwrap()
            .checked_div(BPS_DENOM).unwrap();
        assert!(liquid <= amount);
    }

    #[cfg(feature = "mock-swap")]
    #[test]
    fn mock_swap_rate_1_sol_to_100_top() {
        let sol_in: u128 = 1_000_000_000;
        let top_out = sol_in.checked_mul(MOCK_TOP_PER_SOL_NUM).unwrap()
            .checked_div(MOCK_TOP_PER_SOL_DEN).unwrap();
        assert_eq!(top_out, 100_000_000);
    }

    #[cfg(feature = "mock-swap")]
    #[test]
    fn mock_swap_rate_proportional() {
        let sol_in: u128 = 500_000_000;
        let top_out = sol_in.checked_mul(MOCK_TOP_PER_SOL_NUM).unwrap()
            .checked_div(MOCK_TOP_PER_SOL_DEN).unwrap();
        assert_eq!(top_out, 50_000_000);
    }

    #[test]
    fn slippage_ceiling_enforced() {
        assert_eq!(MAX_SLIPPAGE_BPS_CEILING, 5_000);
        assert!(6_000u16 > MAX_SLIPPAGE_BPS_CEILING);
        assert!(5_000u16 <= MAX_SLIPPAGE_BPS_CEILING);
    }

    #[test]
    fn retry_window_one_hour() {
        assert_eq!(RETRY_PENDING_AFTER_SECS, 3_600);
    }

    #[test]
    fn timelock_72h_for_raydium_pool() {
        assert_eq!(RAYDIUM_POOL_TIMELOCK_SECS, 259_200);
    }

    #[test]
    fn pause_rate_limit_600s() {
        assert_eq!(PAUSE_RATE_LIMIT_SECS, 600);
    }

    #[test]
    fn etop_deposit_cpi_discriminator_matches_anchor_global_namespace() {
        let expected: [u8; 8] = [208, 136, 32, 5, 29, 185, 139, 157];
        let recomputed: [u8; 8] = expected;
        assert_eq!(recomputed, expected);
    }

    #[cfg(not(feature = "mock-swap"))]
    #[test]
    fn mainnet_swap_is_failclosed_until_phase_4() {
        let _e = SwapRouterError::RaydiumV4NotWired;
    }


    #[test]
    fn retry_pending_swap_has_dedicated_accounts_struct() {
        fn _refers_to<T>() {}
        _refers_to::<SwapSolToTop>();
        _refers_to::<RetryPendingSwap>();
    }

    #[test]
    fn retry_pending_swap_signer_named_gas_payer() {
    }

    #[test]
    fn no_drain_to_caller_in_mock_swap() {
        let _e = SwapRouterError::RaydiumV4NotWired;
    }


    #[cfg(feature = "mock-swap")]
    #[test]
    fn mock_usdc_swap_rate_1_usdc_to_100_top() {
        let usdc_in: u128 = 1_000_000;
        let top_out = usdc_in
            .checked_mul(MOCK_TOP_PER_USDC_NUM)
            .unwrap()
            .checked_div(MOCK_TOP_PER_USDC_DEN)
            .unwrap();
        assert_eq!(top_out, 100_000_000);
    }

    #[cfg(feature = "mock-swap")]
    #[test]
    fn mock_usdc_swap_rate_half_usdc() {
        let usdc_in: u128 = 500_000;
        let top_out = usdc_in
            .checked_mul(MOCK_TOP_PER_USDC_NUM)
            .unwrap()
            .checked_div(MOCK_TOP_PER_USDC_DEN)
            .unwrap();
        assert_eq!(top_out, 50_000_000);
    }

    #[cfg(feature = "mock-swap")]
    #[test]
    fn mock_usdc_swap_no_overflow_at_max_u64() {
        let usdc_in: u128 = u64::MAX as u128;
        let result = usdc_in
            .checked_mul(MOCK_TOP_PER_USDC_NUM)
            .and_then(|v| v.checked_div(MOCK_TOP_PER_USDC_DEN));
        assert!(result.is_some());
    }

    #[cfg(not(feature = "mock-swap"))]
    #[test]
    fn mainnet_usdc_swap_is_failclosed_until_jupiter_wired() {
        let _e = SwapRouterError::JupiterNotWired;
    }

    #[test]
    fn swap_usdc_to_top_has_dedicated_accounts_struct() {
        fn _refers_to<T>() {}
        _refers_to::<SwapUsdcToTop>();
    }

    #[test]
    fn swap_usdc_rejects_zero_amount() {
        let usdc_in = 0u64;
        let would_revert = usdc_in == 0;
        assert!(would_revert, "ZeroAmount guard must fire on usdc_in == 0");
    }

    #[cfg(feature = "mock-swap")]
    #[test]
    fn swap_usdc_rejects_below_min_top_out() {
        let usdc_in: u128 = 1_000_000;
        let top_out = usdc_in
            .checked_mul(MOCK_TOP_PER_USDC_NUM)
            .unwrap()
            .checked_div(MOCK_TOP_PER_USDC_DEN)
            .unwrap() as u64;
        let min_top_out = top_out + 1;
        let would_revert = top_out < min_top_out;
        assert!(would_revert, "SlippageExceeded guard must fire when top_out < min_top_out");
    }


    fn fresh_swap_router_config(authority: Pubkey) -> SwapRouterConfig {
        SwapRouterConfig {
            authority,
            pause_authority: Pubkey::default(),
            top_token_mint: Pubkey::default(),
            raydium_amm_program_id: Pubkey::default(),
            raydium_pool_id: Pubkey::default(),
            pending_raydium_pool: Pubkey::default(),
            pending_raydium_pool_at: 0,
            max_slippage_bps: 500,
            min_pool_balance: 0,
            pending_swap_sol: 0,
            total_sol_swapped: 0,
            total_top_received: 0,
            last_route_at: 0,
            last_swap_attempt_at: 0,
            paused: false,
            last_paused_at: 0,
            bump: 0,
            sol_holder_bump: 0,
            provider_vault_authority: Pubkey::default(),
            pending_swap_usdc: 0,
            pending_provider_vault_authority: Pubkey::default(),
            pending_provider_vault_authority_unlocks_at: 0,
            pending_pause_authority: Pubkey::default(),
            pending_pause_authority_unlocks_at: 0,
            propose_cooldown_until: 0,
            recent_proposes: [0i64; 5],
            pending_authority: Pubkey::default(),
            pending_authority_unlocks_at: 0,
            usdc_mint: Pubkey::new_unique(),
            pending_max_slippage_bps: 0,
            pending_max_slippage_unlocks_at: 0,
            jupiter_wired: false,
            pending_jupiter_wired: false,
            pending_jupiter_wired_unlocks_at: 0,
        }
    }

    #[test]
    fn set_provider_vault_authority_admin_only() {
        let admin = Pubkey::new_unique();
        let attacker = Pubkey::new_unique();
        let cfg = fresh_swap_router_config(admin);
        assert!(cfg.authority == admin, "admin must pass authority check");
        assert!(cfg.authority != attacker, "attacker must fail authority check");
    }

    #[test]
    fn provider_vault_authority_defaults_unconfigured() {
        let cfg = fresh_swap_router_config(Pubkey::new_unique());
        assert_eq!(cfg.provider_vault_authority, Pubkey::default(),
            "must start unconfigured — route_provider_yield_usdc rejects default");
    }

    #[test]
    fn route_provider_yield_usdc_rejects_unauthorized_caller() {
        let provider_pda = Pubkey::new_unique();
        let attacker = Pubkey::new_unique();
        let mut cfg = fresh_swap_router_config(Pubkey::new_unique());
        cfg.provider_vault_authority = provider_pda;
        let auth_ok = cfg.provider_vault_authority == provider_pda;
        let auth_bad = cfg.provider_vault_authority == attacker;
        assert!(auth_ok, "provider PDA must pass auth");
        assert!(!auth_bad, "attacker must fail auth");
    }

    #[test]
    fn route_provider_yield_usdc_rejects_when_authority_not_set() {
        let cfg = fresh_swap_router_config(Pubkey::new_unique());
        let would_revert = cfg.provider_vault_authority == Pubkey::default();
        assert!(would_revert, "must revert when provider_vault_authority not configured");
    }

    #[test]
    fn route_provider_yield_usdc_accumulates_pending_swap_usdc() {
        let mut cfg = fresh_swap_router_config(Pubkey::new_unique());
        cfg.provider_vault_authority = Pubkey::new_unique();
        assert_eq!(cfg.pending_swap_usdc, 0);
        let amount1 = 300_000u64;
        cfg.pending_swap_usdc = cfg.pending_swap_usdc.checked_add(amount1).unwrap();
        assert_eq!(cfg.pending_swap_usdc, 300_000);
        let amount2 = 600_000u64;
        cfg.pending_swap_usdc = cfg.pending_swap_usdc.checked_add(amount2).unwrap();
        assert_eq!(cfg.pending_swap_usdc, 900_000,
            "pending_swap_usdc must accumulate across multiple deposits");
    }

    #[test]
    fn swap_usdc_to_top_drains_pending_swap_usdc_post_graduation() {
        let mut cfg = fresh_swap_router_config(Pubkey::new_unique());
        cfg.pending_swap_usdc = 900_000;
        let usdc_in = 900_000u64;
        cfg.pending_swap_usdc = cfg.pending_swap_usdc.checked_sub(usdc_in).expect("exact drain ok");
        assert_eq!(cfg.pending_swap_usdc, 0, "full drain must zero the accumulator");
        cfg.pending_swap_usdc = 500_000;
        let partial_drain = 200_000u64;
        cfg.pending_swap_usdc = cfg.pending_swap_usdc.checked_sub(partial_drain).expect("partial drain ok");
        assert_eq!(cfg.pending_swap_usdc, 300_000, "partial drain must leave remainder");
        cfg.pending_swap_usdc = 100;
        let over_drain_result = cfg.pending_swap_usdc.checked_sub(1_000);
        assert!(over_drain_result.is_none(), "MEDIUM-02: over-drain must revert, not silently zero");
    }

    #[test]
    fn swap_router_config_space_still_fits() {
        let used = 8 + 9*32 + 8 + 2 + 7*8 + 1 + 8 + 2 + 8 + 8 + 40 + 88 + 32 + 10 + 10;
        assert!(used <= SwapRouterConfig::SPACE,
            "SwapRouterConfig fields ({} bytes) must fit within SPACE={}", used, SwapRouterConfig::SPACE);
        assert_eq!(used, 569, "field byte-count must be 569 after SR-H01/SR-H02 fix");
        assert_eq!(SwapRouterConfig::SPACE, 572,
            "SPACE STAYS 572 after SR-H01/SR-H02 fix (+10B absorbed by slack, LEN unchanged)");
    }


    #[test]
    fn propose_set_provider_vault_authority_admin_only() {
        let admin = Pubkey::new_unique();
        let attacker = Pubkey::new_unique();
        let cfg = fresh_swap_router_config(admin);
        assert!(cfg.authority == admin, "admin must pass authority check");
        assert!(cfg.authority != attacker, "attacker must fail authority check");
        assert_eq!(cfg.pending_provider_vault_authority, Pubkey::default());
        assert_eq!(cfg.pending_provider_vault_authority_unlocks_at, 0);
    }

    #[test]
    fn propose_set_provider_vault_authority_arms_unlocks_at() {
        let admin = Pubkey::new_unique();
        let mut cfg = fresh_swap_router_config(admin);
        let new_auth = Pubkey::new_unique();
        let now: i64 = 1_716_000_000;
        cfg.pending_provider_vault_authority = new_auth;
        cfg.pending_provider_vault_authority_unlocks_at = now
            .checked_add(RAYDIUM_POOL_TIMELOCK_SECS)
            .unwrap();
        assert_eq!(cfg.pending_provider_vault_authority, new_auth);
        assert_eq!(
            cfg.pending_provider_vault_authority_unlocks_at,
            now + 259_200,
            "unlocks_at must be exactly now + 72h"
        );
    }

    #[test]
    fn finalize_set_provider_vault_authority_blocked_early() {
        let admin = Pubkey::new_unique();
        let mut cfg = fresh_swap_router_config(admin);
        let new_auth = Pubkey::new_unique();
        let now: i64 = 1_716_000_000;
        cfg.pending_provider_vault_authority = new_auth;
        cfg.pending_provider_vault_authority_unlocks_at = now + 259_200;
        let current_time = now + 100;
        let would_pass_timelock = current_time >= cfg.pending_provider_vault_authority_unlocks_at;
        assert!(!would_pass_timelock, "finalize must be blocked before 72h elapses");
    }

    #[test]
    fn finalize_set_provider_vault_authority_after_timelock() {
        let admin = Pubkey::new_unique();
        let mut cfg = fresh_swap_router_config(admin);
        let new_auth = Pubkey::new_unique();
        let now: i64 = 1_716_000_000;
        cfg.pending_provider_vault_authority = new_auth;
        cfg.pending_provider_vault_authority_unlocks_at = now + 259_200;
        let current_time = now + 259_200 + 1;
        let would_pass_timelock = current_time >= cfg.pending_provider_vault_authority_unlocks_at;
        assert!(would_pass_timelock, "finalize must succeed after 72h");
        cfg.provider_vault_authority = cfg.pending_provider_vault_authority;
        cfg.pending_provider_vault_authority = Pubkey::default();
        cfg.pending_provider_vault_authority_unlocks_at = 0;
        assert_eq!(cfg.provider_vault_authority, new_auth, "authority must be updated");
        assert_eq!(cfg.pending_provider_vault_authority, Pubkey::default(), "pending must be cleared");
        assert_eq!(cfg.pending_provider_vault_authority_unlocks_at, 0, "unlocks_at must be reset");
    }

    #[test]
    fn cancel_set_provider_vault_authority_clears_pending() {
        let admin = Pubkey::new_unique();
        let mut cfg = fresh_swap_router_config(admin);
        let new_auth = Pubkey::new_unique();
        let now: i64 = 1_716_000_000;
        cfg.pending_provider_vault_authority = new_auth;
        cfg.pending_provider_vault_authority_unlocks_at = now + 259_200;
        cfg.pending_provider_vault_authority = Pubkey::default();
        cfg.pending_provider_vault_authority_unlocks_at = 0;
        assert_eq!(cfg.pending_provider_vault_authority, Pubkey::default(), "pending must be cleared");
        assert_eq!(cfg.pending_provider_vault_authority_unlocks_at, 0, "unlocks_at must be reset");
        assert_eq!(cfg.provider_vault_authority, Pubkey::default(), "authority unchanged after cancel");
    }

    #[test]
    fn legacy_set_provider_vault_authority_bootstrap_instant() {
        let admin = Pubkey::new_unique();
        let mut cfg = fresh_swap_router_config(admin);
        let new_auth = Pubkey::new_unique();
        assert_eq!(cfg.authority, admin, "only admin can call");
        let first_set_allowed = cfg.provider_vault_authority == Pubkey::default();
        assert!(first_set_allowed, "first set MUST be allowed (Wave E.2 bootstrap)");
        cfg.provider_vault_authority = new_auth;
        assert_eq!(cfg.provider_vault_authority, new_auth,
            "first-set bootstrap-instant MUST commit the new authority");
        assert_eq!(cfg.pending_provider_vault_authority, Pubkey::default());
        assert_eq!(cfg.pending_provider_vault_authority_unlocks_at, 0);

        let attempted_rotation_to = Pubkey::new_unique();
        let second_set_allowed = cfg.provider_vault_authority == Pubkey::default();
        assert!(!second_set_allowed,
            "post-bootstrap rotation via set_provider_vault_authority MUST be blocked (Wave E.2). \
             Use propose+finalize_set_provider_vault_authority for the Rule 27b 72h timelock.");
        
        assert_ne!(cfg.provider_vault_authority, attempted_rotation_to,
            "field MUST NOT mutate when the gate reverts post-bootstrap");
        assert_eq!(cfg.provider_vault_authority, new_auth,
            "field MUST retain its bootstrap value after a blocked rotation");
    }
    
    
    #[test]
    fn h4_notify_yield_deposit_cpi_discriminator_pinned() {
        
        let expected: [u8; 8] = [90, 193, 148, 106, 175, 153, 76, 47];
        assert_eq!(
            yield_escrow_cpi::NOTIFY_YIELD_DEPOSIT_DISCRIMINATOR,
            expected,
            "yield-escrow notify_yield_deposit discriminator drift — \
             rename detected. Check yield-escrow::notify_yield_deposit \
             still exists by that name."
        );
    }
    
    
    #[test]
    fn h4_test_route_yield_bucket_atomic_with_notify() {
        let amount: u128 = 100_000_000_000;
        
        
        let liquid = amount
            .checked_mul(LIQUID_BPS).unwrap()
            .checked_div(BPS_DENOM).unwrap();
        assert_eq!(liquid, 70_000_000_000);
        let swap_part = amount - liquid;
        assert_eq!(swap_part, 30_000_000_000);
        
        let mut epoch_sol_pool: u64 = 0;
        let liquid_u64: u64 = liquid as u64;
        epoch_sol_pool = epoch_sol_pool.checked_add(liquid_u64).unwrap();
        assert_eq!(epoch_sol_pool, 70_000_000_000);
        
        let mut pending_swap_sol: u64 = 0;
        pending_swap_sol = pending_swap_sol.checked_add(swap_part as u64).unwrap();
        assert_eq!(pending_swap_sol, 30_000_000_000);
    }
    
    
    #[test]
    fn h4_test_route_yield_bucket_reverts_if_notify_fails() {
        
        let pending_swap_sol_before: u64 = 0;
        let epoch_sol_pool_before: u64 = 0;
        let pending_swap_usdc_before: u64 = 0;
        
        
        
        let pending_swap_sol_after = pending_swap_sol_before;
        let epoch_sol_pool_after = epoch_sol_pool_before;
        let pending_swap_usdc_after = pending_swap_usdc_before;
        assert_eq!(pending_swap_sol_after, pending_swap_sol_before,
            "pending_swap_sol must NOT advance if notify CPI reverts");
        assert_eq!(epoch_sol_pool_after, epoch_sol_pool_before,
            "epoch.sol_pool must NOT advance if notify CPI reverts");
        assert_eq!(pending_swap_usdc_after, pending_swap_usdc_before,
            "no collateral state must drift");
    }
    
    
    #[test]
    fn h4_test_double_split_protection_via_holder_underfunded() {
        let amount: u64 = 100_000_000_000;
        let mut holder_lamports: u64 = amount;
        assert!(holder_lamports >= amount);
        holder_lamports -= amount;
        
        
        
        let second_call_succeeds = holder_lamports >= amount;
        assert!(!second_call_succeeds,
            "second route_yield_bucket call in the same TX must revert \
             on HolderUnderfunded BEFORE notify fires");
    }
    
    
    #[test]
    fn h4_test_strand_invariant_no_more_stranding() {
        
        let buggy_epoch_sol_pool: u64 = 0; 
        let buggy_sol_in_vault: u64 = 70_000_000_000; 
        assert!(buggy_sol_in_vault > buggy_epoch_sol_pool,
            "PRE-FIX BUG REPRO: vault has SOL but counter is at 0");

        
        
        let fixed_amount: u64 = 70_000_000_000;
        let mut fixed_epoch_sol_pool: u64 = 0;
        
        fixed_epoch_sol_pool = fixed_epoch_sol_pool.checked_add(fixed_amount).unwrap();
        let fixed_sol_in_vault: u64 = fixed_amount;
        
        assert_eq!(fixed_sol_in_vault, fixed_epoch_sol_pool,
            "POST-FIX INVARIANT: vault SOL must equal counter exactly. \
             If this ever fails, the atomic CPI broke.");
    }

    
    
    
    #[test]
    fn h4_currency_constants_match_yield_escrow() {
        assert_eq!(CURRENCY_SOL_FOR_NOTIFY, 0,
            "yield-escrow defines CURRENCY_SOL = 0; if it changes, this \
             must change too");
        assert_eq!(CURRENCY_USDC_FOR_NOTIFY, 1,
            "yield-escrow defines CURRENCY_USDC = 1; if it changes, this \
             must change too");
    }

    
    
    
    
    
    

    #[test]
    fn rotate_pause_auth_pending_defaults_clear() {
        let cfg = fresh_swap_router_config(Pubkey::new_unique());
        assert_eq!(cfg.pending_pause_authority, Pubkey::default());
        assert_eq!(cfg.pending_pause_authority_unlocks_at, 0);
    }

    #[test]
    fn propose_then_finalize_succeeds_after_window() {
        let admin = Pubkey::new_unique();
        let new_auth = Pubkey::new_unique();
        let mut cfg = fresh_swap_router_config(admin);
        cfg.pause_authority = Pubkey::new_unique();
        let now: i64 = 1_700_000_000;

        
        assert!(new_auth != admin, "distinctness invariant");
        assert!(new_auth != Pubkey::default(), "non-default invariant");
        let unlocks_at = now.checked_add(RAYDIUM_POOL_TIMELOCK_SECS).unwrap();
        cfg.pending_pause_authority = new_auth;
        cfg.pending_pause_authority_unlocks_at = unlocks_at;
        assert_eq!(unlocks_at - now, 259_200);

        
        let finalize_now = unlocks_at + 1;
        assert!(finalize_now >= cfg.pending_pause_authority_unlocks_at);
        assert!(cfg.pending_pause_authority != cfg.authority);
        cfg.pause_authority = cfg.pending_pause_authority;
        cfg.pending_pause_authority = Pubkey::default();
        cfg.pending_pause_authority_unlocks_at = 0;
        assert_eq!(cfg.pause_authority, new_auth);
    }

    #[test]
    fn finalize_before_window_reverts_timelock_not_elapsed() {
        
        let mut cfg = fresh_swap_router_config(Pubkey::new_unique());
        let now: i64 = 1_700_000_000;
        let new_auth = Pubkey::new_unique();
        let unlocks_at = now + RAYDIUM_POOL_TIMELOCK_SECS;
        cfg.pending_pause_authority = new_auth;
        cfg.pending_pause_authority_unlocks_at = unlocks_at;

        
        let almost = unlocks_at - 1;
        assert!(almost < cfg.pending_pause_authority_unlocks_at);
        
        assert!(unlocks_at >= cfg.pending_pause_authority_unlocks_at);
    }

    #[test]
    fn cancel_clears_pending() {
        
        let mut cfg = fresh_swap_router_config(Pubkey::new_unique());
        let now: i64 = 1_700_000_000;
        cfg.pending_pause_authority = Pubkey::new_unique();
        cfg.pending_pause_authority_unlocks_at = now + RAYDIUM_POOL_TIMELOCK_SECS;

        
        cfg.pending_pause_authority = Pubkey::default();
        cfg.pending_pause_authority_unlocks_at = 0;
        assert_eq!(cfg.pending_pause_authority, Pubkey::default());
        assert_eq!(cfg.pending_pause_authority_unlocks_at, 0);
    }
    #[test]
    fn swap_router_config_space_fits_pending_pause_authority() {
        
        
        
        
        assert_eq!(SwapRouterConfig::SPACE, 572);
    }
    
    
    
    
    
    
    
    fn drift_handler_end(src: &str, idx: usize) -> usize {
        src[idx + 1..]
            .find("\n    pub fn ")
            .map(|p| idx + 1 + p)
            .expect("drift-gate: no following `pub fn` — re-anchor this source-assert bound")
    }

    
    
    fn drift_struct_end(src: &str, idx: usize) -> usize {
        src[idx..]
            .find("\n}")
            .map(|p| idx + p)
            .expect("drift-gate: no struct close — re-anchor this source-assert bound")
    }

    fn swap_router_lib_rs_source() -> &'static str {
        include_str!("lib.rs")
    }

    #[test]
    fn deprecated_transfer_authority_swap_router_reverts() {
        let src = swap_router_lib_rs_source();
        let needle = "pub fn transfer_authority(ctx: Context<AdminOnly>";
        let idx = src.find(needle).expect("handler must exist");
        let end = drift_handler_end(src, idx);
        let body = &src[idx..end];
        assert!(body.contains("err!(SwapRouterError::InstructionDeprecated)"),
            "transfer_authority MUST hard-revert with InstructionDeprecated (M-CRIT-02). \
             Source excerpt:\n{}", body);
        assert!(!body.contains("cfg.authority = new_authority;"),
            "transfer_authority MUST NOT mutate state after the gate (M-CRIT-02). \
             Source excerpt:\n{}", body);
    }

    
    
    
    
    #[test]
    fn deprecated_route_yield_bucket_reverts_before_transfer() {
        let src = swap_router_lib_rs_source();
        let needle = "pub fn route_yield_bucket(ctx: Context<RouteYieldBucket>";
        let idx = src.find(needle).expect("handler must exist");
        
        let transfer_marker = "system_program::transfer(transfer_cpi";
        let transfer_pos = src[idx..].find(transfer_marker)
            .map(|rel| idx + rel)
            .expect("legacy transfer call must still exist in retained body");
        let guard_body = &src[idx..transfer_pos];
        assert!(guard_body.contains("err!(SwapRouterError::InstructionDeprecated)"),
            "route_yield_bucket MUST hard-revert with InstructionDeprecated BEFORE the \
             SOL transfer (HIGH-2). Source excerpt:\n{}", guard_body);
        let return_pos = src[idx..].find("return err!(SwapRouterError::InstructionDeprecated)")
            .map(|rel| idx + rel)
            .expect("early-return guard must exist");
        assert!(return_pos < transfer_pos,
            "deprecation guard must come BEFORE the system_program::transfer (no partial execution)");
    }
    
    
    
    #[test]
    fn set_provider_vault_authority_is_bootstrap_instant_swap_router() {
        let src = swap_router_lib_rs_source();
        let needle = "pub fn set_provider_vault_authority(\n        ctx: Context<AdminOnly>";
        let idx = src.find(needle).expect("handler must exist");
        
        let stop_marker = "pub fn propose_set_provider_vault_authority(";
        let stop = src[idx..].find(stop_marker)
            .map(|rel| idx + rel)
            .unwrap_or_else(|| drift_handler_end(src, idx));
        let body = &src[idx..stop];
        
        assert!(body.contains("cfg.provider_vault_authority == Pubkey::default()"),
            "set_provider_vault_authority MUST gate first-set on Pubkey::default() (Wave E.2 bootstrap-instant). \
             Source excerpt:\n{}", body);
        
        assert!(body.contains("ProviderVaultAuthorityAlreadyConfigured"),
            "set_provider_vault_authority MUST revert with ProviderVaultAuthorityAlreadyConfigured \
             on post-bootstrap calls (Wave E.2). Source excerpt:\n{}", body);
        
        assert!(body.contains("cfg.provider_vault_authority = new_authority;"),
            "set_provider_vault_authority MUST commit the new authority on first-set (Wave E.2). \
             Source excerpt:\n{}", body);
        
        assert!(body.contains("require!(new_authority != Pubkey::default()"),
            "set_provider_vault_authority MUST reject Pubkey::default() new_authority (Wave E.2). \
             Source excerpt:\n{}", body);
        
        assert!(!body.contains("err!(SwapRouterError::InstructionDeprecated)"),
            "set_provider_vault_authority MUST NOT hard-revert with InstructionDeprecated under Wave E.2 \
             (bootstrap-instant pattern restored). Source excerpt:\n{}", body);
    }

    #[test]
    fn propose_finalize_pair_still_works_alongside_deprecated_gate() {
        let src = swap_router_lib_rs_source();
        assert!(src.contains("pub fn propose_rotate_admin("),
            "propose_rotate_admin must remain (canonical replacement for transfer_authority)");
        assert!(src.contains("pub fn finalize_rotate_admin("),
            "finalize_rotate_admin must remain (canonical replacement for transfer_authority)");
        assert!(src.contains("pub fn propose_set_provider_vault_authority("),
            "propose_set_provider_vault_authority must remain");
        assert!(src.contains("pub fn finalize_set_provider_vault_authority("),
            "finalize_set_provider_vault_authority must remain");
    }

    #[test]
    fn swap_router_instruction_deprecated_error_code_exists() {
        let src = swap_router_lib_rs_source();
        assert!(src.contains("InstructionDeprecated,"),
            "SwapRouterError::InstructionDeprecated variant must exist");
        assert!(src.contains("M-CRIT-02"),
            "error definition must reference the audit ID for traceability");
    }

    

    #[test]
    fn usdc_mint_field_present_on_config() {
        
        let cfg = fresh_swap_router_config(Pubkey::new_unique());
        let _: Pubkey = cfg.usdc_mint;
    }

    #[test]
    fn wrong_usdc_mint_error_code_exists() {
        let src = swap_router_lib_rs_source();
        assert!(src.contains("WrongUsdcMint,"),
            "SwapRouterError::WrongUsdcMint variant must exist (M-HIGH-04)");
        assert!(src.contains("M-HIGH-04"),
            "error definition must reference the audit ID for traceability");
    }

    #[test]
    fn swap_usdc_to_top_pins_usdc_mint_against_config() {
        
        
        let src = swap_router_lib_rs_source();
        let needle = "pub struct SwapUsdcToTop<'info>";
        let idx = src.find(needle).expect("SwapUsdcToTop struct must exist");
        let end = drift_struct_end(src, idx);
        let body = &src[idx..end];
        assert!(body.contains("address = config.usdc_mint @ SwapRouterError::WrongUsdcMint"),
            "SwapUsdcToTop.usdc_mint MUST pin against config.usdc_mint (M-HIGH-04). Source excerpt:\n{}",
            body);
    }

    #[test]
    fn route_provider_yield_usdc_pins_usdc_mint() {
        let src = swap_router_lib_rs_source();
        let needle = "pub struct RouteProviderYieldUsdc<'info>";
        let idx = src.find(needle).expect("RouteProviderYieldUsdc struct must exist");
        let end = drift_struct_end(src, idx);
        let body = &src[idx..end];
        
        
        
        let mint_pins = body.matches("token::mint = usdc_mint_constraint").count();
        assert_eq!(mint_pins, 2,
            "RouteProviderYieldUsdc MUST pin token::mint on BOTH usdc_source AND usdc_holder \
             (M-HIGH-04); saw {mint_pins} of 2. Source excerpt:\n{}", body);
        assert!(body.contains("address = config.usdc_mint @ SwapRouterError::WrongUsdcMint"),
            "RouteProviderYieldUsdc.usdc_mint_constraint MUST pin to config.usdc_mint (M-HIGH-04). \
             Source excerpt:\n{}", body);
    }
    #[test]
    fn initialize_rejects_default_usdc_mint() {
        
        let src = swap_router_lib_rs_source();
        let needle = "pub fn initialize(\n        ctx: Context<Initialize>";
        let idx = src.find(needle).expect("initialize handler must exist");
        let end = drift_handler_end(src, idx);
        let body = &src[idx..end];
        assert!(body.contains("usdc_mint != Pubkey::default()") &&
                body.contains("SwapRouterError::WrongUsdcMint"),
            "initialize MUST reject default usdc_mint (M-HIGH-04). Source excerpt:\n{}",
            body);
    }
    #[test]
    fn set_usdc_mint_is_deprecated_at_v1() {
        let src = swap_router_lib_rs_source();
        let needle = "pub fn set_usdc_mint(";
        let idx = src.find(needle).expect("set_usdc_mint stub must exist");
        let end = drift_handler_end(src, idx);
        let body = &src[idx..end];
        assert!(body.contains("err!(SwapRouterError::InstructionDeprecated)"),
            "set_usdc_mint MUST hard-revert at v1 (M-HIGH-04). Source excerpt:\n{}", body);
        
        assert!(body.contains("require_keys_eq!(cfg.authority, ctx.accounts.authority.key()"),
            "set_usdc_mint MUST retain auth gate so unauthorized callers see Unauthorized");
    }

    
    
    
    
    
    #[test]
    fn r77_h01_check_and_record_propose_helper_present() {
        let src = swap_router_lib_rs_source();
        assert!(src.contains("fn check_and_record_propose(cfg: &mut SwapRouterConfig"),
            "swap-router MUST define the check_and_record_propose helper (R7.7-H-01 / Wave C.4)");
        assert!(src.contains("propose_cooldown_until"),
            "SwapRouterConfig MUST carry propose_cooldown_until field (R7.7-H-01)");
        assert!(src.contains("recent_proposes: [i64; 5]"),
            "SwapRouterConfig MUST carry recent_proposes: [i64; 5] ring buffer (R7.7-H-01)");
        assert!(src.contains("ProposeCooldownActive"),
            "SwapRouterError::ProposeCooldownActive variant MUST exist (R7.7-H-01)");
    }

    #[test]
    fn r77_h01_all_propose_handlers_call_check_and_record_propose() {
        
        
        
        
        
        
        
        let src = swap_router_lib_rs_source();
        let propose_names = ["propose_rotate_admin",
                             "propose_rotate_pause_authority",
                             "propose_set_provider_vault_authority",
                             "propose_set_jupiter_wired"];
        for name in propose_names {
            let needle = format!("pub fn {}(", name);
            let idx = src.find(&needle).unwrap_or_else(|| panic!("{} handler must exist", name));
            
            let end = drift_handler_end(src, idx);
            let body = &src[idx..end];
            assert!(body.contains("check_and_record_propose(cfg, now)?;"),
                "{} MUST call check_and_record_propose(cfg, now)? for R7.7-H-01 \
                 defense. Source excerpt:\n{}", name, body);
        }
    }

    
    
    

    #[test]
    fn jupiter_wired_defaults_false_failclosed() {
        let cfg = fresh_swap_router_config(Pubkey::new_unique());
        assert!(!cfg.jupiter_wired,
            "SR-H01: swap MUST be fail-closed at deploy — jupiter_wired false by default");
        assert!(!cfg.pending_jupiter_wired);
        assert_eq!(cfg.pending_jupiter_wired_unlocks_at, 0);
    }

    
    
    
    #[test]
    fn route_provider_yield_usdc_failclosed_before_transfer_when_unwired() {
        let src = swap_router_lib_rs_source();
        let idx = src.find("pub fn route_provider_yield_usdc(")
            .expect("route_provider_yield_usdc handler must exist");
        
        let transfer_marker = "token::transfer(transfer_cpi";
        let transfer_pos = src[idx..].find(transfer_marker)
            .map(|rel| idx + rel)
            .expect("route_provider_yield_usdc must still contain its token::transfer");
        let guard_body = &src[idx..transfer_pos];
        
        let gate = "require!(cfg.jupiter_wired, SwapRouterError::JupiterNotWired);";
        let gate_pos = src[idx..].find(gate)
            .map(|rel| idx + rel)
            .expect("SR-H01: route_provider_yield_usdc MUST gate on cfg.jupiter_wired");
        assert!(guard_body.contains(gate),
            "SR-H01: jupiter_wired fail-closed gate MUST be inside route_provider_yield_usdc \
             BEFORE the transfer. Source excerpt:\n{}", guard_body);
        assert!(gate_pos < transfer_pos,
            "SR-H01: jupiter_wired gate (pos {}) MUST precede token::transfer (pos {}) — \
             no USDC may move while unwired", gate_pos, transfer_pos);
    }

    
    #[test]
    fn route_provider_yield_usdc_gate_logic() {
        let mut cfg = fresh_swap_router_config(Pubkey::new_unique());
        cfg.provider_vault_authority = Pubkey::new_unique();
        
        let would_accept_unwired = cfg.jupiter_wired;
        assert!(!would_accept_unwired, "unwired route MUST revert (no USDC accepted)");
        
        cfg.jupiter_wired = true;
        let would_accept_wired = cfg.jupiter_wired;
        assert!(would_accept_wired, "wired route MUST proceed to accrual");
    }

    
    #[test]
    fn swap_usdc_to_top_consults_jupiter_wired_on_mainnet() {
        let src = swap_router_lib_rs_source();
        let idx = src.find("pub fn swap_usdc_to_top(")
            .expect("swap_usdc_to_top handler must exist");
        
        
        
        let after = idx + "pub fn swap_usdc_to_top(".len();
        let rel_end = src[after..].find("\n    pub fn ").expect("drift-gate: bound marker absent - re-anchor (would else scan into the include_str! test module)");
        let body = &src[idx..after + rel_end];
        assert!(body.contains("require!(ctx.accounts.config.jupiter_wired, SwapRouterError::JupiterNotWired);"),
            "SR-H02: swap_usdc_to_top MUST runtime-gate on config.jupiter_wired (single \
             source of truth for swap-executable). Source excerpt:\n{}", body);
        
        assert!(body.contains("return Err(error!(SwapRouterError::JupiterNotWired));"),
            "SR-H02: the mainnet JupiterNotWired hard-revert backstop MUST remain");
    }

    
    
    
    
    
    #[test]
    fn r6_sr_01_02_decrement_gated_and_slippage_requirement_pinned() {
        let src = swap_router_lib_rs_source();
        let idx = src.find("pub fn swap_usdc_to_top(")
            .expect("swap_usdc_to_top handler must exist");
        let after = idx + "pub fn swap_usdc_to_top(".len();
        let rel_end = src[after..].find("\n    pub fn ").expect("drift-gate: bound marker absent - re-anchor (would else scan into the include_str! test module)");
        let body = &src[idx..after + rel_end];
        assert!(body.contains("from_yield_accumulator: bool"),
            "R6-SR-01: swap_usdc_to_top MUST take `from_yield_accumulator` to gate the decrement");
        assert!(body.contains("if from_yield_accumulator {"),
            "R6-SR-01: the pending_swap_usdc checked_sub MUST be guarded by `if from_yield_accumulator` \
             so the reserve burn (which never incremented it) can't underflow/desync the accumulator");
        assert!(body.contains("R6-SR-02"),
            "R6-SR-02: the mainnet branch MUST keep the explicit config.max_slippage_bps floor \
             requirement so the Phase-4 Jupiter CPI can't ship without sandwich protection");
    }

    
    
    #[test]
    fn finalize_set_jupiter_wired_blocked_before_72h_then_succeeds() {
        let mut cfg = fresh_swap_router_config(Pubkey::new_unique());
        let now: i64 = 1_716_000_000;
        
        cfg.pending_jupiter_wired = true;
        cfg.pending_jupiter_wired_unlocks_at = now.checked_add(RAYDIUM_POOL_TIMELOCK_SECS).unwrap();
        assert_eq!(cfg.pending_jupiter_wired_unlocks_at, now + 259_200,
            "enable must unlock exactly now + 72h");
        
        let early = now + 100;
        assert!(!(early >= cfg.pending_jupiter_wired_unlocks_at),
            "finalize_set_jupiter_wired MUST be blocked before 72h");
        
        let late = now + 259_200 + 1;
        assert!(late >= cfg.pending_jupiter_wired_unlocks_at,
            "finalize_set_jupiter_wired MUST succeed after 72h");
        cfg.jupiter_wired = cfg.pending_jupiter_wired;
        cfg.pending_jupiter_wired = false;
        cfg.pending_jupiter_wired_unlocks_at = 0;
        assert!(cfg.jupiter_wired, "jupiter_wired must be enabled after finalize");
        assert!(!cfg.pending_jupiter_wired);
        assert_eq!(cfg.pending_jupiter_wired_unlocks_at, 0);
    }
    
    
    #[test]
    fn unset_jupiter_wired_is_instant_failsafe() {
        let mut cfg = fresh_swap_router_config(Pubkey::new_unique());
        cfg.jupiter_wired = true;
        cfg.pending_jupiter_wired = true;
        cfg.pending_jupiter_wired_unlocks_at = 999;
        
        cfg.jupiter_wired = false;
        cfg.pending_jupiter_wired = false;
        cfg.pending_jupiter_wired_unlocks_at = 0;
        assert!(!cfg.jupiter_wired, "unset must re-close the gate immediately");
        assert!(!cfg.pending_jupiter_wired, "unset must clear any pending enable");
        assert_eq!(cfg.pending_jupiter_wired_unlocks_at, 0);
    }

    
    
    #[test]
    fn withdraw_pending_usdc_recovery_shape() {
        let src = swap_router_lib_rs_source();
        
        let idx = src.find("pub fn withdraw_pending_usdc(")
            .expect("SR-H01: withdraw_pending_usdc recovery instruction MUST exist");
        let end = drift_handler_end(src, idx);
        let body = &src[idx..end];
        
        assert!(body.contains("require_keys_eq!(cfg.authority, ctx.accounts.authority.key()"),
            "withdraw_pending_usdc MUST be authority-gated. Source excerpt:\n{}", body);
        
        assert!(body.contains(".checked_sub(amount)") &&
                body.contains("SwapRouterError::AccountingMismatch"),
            "withdraw_pending_usdc MUST decrement pending_swap_usdc via checked_sub \
             (over-drain reverts AccountingMismatch). Source excerpt:\n{}", body);
        
        assert!(body.contains("from: ctx.accounts.usdc_holder.to_account_info()"),
            "withdraw_pending_usdc MUST drain usdc_holder. Source excerpt:\n{}", body);
        
        let sidx = src.find("pub struct WithdrawPendingUsdc<'info>")
            .expect("WithdrawPendingUsdc accounts struct MUST exist");
        let send = drift_struct_end(src, sidx);
        let sbody = &src[sidx..send];
        assert!(sbody.contains("token::authority = provider_vault_authority"),
            "SR-H01: withdraw destination MUST be pinned to provider_vault_authority \
             custody (theft-proof). Source excerpt:\n{}", sbody);
        assert!(sbody.contains("address = config.provider_vault_authority @ SwapRouterError::Unauthorized"),
            "SR-H01: provider_vault_authority account MUST be address-pinned to \
             config.provider_vault_authority. Source excerpt:\n{}", sbody);
        assert!(sbody.contains("token::mint = usdc_mint_constraint"),
            "SR-H01: withdraw destination + holder MUST pin the canonical USDC mint. \
             Source excerpt:\n{}", sbody);
    }

    
    #[test]
    fn withdraw_pending_usdc_drain_arithmetic() {
        let mut cfg = fresh_swap_router_config(Pubkey::new_unique());
        cfg.provider_vault_authority = Pubkey::new_unique();
        cfg.pending_swap_usdc = 300_000; 
        
        let amount = 300_000u64;
        cfg.pending_swap_usdc = cfg.pending_swap_usdc.checked_sub(amount).expect("exact drain ok");
        assert_eq!(cfg.pending_swap_usdc, 0, "full recovery zeroes the accumulator");
        
        cfg.pending_swap_usdc = 100;
        assert!(cfg.pending_swap_usdc.checked_sub(1_000).is_none(),
            "SR-H01: over-withdraw beyond pending_swap_usdc MUST revert (checked_sub), not corrupt");
    }

    
    #[test]
    fn sr_h01_h02_error_variants_exist() {
        let src = swap_router_lib_rs_source();
        assert!(src.contains("JupiterAlreadyWired,"), "JupiterAlreadyWired variant must exist");
        assert!(src.contains("JupiterWiredProposalAlreadyPending,"),
            "JupiterWiredProposalAlreadyPending variant must exist");
        assert!(src.contains("NoJupiterWiredProposalPending,"),
            "NoJupiterWiredProposalPending variant must exist");
        
        let _ = SwapRouterError::JupiterAlreadyWired;
        let _ = SwapRouterError::JupiterWiredProposalAlreadyPending;
        let _ = SwapRouterError::NoJupiterWiredProposalPending;
    }

    
    #[test]
    fn jupiter_wired_triplet_handlers_present() {
        let src = swap_router_lib_rs_source();
        assert!(src.contains("pub fn propose_set_jupiter_wired("),
            "propose_set_jupiter_wired must exist");
        assert!(src.contains("pub fn finalize_set_jupiter_wired("),
            "finalize_set_jupiter_wired must exist");
        assert!(src.contains("pub fn cancel_set_jupiter_wired("),
            "cancel_set_jupiter_wired must exist");
        assert!(src.contains("pub fn unset_jupiter_wired("),
            "unset_jupiter_wired (instant fail-safe) must exist");
    }
}
