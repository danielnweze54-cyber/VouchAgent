//! M2 testnet 验证 ②：用持有 X402 的账户 approve escrow 并 deposit，
//! 验证 Odra 合约能否跨合约调用 casper-client 部署的标准 CEP-18（兼容性命门）。
//!
//! 环境变量：
//!   ESCROW_HASH = escrow 合约地址（escrow_deploy 打印的）
//!   TEST_AMOUNT = 托管额（X402 最小单位，9 decimals；默认 100 X402）
//! 运行（用持币账户私钥）：
//!   ODRA_CASPER_LIVENET_SECRET_KEY_PATH=<buyer-key> cargo run --bin escrow_test --features livenet

use std::env;
use std::str::FromStr;

use contract::escrow_poc::EscrowPoc;
use odra::casper_types::U256;
use odra::host::HostRefLoader;
use odra::prelude::Address;
use odra_modules::erc20::Erc20;

const X402_TOKEN: &str = "hash-8c5535f6f005c6e47d54372c22eb9af6fcb8e21e098f49af7b9e88123dd07a61";

fn main() {
    let livenet = odra_casper_livenet_env::env();
    let escrow_addr =
        Address::from_str(&env::var("ESCROW_HASH").expect("缺少 ESCROW_HASH")).expect("escrow 地址错误");
    let token_addr = Address::from_str(X402_TOKEN).unwrap();
    let amount = U256::from(
        env::var("TEST_AMOUNT")
            .unwrap_or_else(|_| "100000000000".to_string())
            .parse::<u64>()
            .unwrap(),
    );

    // 1) 调用者(持币方) approve escrow 可动用 amount —— 走标准 CEP-18 approve
    let mut token = Erc20::load(&livenet, token_addr);
    livenet.set_gas(5_000_000_000u64);
    token.approve(&escrow_addr, &amount);
    println!("✅ approve: escrow 可动用 {} (最小单位)", amount);

    // 2) deposit：escrow 内部跨合约 transfer_from(调用者 → escrow 自身)
    //    —— 这一步成功即证明 Odra ContractRef 兼容标准 CEP-18
    let mut escrow = EscrowPoc::load(&livenet, escrow_addr);
    livenet.set_gas(8_000_000_000u64);
    escrow.deposit(amount);
    println!("✅ deposit 成功：Odra 合约跨合约调用标准 CEP-18 transfer_from 兼容！");
}
