//! Script nouns.

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

pub static NOUNS: &[Noun] = &[SCRIPT];

const SCRIPT: Noun = Noun {
    name: "script",
    question: "One script, or the published script lists",
    verbs: &[
        &SCRIPT_INFO,
        &SCRIPT_UTXOS,
        &SCRIPT_REFERENCE_UTXOS,
        &SCRIPT_REDEEMERS,
        &SCRIPT_NATIVE,
        &SCRIPT_PLUTUS,
    ],
};

const SCRIPT_INFO: Verb = Verb {
    name: "script",
    question: "What is this script",
    usage: "at-cardano script <hash>… [--json] [--keyless]",
    notes: "Type, size, the transaction that created it and the first bytes of its body \
            (`POST /script_info`).",
    flags: flags::CORE,
    route: call(
        "script_info",
        Method::Post,
        Body::ScriptHashes,
        Param::None,
        Paging::None,
        render::records::<model::ScriptInfo>,
    ),
};

const SCRIPT_UTXOS: Verb = Verb {
    name: "script utxos",
    question: "Which UTxOs carry this script",
    usage: "at-cardano script utxos <hash> [--limit N] [--offset N] [--all] [--json]",
    notes: "Unspent outputs holding a reference script matching the hash (`GET \
            /script_utxos`). One script per call; the answer is the out-ref, so pair it with \
            `utxo <tx#index>` for the value and assets.",
    flags: flags::PAGED,
    route: call(
        "script_utxos",
        Method::Get,
        Body::None,
        Param::Query("_script_hash"),
        Paging::Page,
        render::rows::<model::ScriptUtxoRow>,
    ),
};

const SCRIPT_REFERENCE_UTXOS: Verb = Verb {
    name: "script reference-utxos",
    question: "Which UTxOs hold these scripts as references",
    usage: "at-cardano script reference-utxos <hash>… [--limit N] [--offset N] [--all] [--json]",
    notes: "Reference-script UTxOs for up to 100 scripts (`POST /reference_script_utxos`), \
            with the value, address and assets of each.",
    flags: flags::PAGED,
    route: call(
        "reference_script_utxos",
        Method::Post,
        Body::ScriptHashes,
        Param::None,
        Paging::Page,
        render::rows::<model::UtxoRow>,
    ),
};

const SCRIPT_REDEEMERS: Verb = Verb {
    name: "script redeemers",
    question: "Which redeemers has this script seen",
    usage: "at-cardano script redeemers <hash> [--limit N] [--offset N] [--all] [--json]",
    notes: "Redeemers for one script (`GET /script_redeemers`): the transaction, the budget \
            used, the fee, the purpose and the datum hash.",
    flags: flags::PAGED,
    route: call(
        "script_redeemers",
        Method::Get,
        Body::None,
        Param::Query("_script_hash"),
        Paging::Page,
        render::records_capped::<model::RedeemerGroup>,
    ),
};

const SCRIPT_NATIVE: Verb = Verb {
    name: "script native",
    question: "Which native scripts are on chain",
    usage: "at-cardano script native [--limit N] [--offset N] [--all] [--json]",
    notes: "The native-script list (`GET /native_script_list`): script hash, creation \
            transaction and size.",
    flags: flags::PAGED,
    route: call(
        "native_script_list",
        Method::Get,
        Body::None,
        Param::None,
        Paging::Page,
        render::rows::<model::ScriptListRow>,
    ),
};

const SCRIPT_PLUTUS: Verb = Verb {
    name: "script plutus",
    question: "Which Plutus scripts are on chain",
    usage: "at-cardano script plutus [--limit N] [--offset N] [--all] [--json]",
    notes: "The Plutus-script list (`GET /plutus_script_list`): script hash, creation \
            transaction and size.",
    flags: flags::PAGED,
    route: call(
        "plutus_script_list",
        Method::Get,
        Body::None,
        Param::None,
        Paging::Page,
        render::rows::<model::ScriptListRow>,
    ),
};
