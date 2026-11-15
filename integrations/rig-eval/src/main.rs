//! Run the playbook's harness comparison over real-work scenarios and score it.
//!
//! `integrations/rig/tests/redirect.rs` proves the plugin mechanics with a scripted model. This
//! binary answers the next question: over a task that needs several reads, how do rig (with and
//! without the playbook plugin), opencode and Pi compare? Each run is the harness's own headless
//! mode in a scenario's working directory, with the same model and the same sealed corpus. The
//! raw stream is kept verbatim beside the score, so every number in the summary can be re-derived
//! from a file.
//!
//! The corpus ships at `integrations/rig-eval/scenarios`; `--scenarios` points elsewhere if you
//! keep a private copy. A corpus inside the checkout is only run where the macOS sandbox is
//! available, because the sandbox is what denies the agent the gold it is scored against. `--out`
//! is explicit: the streams and results are the artefact you keep. `--container` replaces both with
//! a container per trial: the snapshot is mounted at `/repo`, and the image holds no corpus. A
//! scenario's gold is a set of `path:line` citations, plus optional paths and literal strings the
//! answer must contain, and literal strings the tool output must contain. Every check is validated
//! before a run, so stale gold is an error rather than a model failure.
//!
//! ```sh
//! cargo run --manifest-path integrations/rig-eval/Cargo.toml -- --out /tmp/rig-eval --dry-run
//! cargo run --manifest-path integrations/rig-eval/Cargo.toml -- --out /tmp/rig-eval
//! ```
//!
//! It is a separate package on purpose: a runner inside `integrations/rig` is part of the thing
//! the scenarios investigate, and a model asked about the plugin reads it.
//!
//! The rig homes default to `.tmp/rig-home` (gated) and `.tmp/rig-home-baseline` (tools only);
//! `--target rig:vanilla` is the no-plugin control. The model key is read from
//! `.tmp/rig-home/model.env` and never printed.

mod cli;
mod container;
mod driver;
mod export;
mod manifest;
mod process;
mod publish;
mod report;
mod sandbox;
mod scenario;
mod score;
mod snapshot;
mod stream;
mod tag;
mod target;

use std::collections::BTreeMap;
use std::path::Path;
use std::process::ExitCode;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc;
use std::thread;

use anyhow::{bail, Context, Result};

use crate::cli::Args;
use crate::driver::RunContext;
use crate::manifest::Manifest;
use crate::process::Isolation;
use crate::report::RunResult;
use crate::scenario::Scenario;
use crate::target::Target;

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("rig-eval: {error:#}");
            ExitCode::FAILURE
        }
    }
}

fn run() -> Result<()> {
    let args = Args::parse()?;
    if let Some(reuse) = &args.reuse {
        let results = report::read(reuse)?;
        let identity = manifest::read(reuse).ok();
        std::fs::create_dir_all(&args.out)
            .with_context(|| format!("creating {}", args.out.display()))?;
        report::write(&args, &results, identity.as_ref())?;
        if let Some(directory) = &args.publish {
            let identity = identity.as_ref().with_context(|| {
                format!(
                    "--publish needs a manifest; {} has none, so the run cannot name its corpus",
                    reuse.display()
                )
            })?;
            publish(&args, directory, &results, identity)?;
        }
        println!(
            "wrote {} export(s) to {}",
            results.len(),
            args.out.display()
        );
        return Ok(());
    }
    let started_ms = manifest::now_ms()?;
    let template = snapshot::Template::build(&args)?;
    let scenarios = scenario::load(&args, &template.path)?;
    if scenarios.is_empty() {
        bail!("no scenarios in {}", args.scenario_dir.display());
    }

    if args.dry_run {
        report::print_plan(&args, &scenarios);
        snapshot::discard(&template.path);
        return Ok(());
    }

    let vars = cli::read_env(&args.env_file)?;
    std::fs::create_dir_all(&args.out)
        .with_context(|| format!("creating {}", args.out.display()))?;

    let image = match &args.container {
        Some(container) => Some(container::Image::inspect(
            &container.reference,
            container.cpus,
            &container.memory,
        )?),
        None => None,
    };
    let homes = match &image {
        Some(_) => homes(&args)?,
        None => BTreeMap::new(),
    };
    let metadata = match &image {
        Some(image) => {
            let required = homes
                .iter()
                .map(|(label, blob)| (label.as_str(), blob.as_str()))
                .collect();
            let requirements = container::Requirements {
                homes: required,
                rig: args.targets.iter().any(Target::is_rig),
                opencode: args
                    .targets
                    .iter()
                    .any(|target| matches!(target, Target::Opencode(_))),
                pi: args
                    .targets
                    .iter()
                    .any(|target| matches!(target, Target::Pi(_))),
                toolkit: args.targets.iter().any(|target| target.toolkit().on_path()),
            };
            Some(image.preflight(&requirements)?)
        }
        None => None,
    };

    let sandbox = match &image {
        Some(_) => None,
        None => sandbox::profile(&args),
    };
    match (&image, &metadata) {
        (Some(image), Some(metadata)) => {
            println!(
                "# isolation: container {} ({})",
                image.reference, image.digest
            );
            let checkout = manifest::head(&args.root).unwrap_or_else(|| "unknown".to_owned());
            let mismatch = if checkout == metadata.head {
                String::new()
            } else {
                " · the image's toolkit and plugin are from a different revision".to_owned()
            };
            println!(
                "# image: built from {}; checkout at {}{mismatch}",
                metadata.head.get(..12).unwrap_or(&metadata.head),
                checkout.get(..12).unwrap_or(&checkout)
            );
        }
        _ => match &sandbox {
            Some(_) => println!(
                "# sandbox: reads of {} and {} are denied to the agent",
                args.scenario_dir.display(),
                args.out.display()
            ),
            None => {
                println!("# sandbox: not available; the agent can read the corpus and the results")
            }
        },
    }

    let jobs = jobs(&scenarios, &args.targets, args.repeat);
    let results = run_jobs(
        &args,
        &scenarios,
        &jobs,
        &template,
        &vars,
        &sandbox,
        image.as_ref(),
    );
    snapshot::discard(&template.path);
    let results = results?;
    let env_names: Vec<String> = vars.iter().map(|(name, _)| name.clone()).collect();
    let isolation = match (&image, &sandbox) {
        (Some(_), _) => manifest::Isolation::Container,
        (None, Some(_)) => manifest::Isolation::Seatbelt,
        (None, None) => manifest::Isolation::Unsealed,
    };
    let record = match (&image, &metadata) {
        (Some(image), Some(metadata)) => Some(manifest::ContainerRecord {
            image: image.reference.clone(),
            digest: image.digest.clone(),
            cpus: image.cpus,
            memory: image.memory.clone(),
            head: metadata.head.clone(),
            homes: homes.clone(),
        }),
        _ => None,
    };
    let manifest = manifest::build(
        &args,
        &env_names,
        manifest::Identity {
            snapshot: &template.advertised,
            unadvertised: Some(&template.unadvertised),
            started_ms,
            isolation,
            container: record,
        },
        &results,
    )?;
    manifest::write(&args, &manifest)?;
    report::write(&args, &results, Some(&manifest))?;
    if let Some(directory) = &args.publish {
        publish(&args, directory, &results, &manifest)?;
    }
    Ok(())
}

/// Write the site's public projection, using the summary the report just wrote.
fn publish(
    args: &Args,
    directory: &Path,
    results: &[RunResult],
    manifest: &Manifest,
) -> Result<()> {
    let summary_path = args.out.join("summary.md");
    let summary = std::fs::read_to_string(&summary_path)
        .with_context(|| format!("reading {}", summary_path.display()))?;
    let id = crate::publish::write(
        directory,
        args.reuse.as_deref().unwrap_or(&args.out),
        results,
        manifest,
        &summary,
    )?;
    println!("published {id} to {}", directory.join(&id).display());
    Ok(())
}

fn jobs(scenarios: &[Scenario], targets: &[Target], repeat: u32) -> Vec<(usize, usize, u32)> {
    let mut jobs = Vec::new();
    for scenario in 0..scenarios.len() {
        for target in 0..targets.len() {
            for run in 1..=repeat {
                jobs.push((scenario, target, run));
            }
        }
    }
    jobs
}

/// The rig arms a run names, with the settings blob each home's build must match. One label is one
/// home: `rig:gated` and `rig:gated:cli` differ in what the snapshot advertises, not in the image.
fn homes(args: &Args) -> Result<BTreeMap<String, String>> {
    let mut homes = BTreeMap::new();
    for target in &args.targets {
        if let Target::Rig(home, _) = target {
            let settings = home.path.join("settings.toml");
            homes.insert(home.label.clone(), manifest::blob(&settings)?);
        }
    }
    Ok(homes)
}

enum Message {
    Done {
        index: usize,
        result: Box<RunResult>,
    },
    Job {
        index: usize,
        error: String,
    },
    Worker {
        error: String,
    },
}

fn run_jobs(
    args: &Args,
    scenarios: &[Scenario],
    jobs: &[(usize, usize, u32)],
    template: &snapshot::Template,
    env: &[(String, String)],
    sandbox: &Option<String>,
    image: Option<&container::Image>,
) -> Result<Vec<RunResult>> {
    let workers = args.jobs.min(jobs.len()).max(1);
    let base_env = env.to_vec();
    let (present, absent) = match image {
        Some(_) => (
            std::ffi::OsString::from(container::path(true)),
            std::ffi::OsString::from(container::path(false)),
        ),
        None => {
            let parent = std::env::var_os("PATH").unwrap_or_default();
            let present = std::env::join_paths(
                std::iter::once(template.bin.clone()).chain(std::env::split_paths(&parent)),
            )
            .context("building the snapshot PATH")?;
            let absent = std::env::join_paths(
                [
                    template.bin_absent.clone(),
                    template.stubs.clone(),
                    template.bin.clone(),
                ]
                .into_iter()
                .chain(std::env::split_paths(&parent)),
            )
            .context("building the absent PATH")?;
            (present, absent)
        }
    };
    let mut env_present = base_env.clone();
    env_present.push(("PATH".to_owned(), present.to_string_lossy().into_owned()));
    let mut env_absent = base_env;
    env_absent.push(("PATH".to_owned(), absent.to_string_lossy().into_owned()));
    let env_present = &env_present;
    let env_absent = &env_absent;
    let next = AtomicUsize::new(0);
    let (sender, receiver) = mpsc::channel();
    let mut results: Vec<Option<RunResult>> = jobs.iter().map(|_| None).collect();
    let mut errors = Vec::new();

    thread::scope(|scope| {
        for worker_index in 0..workers {
            let sender = sender.clone();
            let next = &next;
            scope.spawn(move || {
                let worker = match snapshot::worker(template, worker_index) {
                    Ok(path) => path,
                    Err(error) => {
                        let _ = sender.send(Message::Worker {
                            error: format!("worker {worker_index}: {error:#}"),
                        });
                        return;
                    }
                };
                loop {
                    let index = next.fetch_add(1, Ordering::Relaxed);
                    if index >= jobs.len() {
                        break;
                    }
                    let (scenario_index, target_index, repeat) = jobs[index];
                    let scenario = &scenarios[scenario_index];
                    let target = &args.targets[target_index];
                    let toolkit = target.toolkit();
                    let env = if toolkit.on_path() {
                        env_present
                    } else {
                        env_absent
                    };
                    if let Err(error) = snapshot::reset(&worker, template.commit_for(toolkit)) {
                        let _ = sender.send(Message::Job {
                            index,
                            error: format!("{error:#}"),
                        });
                        break;
                    }
                    let workdir = worker.join(&scenario.workdir);
                    let arm = target.label().replace(':', "-");
                    let isolation = match image {
                        Some(image) => {
                            let home = match target {
                                Target::Rig(home, _) => Some(home.label.clone()),
                                _ => None,
                            };
                            let sessions = match &home {
                                Some(_) => {
                                    let dir = args
                                        .out
                                        .join("homes")
                                        .join(format!("{}-{arm}-r{repeat}", scenario.name))
                                        .join("sessions");
                                    if let Err(error) = std::fs::create_dir_all(&dir) {
                                        let _ = sender.send(Message::Job {
                                            index,
                                            error: format!(
                                                "creating the sessions mount {}: {error:#}",
                                                dir.display()
                                            ),
                                        });
                                        break;
                                    }
                                    Some(dir)
                                }
                                None => None,
                            };
                            let opencode_config = matches!(target, Target::Opencode(_))
                                .then(|| args.opencode_config.clone());
                            Isolation::Container(container::Trial {
                                image: image.clone(),
                                workspace: worker.clone(),
                                home,
                                sessions,
                                opencode_config,
                            })
                        }
                        None => Isolation::Host {
                            profile: sandbox.clone(),
                        },
                    };
                    let context = RunContext {
                        args,
                        env,
                        isolation: &isolation,
                        arm,
                    };
                    match target::run_one(target, scenario, &workdir, repeat, &context) {
                        Ok(result) => {
                            let _ = sender.send(Message::Done {
                                index,
                                result: Box::new(result),
                            });
                        }
                        Err(error) => {
                            let _ = sender.send(Message::Job {
                                index,
                                error: format!("{error:#}"),
                            });
                            break;
                        }
                    }
                }
                snapshot::discard(&worker);
            });
        }
        drop(sender);

        for message in receiver {
            match message {
                Message::Done { index, result } => {
                    let result = *result;
                    println!(
                        "· {} · {} · run {}/{}",
                        result.scenario, result.target, result.repeat, args.repeat
                    );
                    report::print_run(&result);
                    results[index] = Some(result);
                }
                Message::Job { index, error } => {
                    let (scenario, target, repeat) = jobs[index];
                    eprintln!(
                        "· {} · {} · run {}/{}: {error}",
                        scenarios[scenario].name,
                        args.targets[target].label(),
                        repeat,
                        args.repeat
                    );
                    errors.push(error);
                }
                Message::Worker { error } => {
                    eprintln!("worker: {error}");
                    errors.push(error);
                }
            }
        }
    });

    if !errors.is_empty() {
        bail!(
            "{} job(s) failed; the first was: {}",
            errors.len(),
            errors[0]
        );
    }
    Ok(results.into_iter().flatten().collect())
}
