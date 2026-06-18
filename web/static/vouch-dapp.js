// Vouch dApp 前端模块：连 Casper Wallet + 调 TrustRegistry / X402 合约（真实上链）。
// 复用 spike 验证通的全部要点：Deploy 格式（钱包只认旧 Deploy）/ 签名补算法前缀
// （Casper Wallet 返回 raw 64 字节，需按账户算法补 0x01/0x02）/ 经本机 /rpc 代理提交（规避 CORS）。
//
// casper-js-sdk 5.x 用 jsdelivr +esm（webpack CJS bundle，仅 default 导出），故 default import 再解构。
import Casper from "https://cdn.jsdelivr.net/npm/casper-js-sdk@5.0.12/+esm";
import blake from "https://cdn.jsdelivr.net/npm/blakejs@1.2.1/+esm";
const { blake2bHex } = blake;
const {
  Args, CLValue, Key, KeyTypeID, PublicKey, RpcClient, HttpHandler,
  Deploy, DeployHeader, ExecutableDeployItem, StoredVersionedContractByHash, ContractHash, Duration,
} = Casper;

export const CFG = {
  node: location.origin + "/api/rpc",      // 代理 → node.testnet.casper.network（本地 spike_server / 线上 Vercel function）
  chain: "casper-test",
  x402Pkg: "8c5535f6f005c6e47d54372c22eb9af6fcb8e21e098f49af7b9e88123dd07a61",
  registryPkg: "1722de7aefcc523d5140b47b8ef89d7e4cb6ed5a112d3691f2e90eae262ab8ea",
  gas: 5_000_000_000,                       // 5 CSPR
  DEC: 1_000_000_000n,                      // X402 9 位小数
  txBase: "https://testnet.cspr.live/transaction/",
  treasury: "01d2663aa72842d771ad8e05d9453dac3f7b84fbeea0ad40b7c4c8166e5a0096d1", // 平台收款（部署账户）
};

const rpc = new RpcClient(new HttpHandler(CFG.node));
let provider = null;
let pkHex = null;

export function getAccount() { return pkHex; }
export function isConnected() { return !!pkHex; }

export async function connect() {
  if (!window.CasperWalletProvider) throw new Error("未检测到 Casper Wallet 扩展（请安装 casperwallet.io 并切到 Testnet）");
  provider = window.CasperWalletProvider();
  const ok = await provider.requestConnection();
  if (!ok) throw new Error("钱包连接被拒绝");
  pkHex = await provider.getActivePublicKey();
  return pkHex;
}

export async function disconnect() {
  try { await provider?.disconnectFromSite?.(); } catch { /* 忽略 */ }
  pkHex = null;
}

// ---------- 内部：构造 Deploy → 钱包签名（补前缀）→ 代理提交 ----------
function buildDeploy(pkg, entryPoint, argsMap) {
  const session = new ExecutableDeployItem();
  session.storedVersionedContractByHash = new StoredVersionedContractByHash(
    ContractHash.newContract(pkg), entryPoint, Args.fromMap(argsMap),
  );
  const payment = ExecutableDeployItem.standardPayment(String(CFG.gas));
  const header = DeployHeader.default();
  header.account = PublicKey.fromHex(pkHex);
  header.chainName = CFG.chain;
  header.ttl = new Duration(1800000);
  return Deploy.makeDeploy(header, payment, session);
}

async function signAndSend(deploy) {
  if (!pkHex) throw new Error("请先连接钱包");
  const sig = await provider.sign(JSON.stringify(Deploy.toJSON(deploy)), pkHex);
  if (sig.cancelled) throw new Error("用户取消签名");
  let sigBytes = sig.signature instanceof Uint8Array ? sig.signature : new Uint8Array(sig.signature || []);
  if (sigBytes.length === 64) { // Casper approval 签名需带算法前缀
    const prefix = pkHex.startsWith("02") ? 0x02 : 0x01;
    sigBytes = Uint8Array.from([prefix, ...sigBytes]);
  }
  Deploy.setSignature(deploy, sigBytes, PublicKey.fromHex(pkHex));
  await rpc.putDeploy(deploy);
  return deploy; // 返回已提交的 Deploy（含 hash），供取 hash / 等确认
}

/** 取 deploy hash（hex）。 */
export const hashOf = (deploy) => deploy?.hash?.toHex?.() ?? "";

/** 等待 deploy 上链确认（超时/失败返回 null，不抛）。 */
export async function waitConfirm(deploy) {
  try { return await rpc.waitForDeploy(deploy, 120000); } catch { return null; }
}

// X402 数量（人类单位）→ U256 CLValue（最小单位）
const x402 = (n) => CLValue.newCLUInt256((BigInt(Math.round(Number(n))) * CFG.DEC).toString());

// ---------- 业务 entry points（用户钱包自签）----------

/** 对 TrustRegistry approve 足额 X402（注册质押 / 雇佣托管的必需前置）。amountX402 为人类单位。 */
export async function approveRegistry(amountX402) {
  const deploy = buildDeploy(CFG.x402Pkg, "approve", {
    spender: CLValue.newCLKey(Key.createByType("hash-" + CFG.registryPkg, KeyTypeID.Hash)),
    amount: x402(amountX402),
  });
  return signAndSend(deploy);
}

/** 注册成为 Provider 并质押。需先 approveRegistry(stake)。 */
export async function registerAgent({ metadata, pricePerDay, stake, lockDays }) {
  const deploy = buildDeploy(CFG.registryPkg, "register_agent", {
    metadata_hash: CLValue.newCLString(String(metadata)),
    price_per_day: x402(pricePerDay),
    stake_amount: x402(stake),
    lock_days: CLValue.newCLUInt32(Number(lockDays)),
  });
  return signAndSend(deploy);
}

/** 雇佣某 Provider（合约按 price_per_day×days 托管）。需先 approveRegistry(total)。 */
export async function createHire({ provider: providerId, days, milestones, slaHash }) {
  const deploy = buildDeploy(CFG.registryPkg, "create_hire", {
    provider: CLValue.newCLUint64(Number(providerId)),
    days: CLValue.newCLUInt32(Number(days)),
    milestones: CLValue.newCLUInt32(Number(milestones)),
    sla_hash: CLValue.newCLString(String(slaHash)),
  });
  return signAndSend(deploy);
}

/** 为自己的 agent 提交可验证 claim（须是该 agent owner）。value 为原始最小单位整数。 */
export async function submitClaim({ agentId, topic, value, confidence, sourceCount, payloadHash }) {
  const deploy = buildDeploy(CFG.registryPkg, "submit_claim", {
    agent_id: CLValue.newCLUint64(Number(agentId)),
    topic: CLValue.newCLString(String(topic)),
    value: CLValue.newCLUInt256(String(value)),
    confidence: CLValue.newCLUint8(Number(confidence)),
    source_count: CLValue.newCLUint8(Number(sourceCount)),
    payload_hash: CLValue.newCLString(String(payloadHash)),
  });
  return signAndSend(deploy);
}

// ---------- 组合流程（approve → 等确认 → 业务），onStep(step, hash?) 报告进度 ----------

/** 注册：approve(stake) → 等确认 → register_agent。返回 register 的 deploy hash。 */
export async function registerWithStake({ metadata, pricePerDay, stake, lockDays }, onStep) {
  onStep?.("approve");
  const ad = await approveRegistry(stake);
  onStep?.("wait", hashOf(ad));
  await waitConfirm(ad);
  onStep?.("register");
  return hashOf(await registerAgent({ metadata, pricePerDay, stake, lockDays }));
}

/** 雇佣：approve(price×days) → 等确认 → create_hire。返回 hire 的 deploy hash。 */
export async function hireWithEscrow({ provider, days, milestones, slaHash, pricePerDay }, onStep) {
  const total = Number(pricePerDay) * Number(days);
  onStep?.("approve");
  const ad = await approveRegistry(total);
  onStep?.("wait", hashOf(ad));
  await waitConfirm(ad);
  onStep?.("hire");
  return hashOf(await createHire({ provider, days, milestones, slaHash }));
}

/** 付查询费：CEP-18 transfer X402 给平台 Treasury（Trust Query 付费解锁）。返回 deploy hash。 */
export async function payQuery(amountX402 = 1) {
  const recipientHash = PublicKey.fromHex(CFG.treasury).accountHash().toPrefixedString();
  const deploy = buildDeploy(CFG.x402Pkg, "transfer", {
    recipient: CLValue.newCLKey(Key.createByType(recipientHash, KeyTypeID.Account)),
    amount: x402(amountX402),
  });
  return hashOf(await signAndSend(deploy));
}

/** 访客领测试币：调 serverless /api/faucet（服务端用 faucet 私钥发 CSPR gas + X402 体验代币）。
 *  无需访客自己有任何测试币，连钱包即可领。返回 { ok, cspr:{hash,url}, x402:{hash,url} }。 */
export async function claimFaucet(pubkey) {
  const pk = pubkey || pkHex;
  if (!pk) throw new Error("请先连接钱包");
  const r = await fetch("/api/faucet", {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify({ pubkey: pk }),
  });
  const data = await r.json().catch(() => ({}));
  if (!r.ok || !data.ok) throw new Error(data.error || ("领取失败（HTTP " + r.status + "）"));
  return data;
}

// ---------- 只读：前端直读 Odra state dictionary（实时、免后端/cargo）----------
// Odra 2.x 把状态存在 "state" dictionary，item key = blake2bHex(u32_be(字段index) + mapping_key_bytes)。
// 字段 index = struct 声明顺序 + 1（Odra 占用 index 0）：agents=6(Mapping)、agent_count=7(Var)。
const STATE_UREF = "uref-c2cb2d9da140b6d9cb1b1b1895dbbca0dffe4dbcbef628c8a02dae9c3c4e10ce-007";

function fieldKey(idx, extra) {
  const b = new Uint8Array(4 + (extra ? extra.length : 0));
  new DataView(b.buffer).setUint32(0, idx, false); // u32 big-endian
  if (extra) b.set(extra, 4);
  return blake2bHex(b, undefined, 32);
}
function u64leBytes(n) {
  const b = new Uint8Array(8);
  new DataView(b.buffer).setBigUint64(0, BigInt(n), true);
  return b;
}
function clBytes(res) {
  const sv = res && res.storedValue;
  let cl = sv ? (sv.clValue !== undefined ? sv.clValue : sv) : res;
  if (cl == null) return [];
  if (!Array.isArray(cl)) { try { cl = JSON.parse(JSON.stringify(cl)); } catch { return []; } }
  if (Array.isArray(cl)) return cl.map(Number);
  const b = cl && cl.bytes;
  if (typeof b === "string") { const o = []; for (let i = 0; i < b.length; i += 2) o.push(parseInt(b.slice(i, i + 2), 16)); return o; }
  if (Array.isArray(b)) return b.map(Number);
  return [];
}
// 按 casper bytesrepr 反序列化 AgentProfile（字段顺序见合约 struct）。
function parseAgentProfile(bytes) {
  let p = 0;
  const u8 = () => bytes[p++];
  const u32le = () => { const v = (bytes[p] | bytes[p + 1] << 8 | bytes[p + 2] << 16 | bytes[p + 3] << 24) >>> 0; p += 4; return v; };
  const u64le = () => { let v = 0n; for (let i = 0; i < 8; i++) v |= BigInt(bytes[p + i]) << BigInt(8 * i); p += 8; return v; };
  const str = () => { const n = u32le(); let s = ""; for (let i = 0; i < n; i++) s += String.fromCharCode(bytes[p + i]); p += n; return s; };
  const u256 = () => { const n = bytes[p++]; let v = 0n; for (let i = 0; i < n; i++) v |= BigInt(bytes[p + i]) << BigInt(8 * i); p += n; return v; };
  p += 33;                          // owner (1 tag + 32)
  const metadata = str();           // metadata_hash
  const stake = u256();             // stake
  p += 33;                          // stake_payer
  const lockUntil = u64le();        // lock_until
  const pricePerDay = u256();       // price_per_day
  const reputation = u256();        // reputation
  const claims = u64le();
  const hires = u64le();
  const slashed = u64le();
  const status = u8();              // 0 active 1 paused 2 banned 3 expired
  return {
    metadata, stake, pricePerDay, reputation: Number(reputation),
    lockUntil: Number(lockUntil), claims: Number(claims), hires: Number(hires),
    slashed: Number(slashed), status,
  };
}

/** 前端直读链上全部 Provider（实时、免后端）。返回 [{id, metadata, reputation, stake, pricePerDay, status, ...}]。 */
export async function readAgents(max = 12) {
  // 一次性并行探测 0..max（省掉先读 agent_count 的串行往返），filter 出存在的，全部并行一次往返。
  const results = await Promise.all(
    Array.from({ length: max }, (_, i) =>
      rpc.getDictionaryItem(null, STATE_UREF, fieldKey(6, u64leBytes(i)))
        .then((r) => ({ id: i, ...parseAgentProfile(clBytes(r)) }))
        .catch(() => null)
    )
  );
  return results.filter(Boolean);
}
