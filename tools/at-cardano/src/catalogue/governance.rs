//! Governance nouns: DReps, the committee, proposals and votes.

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

const fn call_epoch(endpoint: &'static str, param: Param, render: Render) -> Route {
    Route::Call(Call {
        endpoint,
        method: Method::Get,
        body: Body::None,
        param,
        paging: Paging::Page,
        epoch: true,
        render,
    })
}

pub static NOUNS: &[Noun] = &[DREP, DREPS, COMMITTEE, PROPOSAL, PROPOSALS, VOTES];

const DREP: Noun = Noun {
    name: "drep",
    question: "One delegated representative",
    verbs: &[
        &DREP_INFO,
        &DREP_METADATA,
        &DREP_VOTES,
        &DREP_DELEGATORS,
        &DREP_HISTORY,
        &DREP_POWER_HISTORY,
        &DREP_UPDATES,
        &DREP_EPOCH_SUMMARY,
    ],
};

const DREP_INFO: Verb = Verb {
    name: "drep",
    question: "What is this DRep",
    usage: "at-cardano drep <drep>… [--json] [--keyless]",
    notes: "Status, deposit, voting power, delegator count and metadata pointer (`POST \
            /drep_info`). The id is CIP-129 bech32, or a 28-byte script hash in hex.",
    flags: flags::CORE,
    route: call(
        "drep_info",
        Method::Post,
        Body::DrepIds,
        Param::None,
        Paging::None,
        render::records::<model::DrepInfo>,
    ),
};

const DREP_METADATA: Verb = Verb {
    name: "drep metadata",
    question: "What metadata has this DRep published",
    usage: "at-cardano drep metadata <drep>… [--json] [--keyless]",
    notes: "The metadata document and whether it validated (`POST /drep_metadata`). The body \
            is under `--json`; the row names the URL, hash and any warning.",
    flags: flags::CORE,
    route: call(
        "drep_metadata",
        Method::Post,
        Body::DrepIds,
        Param::None,
        Paging::None,
        render::rows::<model::DrepMetadataRow>,
    ),
};

const DREP_VOTES: Verb = Verb {
    name: "drep votes",
    question: "How has this DRep voted",
    usage: "at-cardano drep votes <drep> [--limit N] [--offset N] [--all] [--json]",
    notes: "Every governance vote by the DRep (`GET /drep_votes`): proposal, vote, block \
            time and the voting transaction.",
    flags: flags::PAGED,
    route: call(
        "drep_votes",
        Method::Get,
        Body::None,
        Param::Query("_drep_id"),
        Paging::Page,
        render::rows::<model::VoteCastRow>,
    ),
};

const DREP_DELEGATORS: Verb = Verb {
    name: "drep delegators",
    question: "Who delegates to this DRep",
    usage: "at-cardano drep delegators <drep> [--limit N] [--offset N] [--all] [--json]",
    notes: "Live delegators with their stake and the epoch the delegation took effect (`GET \
            /drep_delegators`).",
    flags: flags::PAGED,
    route: call(
        "drep_delegators",
        Method::Get,
        Body::None,
        Param::Query("_drep_id"),
        Paging::Page,
        render::rows::<model::DrepDelegatorRow>,
    ),
};

const DREP_HISTORY: Verb = Verb {
    name: "drep history",
    question: "What voting power has this DRep had",
    usage: "at-cardano drep history [<drep>] [--epoch N] [--limit N] [--offset N] [--all]",
    notes: "Voting power per epoch (`GET /drep_history`); with no target it covers every \
            DRep, and `--epoch` narrows to one.",
    flags: flags::PAGED_EPOCH,
    route: call_epoch(
        "drep_history",
        Param::OptionalQuery("_drep_id"),
        render::rows::<model::DrepPowerRow>,
    ),
};

const DREP_POWER_HISTORY: Verb = Verb {
    name: "drep power-history",
    question: "The same history from the dedicated endpoint",
    usage: "at-cardano drep power-history [<drep>] [--epoch N] [--limit N] [--offset N] [--all]",
    notes: "`GET /drep_voting_power_history`, which answers the same shape as `drep history` \
            from a newer endpoint. Prefer it when `drep history` lags.",
    flags: flags::PAGED_EPOCH,
    route: call_epoch(
        "drep_voting_power_history",
        Param::OptionalQuery("_drep_id"),
        render::rows::<model::DrepPowerRow>,
    ),
};

const DREP_UPDATES: Verb = Verb {
    name: "drep updates",
    question: "How has this DRep's registration changed",
    usage: "at-cardano drep updates [<drep>] [--limit N] [--offset N] [--all] [--json]",
    notes: "Registration and update certificates (`GET /drep_updates`), newest first. With no \
            target it covers every DRep.",
    flags: flags::PAGED,
    route: call(
        "drep_updates",
        Method::Get,
        Body::None,
        Param::OptionalQuery("_drep_id"),
        Paging::Page,
        render::rows::<model::DrepUpdateRow>,
    ),
};

const DREP_EPOCH_SUMMARY: Verb = Verb {
    name: "drep epoch-summary",
    question: "How much voting power did DReps have in an epoch",
    usage: "at-cardano drep epoch-summary <epoch> [--limit N] [--offset N] [--all] [--json]",
    notes: "Total registered DRep voting power for one epoch (`GET /drep_epoch_summary`), \
            with the number of DReps behind it.",
    flags: flags::PAGED,
    route: call(
        "drep_epoch_summary",
        Method::Get,
        Body::None,
        Param::Query("_epoch_no"),
        Paging::Page,
        render::rows::<model::DrepEpochSummaryRow>,
    ),
};

const DREPS: Noun = Noun {
    name: "dreps",
    question: "The DReps themselves",
    verbs: &[&DREPS_LIST],
};

const DREPS_LIST: Verb = Verb {
    name: "dreps",
    question: "Which DReps are registered",
    usage: "at-cardano dreps [--limit N] [--offset N] [--all] [--json]",
    notes: "Every registered DRep (`GET /drep_list`) with status and script flag. Tens of \
            thousands of rows: page it.",
    flags: flags::PAGED,
    route: call(
        "drep_list",
        Method::Get,
        Body::None,
        Param::None,
        Paging::Page,
        render::rows::<model::DrepListRow>,
    ),
};

const COMMITTEE: Noun = Noun {
    name: "committee",
    question: "The constitutional committee",
    verbs: &[&COMMITTEE_INFO, &COMMITTEE_VOTES],
};

const COMMITTEE_INFO: Verb = Verb {
    name: "committee",
    question: "Who is on the committee",
    usage: "at-cardano committee [--json] [--keyless]",
    notes: "The current committee (`GET /committee_info`): the quorum and one line per member \
            with status, cold and hot credentials, and expiry.",
    flags: flags::CORE,
    route: call(
        "committee_info",
        Method::Get,
        Body::None,
        Param::None,
        Paging::None,
        render::records::<model::CommitteeInfo>,
    ),
};

const COMMITTEE_VOTES: Verb = Verb {
    name: "committee votes",
    question: "How has a committee member voted",
    usage: "at-cardano committee votes [<member>] [--limit N] [--offset N] [--all] [--json]",
    notes: "Votes cast by a committee hot credential (`GET /committee_votes`). With no target \
            it covers every member.",
    flags: flags::PAGED,
    route: call(
        "committee_votes",
        Method::Get,
        Body::None,
        Param::OptionalQuery("_cc_hot_id"),
        Paging::Page,
        render::rows::<model::VoteCastRow>,
    ),
};

const PROPOSAL: Noun = Noun {
    name: "proposal",
    question: "One governance action",
    verbs: &[&PROPOSAL_SUMMARY, &PROPOSAL_VOTES, &PROPOSAL_BY_VOTER],
};

const PROPOSAL_SUMMARY: Verb = Verb {
    name: "proposal summary",
    question: "How is the vote going on this proposal",
    usage: "at-cardano proposal summary <proposal> [--json] [--keyless]",
    notes: "Voting summary by body (`GET /proposal_voting_summary`): yes, no and abstain \
            counts and percentages for DReps, pools and the committee, per epoch.",
    flags: flags::CORE,
    route: call(
        "proposal_voting_summary",
        Method::Get,
        Body::None,
        Param::Query("_proposal_id"),
        Paging::None,
        render::rows::<model::ProposalSummaryRow>,
    ),
};

const PROPOSAL_VOTES: Verb = Verb {
    name: "proposal votes",
    question: "Who voted on this proposal, and how",
    usage: "at-cardano proposal votes <proposal> [--limit N] [--offset N] [--all] [--json]",
    notes: "Every vote cast on one proposal (`GET /proposal_votes`), with the voter's role \
            and credential.",
    flags: flags::PAGED,
    route: call(
        "proposal_votes",
        Method::Get,
        Body::None,
        Param::Query("_proposal_id"),
        Paging::Page,
        render::rows::<model::ProposalVoteRow>,
    ),
};

const PROPOSAL_BY_VOTER: Verb = Verb {
    name: "proposal by-voter",
    question: "What has this voter voted on",
    usage: "at-cardano proposal by-voter [<voter>] [--limit N] [--offset N] [--all] [--json]",
    notes: "Proposals a DRep, pool or committee member has voted on (`GET \
            /voter_proposal_list`). With no target it covers every voter.",
    flags: flags::PAGED,
    route: call(
        "voter_proposal_list",
        Method::Get,
        Body::None,
        Param::OptionalQuery("_voter_id"),
        Paging::Page,
        render::rows::<model::ProposalRow>,
    ),
};

const PROPOSALS: Noun = Noun {
    name: "proposals",
    question: "The governance actions themselves",
    verbs: &[&PROPOSALS_LIST],
};

const PROPOSALS_LIST: Verb = Verb {
    name: "proposals",
    question: "Which governance actions are there",
    usage: "at-cardano proposals [--limit N] [--offset N] [--order S] [--all] [--json]",
    notes: "Every governance action (`GET /proposal_list`) with its type, deposit, return \
            address and the epochs at which it was ratified, enacted, dropped or expired.",
    flags: flags::PAGED,
    route: call(
        "proposal_list",
        Method::Get,
        Body::None,
        Param::None,
        Paging::Page,
        render::rows::<model::ProposalRow>,
    ),
};

const VOTES: Noun = Noun {
    name: "votes",
    question: "Every governance vote",
    verbs: &[&VOTES_LIST],
};

const VOTES_LIST: Verb = Verb {
    name: "votes",
    question: "Which votes have been cast",
    usage: "at-cardano votes [--limit N] [--offset N] [--order S] [--all] [--json]",
    notes: "The chain-wide vote list (`GET /vote_list`): voter role and id, proposal, vote, \
            epoch and block. Narrow it with `drep votes`, `pool votes` or `proposal votes`.",
    flags: flags::PAGED,
    route: call(
        "vote_list",
        Method::Get,
        Body::None,
        Param::None,
        Paging::Page,
        render::rows::<model::VoteRow>,
    ),
};
