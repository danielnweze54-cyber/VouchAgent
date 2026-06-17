//! M2 testnet 验证 ①：把 EscrowPoc 部署到 Casper 测试网，绑定既有 X402 CEP-18 代币。
//! 目的：验证 Odra 合约能否跨合约调用 casper-client 部署的标准 CEP-18（兼容性命门）。
//!
//! 运行：cargo run --bin escrow_deploy --features livenet
//! 部署成功后把打印的 escrow 地址填入 escrow_test 的 ESCROW_HASH 环境变量。

use std::str::FromStr;

use contract::escrow_poc::{EscrowPoc, EscrowPocInitArgs};
use odra::host::Deployer;
use odra::prelude::{Address, Addressable};

/// 既有 X402 支付代币（标准 CEP-18，casper-client 部署）。
const X402_TOKEN: &str = "hash-8c5535f6f005c6e47d54372c22eb9af6fcb8e21e098f49af7b9e88123dd07a61";

fn main() {
    let env = odra_casper_livenet_env::env();
    let token = Address::from_str(X402_TOKEN).expect("X402 代币地址格式错误");

    env.set_gas(400_000_000_000u64);
    let escrow = EscrowPoc::deploy(&env, EscrowPocInitArgs { token });

    println!("✅ EscrowPoc 已部署");
    println!("   escrow 地址: {}", escrow.address().to_string());
    println!("   绑定代币:   {}", escrow.get_token().to_string());
}
