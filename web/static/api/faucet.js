// Vercel Serverless Function：访客零门槛领测试币。
// 服务端用 faucet 私钥（环境变量 FAUCET_SECRET_KEY_PEM）直签两笔交易给访客：
//   ① native CSPR transfer（创建账户 + 提供 gas）  ② CEP-18 X402 transfer（体验代币）
// 让没有任何测试币的访客也能在 dApp 上注册/雇佣/付费体验。
//
// API 全部基于 casper-js-sdk 5.0.12 真实类型定义验证（PrivateKey.fromPem / deploy.sign /
// TransferDeployItem.newTransfer），与前端 vouch-dapp.js 同一套 Deploy 构造。
// 服务端可直连节点（无 CORS），故不走 /api/rpc 代理。
//
// 防滥用为 MVP（demo 级，非生产）：输入校验 + 同一 warm 实例内每地址冷却。serverless 多实例/
// 冷启动会让内存限制失效；真正防刷需 IP 限流 / captcha。faucet 账户余额有限，发放量保守。
// ⚠️ 当前 faucet 账户 = 部署账户（亦为主操作账户 + X402 初始供应），demo 期自担风险（用户拍板）。
//    被无限新地址刷干会让整个 dApp 没 gas 瘫痪——生产/长期上线务必换成独立小额隔离账户。
// TODO（加强）：上链查访客 CSPR 余额，已有币则拒发（getAccountInfo/queryBalance 参数签名待定）。
import Casper from "casper-js-sdk";

const {
  PrivateKey, KeyAlgorithm, PublicKey, Key, KeyTypeID, CLValue, Args,
  Deploy, DeployHeader, ExecutableDeployItem, StoredVersionedContractByHash,
  TransferDeployItem, ContractHash, Duration, RpcClient, HttpHandler,
} = Casper;

const NODE = "https://node.testnet.casper.network/rpc";
const CHAIN = "casper-test";
const X402_PKG = "8c5535f6f005c6e47d54372c22eb9af6fcb8e21e098f49af7b9e88123dd07a61";
const DEC = 1_000_000_000n;                  // X402 / CSPR 均 9 位小数
const GIVE_X402 = 500n;                       // 体验代币（够注册质押 100 + 雇佣 + 付费）
const GIVE_CSPR_MOTES = 10n * DEC;            // 10 CSPR（创建账户 ≥2.5 + 多次 gas）
const COOLDOWN_MS = 12 * 3600 * 1000;         // 同地址领取冷却
const TX_BASE = "https://testnet.cspr.live/transaction/";

// warm 实例内的简易速率限制（非生产级，见文件头说明）
globalThis.__faucetSeen = globalThis.__faucetSeen || new Map();

const json = (res, code, obj) => res.status(code).json(obj);
const fail = (res, code, msg) => json(res, code, { error: msg });

export default async function handler(req, res) {
  // CORS 收紧到自有域名（faucet 是同源调用；通配 * 会让任意第三方页面脚本化滥用、放大资金 DoS）
  res.setHeader("Access-Control-Allow-Origin", process.env.FAUCET_ALLOW_ORIGIN || "https://vouch-agent.vercel.app");
  res.setHeader("Access-Control-Allow-Methods", "POST, OPTIONS");
  res.setHeader("Access-Control-Allow-Headers", "Content-Type");
  if (req.method === "OPTIONS") return res.status(204).end();
  if (req.method !== "POST") return fail(res, 405, "POST only");

  // 解析入参
  let body = req.body;
  if (typeof body === "string") { try { body = JSON.parse(body); } catch { body = {}; } }
  const pubkey = String((body && body.pubkey) || "").trim().toLowerCase();
  if (!/^0[12][0-9a-f]+$/.test(pubkey) || (pubkey.length !== 66 && pubkey.length !== 68))
    return fail(res, 400, "公钥格式不合法（应为 01/02 开头的 hex）");

  // 速率限制
  const last = globalThis.__faucetSeen.get(pubkey);
  if (last && Date.now() - last < COOLDOWN_MS)
    return fail(res, 429, "该地址近期已领取，请 12 小时后再试");

  const pem = process.env.FAUCET_SECRET_KEY_PEM;
  if (!pem) return fail(res, 500, "faucet 未配置（缺环境变量 FAUCET_SECRET_KEY_PEM）");

  try {
    // 加载 faucet 私钥（部署账户 01 开头 = ED25519，可用 FAUCET_KEY_ALGO=secp256k1 覆盖）
    const algo = process.env.FAUCET_KEY_ALGO === "secp256k1" ? KeyAlgorithm.SECP256K1 : KeyAlgorithm.ED25519;
    const sk = PrivateKey.fromPem(pem, algo);
    const faucetPub = sk.publicKey;
    const recipientPub = PublicKey.fromHex(pubkey);     // 非法公钥在此抛错 → 被 catch

    const mkHeader = () => {
      const h = DeployHeader.default();
      h.account = faucetPub;
      h.chainName = CHAIN;
      h.ttl = new Duration(1800000);
      return h;
    };
    const rpc = new RpcClient(new HttpHandler(NODE));

    // ① native CSPR transfer：创建账户并提供 gas
    const csprSession = new ExecutableDeployItem();
    csprSession.transfer = TransferDeployItem.newTransfer(String(GIVE_CSPR_MOTES), recipientPub, null, Date.now());
    const csprDeploy = Deploy.makeDeploy(mkHeader(), ExecutableDeployItem.standardPayment("100000000"), csprSession);
    csprDeploy.sign(sk);

    // ② CEP-18 X402 transfer：体验代币（合约内记账，不要求目标账户已存在）
    const recipientKey = CLValue.newCLKey(
      Key.createByType(recipientPub.accountHash().toPrefixedString(), KeyTypeID.Account),
    );
    const x402Session = new ExecutableDeployItem();
    x402Session.storedVersionedContractByHash = new StoredVersionedContractByHash(
      ContractHash.newContract(X402_PKG), "transfer",
      Args.fromMap({ recipient: recipientKey, amount: CLValue.newCLUInt256((GIVE_X402 * DEC).toString()) }),
    );
    const x402Deploy = Deploy.makeDeploy(mkHeader(), ExecutableDeployItem.standardPayment("5000000000"), x402Session);
    x402Deploy.sign(sk);

    // 提交（顺序：先发 CSPR 建账户，再发 X402）
    await rpc.putDeploy(csprDeploy);
    await rpc.putDeploy(x402Deploy);

    globalThis.__faucetSeen.set(pubkey, Date.now());
    const csprHash = csprDeploy.hash.toHex();
    const x402Hash = x402Deploy.hash.toHex();
    return json(res, 200, {
      ok: true,
      cspr: { hash: csprHash, amount: "10", url: TX_BASE + csprHash },
      x402: { hash: x402Hash, amount: "500", url: TX_BASE + x402Hash },
    });
  } catch (e) {
    console.error("[faucet] error:", (e && e.stack) || e); // 详情只进服务端日志，不回传客户端
    return fail(res, 502, "领取失败，请稍后再试");
  }
}
