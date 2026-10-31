//! Stake-pool nouns.

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

/// A history narrowed by the endpoint's own `_epoch_no`.
const fn call_epoch(endpoint: &'static str, method: Method, param: Param, render: Render) -> Route {
    Route::Call(Call {
        endpoint,
        method,
        body: Body::None,
        param,
        paging: Paging::Page,
        epoch: true,
        render,
    })
}

pub static NOUNS: &[Noun] = &[POOL, POOLS];

const POOL: Noun = Noun {
    name: "pool",
    question: "One stake pool",
    verbs: &[
        &POOL_INFO,
        &POOL_DELEGATORS,
        &POOL_DELEGATOR_HISTORY,
        &POOL_INVALID_DELEGATORS,
        &POOL_BLOCKS,
        &POOL_HISTORY,
        &POOL_SNAPSHOT,
        &POOL_RELAYS,
        &POOL_METADATA,
        &POOL_UPDATES,
        &POOL_REGISTRATIONS,
        &POOL_RETIREMENTS,
        &POOL_VOTES,
        &POOL_GROUPS,
        &POOL_POWER_HISTORY,
        &POOL_OWNER_HISTORY,
        &POOL_CALIDUS_KEYS,
    ],
};

const POOL_INFO: Verb = Verb {
    name: "pool",
    question: "What is this pool",
    usage: "at-cardano pool <pool>… [--json] [--keyless]",
    notes: "Status, ticker, margin and costs, pledge, active and live stake, saturation, \
            delegator count, relays and metadata (`POST /pool_info`). Up to 100 pools.",
    flags: flags::CORE,
    route: call(
        "pool_info",
        Method::Post,
        Body::PoolIds,
        Param::None,
        Paging::None,
        render::records::<model::PoolInfo>,
    ),
};

const POOL_DELEGATORS: Verb = Verb {
    name: "pool delegators",
    question: "Who delegates to this pool",
    usage: "at-cardano pool delegators <pool> [--limit N] [--offset N] [--all] [--json]",
    notes: "Live delegators with their stake and the epoch the delegation became active (`GET \
            /pool_delegators`). One pool per call; a saturated pool has thousands, so page it.",
    flags: flags::PAGED,
    route: call(
        "pool_delegators",
        Method::Get,
        Body::None,
        Param::Query("_pool_bech32"),
        Paging::Page,
        render::rows::<model::PoolDelegatorRow>,
    ),
};

const POOL_DELEGATOR_HISTORY: Verb = Verb {
    name: "pool delegator-history",
    question: "Who delegated to this pool in a given epoch",
    usage: "at-cardano pool delegator-history <pool> [--epoch N] [--limit N] [--offset N] [--all]",
    notes: "Per-epoch delegator amounts (`GET /pool_delegators_history`). `--epoch` selects \
            one; without it Koios returns the recent history.",
    flags: flags::PAGED_EPOCH,
    route: call_epoch(
        "pool_delegators_history",
        Method::Get,
        Param::Query("_pool_bech32"),
        render::rows::<model::PoolDelegatorHistoryRow>,
    ),
};

const POOL_INVALID_DELEGATORS: Verb = Verb {
    name: "pool invalid-delegators",
    question: "Which delegations to this pool were invalid",
    usage: "at-cardano pool invalid-delegators <pool> [--epoch N] [--limit N] [--offset N] [--all]",
    notes: "Delegations that did not count for the pool (`GET /pool_invalid_delegators`) — a \
            snapshot artefact rather than a chain error.",
    flags: flags::PAGED_EPOCH,
    route: call_epoch(
        "pool_invalid_delegators",
        Method::Get,
        Param::Query("_pool_bech32"),
        render::rows::<model::PoolInvalidDelegatorRow>,
    ),
};

const POOL_BLOCKS: Verb = Verb {
    name: "pool blocks",
    question: "Which blocks did this pool produce",
    usage: "at-cardano pool blocks <pool> [--epoch N] [--limit N] [--offset N] [--all] [--json]",
    notes: "Produced blocks (`GET /pool_blocks`), newest first, with the epoch, slot and \
            height. `--epoch` narrows to one deviation epoch.",
    flags: flags::PAGED_EPOCH,
    route: call_epoch(
        "pool_blocks",
        Method::Get,
        Param::Query("_pool_bech32"),
        render::rows::<model::PoolBlockRow>,
    ),
};

const POOL_HISTORY: Verb = Verb {
    name: "pool history",
    question: "How did this pool perform, epoch by epoch",
    usage: "at-cardano pool history <pool> [--epoch N] [--limit N] [--offset N] [--all]",
    notes: "Stake, saturation, blocks, delegators, fees, rewards and return on stake per \
            epoch (`GET /pool_history`).",
    flags: flags::PAGED_EPOCH,
    route: call_epoch(
        "pool_history",
        Method::Get,
        Param::Query("_pool_bech32"),
        render::rows::<model::PoolHistoryRow>,
    ),
};

const POOL_SNAPSHOT: Verb = Verb {
    name: "pool stake-snapshot",
    question: "What are this pool's mark, set and go stakes",
    usage: "at-cardano pool stake-snapshot <pool> [--json] [--keyless]",
    notes: "The three stake snapshots used for leaderlog calculation (`GET \
            /pool_stake_snapshot`): mark, set and go, with the network total beside each.",
    flags: flags::CORE,
    route: call(
        "pool_stake_snapshot",
        Method::Get,
        Body::None,
        Param::Query("_pool_bech32"),
        Paging::None,
        render::rows::<model::PoolSnapshotRow>,
    ),
};

const POOL_RELAYS: Verb = Verb {
    name: "pool relays",
    question: "Which relays do pools publish",
    usage: "at-cardano pool relays [--limit N] [--offset N] [--all] [--json]",
    notes: "Every registered relay for every pool (`GET /pool_relays`), grouped by pool. A \
            chain-wide listing, not one pool's.",
    flags: flags::PAGED,
    route: call(
        "pool_relays",
        Method::Get,
        Body::None,
        Param::None,
        Paging::Page,
        render::records_capped::<model::PoolRelayRow>,
    ),
};

const POOL_METADATA: Verb = Verb {
    name: "pool metadata",
    question: "What metadata do pools publish",
    usage: "at-cardano pool metadata [<pool>…] [--json] [--keyless]",
    notes: "The pool registration metadata document (`POST /pool_metadata`), raw and bounded. \
            With no target Koios answers for every pool, so pass one or a batch.",
    flags: flags::CORE,
    route: call(
        "pool_metadata",
        Method::Post,
        Body::PoolIdsOptional,
        Param::None,
        Paging::None,
        render::raw,
    ),
};

const POOL_UPDATES: Verb = Verb {
    name: "pool updates",
    question: "How has this pool's registration changed",
    usage: "at-cardano pool updates [<pool>] [--limit N] [--offset N] [--all] [--json]",
    notes: "One row per registration or retirement certificate (`GET /pool_updates`), newest \
            first. With no target it covers every pool.",
    flags: flags::PAGED,
    route: call(
        "pool_updates",
        Method::Get,
        Body::None,
        Param::OptionalQuery("_pool_bech32"),
        Paging::Page,
        render::rows::<model::PoolUpdateRow>,
    ),
};

const POOL_REGISTRATIONS: Verb = Verb {
    name: "pool registrations",
    question: "Which pools registered in an epoch",
    usage: "at-cardano pool registrations [<epoch>] [--limit N] [--offset N] [--all] [--json]",
    notes: "Registration certificates in one epoch (`GET /pool_registrations`). With no epoch \
            it starts at the newest and pages back.",
    flags: flags::PAGED,
    route: call(
        "pool_registrations",
        Method::Get,
        Body::None,
        Param::OptionalQuery("_epoch_no"),
        Paging::Page,
        render::rows::<model::PoolRegistrationRow>,
    ),
};

const POOL_RETIREMENTS: Verb = Verb {
    name: "pool retirements",
    question: "Which pools retired in an epoch",
    usage: "at-cardano pool retirements [<epoch>] [--limit N] [--offset N] [--all] [--json]",
    notes: "Retirement certificates in one epoch (`GET /pool_retirements`). Koios publishes no \
            response schema for this endpoint, so the answer is raw JSON rather than a \
            projection this tool would have to guess.",
    flags: flags::PAGED,
    route: call(
        "pool_retirements",
        Method::Get,
        Body::None,
        Param::OptionalQuery("_epoch_no"),
        Paging::Page,
        render::raw,
    ),
};

const POOL_VOTES: Verb = Verb {
    name: "pool votes",
    question: "How has this pool voted on governance",
    usage: "at-cardano pool votes <pool> [--limit N] [--offset N] [--all] [--json]",
    notes: "Every governance vote cast by the pool's credential (`GET /pool_votes`).",
    flags: flags::PAGED,
    route: call(
        "pool_votes",
        Method::Get,
        Body::None,
        Param::Query("_pool_bech32"),
        Paging::Page,
        render::rows::<model::VoteCastRow>,
    ),
};

const POOL_GROUPS: Verb = Verb {
    name: "pool groups",
    question: "Which group is this pool listed in",
    usage: "at-cardano pool groups [--limit N] [--offset N] [--all] [--json]",
    notes: "Third-party pool groupings (`GET /pool_groups`): the pool's group, ticker, and \
            the adastat and balanceanalytics labels.",
    flags: flags::PAGED,
    route: call(
        "pool_groups",
        Method::Get,
        Body::None,
        Param::None,
        Paging::Page,
        render::rows::<model::PoolGroupRow>,
    ),
};

const POOL_POWER_HISTORY: Verb = Verb {
    name: "pool power-history",
    question: "What voting power has this pool had",
    usage: "at-cardano pool power-history [<pool>] [--epoch N] [--limit N] [--offset N] [--all]",
    notes: "Voting power per epoch (`GET /pool_voting_power_history`), for one pool or, with \
            no target, for every pool.",
    flags: flags::PAGED_EPOCH,
    route: call_epoch(
        "pool_voting_power_history",
        Method::Get,
        Param::OptionalQuery("_pool_bech32"),
        render::rows::<model::PoolPowerRow>,
    ),
};

const POOL_OWNER_HISTORY: Verb = Verb {
    name: "pool owner-history",
    question: "Who has owned this pool",
    usage: "at-cardano pool owner-history <pool>… [--limit N] [--offset N] [--all] [--json]",
    notes: "Owners per epoch with their declared pledge (`POST /pool_owner_history`).",
    flags: flags::PAGED,
    route: call(
        "pool_owner_history",
        Method::Post,
        Body::PoolIds,
        Param::None,
        Paging::Page,
        render::rows::<model::PoolOwnerRow>,
    ),
};

const POOL_CALIDUS_KEYS: Verb = Verb {
    name: "pool calidus-keys",
    question: "Which pools registered a Calidus key",
    usage: "at-cardano pool calidus-keys [--limit N] [--offset N] [--all] [--json]",
    notes: "Calidus (partner-chain) key registrations (`GET /pool_calidus_keys`), with the \
            owning pool and the registration transaction.",
    flags: flags::PAGED,
    route: call(
        "pool_calidus_keys",
        Method::Get,
        Body::None,
        Param::None,
        Paging::Page,
        render::rows::<model::PoolCalidusRow>,
    ),
};

const POOLS: Noun = Noun {
    name: "pools",
    question: "The pools themselves",
    verbs: &[&POOLS_LIST],
};

const POOLS_LIST: Verb = Verb {
    name: "pools",
    question: "Which pools are there",
    usage: "at-cardano pools [--limit N] [--offset N] [--order S] [--all] [--json]",
    notes: "Every registered pool (`GET /pool_list`) with ticker, status, active stake, \
            margin, fixed cost, pledge and any announced retirement.",
    flags: flags::PAGED,
    route: call(
        "pool_list",
        Method::Get,
        Body::None,
        Param::None,
        Paging::Page,
        render::rows::<model::PoolListRow>,
    ),
};
