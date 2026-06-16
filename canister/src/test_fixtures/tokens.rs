use candid::Principal;
use oisy_trade_types::{Token, TokenId, TokenMetadata};

pub enum SupportedTokens {
    ICP,
    CKBTC,
    CKETH,
    CKUSDC,
    CKUSDT,
    VCHF,
}

impl SupportedTokens {
    pub fn token_id(&self) -> TokenId {
        let ledger_id = match self {
            SupportedTokens::ICP => "ryjl3-tyaaa-aaaaa-aaaba-cai",
            SupportedTokens::CKBTC => "mxzaz-hqaaa-aaaar-qaada-cai",
            SupportedTokens::CKETH => "ss2fx-dyaaa-aaaar-qacoq-cai",
            SupportedTokens::CKUSDC => "xevnm-gaaaa-aaaar-qafnq-cai",
            SupportedTokens::CKUSDT => "cngnf-vqaaa-aaaar-qag4q-cai",
            SupportedTokens::VCHF => "ly36x-wiaaa-aaaai-aqj7q-cai",
        };
        TokenId {
            ledger_id: Principal::from_text(ledger_id).unwrap(),
        }
    }

    pub const fn symbol(&self) -> &'static str {
        match self {
            SupportedTokens::ICP => "ICP",
            SupportedTokens::CKBTC => "ckBTC",
            SupportedTokens::CKETH => "ckETH",
            SupportedTokens::CKUSDC => "ckUSDC",
            SupportedTokens::CKUSDT => "ckUSDT",
            SupportedTokens::VCHF => "VCHF",
        }
    }

    pub const fn decimals(&self) -> u8 {
        match self {
            SupportedTokens::ICP => 8,
            SupportedTokens::CKBTC => 8,
            SupportedTokens::CKETH => 18,
            SupportedTokens::CKUSDC => 6,
            SupportedTokens::CKUSDT => 6,
            SupportedTokens::VCHF => 8,
        }
    }

    pub fn token_metadata(&self) -> TokenMetadata {
        TokenMetadata {
            symbol: self.symbol().to_string(),
            decimals: self.decimals(),
        }
    }

    pub fn token(&self) -> Token {
        Token {
            id: self.token_id(),
            metadata: self.token_metadata(),
        }
    }

    /// One whole token in the smallest denomination.
    pub const fn one(&self) -> u64 {
        10_u64.checked_pow(self.decimals() as u32).unwrap()
    }
}
