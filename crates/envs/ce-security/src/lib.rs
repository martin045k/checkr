mod analysis;

use std::collections::{BTreeMap, BTreeSet};

use analysis::{Security, SecurityLattice};
use ce_core::{define_env, rand, Env, Generate, ValidationResult};
use gcl::{
    ast::{Commands, Target, TargetDef, Variable},
    memory::Memory,
};
use itertools::Itertools;
use rand::seq::SliceRandom;
use serde::{Deserialize, Serialize};
use stdx::stringify::Stringify;

define_env!(SecurityEnv);

#[derive(
    tapi::Tapi, Debug, Default, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize,
)]
#[tapi(path = "SecurityAnalysis")]
pub struct Flow {
    pub from: String,
    pub into: String,
}
pub fn flow(from: impl ToString, to: impl ToString) -> Flow {
    Flow {
        from: from.to_string(),
        into: to.to_string(),
    }
}

#[derive(tapi::Tapi, Debug, Default, Clone, PartialEq, Serialize, Deserialize)]
#[tapi(path = "SecurityAnalysis")]
pub struct SecurityLatticeInput {
    pub rules: Vec<Flow>,
}

#[derive(tapi::Tapi, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[tapi(path = "SecurityAnalysis")]
pub struct Input {
    pub commands: Stringify<Commands>,
    pub classification: BTreeMap<String, String>,
    pub lattice: SecurityLatticeInput,
}

#[derive(tapi::Tapi, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[tapi(path = "SecurityAnalysis")]
pub struct Output {
    pub actual: Vec<Flow>,
    pub allowed: Vec<Flow>,
    pub violations: Vec<Flow>,
    pub is_secure: bool,
}

#[derive(tapi::Tapi, Debug, Default, Clone, PartialEq, Serialize, Deserialize)]
#[tapi(path = "SecurityAnalysis")]
pub struct Meta {
    pub lattice: SecurityLattice,
    pub targets: BTreeSet<TargetDef>,
}

impl Env for SecurityEnv {
    type Input = Input;

    type Output = Output;

    type Meta = Meta;

    fn meta(input: &Self::Input) -> Self::Meta {
        let Ok(commands) =
            input
                .commands
                .try_parse()
                .map_err(ce_core::EnvError::invalid_input_for_program(
                    "failed to parse commands",
                ))
        else {
            return Default::default();
        };

        Meta {
            lattice: SecurityLattice::new(&input.lattice.rules),
            targets: commands.fv().into_iter().map(|t| t.def()).collect(),
        }
    }

    fn run(input: &Self::Input) -> ce_core::Result<Self::Output> {
        let commands =
            input
                .commands
                .try_parse()
                .map_err(ce_core::EnvError::invalid_input_for_program(
                    "failed to parse commands",
                ))?;

        let lattice = SecurityLattice::new(&input.lattice.rules);

        let actual = commands.flows();
        let allowed = lattice
            .all_allowed(
                &input
                    .classification
                    .iter()
                    .map(|(k, v)| (Target::Variable(Variable(k.clone())), v.clone()))
                    .collect(),
            )
            .collect_vec();
        let violations = actual
            .iter()
            .filter(|f| !allowed.contains(f))
            .cloned()
            .collect_vec();

        let is_secure = violations.is_empty();

        Ok(Output {
            actual: actual.into_iter().collect(),
            allowed: allowed.into_iter().collect(),
            violations,
            is_secure,
        })
    }

    fn validate(_input: &Self::Input, _output: &Self::Output) -> ce_core::Result<ValidationResult> {
        Ok(ValidationResult::CorrectTerminated)
    }
}

impl Generate for Input {
    type Context = ();

    fn gen<R: rand::Rng>(_cx: &mut Self::Context, rng: &mut R) -> Self {
        let commands = Commands::gen(&mut Default::default(), rng);

        let lattice_options = [
            // public < private
            vec![flow("public", "private")],
            // unclassified < classified, classified < secret, secret < top_secret
            vec![
                flow("unclassified", "classified"),
                flow("classified", "secret"),
                flow("secret", "top_secret"),
            ],
            // trusted < dubious
            vec![flow("trusted", "dubious")],
            // known_facts < conjecture, conjecture < alternative_facts
            vec![
                flow("known_facts", "conjecture"),
                flow("conjecture", "alternative_facts"),
            ],
            // low < high
            vec![flow("low", "high")],
            // clean < Facebook, clean < Google, clean < Microsoft
            vec![
                flow("clean", "Facebook"),
                flow("clean", "Google"),
                flow("clean", "Microsoft"),
            ],
        ];

        let lattice = SecurityLatticeInput {
            rules: lattice_options.choose(rng).unwrap().clone(),
        };
        let classes = lattice
            .rules
            .iter()
            .flat_map(|f| [f.from.clone(), f.into.clone()])
            .sorted()
            .dedup()
            .collect_vec();

        let classification = Memory::from_targets_with(
            commands.fv(),
            rng,
            |rng, _| classes.choose(rng).unwrap().clone(),
            |rng, _| classes.choose(rng).unwrap().clone(),
        )
        .iter()
        .map(|r| (r.target().name().to_string(), r.value().clone()))
        .collect();

        Input {
            commands: Stringify::new(commands),
            classification,
            lattice,
        }
    }
}
