//! Crypto asset registry — the shared contract between the ticker router
//! ([`crate::ticker`]) and the crypto data layer (`uzi-data::crypto`).
//!
//! The table is the single source of truth for:
//! * which bare symbols resolve to crypto instead of a US listing
//!   ([`CryptoCoin::bare`] is `false` for symbols that are also live US tickers),
//! * the CoinGecko id used to fetch market data,
//! * the Chinese display name / sector used when a coin is rendered as an asset.

/// One crypto asset. `symbol` is the canonical upper-case ticker.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CryptoCoin {
    pub symbol: &'static str,
    /// English display name (a.k.a. what `0_basic.name` carries).
    pub name: &'static str,
    pub name_cn: &'static str,
    pub coingecko_id: &'static str,
    /// Chinese sector label used as `industry`.
    pub sector: &'static str,
    pub consensus: &'static str,
    /// Mainnet genesis / first-block date (`YYYY-MM-DD`).
    pub genesis: &'static str,
    /// Whether the bare symbol (e.g. `BTC`) may be used as a ticker. Coins that
    /// share their symbol with a live US listing require the explicit pair form
    /// (`SOL-USD`) or the `.CRYPTO` suffix.
    pub bare: bool,
}

macro_rules! coins {
    ($(($sym:literal, $name:literal, $cn:literal, $cg:literal, $sector:literal, $cons:literal, $gen:literal, $bare:literal)),* $(,)?) => {
        &[$(CryptoCoin {
            symbol: $sym,
            name: $name,
            name_cn: $cn,
            coingecko_id: $cg,
            sector: $sector,
            consensus: $cons,
            genesis: $gen,
            bare: $bare,
        }),*]
    };
}

/// Curated registry, ordered roughly by market-cap rank at time of writing.
/// `bare = false` marks symbols that collide with listed US equities
/// (SOL/LINK/OP/ARB/COMP/AXS/DASH/STX) — those still work as `SOL-USD`.
pub static COINS: &[CryptoCoin] = coins![
    ("BTC", "Bitcoin", "比特币", "bitcoin", "L1 公链", "PoW", "2009-01-03", true),
    ("ETH", "Ethereum", "以太坊", "ethereum", "L1 公链", "PoS", "2015-07-30", true),
    ("USDT", "Tether", "泰达币", "tether", "稳定币", "中心化储备", "2014-10-06", true),
    ("XRP", "XRP", "瑞波币", "ripple", "支付/跨境结算", "联邦共识", "2012-06-02", true),
    ("BNB", "BNB", "币安币", "binancecoin", "L1 公链", "PoSA", "2017-07-25", true),
    ("USDC", "USD Coin", "USDC", "usd-coin", "稳定币", "中心化储备", "2018-09-26", true),
    ("SOL", "Solana", "Solana", "solana", "L1 公链", "PoH + PoS", "2020-03-16", false),
    ("TRX", "TRON", "波场", "tron", "L1 公链", "DPoS", "2017-09-13", true),
    ("DOGE", "Dogecoin", "狗狗币", "dogecoin", "支付/Meme", "PoW", "2013-12-06", true),
    ("ADA", "Cardano", "艾达币", "cardano", "L1 公链", "PoS (Ouroboros)", "2017-09-29", true),
    ("LINK", "Chainlink", "Chainlink", "chainlink", "预言机", "PoS (质押)", "2019-05-30", false),
    ("AVAX", "Avalanche", "雪崩", "avalanche-2", "L1 公链", "PoS (Snowman)", "2020-09-21", true),
    ("TON", "Toncoin", "TON", "the-open-network", "L1 公链", "PoS (BFT)", "2021-05-13", true),
    ("SHIB", "Shiba Inu", "柴犬币", "shiba-inu", "Meme", "ERC-20", "2020-08-01", true),
    ("DOT", "Polkadot", "波卡", "polkadot", "L1 公链", "NPoS", "2020-08-19", true),
    ("LTC", "Litecoin", "莱特币", "litecoin", "支付", "PoW", "2011-10-07", true),
    ("BCH", "Bitcoin Cash", "比特现金", "bitcoin-cash", "支付", "PoW", "2017-08-01", true),
    ("UNI", "Uniswap", "Uniswap", "uniswap", "DeFi/DEX", "治理代币", "2020-09-17", true),
    ("XLM", "Stellar", "恒星币", "stellar", "支付/跨境结算", "SCP", "2014-07-31", true),
    ("ATOM", "Cosmos", "Cosmos", "cosmos", "L1 公链/互操作", "PoS (Tendermint)", "2019-03-13", true),
    ("XMR", "Monero", "门罗币", "monero", "隐私", "PoW", "2014-04-18", true),
    ("ETC", "Ethereum Classic", "以太经典", "ethereum-classic", "L1 公链", "PoW", "2016-07-20", true),
    ("FIL", "Filecoin", "Filecoin", "filecoin", "DePIN/存储", "PoSt", "2020-10-15", true),
    ("APT", "Aptos", "Aptos", "aptos", "L1 公链", "PoS (BFT)", "2022-10-17", true),
    ("SUI", "Sui", "Sui", "sui", "L1 公链", "PoS (BFT)", "2023-05-03", true),
    ("NEAR", "NEAR Protocol", "NEAR", "near", "L1 公链", "PoS (Nightshade)", "2020-08-11", true),
    ("INJ", "Injective", "Injective", "injective-protocol", "DeFi/L1", "PoS (Tendermint)", "2021-11-25", true),
    ("TIA", "Celestia", "Celestia", "celestia", "模块化/DA", "PoS", "2023-10-31", true),
    ("SEI", "Sei", "Sei", "sei-network", "L1 公链", "PoS (Tendermint)", "2023-08-15", true),
    ("PEPE", "Pepe", "Pepe", "pepe", "Meme", "ERC-20", "2023-04-17", true),
    ("WIF", "dogwifhat", "dogwifhat", "dogwifcoin", "Meme", "SPL", "2023-11-20", true),
    ("BONK", "Bonk", "Bonk", "bonk", "Meme", "SPL", "2022-12-25", true),
    ("AAVE", "Aave", "Aave", "aave", "DeFi/借贷", "治理代币", "2020-10-02", true),
    ("MKR", "Maker", "Maker", "maker", "DeFi/稳定币", "治理代币", "2017-01-29", true),
    ("CRV", "Curve DAO", "Curve", "curve-dao-token", "DeFi/DEX", "治理代币", "2020-08-14", true),
    ("LDO", "Lido DAO", "Lido", "lido-dao", "DeFi/质押", "治理代币", "2021-01-05", true),
    ("RENDER", "Render", "Render", "render-token", "DePIN/算力", "ERC-20", "2020-06-11", true),
    ("GRT", "The Graph", "The Graph", "the-graph", "基础设施/索引", "ERC-20", "2020-12-17", true),
    ("ALGO", "Algorand", "Algorand", "algorand", "L1 公链", "PoS (PPoS)", "2019-06-21", true),
    ("VET", "VeChain", "唯链", "vechain", "供应链", "PoA", "2018-08-01", true),
    ("HBAR", "Hedera", "Hedera", "hedera-hashgraph", "L1 公链", "aBFT", "2019-09-16", true),
    ("ICP", "Internet Computer", "互联网计算机", "internet-computer", "L1 公链", "阈值中继链", "2021-05-10", true),
    ("EOS", "EOS", "EOS", "eos", "L1 公链", "DPoS", "2018-06-14", true),
    ("XTZ", "Tezos", "Tezos", "tezos", "L1 公链", "PoS (LPoS)", "2018-06-30", true),
    ("IMX", "Immutable", "Immutable", "immutable-x", "L2/游戏", "zk-Rollup", "2022-11-04", true),
    ("THETA", "Theta Network", "Theta", "theta-token", "DePIN/流媒体", "PoS", "2019-03-15", true),
    ("EGLD", "MultiversX", "MultiversX", "elrond-erd-2", "L1 公链", "PoS (Secure PoS)", "2020-09-01", true),
    ("FLOW", "Flow", "Flow", "flow", "L1 公链", "PoS (HotStuff)", "2021-01-27", true),
    ("CHZ", "Chiliz", "Chiliz", "chiliz", "粉丝代币", "PoSA", "2019-07-01", true),
    ("ENS", "Ethereum Name Service", "ENS", "ethereum-name-service", "基础设施/域名", "治理代币", "2021-11-09", true),
    ("SNX", "Synthetix", "Synthetix", "havven", "DeFi/衍生品", "治理代币", "2018-03-14", true),
    ("ZEC", "Zcash", "Zcash", "zcash", "隐私", "PoW", "2016-10-28", true),
    ("KSM", "Kusama", "Kusama", "kusama", "L1 公链", "NPoS", "2019-12-31", true),
    ("ZIL", "Zilliqa", "Zilliqa", "zilliqa", "L1 公链", "PoW + pBFT", "2018-01-25", true),
    ("CELO", "Celo", "Celo", "celo", "L1/移动端", "PoS", "2020-05-22", true),
    ("ZRX", "0x Protocol", "0x", "0x", "DeFi/DEX", "治理代币", "2017-08-15", true),
    ("ANKR", "Ankr", "Ankr", "ankr", "基础设施/RPC", "PoS (质押)", "2018-07-01", true),
    ("STORJ", "Storj", "Storj", "storj", "DePIN/存储", "ERC-20", "2017-06-19", true),
    ("OCEAN", "Ocean Protocol", "Ocean", "ocean-protocol", "DePIN/数据", "ERC-20", "2019-05-01", true),
    ("ARB", "Arbitrum", "Arbitrum", "arbitrum", "L2/公链", "Optimistic Rollup", "2023-03-23", false),
    ("OP", "Optimism", "Optimism", "optimism", "L2/公链", "Optimistic Rollup", "2022-05-31", false),
    ("POL", "Polygon", "Polygon", "matic-network", "L2/公链", "PoS", "2019-04-28", true),
    ("FTM", "Fantom", "Fantom", "fantom", "L1 公链", "Lachesis aBFT", "2018-12-27", true),
    ("DAI", "Dai", "Dai", "dai", "稳定币", "超额抵押", "2019-11-18", true),
    ("TUSD", "TrueUSD", "TrueUSD", "true-usd", "稳定币", "中心化储备", "2018-03-06", true),
    ("FDUSD", "First Digital USD", "FDUSD", "first-digital-usd", "稳定币", "中心化储备", "2023-06-01", true),
    ("WBTC", "Wrapped Bitcoin", "Wrapped BTC", "wrapped-bitcoin", "DeFi/封装资产", "托管映射", "2019-01-31", true),
    ("WETH", "Wrapped Ether", "Wrapped ETH", "weth", "DeFi/封装资产", "合约映射", "2017-12-31", true),
    ("OKB", "OKB", "OKB", "okb", "平台币", "中心化", "2019-05-01", true),
    ("CRO", "Cronos", "Cronos", "crypto-com-chain", "平台币/L1", "PoS", "2018-12-14", true),
    ("KAS", "Kaspa", "Kaspa", "kaspa", "L1 公链", "PoW (GHOSTDAG)", "2022-05-26", true),
    ("RUNE", "THORChain", "THORChain", "thorchain", "DeFi/跨链", "Tendermint", "2019-07-23", true),
    ("GALA", "Gala", "Gala", "gala", "游戏", "ERC-20", "2020-09-01", true),
    ("SAND", "The Sandbox", "The Sandbox", "the-sandbox", "游戏/元宇宙", "ERC-20", "2020-08-14", true),
    ("MANA", "Decentraland", "Decentraland", "decentraland", "游戏/元宇宙", "ERC-20", "2017-09-01", true),
    ("AXS", "Axie Infinity", "Axie", "axie-infinity", "游戏", "ERC-20", "2020-09-01", false),
    ("DASH", "Dash", "Dash", "dash", "支付/隐私", "PoW + Masternode", "2014-01-18", false),
    ("COMP", "Compound", "Compound", "compound-governance-token", "DeFi/借贷", "治理代币", "2020-06-15", false),
    ("FET", "Artificial Superintelligence Alliance", "Fetch.ai", "artificial-superintelligence-alliance", "AI", "Cosmos SDK", "2019-03-01", true),
    ("TAO", "Bittensor", "Bittensor", "bittensor", "AI/算力", "PoS (Yuma)", "2021-11-01", true),
    ("PYTH", "Pyth Network", "Pyth", "pyth-network", "预言机", "治理代币", "2023-11-20", true),
    ("JUP", "Jupiter", "Jupiter", "jupiter-exchange-solana", "DeFi/DEX", "治理代币", "2024-01-31", true),
    ("STX", "Stacks", "Stacks", "blockstack", "L2/比特币", "PoX", "2019-04-23", false),
    ("ORDI", "ORDI", "ORDI", "ordi", "Meme/铭文", "BRC-20", "2023-03-08", true),
];

/// Quote currencies accepted after `-` / `/` or concatenated onto a base
/// symbol. `USD` and the stablecoins are the practical set.
pub static QUOTES: &[&str] = &[
    "USD", "USDT", "USDC", "BUSD", "FDUSD", "TUSD", "DAI", "EUR", "BTC", "ETH",
];

/// Registry lookup by canonical symbol.
pub fn find(symbol: &str) -> Option<&'static CryptoCoin> {
    let up = symbol.to_ascii_uppercase();
    COINS.iter().find(|c| c.symbol == up)
}

/// Registry lookup that additionally requires the coin to be addressable as a
/// bare ticker (no US-listing collision).
pub fn find_bare(symbol: &str) -> Option<&'static CryptoCoin> {
    find(symbol).filter(|c| c.bare)
}

/// Split a concatenated pair (`BTCUSDT`) into `(base, quote)`. The base must be
/// a registered coin so arbitrary strings never become crypto.
pub fn split_concat(pair: &str) -> Option<(&'static CryptoCoin, &'static str)> {
    let up = pair.to_ascii_uppercase();
    for quote in QUOTES {
        if let Some(base) = up.strip_suffix(quote) {
            if base.len() < 2 {
                continue;
            }
            if let Some(coin) = find_bare(base) {
                return Some((coin, quote));
            }
        }
    }
    None
}

/// True when `quote` is a recognised quote currency.
pub fn is_quote(quote: &str) -> bool {
    let up = quote.to_ascii_uppercase();
    QUOTES.iter().any(|q| *q == up)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn registry_symbols_are_unique_and_upper_case() {
        let mut seen = std::collections::HashSet::new();
        for c in COINS {
            assert_eq!(c.symbol, c.symbol.to_ascii_uppercase(), "{}", c.symbol);
            assert!(seen.insert(c.symbol), "duplicate symbol {}", c.symbol);
            assert!(!c.coingecko_id.is_empty(), "{}", c.symbol);
            assert_eq!(c.genesis.len(), 10, "{}", c.symbol);
        }
    }

    #[test]
    fn us_listing_collisions_are_not_bare() {
        for sym in ["SOL", "LINK", "OP", "ARB", "COMP", "AXS", "DASH", "STX"] {
            assert!(find(sym).is_some(), "{sym} should stay registered");
            assert!(find_bare(sym).is_none(), "{sym} must require an explicit pair");
        }
        assert!(find_bare("BTC").is_some());
    }

    #[test]
    fn concat_split_only_accepts_registered_bases() {
        let (coin, quote) = split_concat("btcusdt").unwrap();
        assert_eq!((coin.symbol, quote), ("BTC", "USDT"));
        let (coin, quote) = split_concat("ETHUSDC").unwrap();
        assert_eq!((coin.symbol, quote), ("ETH", "USDC"));
        // Not registered / not a bare symbol → never crypto.
        assert!(split_concat("FOOUSDT").is_none());
        assert!(split_concat("SOLUSDT").is_none());
        assert!(split_concat("AAPL").is_none());
    }
}
