use serde::{Deserialize, Serialize};

use crate::catalog::models::InstallMethod;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Candidate {
    pub package: String,
    pub method: InstallMethod,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ResolverDecision {
    pub exact: Vec<Candidate>,
    pub prefix: Vec<Candidate>,
    pub contains: Vec<Candidate>,
}

impl ResolverDecision {
    pub fn auto_candidate(&self) -> Option<&Candidate> {
        if self.exact.len() == 1 {
            return self.exact.first();
        }
        if self.exact.is_empty() && self.prefix.len() == 1 {
            return self.prefix.first();
        }
        if self.exact.is_empty() && self.prefix.is_empty() && self.contains.len() == 1 {
            return self.contains.first();
        }
        None
    }

    pub fn requires_user_selection(&self) -> bool {
        self.auto_candidate().is_none()
            && (!self.exact.is_empty() || !self.prefix.is_empty() || !self.contains.is_empty())
    }
}

pub fn rank_candidates(hint: &str, candidates: &[Candidate]) -> ResolverDecision {
    let mut exact = Vec::new();
    let mut prefix = Vec::new();
    let mut contains = Vec::new();

    let hint_lc = hint.to_ascii_lowercase();
    for candidate in candidates {
        let package = candidate.package.to_ascii_lowercase();
        if package == hint_lc {
            exact.push(candidate.clone());
            continue;
        }
        if package.starts_with(&hint_lc) {
            prefix.push(candidate.clone());
            continue;
        }
        if package.contains(&hint_lc) {
            contains.push(candidate.clone());
        }
    }

    ResolverDecision {
        exact,
        prefix,
        contains,
    }
}
