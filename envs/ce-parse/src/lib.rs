use ce_core::{define_env, rand, Env, Generate, ValidationResult};
use gcl::{ast::Commands, stringify::Stringify};
use serde::{Deserialize, Serialize};

define_env!(ParseEnv);

#[derive(tapi::Tapi, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[tapi(path = "Parser")]
pub struct ParseInput {
    commands: Stringify<Commands>,
}

#[derive(tapi::Tapi, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[tapi(path = "Parser")]
pub struct ParseOutput {
    pretty: Stringify<Commands>,
}

impl Env for ParseEnv {
    type Input = ParseInput;

    type Output = ParseOutput;

    type Meta = ();

    fn run(input: &Self::Input) -> ce_core::Result<Self::Output> {
        Ok(ParseOutput {
            pretty: Stringify::new(input.commands.try_parse().map_err(|err| {
                ce_core::EnvError::InvalidInputForProgram {
                    message: "failed to parse commands".to_string(),
                    source: Some(Box::new(err)),
                }
            })?),
        })
    }

    fn validate(_input: &Self::Input, output: &Self::Output) -> ce_core::Result<ValidationResult> {
        match output.pretty.try_parse() {
            Ok(_) => Ok(ValidationResult::CorrectTerminated),
            Err(err) => Ok(ValidationResult::Mismatch {
                reason: format!("failed to parse pretty output: {:?}", err),
            }),
        }
    }
}

impl Generate for ParseInput {
    type Context = ();

    fn gen<R: rand::Rng>(_cx: &mut Self::Context, rng: &mut R) -> Self {
        Self {
            commands: Stringify::new(Commands::gen(&mut Default::default(), rng)),
        }
    }
}
