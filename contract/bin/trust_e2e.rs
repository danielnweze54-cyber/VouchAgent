//! Vouch TrustRegistry 端到端 testnet 实测（STEP 环境变量驱动，逐步用不同角色账户跑）。
//!
//! 通用环境变量：STEP（步骤名）、REGISTRY_HASH（registry 地址）。
//! 角色由 ODRA_CASPER_LIVENET_SECRET_KEY_PATH 决定。
//! 约定：第一个 agent/claim/hire 的 id 均为 0。
//!
//! 步骤：
//!   approve   调用者 approve registry 动用 AMOUNT（X402）
//!   fund      调用者 transfer AMOUNT X402 给 TARGET
//!   register  provider 注册并质押（STAKE/PRICE/LOCK_DAYS）
//!   claim     provider 提交 claim（AGENT_ID/TOPIC/VALUE）
//!   verdict   verifier 记录裁决（CLAIM_ID）
//!   hire      consumer 雇佣（AGENT_ID/DAYS/MILESTONES）
//!   hverdict  verifier 记录履约里程碑（HIRE_ID/PASSED）
//!   settle    结算放款（HIRE_ID）
//!   agent     读取 agent 状态（AGENT_ID）

use std::env;
use std::str::FromStr;

use contract::trust_registry::TrustRegistry;
use odra::casper_types::U256;
use odra::host::HostRefLoader;
use odra::prelude::Address;
use odra_modules::erc20::Erc20;

const X402: &str = "hash-8c5535f6f005c6e47d54372c22eb9af6fcb8e21e098f49af7b9e88123dd07a61";

fn a(key: &str) -> Address {
    Address::from_str(&env::var(key).unwrap_or_else(|_| panic!("缺少 {key}"))).expect("地址格式错误")
}
fn u256(key: &str, default: u64) -> U256 {
    U256::from(env::var(key).ok().and_then(|v| v.parse::<u64>().ok()).unwrap_or(default))
}
fn u64v(key: &str, default: u64) -> u64 {
    env::var(key).ok().and_then(|v| v.parse().ok()).unwrap_or(default)
}
fn s(key: &str, default: &str) -> String {
    env::var(key).unwrap_or_else(|_| default.to_string())
}

fn main() {
    let lv = odra_casper_livenet_env::env();
    let step = env::var("STEP").expect("缺少 STEP");
    let token = Address::from_str(X402).unwrap();

    match step.as_str() {
        "approve" => {
            let reg = a("REGISTRY_HASH");
            let amount = u256("AMOUNT", 0);
            let mut t = Erc20::load(&lv, token);
            lv.set_gas(5_000_000_000u64);
            t.approve(&reg, &amount);
            println!("OK approve registry 可动用 {}", amount);
        }
        "fund" => {
            let target = a("TARGET");
            let amount = u256("AMOUNT", 0);
            let mut t = Erc20::load(&lv, token);
            lv.set_gas(5_000_000_000u64);
            t.transfer(&target, &amount);
            println!("OK 转 {} X402 给 {}", amount, target.to_string());
        }
        "register" => {
            let mut r = TrustRegistry::load(&lv, a("REGISTRY_HASH"));
            lv.set_gas(15_000_000_000u64);
            let id = r.register_agent(
                s("META", "rwa-oracle-agent"),
                u256("PRICE", 10_000_000_000),
                u256("STAKE", 100_000_000_000),
                u64v("LOCK_DAYS", 30) as u32,
            );
            println!("OK register agent_id={}", id);
        }
        "claim" => {
            let mut r = TrustRegistry::load(&lv, a("REGISTRY_HASH"));
            lv.set_gas(15_000_000_000u64);
            let cid = r.submit_claim(
                u64v("AGENT_ID", 0),
                s("TOPIC", "XAU/USD"),
                u256("VALUE", 4_330_000_000),
                95,
                2,
                s("PAYLOAD", "payload-hash"),
            );
            println!("OK claim_id={}", cid);
        }
        "verdict" => {
            let mut r = TrustRegistry::load(&lv, a("REGISTRY_HASH"));
            lv.set_gas(15_000_000_000u64);
            r.record_verdict(u64v("CLAIM_ID", 0), true, 95, 3, 0, s("REASON", "reason-hash"));
            println!("OK verdict claim={} -> accurate", u64v("CLAIM_ID", 0));
        }
        "hire" => {
            let mut r = TrustRegistry::load(&lv, a("REGISTRY_HASH"));
            lv.set_gas(15_000_000_000u64);
            let hid = r.create_hire(
                u64v("AGENT_ID", 0),
                u64v("DAYS", 10) as u32,
                u64v("MILESTONES", 10) as u32,
                s("SLA", "sla-hash"),
            );
            println!("OK hire_id={}", hid);
        }
        "hverdict" => {
            let mut r = TrustRegistry::load(&lv, a("REGISTRY_HASH"));
            lv.set_gas(15_000_000_000u64);
            r.record_hire_verdict(u64v("HIRE_ID", 0), u64v("PASSED", 10) as u32, s("REASON", "sla-ok"));
            println!("OK hverdict hire={} passed={}", u64v("HIRE_ID", 0), u64v("PASSED", 10));
        }
        "settle" => {
            let mut r = TrustRegistry::load(&lv, a("REGISTRY_HASH"));
            lv.set_gas(15_000_000_000u64);
            r.settle_hire(u64v("HIRE_ID", 0));
            println!("OK settle hire={} 已结算放款", u64v("HIRE_ID", 0));
        }
        "agent" => {
            let r = TrustRegistry::load(&lv, a("REGISTRY_HASH"));
            let ag = r.get_agent(u64v("AGENT_ID", 0));
            println!(
                "agent {} reputation={} stake={} hires={} status={}",
                u64v("AGENT_ID", 0),
                ag.reputation,
                ag.stake,
                ag.hires_count,
                ag.status
            );
        }
        other => println!("未知 STEP: {other}"),
    }
}
