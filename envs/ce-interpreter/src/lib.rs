#[cfg(test)]
mod tests;

use std::collections::BTreeSet;

use ce_core::{
    define_env,
    rand::{self, seq::SliceRandom},
    Env, Generate, ValidationResult,
};
use gcl::{
    ast::{Commands, Int, TargetDef},
    interpreter::{Execution, InterpreterMemory, Step, TerminationState},
    pg::{Determinism, Node},
    stringify::Stringify,
};
use serde::{Deserialize, Serialize};

define_env!(InterpreterEnv);

#[derive(tapi::Tapi, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[tapi(path = "Interpreter")]
pub struct Input {
    pub commands: Stringify<Commands>,
    pub determinism: Determinism,
    pub assignment: InterpreterMemory,
    pub trace_length: Int,
}

#[derive(tapi::Tapi, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[tapi(path = "Interpreter")]
pub struct Output {
    pub initial_node: String,
    pub final_node: String,
    pub dot: String,
    pub trace: Vec<Step>,
    pub termination: TerminationState,
}

impl Env for InterpreterEnv {
    type Input = Input;

    type Output = Output;

    type Meta = BTreeSet<TargetDef>;

    fn meta(input: &Self::Input) -> Self::Meta {
        if let Ok(commands) = input.commands.try_parse() {
            commands.fv().into_iter().map(|t| t.def()).collect()
        } else {
            Default::default()
        }
    }

    fn run(input: &Self::Input) -> ce_core::Result<Self::Output> {
        let pg = gcl::pg::ProgramGraph::new(
            input.determinism,
            &input.commands.try_parse().map_err(|err| {
                ce_core::EnvError::InvalidInputForProgram {
                    message: "failed to parse commands".to_string(),
                    source: Some(Box::new(err)),
                }
            })?,
        );

        let mut exe = Execution::new(input.assignment.clone());

        for _ in 0..input.trace_length {
            if let Some(next) = exe.nexts(&pg).first().cloned() {
                if next.is_stuck(&pg) {
                    exe = next;
                    break;
                }
                exe = next;
                continue;
            }

            break;
        }

        Ok(Output {
            initial_node: Node::Start.to_string(),
            final_node: Node::End.to_string(),
            dot: pg.dot(),
            trace: exe.trace().iter().map(|(s, _)| s.clone()).collect(),
            termination: exe.state(&pg),
        })
    }

    fn validate(input: &Self::Input, output: &Self::Output) -> ce_core::Result<ValidationResult> {
        let pg = gcl::pg::ProgramGraph::new(
            input.determinism,
            &input.commands.try_parse().map_err(|err| {
                ce_core::EnvError::InvalidInputForProgram {
                    message: "failed to parse commands".to_string(),
                    source: Some(Box::new(err)),
                }
            })?,
        );
        let mut possible_executions = vec![Execution::new(input.assignment.clone())];

        for step in &output.trace {
            possible_executions = possible_executions
                .iter()
                .flat_map(|exe| exe.nexts(&pg))
                .filter(|exe| exe.current_mem() == &step.memory)
                .collect();

            if possible_executions.is_empty() {
                return Ok(ValidationResult::Mismatch {
                    reason: "No possible execution found".to_string(),
                });
            }
        }

        if output.termination == TerminationState::Running && !possible_executions.is_empty() {
            return Ok(ValidationResult::CorrectNonTerminated {
                iterations: output.trace.len() as u64,
            });
        }

        if output.termination == TerminationState::Terminated {
            if possible_executions.iter().any(|s| s.is_finished()) {
                return Ok(ValidationResult::CorrectTerminated);
            }
            return Ok(ValidationResult::Mismatch {
                reason: "No execution reached the end".to_string(),
            });
        }

        if output.trace.len() < input.trace_length as usize
            || output.termination == TerminationState::Stuck
        {
            if output.termination == TerminationState::Running {
                return Ok(ValidationResult::Mismatch {
                    reason: "Not enough traces were produced".to_string(),
                });
            }

            if !possible_executions.iter().any(|exe| exe.is_stuck(&pg)) {
                return Ok(ValidationResult::Mismatch {
                    reason: "No stuck execution found".to_string(),
                });
            }

            return Ok(ValidationResult::CorrectTerminated);
        }

        // TODO: check termination status is correct

        Ok(ValidationResult::CorrectTerminated)
    }
}

impl Generate for Input {
    type Context = ();

    fn gen<R: rand::Rng>(_cx: &mut Self::Context, mut rng: &mut R) -> Self {
        let commands = gcl::ast::Commands::gen(&mut Default::default(), rng);
        let initial_memory = gcl::memory::Memory::from_targets_with(
            commands.fv(),
            &mut rng,
            |rng, _| rng.gen_range(-10..=10),
            |rng, _| {
                let len = rng.gen_range(5..=10);
                (0..len).map(|_| rng.gen_range(-10..=10)).collect()
            },
        );
        let assignment = InterpreterMemory {
            variables: initial_memory.variables,
            arrays: initial_memory.arrays,
        };

        let determinism = *[Determinism::Deterministic, Determinism::NonDeterministic]
            .choose(rng)
            .unwrap();

        Input {
            commands: Stringify::new(commands),
            determinism,
            assignment,
            trace_length: rng.gen_range(10..=15),
        }
    }
}
