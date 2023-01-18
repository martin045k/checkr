use std::{
    collections::{HashMap, HashSet},
    fmt::Display,
};

use itertools::Itertools;
use serde::{Deserialize, Serialize};

use crate::{
    ast::{Array, Command, Commands, Guard, Variable},
    env::ToMarkdown,
    gcl,
    parse::ParseError,
};

#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct Flow<T> {
    pub from: T,
    pub into: T,
}

impl<T> std::fmt::Debug for Flow<T>
where
    T: std::fmt::Debug,
{
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Flow({:?} -> {:?})", self.from, self.into)
    }
}
impl<T> Flow<T> {
    fn map<S>(&self, f: impl Fn(&T) -> S) -> Flow<S> {
        Flow {
            from: f(&self.from),
            into: f(&self.into),
        }
    }
}

impl<T> Display for Flow<T>
where
    T: Display,
{
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{} -> {}", self.from, self.into)
    }
}

impl Commands {
    pub fn flows(&self) -> HashSet<Flow<Variable>> {
        self.sec(&Default::default())
    }
    fn sec(&self, implicit: &HashSet<Variable>) -> HashSet<Flow<Variable>> {
        self.0.iter().flat_map(|c| c.sec(implicit)).collect()
    }
}

impl Command {
    fn sec(&self, implicit: &HashSet<Variable>) -> HashSet<Flow<Variable>> {
        match self {
            Command::Assignment(Variable(x), a) => implicit
                .iter()
                .cloned()
                .chain(a.fv())
                .map(|i| Flow {
                    from: i,
                    into: Variable(x.clone()),
                })
                .collect(),
            Command::Skip => HashSet::default(),
            Command::If(c) | Command::Loop(c) => {
                c.iter()
                    .fold(
                        (implicit.clone(), HashSet::default()),
                        |(implicit, flows), guard| {
                            let (new_implicit, new_flows) = guard.sec2(&implicit);

                            (
                                implicit.union(&new_implicit).cloned().collect(),
                                flows.union(&new_flows).cloned().collect(),
                            )
                        },
                    )
                    .1
            }
            Command::ArrayAssignment(Array(arr, idx), a) => implicit
                .iter()
                .cloned()
                .chain(a.fv())
                .chain(idx.fv())
                // TODO: Should this really be variable?
                .map(|i| Flow {
                    from: i,
                    into: Variable(arr.clone()),
                })
                .collect(),
            Command::Break => HashSet::default(),
            Command::Continue => HashSet::default(),
        }
    }
}

impl Guard {
    fn sec2(&self, implicit: &HashSet<Variable>) -> (HashSet<Variable>, HashSet<Flow<Variable>>) {
        let implicit = implicit.iter().cloned().chain(self.0.fv()).collect();
        let flows = self.1.sec(&implicit);
        (implicit, flows)
    }
}

#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct SecurityClass(pub String);

impl std::fmt::Debug for SecurityClass {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "SecurityClass({})", self.0)
    }
}
impl std::fmt::Display for SecurityClass {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SecurityLattice {
    allowed: HashSet<Flow<SecurityClass>>,
}

impl SecurityLattice {
    pub fn new(flows: &[Flow<SecurityClass>]) -> SecurityLattice {
        let mut allowed: HashSet<Flow<SecurityClass>> = flows.iter().cloned().collect();
        let mut last_len = 0;
        loop {
            let mut to_add: HashSet<Flow<SecurityClass>> = HashSet::new();
            for f in &allowed {
                for a in &allowed {
                    // a -> b, b -> c
                    // --------------
                    //     a -> c
                    let new_flow = Flow {
                        from: f.from.clone(),
                        into: a.into.clone(),
                    };
                    if f.into == a.from && !allowed.contains(&new_flow) {
                        to_add.insert(new_flow);
                    }
                }
            }

            for f in to_add {
                allowed.insert(f);
            }

            if allowed.len() == last_len {
                break;
            }
            last_len = allowed.len();
        }

        SecurityLattice { allowed }
    }
    pub fn parse(src: &str) -> anyhow::Result<SecurityLattice> {
        let flows = gcl::SecurityLatticeParser::new()
            .parse(src)
            .map_err(|e| ParseError::new(src, e))?;

        Ok(Self::new(&flows))
    }
    pub fn allows(&self, f: &Flow<SecurityClass>) -> bool {
        f.from == f.into || self.allowed.contains(f)
    }

    fn all_allowed<'a>(
        &'a self,
        classification: &'a HashMap<Variable, SecurityClass>,
    ) -> impl Iterator<Item = Flow<Variable>> + 'a {
        classification
            .iter()
            .cartesian_product(classification.iter())
            .filter_map(|((a, a_class), (b, b_class))| {
                if self.allows(&Flow {
                    from: a_class.clone(),
                    into: b_class.clone(),
                }) {
                    Some(Flow {
                        from: a.clone(),
                        into: b.clone(),
                    })
                } else {
                    None
                }
            })
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct SecurityAnalysisOutput {
    pub actual: Vec<Flow<Variable>>,
    pub allowed: Vec<Flow<Variable>>,
    pub violations: Vec<Flow<Variable>>,
}

impl SecurityAnalysisOutput {
    pub fn run(
        mapping: &HashMap<Variable, SecurityClass>,
        lattice: &SecurityLattice,
        cmds: &Commands,
    ) -> Self {
        let allowed = lattice.all_allowed(mapping).collect();
        let actual = cmds.flows();
        let violations = actual
            .iter()
            .cloned()
            .filter(|flow| !lattice.allows(&flow.map(|f| mapping[f].clone())))
            .collect();

        Self {
            actual: actual.into_iter().collect(),
            allowed,
            violations,
        }
    }
}
