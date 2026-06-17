//! M2 PoC：验证 Odra 合约能否跨合约调用 CEP-18 代币，实现"合约托管资金"（escrow）。
//!
//! 这是 Vouch 信任层「入驻押金 + 雇佣托管 + 罚没赔付」的技术地基：
//! 合约必须能 (1) 把代币从调用者托管进自身地址，(2) 再把代币释放给指定账户。
//! 验证路径：`transfer_from(调用者 → 合约自身)` 托管 → `transfer(合约 → 受益人)` 释放。
//!
//! 本地单测用 odra-modules 的 Erc20（CEP-18 兼容）做对手方代币；
//! 与 testnet 上既有 X402 代币的兼容性需另行链上实测（入口点名/参数序列化）。

use odra::casper_types::U256;
use odra::prelude::*;
use odra::ContractRef;

/// 外部 CEP-18 代币接口（方法签名匹配 odra-modules Erc20 与标准 CEP-18）。
#[odra::external_contract]
pub trait Cep18Token {
    fn transfer(&mut self, recipient: &Address, amount: &U256);
    fn transfer_from(&mut self, owner: &Address, recipient: &Address, amount: &U256);
    fn balance_of(&self, address: &Address) -> U256;
}

/// 合约错误码。
#[odra::odra_error]
pub enum EscrowError {
    /// 尚未绑定代币地址。
    NotInitialized = 1,
}

/// 最小托管合约 PoC。
#[odra::module(errors = EscrowError)]
pub struct EscrowPoc {
    /// 托管所用的 CEP-18 代币地址。
    token: Var<Address>,
}

#[odra::module]
impl EscrowPoc {
    /// 初始化，绑定要托管的 CEP-18 代币合约地址。
    pub fn init(&mut self, token: Address) {
        self.token.set(token);
    }

    /// 把 `amount` 代币从调用者托管进本合约。
    /// 前提：调用者已先对本合约 `approve` 了足额额度。
    pub fn deposit(&mut self, amount: U256) {
        let depositor = self.env().caller();
        let me = self.env().self_address();
        let token = self.token.get_or_revert_with(EscrowError::NotInitialized);
        Cep18TokenContractRef::new(self.env(), token).transfer_from(&depositor, &me, &amount);
    }

    /// 把本合约托管的 `amount` 代币释放给 `to`。
    /// SECURITY: 这是兼容性 PoC，**故意不加访问控制**；切勿向此合约转入真实资金，
    /// 生产托管逻辑见 trust_registry.rs（带鉴权 + 信誉 + 争议）。
    pub fn release(&mut self, to: Address, amount: U256) {
        let token = self.token.get_or_revert_with(EscrowError::NotInitialized);
        Cep18TokenContractRef::new(self.env(), token).transfer(&to, &amount);
    }

    /// 本合约当前托管的代币余额。
    pub fn held(&self) -> U256 {
        let me = self.env().self_address();
        let token = self.token.get_or_revert_with(EscrowError::NotInitialized);
        Cep18TokenContractRef::new(self.env(), token).balance_of(&me)
    }

    /// 读取绑定的代币地址。
    pub fn get_token(&self) -> Address {
        self.token.get_or_revert_with(EscrowError::NotInitialized)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use odra::host::{Deployer, HostRef};
    use odra_modules::erc20::{Erc20, Erc20HostRef, Erc20InitArgs};

    /// 部署一个 CEP-18 代币（account(0) 持全部初始供应）+ 一个 escrow 合约。
    fn setup() -> (odra::host::HostEnv, Erc20HostRef, EscrowPocHostRef) {
        let env = odra_test::env();
        let token = Erc20::deploy(
            &env,
            Erc20InitArgs {
                name: String::from("Vouch Test Token"),
                symbol: String::from("VTT"),
                decimals: 9,
                initial_supply: Some(U256::from(1_000_000u64)),
            },
        );
        let escrow = EscrowPoc::deploy(
            &env,
            EscrowPocInitArgs {
                token: token.address(),
            },
        );
        (env, token, escrow)
    }

    #[test]
    fn escrow_holds_and_releases_via_cross_contract() {
        let (env, mut token, mut escrow) = setup();
        let owner = env.get_account(0);
        let beneficiary = env.get_account(1);

        // owner 授权 escrow 合约可动用 1000
        token.approve(&escrow.address(), &U256::from(1_000u64));
        assert_eq!(
            token.allowance(&owner, &escrow.address()),
            U256::from(1_000u64)
        );

        // owner 把 1000 托管进 escrow（escrow 内部跨合约 transfer_from）
        escrow.deposit(U256::from(1_000u64));
        assert_eq!(escrow.held(), U256::from(1_000u64));
        assert_eq!(token.balance_of(&escrow.address()), U256::from(1_000u64));
        assert_eq!(token.balance_of(&owner), U256::from(999_000u64));

        // escrow 释放 400 给受益人，剩 600 仍托管
        escrow.release(beneficiary, U256::from(400u64));
        assert_eq!(token.balance_of(&beneficiary), U256::from(400u64));
        assert_eq!(escrow.held(), U256::from(600u64));
    }

    #[test]
    fn deposit_without_allowance_fails() {
        let (env, _token, mut escrow) = setup();
        // 未 approve 直接 deposit 应失败（allowance 不足）
        env.set_caller(env.get_account(2));
        let result = escrow.try_deposit(U256::from(100u64));
        assert!(result.is_err());
    }
}
