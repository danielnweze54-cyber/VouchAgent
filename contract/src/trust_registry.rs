//! Vouch 信任层核心合约：TrustRegistry + HireEscrow。
//!
//! 一个合约承载两个系统，共享同一信誉账本：
//! - **查询/信誉系统**：Provider Agent 注册并质押（带锁定期）→ 提交可验证 claim
//!   → 验证网络记录 verdict → 链上累积 per-agent 信誉分。
//! - **雇佣托管系统**：Consumer 按「天数×单价」托管资金 → 验证网络按客观 SLA 判履约
//!   → 达标按里程碑放款给 Provider（平台抽佣金）/ 不达标退款 Consumer 并罚没 Provider 押金。
//!
//! 资金全部由合约托管（CEP-18），跨合约 transfer_from/transfer 已在 escrow PoC 链上验证兼容。

use crate::escrow_poc::Cep18TokenContractRef;
use odra::casper_types::U256;
use odra::prelude::*;
use odra::ContractRef;

/// 新 agent 注册时的初始信誉分。
const INITIAL_REPUTATION: u32 = 50;
/// 信誉分上限。
const MAX_REPUTATION: u32 = 1000;
/// 一天的毫秒数（区块时间戳单位为毫秒）。
const DAY_MS: u64 = 86_400_000;
/// 万分比基数。
const BPS_DENOM: u32 = 10_000;

/// Provider Agent 档案。
#[odra::odra_type]
pub struct AgentProfile {
    /// 运营者地址（放款收款方）。
    pub owner: Address,
    /// 元数据哈希（名称/描述/端点，详情链下）。
    pub metadata_hash: String,
    /// 当前押金余额（X402 最小单位）。
    pub stake: U256,
    /// 押金原付款地址，到期退回这里。
    pub stake_payer: Address,
    /// 入驻到期时间戳（毫秒）；到期且无未决可释放押金。
    pub lock_until: u64,
    /// 自报单价（每天）。
    pub price_per_day: U256,
    /// 当前信誉分 [0, MAX]。
    pub reputation: U256,
    /// 累计提交 claim 数。
    pub claims_count: u64,
    /// 累计被雇佣次数。
    pub hires_count: u64,
    /// 被罚没次数。
    pub slashed_count: u64,
    /// 0 active / 1 paused / 2 banned / 3 expired。
    pub status: u8,
}

/// 一条可验证断言及其判决结果。
#[odra::odra_type]
pub struct Claim {
    pub agent: u64,
    pub topic: String,
    pub value: U256,
    /// Provider 自报置信度。
    pub confidence: u8,
    pub source_count: u8,
    pub payload_hash: String,
    pub timestamp: u64,
    /// 0 pending / 1 accurate / 2 inaccurate。
    pub verdict_status: u8,
    /// 对抗式投票分布（上链 = 可视化分歧的证据）。
    pub votes_for: u8,
    pub votes_against: u8,
    /// 验证网络综合置信度。
    pub verdict_confidence: u8,
}

/// 一张雇佣单（托管 + SLA + 结算状态）。
#[odra::odra_type]
pub struct Hire {
    pub consumer: Address,
    pub provider: u64,
    pub price_per_day: U256,
    pub total: U256,
    /// 当前仍托管在合约的余额。
    pub escrow: U256,
    /// 已结算给 Provider（含佣金）的累计额。
    pub settled: U256,
    pub sla_hash: String,
    pub milestones_total: u32,
    pub milestones_passed: u32,
    /// 0 active / 1 settled / 2 refunded。
    pub status: u8,
    pub created_at: u64,
    pub ends_at: u64,
}

/// 合约错误码。
#[odra::odra_error]
pub enum Error {
    NotOwner = 1,
    NotVerifier = 2,
    NotInitialized = 3,
    AgentNotFound = 4,
    ClaimNotFound = 5,
    HireNotFound = 6,
    InvalidConfidence = 7,
    InvalidParams = 8,
    StillLocked = 9,
    HireExceedsLock = 10,
    StakeRemaining = 11,
    HireNotActive = 12,
    NothingToSettle = 13,
}

#[odra::event]
pub struct AgentRegistered {
    pub agent_id: u64,
    pub owner: Address,
    pub stake: U256,
    pub lock_until: u64,
}

#[odra::event]
pub struct ClaimSubmitted {
    pub claim_id: u64,
    pub agent: u64,
    pub topic: String,
}

#[odra::event]
pub struct VerdictRecorded {
    pub claim_id: u64,
    pub accurate: bool,
    pub votes_for: u8,
    pub votes_against: u8,
}

#[odra::event]
pub struct HireCreated {
    pub hire_id: u64,
    pub consumer: Address,
    pub provider: u64,
    pub total: U256,
}

#[odra::event]
pub struct HireSettled {
    pub hire_id: u64,
    pub provider_paid: U256,
    pub commission: U256,
}

#[odra::event]
pub struct HireRefunded {
    pub hire_id: u64,
    pub refunded: U256,
    pub slashed: U256,
}

#[odra::event]
pub struct ReputationUpdated {
    pub agent: u64,
    pub new_score: U256,
}

#[odra::event]
pub struct StakeReleased {
    pub agent: u64,
    pub amount: U256,
}

#[odra::module(
    events = [AgentRegistered, ClaimSubmitted, VerdictRecorded, HireCreated, HireSettled, HireRefunded, ReputationUpdated, StakeReleased],
    errors = Error
)]
pub struct TrustRegistry {
    owner: Var<Address>,
    /// 质押 / 托管 / 佣金统一用的 CEP-18 代币。
    payment_token: Var<Address>,
    /// 平台佣金（万分比），如 1000 = 10%。
    commission_bps: Var<u32>,
    /// 罚没系数（万分比），如 10000 = 1.0（罚没=未交付金额）。
    slash_factor_bps: Var<u32>,
    /// 授权的验证网络地址。
    verifiers: Mapping<Address, bool>,
    agents: Mapping<u64, AgentProfile>,
    agent_count: Var<u64>,
    claims: Mapping<u64, Claim>,
    claim_count: Var<u64>,
    hires: Mapping<u64, Hire>,
    hire_count: Var<u64>,
}

#[odra::module]
impl TrustRegistry {
    /// 初始化：调用者成为 owner；绑定支付代币、首个验证网络地址、佣金率。
    pub fn init(&mut self, payment_token: Address, verifier: Address, commission_bps: u32) {
        self.owner.set(self.env().caller());
        self.payment_token.set(payment_token);
        self.commission_bps.set(commission_bps);
        self.slash_factor_bps.set(BPS_DENOM); // 默认 1.0
        self.verifiers.set(&verifier, true);
        self.agent_count.set(0);
        self.claim_count.set(0);
        self.hire_count.set(0);
    }

    // ---------- 治理 ----------

    pub fn set_verifier(&mut self, verifier: Address, enabled: bool) {
        self.assert_owner();
        self.verifiers.set(&verifier, enabled);
    }

    pub fn set_commission(&mut self, commission_bps: u32) {
        self.assert_owner();
        self.commission_bps.set(commission_bps);
    }

    pub fn set_slash_factor(&mut self, slash_factor_bps: u32) {
        self.assert_owner();
        self.slash_factor_bps.set(slash_factor_bps);
    }

    // ---------- Provider 生命周期 ----------

    /// 注册 Provider 并质押押金（锁定 lock_days 天）。
    /// 调用者须先对本合约 approve 足额 CEP-18。返回新 agent id。
    pub fn register_agent(
        &mut self,
        metadata_hash: String,
        price_per_day: U256,
        stake_amount: U256,
        lock_days: u32,
    ) -> u64 {
        if lock_days == 0 || stake_amount.is_zero() {
            self.env().revert(Error::InvalidParams);
        }
        let caller = self.env().caller();
        self.pull(&caller, stake_amount);

        let now = self.env().get_block_time();
        let lock_until = now.saturating_add((lock_days as u64).saturating_mul(DAY_MS));
        let id = self.agent_count.get_or_default();
        self.agents.set(
            &id,
            AgentProfile {
                owner: caller,
                metadata_hash,
                stake: stake_amount,
                stake_payer: caller,
                lock_until,
                price_per_day,
                reputation: U256::from(INITIAL_REPUTATION),
                claims_count: 0,
                hires_count: 0,
                slashed_count: 0,
                status: 0,
            },
        );
        self.agent_count.set(id + 1);
        self.env().emit_event(AgentRegistered {
            agent_id: id,
            owner: caller,
            stake: stake_amount,
            lock_until,
        });
        id
    }

    pub fn set_price(&mut self, agent_id: u64, price_per_day: U256) {
        let mut a = self.load_agent(agent_id);
        self.assert_agent_owner(&a);
        a.price_per_day = price_per_day;
        self.agents.set(&agent_id, a);
    }

    /// 补缴押金。
    pub fn top_up_stake(&mut self, agent_id: u64, amount: U256) {
        let mut a = self.load_agent(agent_id);
        let caller = self.env().caller();
        self.pull(&caller, amount);
        a.stake += amount;
        // 注意：不在此自动解除 paused，避免第三方补缴绕过暂停；恢复由 owner 显式操作。
        self.agents.set(&agent_id, a);
    }

    /// 续期：延长锁定期并追加押金。
    pub fn renew_registration(&mut self, agent_id: u64, extra_days: u32, extra_stake: U256) {
        let mut a = self.load_agent(agent_id);
        let caller = self.env().caller();
        if !extra_stake.is_zero() {
            self.pull(&caller, extra_stake);
            a.stake += extra_stake;
        }
        let now = self.env().get_block_time();
        let base = if a.lock_until > now { a.lock_until } else { now };
        a.lock_until = base.saturating_add((extra_days as u64).saturating_mul(DAY_MS));
        if a.status == 3 {
            a.status = 0;
        }
        self.agents.set(&agent_id, a);
    }

    /// 到期返还剩余押金到原付款地址（keeper 自动触发；Provider 亦可自调）。
    pub fn release_expired_stake(&mut self, agent_id: u64) {
        let mut a = self.load_agent(agent_id);
        let now = self.env().get_block_time();
        if now < a.lock_until {
            self.env().revert(Error::StillLocked);
        }
        let amount = a.stake;
        if amount.is_zero() {
            self.env().revert(Error::StakeRemaining);
        }
        let payer = a.stake_payer;
        self.pay(&payer, amount);
        a.stake = U256::zero();
        a.status = 3; // expired
        self.agents.set(&agent_id, a);
        self.env().emit_event(StakeReleased {
            agent: agent_id,
            amount,
        });
    }

    // ---------- 日常 claim 与判决 ----------

    pub fn submit_claim(
        &mut self,
        agent_id: u64,
        topic: String,
        value: U256,
        confidence: u8,
        source_count: u8,
        payload_hash: String,
    ) -> u64 {
        if confidence > 100 {
            self.env().revert(Error::InvalidConfidence);
        }
        let mut a = self.load_agent(agent_id);
        self.assert_agent_owner(&a);

        let id = self.claim_count.get_or_default();
        let now = self.env().get_block_time();
        self.claims.set(
            &id,
            Claim {
                agent: agent_id,
                topic: topic.clone(),
                value,
                confidence,
                source_count,
                payload_hash,
                timestamp: now,
                verdict_status: 0,
                votes_for: 0,
                votes_against: 0,
                verdict_confidence: 0,
            },
        );
        self.claim_count.set(id + 1);
        a.claims_count += 1;
        self.agents.set(&agent_id, a);
        self.env().emit_event(ClaimSubmitted {
            claim_id: id,
            agent: agent_id,
            topic,
        });
        id
    }

    /// 验证网络记录对某 claim 的对抗式裁决，并据此更新 Provider 信誉。
    pub fn record_verdict(
        &mut self,
        claim_id: u64,
        accurate: bool,
        confidence: u8,
        votes_for: u8,
        votes_against: u8,
        _reasoning_hash: String,
    ) {
        self.assert_verifier();
        if confidence > 100 {
            self.env().revert(Error::InvalidConfidence);
        }
        let mut c = self.load_claim(claim_id);
        c.verdict_status = if accurate { 1 } else { 2 };
        c.votes_for = votes_for;
        c.votes_against = votes_against;
        c.verdict_confidence = confidence;
        let agent_id = c.agent;
        self.claims.set(&claim_id, c);

        if accurate {
            self.bump_reputation(agent_id, true, 1 + (confidence as u32) / 25);
        } else {
            self.bump_reputation(agent_id, false, 2 + (confidence as u32) / 20);
        }
        self.env().emit_event(VerdictRecorded {
            claim_id,
            accurate,
            votes_for,
            votes_against,
        });
    }

    // ---------- 雇佣托管 ----------

    /// Consumer 雇佣某 Provider：托管 price_per_day×days，按 milestones 个里程碑结算。
    /// 调用者须先对本合约 approve 足额 CEP-18。返回 hire id。
    pub fn create_hire(
        &mut self,
        provider: u64,
        days: u32,
        milestones: u32,
        sla_hash: String,
    ) -> u64 {
        if days == 0 || milestones == 0 {
            self.env().revert(Error::InvalidParams);
        }
        let mut a = self.load_agent(provider);
        let now = self.env().get_block_time();
        let ends_at = now.saturating_add((days as u64).saturating_mul(DAY_MS));
        // 雇佣结束日不得晚于入驻到期日（否则押金到期退了却还在履约）。
        if ends_at > a.lock_until {
            self.env().revert(Error::HireExceedsLock);
        }
        let total = a.price_per_day * U256::from(days);
        let consumer = self.env().caller();
        self.pull(&consumer, total);

        let id = self.hire_count.get_or_default();
        self.hires.set(
            &id,
            Hire {
                consumer,
                provider,
                price_per_day: a.price_per_day,
                total,
                escrow: total,
                settled: U256::zero(),
                sla_hash,
                milestones_total: milestones,
                milestones_passed: 0,
                status: 0,
                created_at: now,
                ends_at,
            },
        );
        self.hire_count.set(id + 1);
        a.hires_count += 1;
        self.agents.set(&provider, a);
        self.env().emit_event(HireCreated {
            hire_id: id,
            consumer,
            provider,
            total,
        });
        id
    }

    /// 验证网络按客观 SLA 记录已通过的里程碑数。
    pub fn record_hire_verdict(
        &mut self,
        hire_id: u64,
        milestones_passed: u32,
        _reasoning_hash: String,
    ) {
        self.assert_verifier();
        let mut h = self.load_hire(hire_id);
        if h.status != 0 {
            self.env().revert(Error::HireNotActive);
        }
        // 里程碑只能单调递增，禁止回退（防 verifier 调低卡死已完成的结算）。
        if milestones_passed > h.milestones_total || milestones_passed < h.milestones_passed {
            self.env().revert(Error::InvalidParams);
        }
        h.milestones_passed = milestones_passed;
        self.hires.set(&hire_id, h);
    }

    /// 按已通过里程碑结算给 Provider（扣佣金）。可多次调用，幂等累计。
    /// 仅验证网络可调——由它在确认 SLA 后统一放款，避免任意人抢跑结算、关死退款仲裁窗口。
    pub fn settle_hire(&mut self, hire_id: u64) {
        self.assert_verifier();
        let mut h = self.load_hire(hire_id);
        if h.status != 0 {
            self.env().revert(Error::HireNotActive);
        }
        let payable = h.total * U256::from(h.milestones_passed) / U256::from(h.milestones_total);
        if payable <= h.settled {
            self.env().revert(Error::NothingToSettle);
        }
        let due = payable - h.settled;
        let commission = due * U256::from(self.commission_bps.get_or_default()) / U256::from(BPS_DENOM);
        let provider_amount = due - commission;

        let a = self.load_agent(h.provider);
        self.pay(&a.owner, provider_amount); // 佣金留在合约（平台 Treasury）

        h.settled += due;
        h.escrow -= due;
        if h.settled >= h.total {
            h.status = 1; // settled
        }
        let provider = h.provider;
        self.hires.set(&hire_id, h);

        // 履约加分。
        self.bump_reputation(provider, true, 3);
        self.env().emit_event(HireSettled {
            hire_id,
            provider_paid: provider_amount,
            commission,
        });
    }

    /// 不达标：退未交付托管给 Consumer + 从 Provider 押金罚没赔付 + 信誉重挫。仅验证网络可调。
    pub fn refund_hire(&mut self, hire_id: u64) {
        self.assert_verifier();
        let mut h = self.load_hire(hire_id);
        if h.status != 0 {
            self.env().revert(Error::HireNotActive);
        }
        let remaining = h.escrow;
        if remaining > U256::zero() {
            self.pay(&h.consumer, remaining);
        }
        // 罚没：未交付金额 × slash_factor，从 Provider 押金扣，赔付 Consumer。
        let slash_target = remaining * U256::from(self.slash_factor_bps.get_or_default()) / U256::from(BPS_DENOM);
        let mut a = self.load_agent(h.provider);
        let slash = if slash_target > a.stake { a.stake } else { slash_target };
        if slash > U256::zero() {
            self.pay(&h.consumer, slash);
            a.stake -= slash;
            a.slashed_count += 1;
            if a.stake.is_zero() {
                a.status = 1; // paused：押金耗尽，停止接新单
            }
        }
        let provider = h.provider;
        self.agents.set(&provider, a);

        h.escrow = U256::zero();
        h.status = 2; // refunded
        self.hires.set(&hire_id, h);

        self.bump_reputation(provider, false, 20);
        self.env().emit_event(HireRefunded {
            hire_id,
            refunded: remaining,
            slashed: slash,
        });
    }

    // ---------- 只读 ----------

    pub fn get_agent(&self, agent_id: u64) -> AgentProfile {
        self.load_agent(agent_id)
    }
    pub fn get_reputation(&self, agent_id: u64) -> U256 {
        self.load_agent(agent_id).reputation
    }
    pub fn get_claim(&self, claim_id: u64) -> Claim {
        self.load_claim(claim_id)
    }
    pub fn get_hire(&self, hire_id: u64) -> Hire {
        self.load_hire(hire_id)
    }
    pub fn get_agent_count(&self) -> u64 {
        self.agent_count.get_or_default()
    }
    pub fn get_hire_count(&self) -> u64 {
        self.hire_count.get_or_default()
    }
    pub fn get_payment_token(&self) -> Address {
        self.payment_token.get_or_revert_with(Error::NotInitialized)
    }
    pub fn is_verifier(&self, who: Address) -> bool {
        self.verifiers.get(&who).unwrap_or(false)
    }

    // ---------- 内部 ----------

    /// 从 from 把代币托管进本合约（需 from 已 approve）。
    fn pull(&self, from: &Address, amount: U256) {
        let token = self.payment_token.get_or_revert_with(Error::NotInitialized);
        let me = self.env().self_address();
        Cep18TokenContractRef::new(self.env(), token).transfer_from(from, &me, &amount);
    }

    /// 从本合约把代币转给 to。
    fn pay(&self, to: &Address, amount: U256) {
        let token = self.payment_token.get_or_revert_with(Error::NotInitialized);
        Cep18TokenContractRef::new(self.env(), token).transfer(to, &amount);
    }

    fn bump_reputation(&mut self, agent_id: u64, up: bool, mag: u32) {
        let mut a = self.load_agent(agent_id);
        let cur = a.reputation;
        let m = U256::from(mag);
        let new_score = if up {
            let s = cur + m;
            let cap = U256::from(MAX_REPUTATION);
            if s > cap {
                cap
            } else {
                s
            }
        } else if cur > m {
            cur - m
        } else {
            U256::zero()
        };
        a.reputation = new_score;
        self.agents.set(&agent_id, a);
        self.env().emit_event(ReputationUpdated {
            agent: agent_id,
            new_score,
        });
    }

    fn load_agent(&self, agent_id: u64) -> AgentProfile {
        self.agents.get(&agent_id).unwrap_or_revert_with(&self.env(), Error::AgentNotFound)
    }
    fn load_claim(&self, claim_id: u64) -> Claim {
        self.claims.get(&claim_id).unwrap_or_revert_with(&self.env(), Error::ClaimNotFound)
    }
    fn load_hire(&self, hire_id: u64) -> Hire {
        self.hires.get(&hire_id).unwrap_or_revert_with(&self.env(), Error::HireNotFound)
    }

    fn assert_owner(&self) {
        let owner = self.owner.get_or_revert_with(Error::NotOwner);
        if self.env().caller() != owner {
            self.env().revert(Error::NotOwner);
        }
    }
    fn assert_verifier(&self) {
        if !self.verifiers.get(&self.env().caller()).unwrap_or(false) {
            self.env().revert(Error::NotVerifier);
        }
    }
    fn assert_agent_owner(&self, a: &AgentProfile) {
        if self.env().caller() != a.owner {
            self.env().revert(Error::NotOwner);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use odra::host::{Deployer, HostRef};
    use odra_modules::erc20::{Erc20, Erc20HostRef, Erc20InitArgs};

    struct Setup {
        env: odra::host::HostEnv,
        token: Erc20HostRef,
        reg: TrustRegistryHostRef,
        verifier: Address,
        provider: Address,
        consumer: Address,
    }

    fn setup() -> Setup {
        let env = odra_test::env();
        let owner = env.get_account(0);
        let verifier = env.get_account(1);
        let provider = env.get_account(2);
        let consumer = env.get_account(3);

        let mut token = Erc20::deploy(
            &env,
            Erc20InitArgs {
                name: String::from("Vouch"),
                symbol: String::from("VTT"),
                decimals: 9,
                initial_supply: Some(U256::from(1_000_000u64)),
            },
        );
        env.set_caller(owner);
        token.transfer(&provider, &U256::from(10_000u64));
        token.transfer(&consumer, &U256::from(10_000u64));

        let reg = TrustRegistry::deploy(
            &env,
            TrustRegistryInitArgs {
                payment_token: token.address(),
                verifier,
                commission_bps: 1000,
            },
        );
        Setup { env, token, reg, verifier, provider, consumer }
    }

    /// provider approve + 注册并质押 1000，单价 100/天，锁定 100 天。
    fn register(s: &mut Setup) -> u64 {
        s.env.set_caller(s.provider);
        s.token.approve(&s.reg.address(), &U256::from(1_000u64));
        s.reg
            .register_agent(String::from("meta"), U256::from(100u64), U256::from(1_000u64), 100)
    }

    #[test]
    fn register_holds_stake_and_inits_reputation() {
        let mut s = setup();
        let aid = register(&mut s);
        let a = s.reg.get_agent(aid);
        assert_eq!(a.stake, U256::from(1_000u64));
        assert_eq!(a.reputation, U256::from(50u64));
        assert_eq!(a.status, 0);
        assert_eq!(s.token.balance_of(&s.reg.address()), U256::from(1_000u64));
    }

    #[test]
    fn claim_verdict_updates_reputation() {
        let mut s = setup();
        let aid = register(&mut s);
        s.env.set_caller(s.provider);
        let cid = s.reg.submit_claim(
            aid,
            String::from("XAU/USD"),
            U256::from(4_330_000_000u64),
            95,
            2,
            String::from("h"),
        );
        s.env.set_caller(s.verifier);
        s.reg.record_verdict(cid, true, 95, 3, 0, String::from("r"));
        assert_eq!(s.reg.get_reputation(aid), U256::from(54u64)); // 50 + (1 + 95/25)

        s.env.set_caller(s.provider);
        let cid2 = s.reg.submit_claim(aid, String::from("XAU/USD"), U256::from(1u64), 80, 2, String::from("h"));
        s.env.set_caller(s.verifier);
        s.reg.record_verdict(cid2, false, 80, 0, 3, String::from("r"));
        assert_eq!(s.reg.get_reputation(aid), U256::from(48u64)); // 54 - (2 + 80/20)
    }

    #[test]
    fn hire_settle_pays_provider_and_takes_commission() {
        let mut s = setup();
        let aid = register(&mut s);
        s.env.set_caller(s.consumer);
        s.token.approve(&s.reg.address(), &U256::from(1_000u64));
        let hid = s.reg.create_hire(aid, 10, 10, String::from("sla"));
        assert_eq!(s.reg.get_hire(hid).total, U256::from(1_000u64));

        s.env.set_caller(s.verifier);
        s.reg.record_hire_verdict(hid, 10, String::from("r"));

        let before = s.token.balance_of(&s.provider);
        s.reg.settle_hire(hid);
        assert_eq!(s.token.balance_of(&s.provider) - before, U256::from(900u64)); // 1000 - 10%
        assert_eq!(s.reg.get_hire(hid).status, 1);
    }

    #[test]
    fn hire_refund_slashes_provider_and_pays_consumer() {
        let mut s = setup();
        let aid = register(&mut s);
        s.env.set_caller(s.consumer);
        s.token.approve(&s.reg.address(), &U256::from(1_000u64));
        let hid = s.reg.create_hire(aid, 10, 10, String::from("sla"));

        let consumer_before = s.token.balance_of(&s.consumer);
        s.env.set_caller(s.verifier);
        s.reg.refund_hire(hid);
        // 退托管 1000 + 罚没押金 1000（slash factor 1.0）
        assert_eq!(s.token.balance_of(&s.consumer) - consumer_before, U256::from(2_000u64));
        let a = s.reg.get_agent(aid);
        assert_eq!(a.stake, U256::zero());
        assert_eq!(a.slashed_count, 1);
        assert_eq!(s.reg.get_hire(hid).status, 2);
    }

    #[test]
    fn non_verifier_cannot_record_verdict() {
        let mut s = setup();
        let aid = register(&mut s);
        s.env.set_caller(s.provider);
        let cid = s.reg.submit_claim(aid, String::from("X"), U256::from(1u64), 50, 1, String::from("h"));
        s.env.set_caller(s.consumer);
        let r = s.reg.try_record_verdict(cid, true, 50, 1, 0, String::from("r"));
        assert_eq!(r, Err(Error::NotVerifier.into()));
    }

    #[test]
    fn stake_locked_then_released_on_expiry() {
        let mut s = setup();
        let aid = register(&mut s);
        let r = s.reg.try_release_expired_stake(aid);
        assert_eq!(r, Err(Error::StillLocked.into()));

        s.env.advance_block_time(101 * DAY_MS);
        let before = s.token.balance_of(&s.provider);
        s.reg.release_expired_stake(aid);
        assert_eq!(s.token.balance_of(&s.provider) - before, U256::from(1_000u64));
        assert_eq!(s.reg.get_agent(aid).stake, U256::zero());
        assert_eq!(s.reg.get_agent(aid).status, 3);
    }
}
