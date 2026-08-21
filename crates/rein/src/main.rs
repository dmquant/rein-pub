//! `rein` — grammar `rein [global] <resource> <action> [args] [options]` (§9).
//! M1 surface; resources grow only as each earns a consumer (§12).

mod cmds;
mod out;
mod tui;

use clap::{Parser, Subcommand};
use cmds::{CliError, Ctx, WaitAssertion};
use out::{CmdOutput, OutputFormat};
use rein_runtime::clock::{Clock, SystemClock};
use std::path::PathBuf;

#[derive(Parser, Debug)]
#[command(
    name = "rein",
    version,
    about = "Rein — a financial research harness. Executes bounded research as fenced attempts with receipts."
)]
struct Cli {
    /// Workspace directory (defaults to the nearest `.rein` above the cwd)
    #[arg(long, global = true, env = "REIN_WORKSPACE")]
    workspace: Option<PathBuf>,

    /// Config root — credentials live here, never in the workspace (invariant 27)
    #[arg(long, global = true, env = "REIN_CONFIG_ROOT")]
    config_root: Option<PathBuf>,

    /// Output format: table | json | yaml | ndjson
    #[arg(long, global = true, default_value = "table", env = "REIN_OUTPUT")]
    output: OutputFormat,

    #[command(subcommand)]
    command: Cmd,
}

#[derive(Subcommand, Debug)]
enum Cmd {
    /// Initialize a workspace (.rein layout, §11)
    Init {
        #[arg(long, default_value = "local")]
        workspace_ref: String,
    },
    /// Workspace summary
    Status,
    /// Diagnostics: ledger integrity, append-only triggers, CAS probe, config boundary
    Doctor,
    /// Attach the four-screen TUI (§10)
    Tui,
    /// Provider pins (invariant 8)
    Provider {
        #[command(subcommand)]
        cmd: ProviderCmd,
    },
    /// Hands (execution binding; fixtures at M1)
    Hand {
        #[command(subcommand)]
        cmd: HandCmd,
    },
    /// Missions
    Mission {
        #[command(subcommand)]
        cmd: MissionCmd,
    },
    /// Epochs (frozen research periods)
    Epoch {
        #[command(subcommand)]
        cmd: EpochCmd,
    },
    /// Plans (immutable task DAGs)
    Plan {
        #[command(subcommand)]
        cmd: PlanCmd,
    },
    /// Tasks
    Task {
        #[command(subcommand)]
        cmd: TaskCmd,
    },
    /// Attempts (one fenced try under one ContextPack)
    Attempt {
        #[command(subcommand)]
        cmd: AttemptCmd,
    },
    /// Ergonomic alias: run a task once
    Run {
        task: String,
        #[arg(long)]
        hand: Option<String>,
        #[arg(long)]
        wait: bool,
        #[arg(long, requires = "wait")]
        require: Option<WaitAssertion>,
    },
    /// Committed artifacts (content-addressed)
    Artifact {
        #[command(subcommand)]
        cmd: ArtifactCmd,
    },
    /// Validation receipts
    Validation {
        #[command(subcommand)]
        cmd: ValidationCmd,
    },
    /// Strict replay against the original record
    Replay {
        #[command(subcommand)]
        cmd: ReplayCmd,
    },
    /// Run event streams
    Events {
        #[command(subcommand)]
        cmd: EventsCmd,
    },
    /// Data tools (M2): stamped pulls captured to CAS or refused
    Data {
        #[command(subcommand)]
        cmd: DataCmd,
    },
    /// The workspace capture index
    Capture {
        #[command(subcommand)]
        cmd: CaptureCmd,
    },
    /// Evidence bundles (§8): assemble and deterministically verify
    Evidence {
        #[command(subcommand)]
        cmd: EvidenceCmd,
    },
    /// The recovery queue across the workspace
    Recover,
    /// Skill playbooks: list, validate, generate drafts from run evidence,
    /// and promote — generation drafts, validation gates, the operator
    /// promotes. Nothing self-authorizes.
    Skill {
        #[command(subcommand)]
        cmd: SkillCmd,
    },
    /// Evaluation, two-track (§4): financegym research scoring + internal
    /// settled-material hand ranking. Scores never touch TerminalOutcome.
    Eval {
        #[command(subcommand)]
        cmd: EvalCmd,
    },
}

#[derive(Subcommand, Debug)]
enum EvalCmd {
    /// Score a FinanceGym-style question set (bundled sample when no file)
    Financegym {
        #[arg(short, long)]
        file: Option<String>,
        /// JSON map of question id → answer text to score
        #[arg(long)]
        answers: Option<String>,
        /// JSON map of question id → rubric tier 0–4 from an external grader
        #[arg(long)]
        grades: Option<String>,
    },
    /// Rank hands on the estate's own settled valuations
    Internal,
    /// Batch-answer a question file: each question runs as a real, resumable
    /// attempt through the chosen hand; answers land in one JSON file
    Answers {
        #[arg(short, long)]
        file: Option<String>,
        #[arg(long)]
        hand: String,
        /// Answer at most this many questions this run
        #[arg(long)]
        limit: Option<usize>,
        #[arg(long, default_value_t = 0)]
        offset: usize,
        #[arg(long, default_value = "answers.json")]
        out: String,
    },
    /// Grade answered questions with an external judge model per the 0–4
    /// rubric. Tiers land in a grades file for `--grades` — never in outcomes
    Grade {
        #[arg(short, long)]
        file: Option<String>,
        /// JSON map of question id → answer text (from `eval answers`)
        #[arg(long)]
        answers: String,
        /// id → tier map (resumable; reasons land in <out>.reasons.json)
        #[arg(long, default_value = "grades.json")]
        out: String,
        /// Judge binary (defaults to config agy_path, then `agy`)
        #[arg(long)]
        judge: Option<String>,
        /// Judge model (defaults to config agy_model)
        #[arg(long)]
        judge_model: Option<String>,
        /// Grade at most this many this run
        #[arg(long)]
        limit: Option<usize>,
        #[arg(long, default_value_t = 0)]
        offset: usize,
    },
}

#[derive(Subcommand, Debug)]
enum SkillCmd {
    /// Installed skills and drafts, each with its validation status
    List,
    /// Deterministic checks on one skill file (name, path, or draft)
    Validate { name: String },
    /// Draft a new skill, distilling lessons from named attempts
    New {
        name: String,
        #[arg(long = "applies-to", default_value = "research")]
        applies_to: String,
        /// Attempts whose receipts become the draft's evidence (repeatable)
        #[arg(long = "from-attempt")]
        from_attempt: Vec<String>,
    },
    /// Move a VALID draft into force (operator act; refuses invalid drafts)
    Promote {
        name: String,
        /// Install under this task-type file name instead of the skill name
        #[arg(long = "as")]
        as_type: Option<String>,
    },
}

#[derive(Subcommand, Debug)]
enum EvidenceCmd {
    /// Publish a bundle summary to an AGORA room (explicit, never ambient)
    Publish {
        attempt: String,
        #[arg(long)]
        room: Option<String>,
        #[arg(long)]
        hub: Option<String>,
    },
    /// Assemble an attempt's evidence into a .tar.zst bundle
    Bundle {
        attempt: String,
        #[arg(long)]
        out: Option<String>,
    },
    /// Re-check every digest, sequence and receipt in a bundle
    Verify { path: String },
    /// Alias of bundle (export to a path)
    Export {
        attempt: String,
        #[arg(long)]
        out: Option<String>,
    },
}

#[derive(Subcommand, Debug)]
enum DataCmd {
    /// Pull FMP equity endpoints for a symbol under the sealed epoch
    PullEquity {
        symbol: String,
        /// quote,profile,income,income-q,balance,balance-q,cashflow,
        /// cashflow-q,estimates,prices,transcripts | all
        #[arg(long, default_value = "all")]
        kinds: String,
    },
    /// SearXNG search (hits only; capture happens on fetch)
    Search { query: String },
    /// Fetch a URL and capture today's bytes (production mode)
    Fetch { url: String },
    /// Pin a local file as an operator-provenance capture — the input path
    /// for ops task types (verify/settle/monitor)
    Pin {
        file: String,
        /// The note tag hands find inputs by (e.g. "claims", "meta", "series-prior")
        #[arg(long)]
        note: String,
        /// What time the content is *about*, RFC3339 (optional)
        #[arg(long)]
        as_of: Option<String>,
    },
}

#[derive(Subcommand, Debug)]
enum CaptureCmd {
    List,
}

#[derive(Subcommand, Debug)]
enum ProviderCmd {
    /// Add or update a pin (exactly one of --digest / --pin-method)
    Add {
        name: String,
        #[arg(long)]
        coordinate: String,
        #[arg(long)]
        digest: Option<String>,
        #[arg(long)]
        pin_method: Option<String>,
    },
    /// Alias of add
    Pin {
        name: String,
        #[arg(long)]
        coordinate: String,
        #[arg(long)]
        digest: Option<String>,
        #[arg(long)]
        pin_method: Option<String>,
    },
    List,
    Verify,
    /// Regenerate providers.lock (deterministic except one labeled timestamp)
    Lock,
}

#[derive(Subcommand, Debug)]
enum HandCmd {
    List,
    Show {
        selector: String,
    },
    /// Conformance probe in a scratch sandbox (no ledger writes)
    Test {
        selector: String,
    },
}

#[derive(Subcommand, Debug)]
enum MissionCmd {
    Create {
        name: String,
        #[arg(long)]
        objective: String,
    },
    List,
    Show {
        name: String,
    },
    Close {
        name: String,
    },
}

#[derive(Subcommand, Debug)]
enum EpochCmd {
    Open {
        name: String,
        #[arg(long)]
        mission: String,
        #[arg(long)]
        source_cutoff: String,
        #[arg(long)]
        knowledge_cutoff: Option<String>,
        #[arg(long, default_value = "production")]
        pit_mode: String,
        #[arg(long)]
        seal: bool,
        #[arg(long, default_value_t = 24)]
        max_steps: u32,
        #[arg(long, default_value_t = 240_000)]
        per_step_timeout_ms: u64,
    },
    Seal {
        name: String,
    },
    List,
    Show {
        name: String,
    },
}

#[derive(Subcommand, Debug)]
enum PlanCmd {
    /// Apply a plan file (yaml/json): {plan_ref, nodes:[{task_ref, depends_on, task_type}]}
    Apply {
        #[arg(short, long)]
        file: String,
    },
    Validate {
        #[arg(short, long)]
        file: String,
    },
    Show {
        name: String,
    },
    /// Run every ready task in the plan to adjudication
    Run {
        name: String,
        #[arg(long)]
        hand: Option<String>,
    },
}

#[derive(Subcommand, Debug)]
enum TaskCmd {
    Add {
        name: String,
        #[arg(long)]
        plan: String,
        #[arg(long = "type", default_value = "research")]
        task_type: String,
        #[arg(long)]
        contract_file: Option<String>,
        /// Pin a capture as input (repeatable): capture:<sha256:…>
        #[arg(long = "input")]
        inputs: Vec<String>,
        /// Instrument in scope (repeatable): security:<sym>
        #[arg(long = "universe")]
        universe: Vec<String>,
    },
    List,
    Show {
        name: String,
    },
    /// Tasks whose dependencies are satisfied and which are not
    Ready,
}

#[derive(Subcommand, Debug)]
enum AttemptCmd {
    Start {
        task: String,
        #[arg(long)]
        hand: Option<String>,
        #[arg(long)]
        wait: bool,
        #[arg(long, requires = "wait")]
        require: Option<WaitAssertion>,
    },
    List,
    Show {
        id: String,
    },
    Watch {
        id: String,
    },
    /// Bounded cancellation: honored at the next phase boundary
    Cancel {
        id: String,
    },
    /// Recovery console (§8): diagnosis first, then one of exactly three
    /// safe actions. Force-success does not exist.
    Recover {
        id: String,
        #[arg(long)]
        action: Option<String>,
        /// Deliberately change the executor for this recovery (default:
        /// the attempt's original hand)
        #[arg(long)]
        hand: Option<String>,
    },
    /// Operational retry under the byte-identical ContextPack (invariant 6)
    Retry {
        id: String,
        #[arg(long)]
        hand: Option<String>,
    },
    /// Close a recovery-pending attempt as unknown — the explicit path
    /// (invariant 5). Force-success does not exist.
    Close {
        id: String,
        #[arg(long, default_value = "closed_as_unknown_by_operator")]
        reason: String,
    },
}

#[derive(Subcommand, Debug)]
enum ArtifactCmd {
    List {
        #[arg(long)]
        attempt: Option<String>,
    },
    Cat {
        digest: String,
    },
    Verify {
        digest: Option<String>,
    },
}

#[derive(Subcommand, Debug)]
enum ValidationCmd {
    List { attempt: String },
}

#[derive(Subcommand, Debug)]
enum ReplayCmd {
    Attempt {
        id: String,
        #[arg(long)]
        strict: bool,
        /// Accepted for §9 compatibility; comparison to the original is what
        /// replay always does.
        #[arg(long)]
        compare_to_original: bool,
    },
}

#[derive(Subcommand, Debug)]
enum EventsCmd {
    List {
        run: String,
    },
    Tail {
        run: String,
        #[arg(long, default_value_t = 20)]
        n: usize,
    },
}

fn dispatch(cli: &Cli, ctx: &Ctx) -> Result<CmdOutput, CliError> {
    match &cli.command {
        Cmd::Init { workspace_ref } => cmds::init(ctx, workspace_ref),
        Cmd::Status => cmds::status(ctx),
        Cmd::Doctor => cmds::doctor(ctx),
        Cmd::Tui => cmds::tui(ctx),
        Cmd::Provider { cmd } => match cmd {
            ProviderCmd::Add {
                name,
                coordinate,
                digest,
                pin_method,
            }
            | ProviderCmd::Pin {
                name,
                coordinate,
                digest,
                pin_method,
            } => cmds::provider_add(
                ctx,
                name,
                coordinate,
                digest.as_deref(),
                pin_method.as_deref(),
            ),
            ProviderCmd::List => cmds::provider_list(ctx),
            ProviderCmd::Verify => cmds::provider_verify(ctx),
            ProviderCmd::Lock => cmds::provider_lock(ctx),
        },
        Cmd::Hand { cmd } => match cmd {
            HandCmd::List => cmds::hand_list(ctx),
            HandCmd::Show { selector } => cmds::hand_show(ctx, selector),
            HandCmd::Test { selector } => cmds::hand_test(ctx, selector),
        },
        Cmd::Mission { cmd } => match cmd {
            MissionCmd::Create { name, objective } => cmds::mission_create(ctx, name, objective),
            MissionCmd::List => cmds::mission_list(ctx),
            MissionCmd::Show { name } => cmds::mission_show(ctx, name),
            MissionCmd::Close { name } => cmds::mission_close(ctx, name),
        },
        Cmd::Epoch { cmd } => match cmd {
            EpochCmd::Open {
                name,
                mission,
                source_cutoff,
                knowledge_cutoff,
                pit_mode,
                seal,
                max_steps,
                per_step_timeout_ms,
            } => cmds::epoch_open(
                ctx,
                name,
                mission,
                source_cutoff,
                knowledge_cutoff.as_deref(),
                pit_mode,
                *seal,
                *max_steps,
                *per_step_timeout_ms,
            ),
            EpochCmd::Seal { name } => cmds::epoch_seal(ctx, name),
            EpochCmd::List => cmds::epoch_list(ctx),
            EpochCmd::Show { name } => cmds::epoch_show(ctx, name),
        },
        Cmd::Plan { cmd } => match cmd {
            PlanCmd::Apply { file } => cmds::plan_apply(ctx, file),
            PlanCmd::Validate { file } => cmds::plan_validate(ctx, file),
            PlanCmd::Show { name } => cmds::plan_show(ctx, name),
            PlanCmd::Run { name, hand } => cmds::plan_run(ctx, name, hand.as_deref()),
        },
        Cmd::Task { cmd } => match cmd {
            TaskCmd::Add {
                name,
                plan,
                task_type,
                contract_file,
                inputs,
                universe,
            } => cmds::task_add(
                ctx,
                name,
                plan,
                task_type,
                contract_file.as_deref(),
                inputs,
                universe,
            ),
            TaskCmd::List => cmds::task_list(ctx),
            TaskCmd::Show { name } => cmds::task_show(ctx, name),
            TaskCmd::Ready => cmds::task_ready(ctx),
        },
        Cmd::Attempt { cmd } => match cmd {
            AttemptCmd::Start {
                task,
                hand,
                wait,
                require,
            } => cmds::attempt_start(ctx, task, hand.as_deref(), *wait, *require),
            AttemptCmd::List => cmds::attempt_list(ctx),
            AttemptCmd::Show { id } => cmds::attempt_show(ctx, id),
            AttemptCmd::Watch { id } => cmds::attempt_watch(ctx, id),
            AttemptCmd::Cancel { id } => cmds::attempt_cancel(ctx, id),
            AttemptCmd::Recover { id, action, hand } => {
                cmds::attempt_recover(ctx, id, action.as_deref(), hand.as_deref())
            }
            AttemptCmd::Retry { id, hand } => cmds::attempt_retry(ctx, id, hand.as_deref()),
            AttemptCmd::Close { id, reason } => cmds::attempt_close(ctx, id, reason),
        },
        Cmd::Run {
            task,
            hand,
            wait,
            require,
        } => cmds::attempt_start(ctx, task, hand.as_deref(), *wait, *require),
        Cmd::Artifact { cmd } => match cmd {
            ArtifactCmd::List { attempt } => cmds::artifact_list(ctx, attempt.as_deref()),
            ArtifactCmd::Cat { digest } => cmds::artifact_cat(ctx, digest),
            ArtifactCmd::Verify { digest } => cmds::artifact_verify(ctx, digest.as_deref()),
        },
        Cmd::Validation { cmd } => match cmd {
            ValidationCmd::List { attempt } => cmds::validation_list(ctx, attempt),
        },
        Cmd::Replay { cmd } => match cmd {
            ReplayCmd::Attempt { id, strict, .. } => cmds::replay_attempt(ctx, id, *strict),
        },
        Cmd::Events { cmd } => match cmd {
            EventsCmd::List { run } => cmds::events_list(ctx, run, None),
            EventsCmd::Tail { run, n } => cmds::events_list(ctx, run, Some(*n)),
        },
        Cmd::Data { cmd } => match cmd {
            DataCmd::PullEquity { symbol, kinds } => cmds::data_pull_equity(ctx, symbol, kinds),
            DataCmd::Search { query } => cmds::data_search(ctx, query),
            DataCmd::Fetch { url } => cmds::data_fetch(ctx, url),
            DataCmd::Pin { file, note, as_of } => cmds::data_pin(ctx, file, note, as_of.as_deref()),
        },
        Cmd::Capture { cmd } => match cmd {
            CaptureCmd::List => cmds::capture_list(ctx),
        },
        Cmd::Evidence { cmd } => match cmd {
            EvidenceCmd::Bundle { attempt, out } | EvidenceCmd::Export { attempt, out } => {
                cmds::evidence_bundle(ctx, attempt, out.as_deref())
            }
            EvidenceCmd::Verify { path } => cmds::evidence_verify(ctx, path),
            EvidenceCmd::Publish { attempt, room, hub } => {
                cmds::evidence_publish(ctx, attempt, room.as_deref(), hub.as_deref())
            }
        },
        Cmd::Recover => cmds::recover_queue(ctx),
        Cmd::Skill { cmd } => match cmd {
            SkillCmd::List => cmds::skill_list(ctx),
            SkillCmd::Validate { name } => cmds::skill_validate(ctx, name),
            SkillCmd::New {
                name,
                applies_to,
                from_attempt,
            } => cmds::skill_new(ctx, name, applies_to, from_attempt),
            SkillCmd::Promote { name, as_type } => {
                cmds::skill_promote(ctx, name, as_type.as_deref())
            }
        },
        Cmd::Eval { cmd } => match cmd {
            EvalCmd::Financegym {
                file,
                answers,
                grades,
            } => cmds::eval_financegym(ctx, file.as_deref(), answers.as_deref(), grades.as_deref()),
            EvalCmd::Internal => cmds::eval_internal(ctx),
            EvalCmd::Answers {
                file,
                hand,
                limit,
                offset,
                out,
            } => cmds::eval_answers(ctx, file.as_deref(), hand, *limit, *offset, out),
            EvalCmd::Grade {
                file,
                answers,
                out,
                judge,
                judge_model,
                limit,
                offset,
            } => cmds::eval_grade(
                ctx,
                file.as_deref(),
                answers,
                out,
                judge.as_deref(),
                judge_model.as_deref(),
                *limit,
                *offset,
            ),
        },
    }
}

fn command_name(cmd: &Cmd) -> String {
    let dbg = format!("{cmd:?}");
    dbg.split([' ', '(', '{'])
        .next()
        .unwrap_or("unknown")
        .to_lowercase()
}

fn main() {
    let cli = Cli::parse();
    let ctx = Ctx {
        start_dir: cli
            .workspace
            .clone()
            .unwrap_or_else(|| std::env::current_dir().expect("cwd")),
        config_root: cli
            .config_root
            .clone()
            .unwrap_or_else(rein_runtime::workspace::default_config_root),
    };
    let name = command_name(&cli.command);
    let result = dispatch(&cli, &ctx);
    let out = match result {
        Ok(out) => out,
        Err(e) => CmdOutput::error(e.exit, e.message),
    };
    let at = SystemClock.now().canonical();
    let exit = out::emit(&name, out, cli.output, at);
    std::process::exit(exit);
}
