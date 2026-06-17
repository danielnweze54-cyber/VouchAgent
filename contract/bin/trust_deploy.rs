//! 把 Vouch TrustRegistry 部署到 Casper 测试网。
//! 部署账户兼任 owner 与首个 verifier（验证网络），绑定 X402 CEP-18 代币，佣金 10%。
//! 运行：cargo run --bin trust_deploy --features livenet

use std::str::FromStr;

use contract::trust_registry::{TrustRegistry, TrustRegistryInitArgs};
use odra::host::Deployer;
use odra::prelude::{Address, Addressable};

const X402_TOKEN: &str = "hash-8c5535f6f005c6e47d54372c22eb9af6fcb8e21e098f49af7b9e88123dd07a61";

fn main() {
    let env = odra_casper_livenet_env::env();
    let token = Address::from_str(X402_TOKEN).expect("X402 代币地址格式错误");
    let verifier = env.caller(); // PoC：部署账户兼任验证网络

    env.set_gas(500_000_000_000u64);
    let reg = TrustRegistry::deploy(
        &env,
        TrustRegistryInitArgs {
            payment_token: token,
            verifier,
            commission_bps: 1000, // 10%
        },
    );

    println!("✅ TrustRegistry 部署成功");
    println!("   registry 地址: {}", reg.address().to_string());
    println!("   支付代币:     {}", reg.get_payment_token().to_string());
}
