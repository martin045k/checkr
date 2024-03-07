//! Config definitions for program inputs and groups of group.

use std::{fs, path::Path};

use ce_shell::{Analysis, Input};
use color_eyre::{eyre::Context, Result};
use indexmap::IndexMap;
use rand::SeedableRng;
use serde::{Deserialize, Serialize};

#[derive(tapi::Tapi, Debug, Default, Clone, Serialize, Deserialize)]
pub struct GroupsConfig {
    #[serde(default)]
    pub ignored_authors: Vec<String>,
    pub groups: Vec<GroupConfig>,
}

#[derive(tapi::Tapi, Debug, Default, Clone, Serialize, Deserialize)]
pub struct ProgramsConfig {
    #[serde(default)]
    pub deadlines: IndexMap<Analysis, ProgramsDeadline>,
    #[serde(default)]
    pub envs: IndexMap<Analysis, ProgramsEnvConfig>,
}

#[derive(tapi::Tapi, Debug, Default, Clone, Serialize, Deserialize)]
pub struct ProgramsDeadline {
    pub time: Option<chrono::DateTime<chrono::FixedOffset>>,
}

#[derive(tapi::Tapi, Debug, Default, Clone, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ProgramsEnvConfig {
    pub programs: Vec<ProgramConfig>,
}

#[derive(tapi::Tapi, Debug, Clone, Serialize, Deserialize)]
pub struct ProgramConfig {
    pub seed: Option<u64>,
    pub input: Option<String>,
    #[serde(default)]
    pub shown: bool,
}

#[derive(tapi::Tapi, Debug, Default, Clone, Serialize, Deserialize)]
pub struct CanonicalProgramsConfig {
    #[serde(default)]
    pub envs: IndexMap<Analysis, CanonicalProgramsEnvConfig>,
}
#[derive(
    Debug, Default, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize,
)]
pub struct ProgramId(usize);
impl CanonicalProgramsConfig {
    pub(crate) fn get(&self, analysis: Analysis, input: ProgramId) -> &CanonicalProgramConfig {
        &self.envs[&analysis].programs[input.0]
    }
}

// impl CanonicalProgramConfig {
//     pub fn generated_program(&self, analysis: Analysis) -> Result<GeneratedProgram> {
//         let builder = analysis.setup_generation();
//         Ok(builder.from_cmds_and_input(
//             gcl::parse::parse_commands(&self.src).unwrap(),
//             analysis.input_from_str(&self.input)?,
//         ))
//     }
// }
#[derive(tapi::Tapi, Debug, Default, Clone, Serialize, Deserialize)]
#[serde(transparent)]
pub struct CanonicalProgramsEnvConfig {
    pub programs: Vec<CanonicalProgramConfig>,
}
impl CanonicalProgramsEnvConfig {
    pub(crate) fn programs(&self) -> impl Iterator<Item = (ProgramId, &CanonicalProgramConfig)> {
        self.programs
            .iter()
            .enumerate()
            .map(|(idx, p)| (ProgramId(idx), p))
    }
}
#[derive(tapi::Tapi, Debug, Clone, Serialize, Deserialize)]
pub struct CanonicalProgramConfig {
    pub input: String,
    pub shown: bool,
}
impl ProgramsConfig {
    pub fn extend(&mut self, other: Self) {
        for (analysis, env) in other.envs {
            self.envs
                .entry(analysis)
                .or_default()
                .programs
                .extend_from_slice(&env.programs);
        }
    }
    pub fn canonicalize(&self) -> Result<CanonicalProgramsConfig> {
        let envs = self
            .envs
            .iter()
            .map(|(&analysis, env)| {
                (
                    analysis,
                    CanonicalProgramsEnvConfig {
                        programs: env
                            .programs
                            .iter()
                            .map(|p| p.canonicalize(analysis).unwrap())
                            .collect(),
                    },
                )
            })
            .collect();

        Ok(CanonicalProgramsConfig { envs })
    }

    pub(crate) fn inputs(&self) -> impl Iterator<Item = Input> + '_ {
        self.envs.iter().flat_map(|(analysis, p)| {
            p.programs.iter().map(move |p| {
                let c = p.canonicalize(*analysis).unwrap();
                analysis.input_from_str(&c.input).unwrap()
            })
        })
    }
}
impl ProgramConfig {
    fn canonicalize(&self, analysis: Analysis) -> Result<CanonicalProgramConfig> {
        Ok(match self {
            ProgramConfig {
                seed: Some(seed),
                input: None,
                ..
            } => CanonicalProgramConfig {
                input: analysis
                    .gen_input(&mut rand::rngs::SmallRng::seed_from_u64(*seed))
                    .to_string(),
                shown: self.shown,
            },
            ProgramConfig {
                input: Some(input), ..
            } => CanonicalProgramConfig {
                input: input.to_string(),
                shown: self.shown,
            },
            pc => todo!("{pc:?}"),
        })
    }
    // pub fn canonicalize(&self, analysis: Analysis) -> Result<CanonicalProgramConfig> {
    //     let p = self.generated_program(analysis)?;

    //     Ok(CanonicalProgramConfig {
    //         src: p.cmds.to_string(),
    //         input: p.input.to_string(),
    //         shown: self.shown,
    //     })
    // }
}

#[derive(tapi::Tapi, Default, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct GroupName(pub String);

impl std::fmt::Debug for GroupName {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:?}", self.0)
    }
}

impl GroupName {
    pub fn as_str(&self) -> &str {
        self
    }
}

impl std::fmt::Display for GroupName {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl std::ops::Deref for GroupName {
    type Target = str;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

#[derive(tapi::Tapi, Debug, Default, Clone, Hash, Serialize, Deserialize)]
pub struct GroupConfig {
    pub name: GroupName,
    pub git: Option<String>,
    pub path: Option<String>,
    pub run: Option<String>,
}

pub fn read_programs(programs: impl AsRef<Path>) -> Result<ProgramsConfig> {
    let p = programs.as_ref();
    let src =
        fs::read_to_string(p).wrap_err_with(|| format!("could not read programs at {p:?}"))?;
    let parsed =
        toml::from_str(&src).wrap_err_with(|| format!("error parsing programs from file {p:?}"))?;
    Ok(parsed)
}
pub fn read_groups(groups: impl AsRef<Path>) -> Result<GroupsConfig> {
    let p = groups.as_ref();
    let src = fs::read_to_string(p).wrap_err_with(|| format!("could not read groups at {p:?}"))?;
    let parsed =
        toml::from_str(&src).wrap_err_with(|| format!("error parsing groups from file {p:?}"))?;
    Ok(parsed)
}
