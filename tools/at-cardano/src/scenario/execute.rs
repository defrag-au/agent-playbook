//! Running the steps, and the transcript a run leaves.
//!
//! The rule is simple and worth stating because it decides the exit code: **obtaining an answer
//! is fatal, checking one is not.** A target that cannot be answered stops the run with the
//! exit code the CLI would have used — the steps after it would be built on an answer that does
//! not exist. A save or an assert over an answer that did arrive is recorded and the run
//! continues, so one bad expectation does not hide the four after it. A run that finished with
//! failed checks exits 2, the same code as a file that does not parse.

use std::collections::HashMap;
use std::time::Instant;

use crate::contract::{self, Exit, Fail, Outcome, Report};
use crate::koios::Network;
use crate::verbs::{self, Options};

use super::{
    extract, Assert, Check, Echo, Row, Save, Scenario, Step, Target, Template, MAX_PRINTED_LINES,
};

pub fn run(options: &Options) -> Result<Outcome, Fail> {
    let path = options.scenario.as_deref().ok_or_else(|| {
        Fail::usage(
            "`run` needs --scenario <file.dsl> · the grammar is in tools/at-cardano/SCENARIO.md",
        )
    })?;
    let scenario = super::parse::file(path)?;

    if options.check {
        return Ok(check(&scenario, options, path));
    }

    let network = if options.network_explicit {
        options.network
    } else {
        scenario.network.unwrap_or(options.network)
    };
    let steps = scenario.steps.len();
    let pending = pending_saves(&scenario.steps);

    let mut runner = Runner {
        network,
        keyless: options.keyless || scenario.keyless,
        bindings: HashMap::new(),
        saves: vec![None; steps],
        answers: HashMap::new(),
        report: Report::new(),
        steps,
        targets: 0,
        passed: 0,
        failed: 0,
        printed: 0,
        withheld: 0,
        deadline: Instant::now() + scenario.timeout,
        started: Instant::now(),
    };

    runner.header(&scenario, options, path);
    for (index, step) in scenario.steps.iter().enumerate() {
        if let Some(exit) = runner.step(index, step, &pending) {
            return Ok(runner.finish(Some(exit)));
        }
    }
    Ok(runner.finish(None))
}

/// `run --check`: the file parsed, every verb and every reference resolved, and nothing was
/// opened. It answers with its own frame — a valid file is a result even though no row was
/// printed — so CI can gate a scenario without spending a request.
fn check(scenario: &Scenario, options: &Options, path: &str) -> Outcome {
    let network = if options.network_explicit {
        options.network
    } else {
        scenario.network.unwrap_or(options.network)
    };
    let mut targets = 0;
    let mut saves = 0;
    let mut checks = 0;
    let mut echoes = 0;
    for step in &scenario.steps {
        match step {
            Step::Target(_) => targets += 1,
            Step::Save(_) => saves += 1,
            Step::Assert(_) => checks += 1,
            Step::Echo(_) => echoes += 1,
        }
    }

    let mut report = Report::new();
    report.header(crate::TOOL, "run", network.label(), path);
    report.bound(format!(
        "parses · {} · {} · {} · {} · {}",
        contract::plural(scenario.steps.len(), "step"),
        contract::plural(targets, "target"),
        contract::plural(saves, "save"),
        contract::plural(checks, "assert"),
        contract::plural_of(echoes, "echo", "echoes")
    ));
    report.bound(format!(
        "timeout {}s · no request was made",
        scenario.timeout.as_secs()
    ));
    Outcome::from_frame(report)
}

/// A save is read out of an answer as soon as the answer arrives, not when the save's line is
/// reached: that way no response body is held past the step that produced it.
struct Pending {
    step: usize,
    target: String,
    row: Row,
    column: String,
}

#[derive(Clone)]
struct Answer {
    count: Result<usize, String>,
    text: String,
}

struct Runner {
    network: Network,
    keyless: bool,
    bindings: HashMap<String, String>,
    saves: Vec<Option<Result<String, String>>>,
    answers: HashMap<String, Answer>,
    report: Report,
    steps: usize,
    targets: usize,
    passed: usize,
    failed: usize,
    printed: usize,
    withheld: usize,
    deadline: Instant,
    started: Instant,
}

impl Runner {
    fn header(&mut self, scenario: &Scenario, options: &Options, path: &str) {
        self.report
            .header(crate::TOOL, "run", self.network.label(), path);
        self.report.bound(format!(
            "{} · timeout {}s",
            contract::plural(scenario.steps.len(), "step"),
            scenario.timeout.as_secs()
        ));
        if let (true, Some(from_file)) = (options.network_explicit, scenario.network) {
            if from_file != self.network {
                self.report.bound(format!(
                    "--network {} overrides the run line's {}",
                    self.network.label(),
                    from_file.label()
                ));
            }
        }
    }

    fn step(&mut self, index: usize, step: &Step, pending: &[Pending]) -> Option<Exit> {
        match step {
            Step::Target(target) => self.target(target, pending),
            Step::Save(save) => {
                self.save(index, save);
                None
            }
            Step::Assert(assert) => {
                self.assert(assert);
                None
            }
            Step::Echo(echo) => {
                self.echo(echo);
                None
            }
        }
    }

    fn target(&mut self, step: &Target, pending: &[Pending]) -> Option<Exit> {
        let what = format!("target {}", step.name);
        if Instant::now() >= self.deadline {
            self.fail_line(
                step.line,
                &what,
                "the run timeout expired before this step · raise `timeout=` on the run line",
            );
            return Some(Exit::Environment);
        }

        let positionals = match self.fill_all(step.line, &what, &step.positionals) {
            Ok(values) => values,
            Err(()) => return None,
        };

        let mut options = Options {
            network: self.network,
            keyless: self.keyless,
            ..Options::default()
        };
        for field in &step.fields {
            if let Err(message) = field.apply(&mut options, &self.bindings) {
                self.fail_line(step.line, &what, &message);
                return Some(Exit::Usage);
            }
        }

        let answered = match verbs::run_with_answer(step.verb, &positionals, &options) {
            Ok(answered) => answered,
            Err(fail) => {
                self.fail_line(step.line, &what, &fail.message);
                return Some(fail.exit);
            }
        };

        let Some(bytes) = answered.answer.as_deref() else {
            self.fail_line(step.line, &what, "the answer carried no JSON body");
            return Some(Exit::Environment);
        };

        let count = extract::count(bytes);
        let text = answered.outcome.report.content_text();
        self.push(&answered.outcome.report);
        self.targets += 1;

        for candidate in pending
            .iter()
            .filter(|candidate| candidate.target == step.name)
        {
            self.saves[candidate.step] =
                Some(extract::scalar(bytes, candidate.row, &candidate.column));
        }
        self.answers
            .insert(step.name.clone(), Answer { count, text });
        None
    }

    fn save(&mut self, index: usize, step: &Save) {
        let what = format!("save {}", step.bind);
        match self.saves[index].take() {
            None => self.fail_line(
                step.line,
                &what,
                &format!(
                    "target `{}` did not answer, so there was nothing to save",
                    step.source
                ),
            ),
            Some(Ok(value)) => {
                self.report.bound(format!("save {} = {}", step.bind, value));
                self.bindings.insert(step.bind.clone(), value);
                self.passed += 1;
            }
            Some(Err(message)) => self.fail_line(step.line, &what, &message),
        }
    }

    fn assert(&mut self, step: &Assert) {
        let what = format!("assert {}", step.source);
        let Some(answer) = self.answers.get(&step.source).cloned() else {
            self.fail_line(
                step.line,
                &what,
                &format!("target `{}` did not answer", step.source),
            );
            return;
        };

        let mut labels = Vec::new();
        let mut unmet = Vec::new();
        for check in &step.checks {
            match check {
                Check::Count(compare, expected) => {
                    labels.push(format!("count {} {expected}", compare.symbol()));
                    match &answer.count {
                        Ok(actual) if compare.holds(*actual, *expected) => {}
                        Ok(actual) => unmet.push(format!(
                            "count was {actual}, expected {}{expected}",
                            compare.symbol()
                        )),
                        Err(message) => unmet.push(message.clone()),
                    }
                }
                Check::Contains(template) => {
                    labels.push(format!("contains \"{}\"", template.source));
                    match template.fill(&self.bindings) {
                        Ok(needle) if answer.text.contains(&needle) => {}
                        Ok(needle) => unmet.push(format!("the answer does not contain `{needle}`")),
                        Err(message) => unmet.push(message),
                    }
                }
                Check::Absent(template) => {
                    labels.push(format!("absent \"{}\"", template.source));
                    match template.fill(&self.bindings) {
                        Ok(needle) if !answer.text.contains(&needle) => {}
                        Ok(needle) => unmet.push(format!("the answer contains `{needle}`")),
                        Err(message) => unmet.push(message),
                    }
                }
            }
        }

        if unmet.is_empty() {
            self.passed += 1;
            self.report.bound(format!(
                "pass line {} · assert {} · {}",
                step.line,
                step.source,
                labels.join(" · ")
            ));
            return;
        }

        let message = match &step.message {
            Some(template) => match template.fill(&self.bindings) {
                Ok(text) => format!(" · {text}"),
                Err(error) => format!(" · {error}"),
            },
            None => String::new(),
        };
        for failure in unmet {
            self.fail_line(step.line, &what, &format!("{failure}{message}"));
        }
    }

    fn echo(&mut self, step: &Echo) {
        match step.text.fill(&self.bindings) {
            Ok(text) => {
                self.report.content(text);
                self.passed += 1;
            }
            Err(message) => self.fail_line(step.line, "echo", &message),
        }
    }

    fn fill_all(
        &mut self,
        line: usize,
        what: &str,
        templates: &[Template],
    ) -> Result<Vec<String>, ()> {
        let mut values = Vec::new();
        for template in templates {
            match template.fill(&self.bindings) {
                Ok(value) => values.push(value),
                Err(message) => {
                    self.fail_line(line, what, &message);
                    return Err(());
                }
            }
        }
        Ok(values)
    }

    /// Each answer is printed exactly as the verb would print it alone — its own header, its
    /// own bounds — so a transcript line can be cited without saying it came from a scenario.
    fn push(&mut self, report: &Report) {
        for line in report.render().lines() {
            if line.starts_with("# ") {
                self.report.note(line);
            } else if line.is_empty() {
                self.report.note("");
            } else if self.printed < MAX_PRINTED_LINES {
                self.report.content(line);
                self.printed += 1;
            } else {
                self.withheld += 1;
            }
        }
    }

    fn fail_line(&mut self, line: usize, what: &str, message: &str) {
        self.report
            .bound(format!("FAIL line {line} · {what} · {message}"));
        self.failed += 1;
    }

    fn finish(mut self, fatal: Option<Exit>) -> Outcome {
        let elapsed = self.started.elapsed().as_secs_f64();
        self.report.bound(format!(
            "{} · {} · {} passed · {} failed · {elapsed:.1}s",
            contract::plural(self.steps, "step"),
            contract::plural(self.targets, "target"),
            self.passed,
            self.failed
        ));
        if self.withheld > 0 {
            self.report.bound(format!(
                "{} content lines printed · {} withheld by the output bound; checks ran on the \
                 whole answer",
                self.printed, self.withheld
            ));
        }
        if let Some(exit) = fatal {
            self.report.bound(format!(
                "stopped at a step that could not be answered · exit {}",
                exit.code()
            ));
        }
        let exit = fatal.unwrap_or(if self.failed > 0 {
            Exit::Usage
        } else {
            Exit::Results
        });
        Outcome {
            report: self.report,
            exit,
        }
    }
}

fn pending_saves(steps: &[Step]) -> Vec<Pending> {
    steps
        .iter()
        .enumerate()
        .filter_map(|(index, step)| match step {
            Step::Save(save) => Some(Pending {
                step: index,
                target: save.source.clone(),
                row: save.row,
                column: save.column.clone(),
            }),
            _ => None,
        })
        .collect()
}
