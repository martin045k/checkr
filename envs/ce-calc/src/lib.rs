use ce_core::{define_env, gen::GclGenContext, rand, Env, Generate, ValidationResult};
use gcl::{ast::AExpr, stringify::Stringify};
use serde::{Deserialize, Serialize};

define_env!(CalcEnv);

#[derive(tapi::Tapi, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[tapi(path = "Calc")]
pub struct Input {
    pub expression: Stringify<AExpr>,
}

#[derive(tapi::Tapi, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[tapi(path = "Calc")]
pub struct Output {
    pub result: String,
    pub error: String,
}

impl Env for CalcEnv {
    type Input = Input;

    type Output = Output;

    type Meta = ();

    fn run(input: &Self::Input) -> ce_core::Result<Self::Output> {
        let expr = input.expression.try_parse().map_err(|err| {
            ce_core::EnvError::InvalidInputForProgram {
                message: "failed to parse expression".to_string(),
                source: Some(Box::new(err)),
            }
        })?;
        let (result, error) = match expr.semantics(&gcl::semantics::EmptySemanticsContext) {
            Ok(result) => (result.to_string(), String::new()),
            Err(err) => {
                let error = format!("{}", err);
                (String::new(), error)
            }
        };

        Ok(Output { result, error })
    }

    fn validate(input: &Self::Input, output: &Self::Output) -> ce_core::Result<ValidationResult> {
        let reference = Self::run(input)?;

        Ok(
            match (
                &reference.result,
                &output.result,
                !reference.error.is_empty(),
                !output.error.is_empty(),
            ) {
                // Both errors are present
                (_, _, true, true) => ValidationResult::CorrectTerminated,
                // Both results are present
                (r, o, _, _) if r == o => ValidationResult::CorrectTerminated,
                (_, _, _, _) => {
                    let info = format!(
                        "Output: result={:?}, error={:?}; Reference: result={:?}, error={:?}",
                        output.result, output.error, reference.result, reference.error,
                    );
                    ValidationResult::Mismatch {
                        reason: format!("Did not produce same as reference. {info}"),
                    }
                }
            },
        )
    }
}

impl Generate for Input {
    type Context = ();

    fn gen<R: rand::Rng>(_cx: &mut Self::Context, rng: &mut R) -> Self {
        let mut gen_expr = || {
            AExpr::gen(
                &mut GclGenContext {
                    names: Vec::new(),
                    ..GclGenContext::new(25, rng)
                },
                rng,
            )
        };

        let mut expr = gen_expr();
        for _ in 0..10 {
            if expr
                .semantics(&gcl::semantics::EmptySemanticsContext)
                .is_ok()
            {
                break;
            }
            expr = gen_expr();
        }

        Input {
            expression: Stringify::new(expr),
        }
    }
}
