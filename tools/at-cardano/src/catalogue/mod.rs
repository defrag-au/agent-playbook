//! `at-cardano`'s grammar: a noun (what you are asking about) and an aspect (what you want
//! from it).
//!
//! `at-cardano pool delegators <pool>` reads as the question it answers, and the noun is what
//! makes the grammar learnable at 100 endpoints: `at-cardano --help` lists 28 nouns, and
//! `at-cardano pool --help` lists that noun's aspects rather than asking a reader to scan a
//! hundred hyphenated names. `at-cardano pool` alone is the noun's principal question.
//!
//! The vocabulary — [`Flag`], [`SUMMARY`], the exit codes — is `at-core`'s, shared with the
//! rest of the toolkit. The table carries the *request* as well as the help text, so the
//! parser, the help and the dispatch read one description: a verb cannot advertise an
//! endpoint it does not call, and `endpoints` can print what answers what.

pub mod assets;
pub mod chain;
pub mod flags;
pub mod governance;
pub mod ledger;
pub mod pools;
pub mod scripts;
pub mod toolkit;

pub use at_core::catalogue::{exit_codes, Flag, SUMMARY};

use crate::koios::Method;
use crate::verbs::render::Render;

pub struct Verb {
    /// The noun, then the aspect: `pool`, `pool delegators`.
    pub name: &'static str,
    /// The question this verb answers, in the words a caller would ask it.
    pub question: &'static str,
    /// The canonical one-line invocation.
    pub usage: &'static str,
    /// What the answer counts, what it refuses, and what it does not do.
    pub notes: &'static str,
    pub flags: &'static [Flag],
    pub route: Route,
}

pub struct Noun {
    /// The noun as it is typed.
    pub name: &'static str,
    /// What the noun is, in one line.
    pub question: &'static str,
    /// Principal question first where the noun has one.
    pub verbs: &'static [&'static Verb],
}

impl Noun {
    /// The verb whose name is the bare noun — `at-cardano pool`, `at-cardano tip`.
    pub fn default_verb(&self) -> Option<&'static Verb> {
        self.verbs
            .iter()
            .copied()
            .find(|verb| verb.name == self.name)
    }

    /// Resolve an aspect typed after the noun. The bare noun also resolves, and so does the
    /// full name, so `find("pool")`, `find("delegators")` and `find("pool delegators")`
    /// all answer from the noun's table.
    pub fn find(&self, aspect: &str) -> Option<&'static Verb> {
        let full = format!("{} ", self.name);
        let wanted = if aspect == self.name || aspect.starts_with(&full) {
            aspect.to_string()
        } else {
            format!("{}{aspect}", full)
        };
        self.verbs.iter().copied().find(|verb| verb.name == wanted)
    }

    /// Aspects in table order, with the default spelled as the noun itself.
    pub fn aspects(&self) -> Vec<&'static str> {
        self.verbs
            .iter()
            .map(|verb| match verb.name.strip_prefix(self.name) {
                Some("") => self.name,
                Some(rest) => rest.trim_start(),
                None => verb.name,
            })
            .collect()
    }
}

pub enum Route {
    /// `auth` — the key source and endpoint, with no request.
    Auth,
    /// `endpoints` — the complete Koios table, with no request.
    Endpoints,
    /// `query` — one endpoint name, GET or POST, raw JSON.
    Query,
    /// `run` — a scenario file, executed step by step.
    Scenario,
    Call(Call),
}

/// One request: what to call, how the targets become a body or query parameters, and how to
/// print the answer.
pub struct Call {
    pub endpoint: &'static str,
    pub method: Method,
    pub body: Body,
    pub param: Param,
    pub paging: Paging,
    /// Send the caller's `--epoch` as `_epoch_no`.
    pub epoch: bool,
    pub render: Render,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Body {
    None,
    Addresses,
    AddressesExtended,
    Stake,
    StakeExtended,
    StakeFirstOnly,
    StakeEpoch,
    TxHashes,
    TxInfo,
    Credentials,
    CredentialsExtended,
    UtxoRefs,
    DatumHashes,
    ScriptHashes,
    AssetList,
    AssetListExtended,
    PoolIds,
    PoolIdsOptional,
    DrepIds,
    BlockHashes,
    BlockTxInfo,
}

/// How the caller's targets become query parameters.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Param {
    /// The targets are only ever a request body.
    None,
    /// Exactly one target becomes this parameter.
    Query(&'static str),
    /// Zero or one target; omitted when there is none.
    OptionalQuery(&'static str),
    /// One asset unit becomes `_asset_policy` and `_asset_name`.
    AssetUnit,
    /// Hashes, or one height resolved to its hash before the body is built.
    BlockRef,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Paging {
    /// One request, one answer per target.
    None,
    /// `limit`/`offset`, `--all`, and a `# next:` line with the following page.
    Page,
}

/// The kind of value a verb takes, which is what validation and the `endpoints` table read.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Target {
    None,
    Address,
    Stake,
    Hex32,
    Hex28,
    Pool,
    Drep,
    Proposal,
    Voter,
    Epoch,
    Label,
    AssetUnit,
    UtxoRef,
    BlockRef,
    Endpoint,
}

impl Call {
    /// What the targets must look like. Derived rather than declared, so the table cannot
    /// validate one kind while the request sends another.
    pub fn target(&self) -> Target {
        match self.param {
            Param::AssetUnit => return Target::AssetUnit,
            Param::BlockRef => return Target::BlockRef,
            Param::Query(param) | Param::OptionalQuery(param) => {
                return match param {
                    "_asset_policy" => Target::Hex28,
                    "_stake_address" => Target::Stake,
                    "_pool_bech32" => Target::Pool,
                    "_drep_id" => Target::Drep,
                    "_proposal_id" => Target::Proposal,
                    "_voter_id" | "_cc_hot_id" => Target::Voter,
                    "_epoch_no" => Target::Epoch,
                    "_label" => Target::Label,
                    "_script_hash" => Target::Hex28,
                    _ => Target::None,
                };
            }
            Param::None => {}
        }
        match self.body {
            Body::Addresses | Body::AddressesExtended => Target::Address,
            Body::Stake | Body::StakeExtended | Body::StakeFirstOnly | Body::StakeEpoch => {
                Target::Stake
            }
            Body::TxHashes | Body::TxInfo | Body::DatumHashes => Target::Hex32,
            Body::Credentials | Body::CredentialsExtended | Body::ScriptHashes => Target::Hex28,
            Body::UtxoRefs => Target::UtxoRef,
            Body::AssetList | Body::AssetListExtended => Target::AssetUnit,
            Body::PoolIds | Body::PoolIdsOptional => Target::Pool,
            Body::DrepIds => Target::Drep,
            Body::BlockHashes | Body::BlockTxInfo => Target::BlockRef,
            Body::None => Target::None,
        }
    }
}

/// Every noun, grouped in the module that owns its domain.
pub static NOUNS: &[&[Noun]] = &[
    chain::NOUNS,
    ledger::NOUNS,
    assets::NOUNS,
    pools::NOUNS,
    governance::NOUNS,
    scripts::NOUNS,
    toolkit::NOUNS,
];

pub fn nouns() -> impl Iterator<Item = &'static Noun> {
    NOUNS.iter().flat_map(|group| group.iter())
}

pub fn verbs() -> impl Iterator<Item = &'static Verb> {
    nouns().flat_map(|noun| noun.verbs.iter().copied())
}

pub fn find(name: &str) -> Option<&'static Verb> {
    verbs().find(|verb| verb.name == name)
}

pub fn find_noun(name: &str) -> Option<&'static Noun> {
    nouns().find(|noun| noun.name == name)
}

pub fn names() -> String {
    nouns().map(|noun| noun.name).collect::<Vec<_>>().join(", ")
}

pub fn flag_names(verb: &Verb) -> String {
    let mut names: Vec<&str> = verb.flags.iter().map(|flag| flag.name).collect();
    names.push("-h");
    names.join(", ")
}

/// One line per noun, then the noun's principal usage — the catalogue at the resolution an
/// agent needs to pick a noun rather than to run one.
pub fn verb_lines() -> Vec<String> {
    let mut lines = Vec::new();
    for noun in nouns() {
        lines.push(format!("  {:<10} {}", noun.name, noun.question));
        let aspects: Vec<&str> = noun.aspects();
        // A noun without a principal question already names its aspects in the usage line; a
        // noun with one names the rest.
        let tail = if noun.default_verb().is_some() && aspects.len() > 1 {
            let extras: Vec<&str> = aspects
                .iter()
                .copied()
                .filter(|aspect| *aspect != noun.name)
                .collect();
            format!(" · also: {}", extras.join(", "))
        } else {
            String::new()
        };
        lines.push(format!(
            "  {:<10} {}{tail}",
            "",
            match noun.default_verb() {
                Some(verb) => verb.usage.to_string(),
                None => format!(
                    "{} {} <{}>",
                    crate::TOOL,
                    noun.name,
                    noun.aspects().join("|")
                ),
            }
        ));
    }
    lines
}

pub fn help_all() -> String {
    let mut out = format!(
        "{} {} — read-only Cardano chain enquiry through Koios\n{}",
        crate::TOOL,
        crate::VERSION,
        "Read-only: HTTPS GET/POST to Koios only, /submittx refused, no writes, no subprocess, \
         and the key is never printed.\n"
    );
    out.push('\n');
    for line in verb_lines() {
        out.push_str(&line);
        out.push('\n');
    }

    out.push_str("\nScenarios\n");
    out.push_str(&format!(
        "  {} run --scenario <file.dsl> [--check]\n",
        crate::TOOL
    ));
    out.push_str("    target <name> <noun> [<aspect>] [positionals…] [fields…]\n");
    out.push_str("    save <target>.<row>.<column> as <name>\n");
    out.push_str("    assert <name> count=3 | count>0 | contains=\"…\" | absent=\"…\"\n");
    out.push_str("    echo \"a line with ${name}\"\n");
    out.push_str(
        "  `run --help` names the fields; `--check` parses the file and stops before a request.\n",
    );

    out.push_str("\nDiscovery (none of these makes a request)\n");
    out.push_str(&format!(
        "  {0} help <noun> [<aspect>]   one noun's aspects, or one aspect's flags\n  \
         {0} endpoints [<text>]       every Koios path, and the verb that answers it\n  \
         {0} auth                     the endpoint and the key source\n  \
         {0} run --check --scenario <file.dsl>   validates a scenario without a request\n",
        crate::TOOL
    ));

    out.push_str("\nExit codes\n");
    for (code, meaning) in exit_codes() {
        out.push_str(&format!("  {code:<6} {meaning}\n"));
    }
    out.push_str("  (`run` also exits 2 when a scenario check does not pass)\n");
    out.push_str("\nA `# ` line in the output is metadata: what was read, and what was not.\n");
    out
}

pub fn help_for(verb: &Verb) -> String {
    let mut out = format!("{} {}\n\n  {}\n\n", crate::TOOL, verb.name, verb.question);
    out.push_str(&format!("Usage\n  {}\n\n", verb.usage));
    out.push_str(&format!("{}\n\n", wrap(verb.notes, 76)));
    out.push_str("Flags\n");
    for flag in verb.flags {
        out.push_str(&flag_line(flag));
    }
    out.push_str(&format!("  {:<28} {}\n", "-h", "Same as --help"));
    out
}

pub fn help_noun(noun: &Noun) -> String {
    let mut out = format!("{} {} — {}\n\n", crate::TOOL, noun.name, noun.question);
    for verb in noun.verbs {
        let aspect = match verb.name.strip_prefix(noun.name) {
            Some("") => noun.name,
            Some(rest) => rest.trim_start(),
            None => verb.name,
        };
        out.push_str(&format!("  {aspect:<22} {}\n", verb.question));
        out.push_str(&format!("  {:<22} {}\n", "", verb.usage));
    }
    if noun.verbs.len() > 1 {
        out.push_str(&format!(
            "\nRun `{} {} <aspect> --help` for one aspect's flags.\n",
            crate::TOOL,
            noun.name
        ));
    } else {
        out.push_str(&format!(
            "\nRun `{} {} --help` for its flags.\n",
            crate::TOOL,
            noun.name
        ));
    }
    out
}

fn flag_line(flag: &Flag) -> String {
    let head = if flag.takes_value {
        format!("{} {}", flag.name, flag.value)
    } else {
        flag.name.to_string()
    };
    format!("  {head:<28} {}\n", flag.one_line)
}

/// Wrap to a width, because a help screen an agent reads has to fit one.
fn wrap(text: &str, width: usize) -> String {
    let mut out = String::new();
    let mut column = 0;
    for word in text.split_whitespace() {
        if column > 0 && column + 1 + word.len() > width {
            out.push('\n');
            column = 0;
        } else if column > 0 {
            out.push(' ');
            column += 1;
        }
        out.push_str(word);
        column += word.len();
    }
    out
}

/// A verb's aspect name, with the noun it belongs to. Used by the `# next:` line so a paged
/// answer names the command that produced it.
pub fn command_name(verb: &Verb) -> &'static str {
    verb.name
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_noun_can_resolve_its_own_aspects() {
        for noun in nouns() {
            for verb in noun.verbs {
                assert!(
                    noun.find(verb.name).is_some(),
                    "{} does not resolve {}",
                    noun.name,
                    verb.name
                );
            }
        }
    }

    #[test]
    fn a_verb_name_belongs_to_exactly_one_noun() {
        let mut seen = Vec::new();
        for verb in verbs() {
            assert!(!seen.contains(&verb.name), "{} is listed twice", verb.name);
            seen.push(verb.name);
        }
    }

    #[test]
    fn a_verb_starting_with_its_noun_has_a_resolvable_aspect() {
        for noun in nouns() {
            for verb in noun.verbs {
                if let Some(rest) = verb.name.strip_prefix(noun.name) {
                    assert!(
                        rest.is_empty() || rest.starts_with(' '),
                        "{} is not a `{}` aspect",
                        verb.name,
                        noun.name
                    );
                }
            }
        }
    }
}
