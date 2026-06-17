# Vouch —— Casper Agent 经济的信任层

<div align="center">

### [English](README.md) · [ 简体中文 ]

</div>

> Casper 上第一个为 AI agent 服务的去中心化**信任层** —— 机器经济的**「征信局 + 担保托管 + 法院」**。在你信任（甚至付钱给）一个素未谋面的 agent 之前，Vouch 让你查它的链上信誉、通过**质押托管**雇佣它、并由**多智能体验证网络**裁决结果 —— 诚实有奖，说谎被罚没。
>
> **Casper Agentic Buildathon 2026 参赛作品。**

---

## 问题

Agent 经济正在到来，但它的核心有个窟窿：**你凭什么相信一个素未谋面的 AI agent 给你的数据或服务，更别说付钱给它？** 区块链能证明一个 agent「提交过」某条数据，却无法证明它「是真的」。没有信誉、没有问责、agent 说谎或不履约时也没有追索手段。缺了这层信任与问责，x402 机器对机器支付就无法安全地规模化。

Vouch 就是这层缺失的基础设施。

## Vouch 做什么

Vouch 是**共享同一条链上信誉账本的两个系统**：

| 系统 | 做什么 | 收费 |
|---|---|---|
| **① 信誉查询** | 付费查询任意入驻 agent 的链上信誉、claim 历史、裁决记录 | x402 按次微支付 |
| **② 雇佣托管** | 选中 agent → 按「天数 × 单价」托管付款 → 验证网络按客观 SLA 判履约 → 达标按里程碑放款（抽 10% 佣金）/ 不达标退款 + **罚没押金** | 平台抽 10% 佣金 |

**信任飞轮**：

```
Provider Agent 入驻：注册 + 质押押金（X402 代币，带锁定期）
        │                                       ▲ 信誉累积
        ▼  提交可验证 claim                       │
┌─ 多智能体验证网络（对抗式）─────────────────────┐
│  Coordinator → Judge（独立重新取证）            │
│  → Risk（欺诈/异常）→ N 个人格互异的 Verifier    │
│  独立投票 → 加权收敛                            │
└────────────────────────────────────────────┘
        │  裁决（真/假 + 票型）上链
        ▼
┌─ 链上信任账本（Odra 合约）────────────────────┐
│  per-agent 信誉 + 质押池 + 雇佣单               │
│   ├ 准确    → 信誉↑、雇佣放款（扣佣金）          │
│   └ 不准确/SLA 不达标 → 退款 Consumer +         │
│      罚没 Provider 押金 + 信誉↓                 │
└────────────────────────────────────────────┘
        │
        ▼  x402 + 10% 佣金 = 自给自足
   Treasury（收入 − 验证成本）
```

## 对抗式验证 —— 核心差异化

多数所谓「AI 验证」只是单个 LLM 给数据盖个章。Vouch 跑的是**一个由人格与温度刻意各异的独立验证者组成的评审团**（一个严格核验员、一个异常猎手、一个市场常识官）。每个验证者**自己独立重新取证** —— 绝不轻信 Provider 自报的数值 —— 独立投票，再由网络按加权多数收敛。**分歧会被记录上链。**

testnet 上两次真实运行，同一个 Provider，相反的结局：

| 场景 | 票型 | 裁决 | 信誉 |
|---|---|---|---|
| **诚实 claim**（真实金价） | **3 : 0** | 准确 | 50 → **54**（+4） |
| **恶意 claim**（虚高约 15%） | **0 : 3** | 不准确 | 54 → **48**（−6） |

三个 AI 各自独立抓到真实价格、识破了谎报、合约把说谎者的信誉打了下去 —— 全自主，真实 LLM 调用、真实链上交易。这才是可验证的信任，而不是盖章。

## 已上线 Casper 测试网 ✅

以下全部**已部署并产生真实链上交易**：

| | |
|---|---|
| **Vouch TrustRegistry 合约** | `hash-1722de7aefcc523d5140b47b8ef89d7e4cb6ed5a112d3691f2e90eae262ab8ea` |
| **X402 支付 / 质押代币（CEP-18）** | `hash-8c5535f6f005c6e47d54372c22eb9af6fcb8e21e098f49af7b9e88123dd07a61` |
| 合约部署 | [`cf9507b1…`](https://testnet.cspr.live/transaction/cf9507b126224d161250a07dfb4a2945c9216b602ef7776f5720af20cf611be8) |
| Provider 注册 + 质押 100 X402 | [`0a06e7a1…`](https://testnet.cspr.live/transaction/0a06e7a1ff5ad3da48b6d81a685ac279b4875e9ab8310fb5a837da94158bd2a2) |
| 诚实 claim → 裁决 **3:0 准确** | [`35d6bb4e…`](https://testnet.cspr.live/transaction/35d6bb4e2b2b5ed0675b95c766e0159b3d3b31bd2bb2818df2c0984c2b9cc1e3) |
| 恶意 claim → 裁决 **0:3 不准确** | [`b27cb62a…`](https://testnet.cspr.live/transaction/b27cb62a3caf5f076406de6e516479d93bdfdd1e6f9c6b18f3c07ffb5e0fecdd) |
| 跨合约托管 PoC（CEP-18 托管） | [`99bd01c0…`](https://testnet.cspr.live/transaction/99bd01c0325ca25a7622cd821de4d2913cffef49e908d96fe535dd63f261f5e2) |

## 智能合约：`TrustRegistry`（Rust / Odra）

一个合约承载注册、claim/裁决、雇佣托管与罚没 —— 所有资金都由合约本身通过**跨合约 CEP-18 `transfer_from`/`transfer`** 持有与支付（已在 testnet 验证）。

| 入口点 | 说明 |
|---|---|
| `register_agent(meta, price_per_day, stake, lock_days)` | 注册 + 托管一笔有锁定期的押金 |
| `submit_claim(...)` / `record_verdict(...)` | Provider 提交 claim；验证网络写入裁决（信誉按置信度加权） |
| `create_hire(provider, days, milestones, sla)` | Consumer 托管 `单价×天数`；雇佣不得跨过押金锁定期 |
| `record_hire_verdict` / `settle_hire` / `refund_hire` | 验证网络记录 SLA 里程碑；达标按里程碑放款（扣 10% 佣金），不达标退款 + 罚没押金 |
| `release_expired_stake` | 锁定期满后把押金退回原付款地址（keeper 触发） |

带 `owner` / `verifier` 角色鉴权、置信度加权信誉 `[0,1000]`、罚没系数与佣金可治理。**15 个单元测试全绿**，发布前已通过安全审查。

## 多智能体验证网络（Python + DeepSeek）

| 组件 | 角色 |
|---|---|
| **Coordinator**（`coordinator.py`） | LLM 工具调用编排验证流程 |
| **Judge**（`judge.py`） | 独立重新抓多源证据、交叉验证 → 共识价 + 置信度 |
| **Risk**（`risk.py`） | 欺诈 / 异常评估 |
| **Verifiers**（`verifier_network.py`） | N 个人格互异的验证者对抗式投票 → 加权裁决（票型上链） |
| `vouch_chain.py` | Python ↔ TrustRegistry 桥接（submit_claim / record_verdict / …） |

## 首个入驻 Provider：RWA 价格预言机

Vouch 自己用自己 —— 原来的自主 **RWA 价格预言机**（多源抓黄金/BTC/汇率 + LLM 交叉验证）作为 **Provider #0** 入驻：它质押押金、提交价格 claim、被对抗式验证、累积链上信誉。预言机的 fetcher/judge 栈作为 Provider 实现保留下来。

## x402 微支付

Vouch 的所有价值流转 —— 质押、雇佣托管、佣金、按次付费的信誉查询 —— 都用 **CEP-18 代币经 [x402](https://x402.org)** 在 Casper 上结算，自托管 [make-software/casper-x402](https://github.com/make-software/casper-x402) facilitator。详见 [`x402/README.md`](x402/README.md)。

## 技术栈

- **合约：** Rust + [Odra 2.8](https://odra.dev/)，跨合约 CEP-18 托管，经 `odra-casper-livenet-env` 部署
- **Agent：** Python —— 验证网络（Coordinator/Judge/Risk/Verifiers）、Provider 预言机
- **LLM：** [DeepSeek](https://platform.deepseek.com/)（OpenAI 兼容，function-calling）
- **支付：** x402 + CEP-18 代币，facilitator 在 Casper 测试网结算

## 目录结构

```
contract/   Rust / Odra
  src/trust_registry.rs   Vouch 核心：TrustRegistry + HireEscrow（+ 6 个业务闭环测试）
  src/escrow_poc.rs       CEP-18 跨合约托管 PoC
  src/rwa_oracle.rs       RWA 价格预言机（入驻 Provider #0 / 底座）
  bin/                    trust_deploy / trust_e2e / escrow_*（Odra livenet）
agent/      Python
  verifier_network.py     对抗式验证者评审团
  vouch_chain.py          TrustRegistry 桥接
  vouch_cycle.py          一轮自主闭环（claim → 验证 → 裁决 → 信誉）
  fetcher/judge/risk/coordinator.py   Provider 预言机 + 各 agent
web/        FastAPI 看板 + x402 付费端点
x402/       x402 集成指南 + 配置
```

## 快速开始

```bash
# 合约：测试 + 构建 + 部署
cd contract
cargo odra test                                   # 15 个单元测试
cargo odra build -c TrustRegistry
cargo run --bin trust_deploy --features livenet   # 先配好 contract/.env

# 一轮自主验证（Provider 提交 claim → 对抗式裁决 → 上链）
cd agent
python3 -m venv venv && venv/bin/pip install -r requirements.txt
cp .env.example .env                              # 配置 DEEPSEEK_API_KEY
REGISTRY_HASH=hash-... PROVIDER_KEY=... VERIFIER_KEY=... venv/bin/python vouch_cycle.py
# 加 FAKE_VALUE=4970 可观看验证者当场抓出说谎的 Provider
```

## 许可证

MIT
