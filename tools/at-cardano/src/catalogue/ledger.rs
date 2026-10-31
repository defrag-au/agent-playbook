//! Address, credential and stake-account nouns.

use crate::koios::Method;
use crate::model;
use crate::verbs::render::{self, Render};

use super::{flags, Body, Call, Noun, Paging, Param, Route, Verb};

const fn call(
    endpoint: &'static str,
    method: Method,
    body: Body,
    param: Param,
    paging: Paging,
    render: Render,
) -> Route {
    Route::Call(Call {
        endpoint,
        method,
        body,
        param,
        paging,
        epoch: false,
        render,
    })
}

pub static NOUNS: &[Noun] = &[ADDRESS, ADDRESSES, CREDENTIAL, ACCOUNT, ACCOUNTS];

const ADDRESS: Noun = Noun {
    name: "address",
    question: "One payment address",
    verbs: &[
        &ADDRESS_INFO,
        &ADDRESS_UTXOS,
        &ADDRESS_TXS,
        &ADDRESS_ASSETS,
        &ADDRESS_OUTPUTS,
    ],
};

const ADDRESS_INFO: Verb = Verb {
    name: "address",
    question: "What does this address hold",
    usage: "at-cardano address <addr>… [--network N] [--json] [--keyless]",
    notes: "Balance, stake key and whether it is a script address (`POST /address_info`), \
            with the number of UTxOs behind the balance. Up to 100 addresses in one request.",
    flags: flags::CORE,
    route: call(
        "address_info",
        Method::Post,
        Body::Addresses,
        Param::None,
        Paging::None,
        render::records::<model::AddressInfo>,
    ),
};

const ADDRESS_UTXOS: Verb = Verb {
    name: "address utxos",
    question: "Which UTxOs sit at these addresses",
    usage: "at-cardano address utxos <addr>… [--limit N] [--offset N] [--all] [--no-extended]",
    notes: "UTxOs at up to 100 addresses (`POST /address_utxos`, `_extended` on by default). \
            The row names the out-ref, lovelace, asset count, datum marker and address; \
            `--json` carries the inline datum and every asset.",
    flags: flags::PAGED_EXTENDED,
    route: call(
        "address_utxos",
        Method::Post,
        Body::AddressesExtended,
        Param::None,
        Paging::Page,
        render::rows::<model::UtxoRow>,
    ),
};

const ADDRESS_TXS: Verb = Verb {
    name: "address txs",
    question: "Which transactions touched these addresses",
    usage: "at-cardano address txs <addr>… [--limit N] [--offset N] [--order S] [--all]",
    notes: "Transaction hashes for up to 100 addresses (`POST /address_txs`), newest first \
            unless `--order` says otherwise.",
    flags: flags::PAGED,
    route: call(
        "address_txs",
        Method::Post,
        Body::Addresses,
        Param::None,
        Paging::Page,
        render::rows::<model::AddressTxRow>,
    ),
};

const ADDRESS_ASSETS: Verb = Verb {
    name: "address assets",
    question: "Which assets do these addresses hold",
    usage: "at-cardano address assets <addr>… [--limit N] [--offset N] [--all] [--json]",
    notes: "Aggregated native-asset holdings per address (`POST /address_assets`). ADA is not \
            a native asset and does not appear here.",
    flags: flags::PAGED,
    route: call(
        "address_assets",
        Method::Post,
        Body::Addresses,
        Param::None,
        Paging::Page,
        render::rows::<model::AssetHoldingRow>,
    ),
};

const ADDRESS_OUTPUTS: Verb = Verb {
    name: "address outputs",
    question: "What has this address ever received",
    usage: "at-cardano address outputs <addr>… [--limit N] [--offset N] [--all] [--json]",
    notes: "Every output ever paid to the address, spent or not (`POST /address_outputs`). \
            The row marks spent outputs; `address utxos` is the unspent half.",
    flags: flags::PAGED,
    route: call(
        "address_outputs",
        Method::Post,
        Body::Addresses,
        Param::None,
        Paging::Page,
        render::rows::<model::UtxoRow>,
    ),
};

const ADDRESSES: Noun = Noun {
    name: "addresses",
    question: "The addresses themselves",
    verbs: &[&ADDRESSES_LIST],
};

const ADDRESSES_LIST: Verb = Verb {
    name: "addresses",
    question: "Which addresses are there",
    usage: "at-cardano addresses [--limit N] [--offset N] [--order S] [--all] [--json]",
    notes: "Every payment address the chain has seen (`GET /address_list`), with its payment \
            credential and stake key. Millions of rows: page it, and prefer `address <addr>` \
            when the address is already known.",
    flags: flags::PAGED,
    route: call(
        "address_list",
        Method::Get,
        Body::None,
        Param::None,
        Paging::Page,
        render::rows::<model::AddressListRow>,
    ),
};

const CREDENTIAL: Noun = Noun {
    name: "credential",
    question: "One payment credential, with no stake key",
    verbs: &[&CREDENTIAL_UTXOS, &CREDENTIAL_TXS],
};

const CREDENTIAL_UTXOS: Verb = Verb {
    name: "credential",
    question: "Which UTxOs sit at these payment credentials",
    usage: "at-cardano credential <credential>… [--limit N] [--offset N] [--all] [--no-extended]",
    notes: "UTxOs by payment credential rather than address (`POST /credential_utxos`). The \
            credential is the 28-byte key hash in hex, which is what a script or a contract \
            knows an output by.",
    flags: flags::PAGED_EXTENDED,
    route: call(
        "credential_utxos",
        Method::Post,
        Body::CredentialsExtended,
        Param::None,
        Paging::Page,
        render::rows::<model::UtxoRow>,
    ),
};

const CREDENTIAL_TXS: Verb = Verb {
    name: "credential txs",
    question: "Which transactions touched these payment credentials",
    usage: "at-cardano credential txs <credential>… [--limit N] [--offset N] [--all] [--json]",
    notes: "Transactions by payment credential (`POST /credential_txs`), the credential \
            counterpart of `address txs`.",
    flags: flags::PAGED,
    route: call(
        "credential_txs",
        Method::Post,
        Body::Credentials,
        Param::None,
        Paging::Page,
        render::rows::<model::AddressTxRow>,
    ),
};

const ACCOUNT: Noun = Noun {
    name: "account",
    question: "One stake account",
    verbs: &[
        &ACCOUNT_INFO,
        &ACCOUNT_CACHED,
        &ACCOUNT_UTXOS,
        &ACCOUNT_TXS,
        &ACCOUNT_ASSETS,
        &ACCOUNT_REWARDS,
        &ACCOUNT_REWARD_HISTORY,
        &ACCOUNT_ADDRESSES,
        &ACCOUNT_UPDATES,
        &ACCOUNT_UPDATE_HISTORY,
        &ACCOUNT_HISTORY,
        &ACCOUNT_STAKE_HISTORY,
    ],
};

const ACCOUNT_INFO: Verb = Verb {
    name: "account",
    question: "What does this stake key hold",
    usage: "at-cardano account <stake>… [--json] [--keyless]",
    notes: "Status, delegation (pool and DRep), balance, rewards available and withdrawn, \
            deposit, reserves and treasury (`POST /account_info`).",
    flags: flags::CORE,
    route: call(
        "account_info",
        Method::Post,
        Body::Stake,
        Param::None,
        Paging::None,
        render::records::<model::AccountInfo>,
    ),
};

const ACCOUNT_CACHED: Verb = Verb {
    name: "account cached",
    question: "The same account record, from the cache",
    usage: "at-cardano account cached <stake>… [--json] [--keyless]",
    notes: "`POST /account_info_cached`, the same shape as `account` but served from Koios's \
            cache. It may lag by an epoch; use it when a run is being rate-limited.",
    flags: flags::CORE,
    route: call(
        "account_info_cached",
        Method::Post,
        Body::Stake,
        Param::None,
        Paging::None,
        render::records::<model::AccountInfo>,
    ),
};

const ACCOUNT_UTXOS: Verb = Verb {
    name: "account utxos",
    question: "Which UTxOs sit under these stake keys",
    usage: "at-cardano account utxos <stake>… [--limit N] [--offset N] [--all] [--no-extended]",
    notes: "UTxOs at every address delegated to the stake key (`POST /account_utxos`, \
            `_extended` on by default).",
    flags: flags::PAGED_EXTENDED,
    route: call(
        "account_utxos",
        Method::Post,
        Body::StakeExtended,
        Param::None,
        Paging::Page,
        render::rows::<model::UtxoRow>,
    ),
};

const ACCOUNT_TXS: Verb = Verb {
    name: "account txs",
    question: "Which transactions touched this stake key",
    usage: "at-cardano account txs <stake> [--json] [--keyless]",
    notes: "The most recent transactions for one stake key (`GET /account_txs`). The endpoint \
            takes one stake address and offers no offset, so it answers a recent window \
            rather than a page; `address txs` pages the addresses instead.",
    flags: flags::CORE,
    route: call(
        "account_txs",
        Method::Get,
        Body::None,
        Param::Query("_stake_address"),
        Paging::None,
        render::rows::<model::AddressTxRow>,
    ),
};

const ACCOUNT_ASSETS: Verb = Verb {
    name: "account assets",
    question: "Which assets sit under these stake keys",
    usage: "at-cardano account assets <stake>… [--limit N] [--offset N] [--all] [--json]",
    notes: "Aggregated native-asset holdings per stake address (`POST /account_assets`).",
    flags: flags::PAGED,
    route: call(
        "account_assets",
        Method::Post,
        Body::Stake,
        Param::None,
        Paging::Page,
        render::rows::<model::AssetHoldingRow>,
    ),
};

const ACCOUNT_REWARDS: Verb = Verb {
    name: "account rewards",
    question: "What has this stake key earned",
    usage: "at-cardano account rewards <stake>… [--epoch N] [--limit N] [--offset N] [--all]",
    notes: "Rewards grouped by stake address (`POST /account_rewards`), naming the earning \
            and spendable epochs, the source and the pool. `--epoch` narrows to one.",
    flags: flags::PAGED_EPOCH,
    route: call(
        "account_rewards",
        Method::Post,
        Body::StakeEpoch,
        Param::None,
        Paging::Page,
        render::records_capped::<model::AccountRewardRow>,
    ),
};

const ACCOUNT_REWARD_HISTORY: Verb = Verb {
    name: "account reward-history",
    question: "Every reward this stake key has earned",
    usage: "at-cardano account reward-history <stake>… [--epoch N] [--limit N] [--offset N]",
    notes: "The flat reward history (`POST /account_reward_history`): one row per epoch, with \
            the earning and spendable epoch, amount, source and pool.",
    flags: flags::PAGED_EPOCH,
    route: call(
        "account_reward_history",
        Method::Post,
        Body::StakeEpoch,
        Param::None,
        Paging::Page,
        render::rows::<model::AccountRewardHistoryRow>,
    ),
};

const ACCOUNT_ADDRESSES: Verb = Verb {
    name: "account addresses",
    question: "Which addresses does this stake key control",
    usage: "at-cardano account addresses <stake>… [--json] [--keyless]",
    notes: "Every payment address delegated to the stake key (`POST /account_addresses`).",
    flags: flags::CORE,
    route: call(
        "account_addresses",
        Method::Post,
        Body::Stake,
        Param::None,
        Paging::None,
        render::records_capped::<model::AccountAddressRow>,
    ),
};

const ACCOUNT_UPDATES: Verb = Verb {
    name: "account updates",
    question: "What certificates has this stake key seen",
    usage: "at-cardano account updates <stake>… [--json] [--keyless]",
    notes: "Registration, delegation, withdrawal and deregistration certificates, grouped by \
            stake address (`POST /account_updates`).",
    flags: flags::CORE,
    route: call(
        "account_updates",
        Method::Post,
        Body::Stake,
        Param::None,
        Paging::None,
        render::records_capped::<model::AccountUpdateRow>,
    ),
};

const ACCOUNT_UPDATE_HISTORY: Verb = Verb {
    name: "account update-history",
    question: "The flat certificate history",
    usage: "at-cardano account update-history <stake>… [--limit N] [--offset N] [--all]",
    notes: "One row per certificate (`POST /account_update_history`), where `account updates` \
            groups them by stake address.",
    flags: flags::PAGED,
    route: call(
        "account_update_history",
        Method::Post,
        Body::Stake,
        Param::None,
        Paging::Page,
        render::rows::<model::AccountUpdateHistoryRow>,
    ),
};

const ACCOUNT_HISTORY: Verb = Verb {
    name: "account history",
    question: "Which pool has this stake key been with, epoch by epoch",
    usage: "at-cardano account history <stake>… [--json] [--keyless]",
    notes: "Active stake per epoch, nested under each stake address (`POST \
            /account_history`).",
    flags: flags::CORE,
    route: call(
        "account_history",
        Method::Post,
        Body::Stake,
        Param::None,
        Paging::None,
        render::records_capped::<model::AccountHistoryRow>,
    ),
};

const ACCOUNT_STAKE_HISTORY: Verb = Verb {
    name: "account stake-history",
    question: "The same history, one row per epoch",
    usage: "at-cardano account stake-history <stake>… [--limit N] [--offset N] [--all]",
    notes: "Active stake per epoch as flat rows (`POST /account_stake_history`), easier to \
            sort and page than `account history`.",
    flags: flags::PAGED,
    route: call(
        "account_stake_history",
        Method::Post,
        Body::Stake,
        Param::None,
        Paging::Page,
        render::rows::<model::AccountStakeRow>,
    ),
};

const ACCOUNTS: Noun = Noun {
    name: "accounts",
    question: "The stake accounts themselves",
    verbs: &[&ACCOUNTS_LIST],
};

const ACCOUNTS_LIST: Verb = Verb {
    name: "accounts",
    question: "Which stake accounts are there",
    usage: "at-cardano accounts [--limit N] [--offset N] [--order S] [--all] [--json]",
    notes: "Every stake address the chain has seen (`GET /account_list`), with its script \
            hash. Millions of rows: page it rather than asking for everything.",
    flags: flags::PAGED,
    route: call(
        "account_list",
        Method::Get,
        Body::None,
        Param::None,
        Paging::Page,
        render::rows::<model::AccountListRow>,
    ),
};
